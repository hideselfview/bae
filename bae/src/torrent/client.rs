// NOTE: This module requires libtorrent C++ library to be installed on the system.
// On macOS: brew install libtorrent-rasterbar
// The libtorrent-rs crate provides Rust bindings to the C++ library.
//
// The libtorrent-rs crate (v0.1.1) provides a minimal API. We use what's available
// and parse bencoded data for additional torrent metadata.

use crate::torrent::ffi::{
    create_bae_storage_constructor, create_session_params_default,
    create_session_params_with_storage, create_session_with_params, get_session_ptr,
    load_torrent_file, parse_magnet_uri, session_add_torrent, torrent_get_file_list,
    torrent_get_name, torrent_get_num_pieces, torrent_get_piece_length, torrent_get_progress,
    torrent_get_storage_index, torrent_get_total_size, torrent_has_metadata, torrent_have_piece,
    torrent_set_file_priorities, Session, TorrentFileInfo, TorrentHandle as FfiTorrentHandle,
};
use crate::torrent::storage::BaeStorage;
use cxx::UniquePtr;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::error;

#[derive(Error, Debug)]
pub enum TorrentError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Libtorrent error: {0}")]
    Libtorrent(String),
    #[error("Invalid torrent: {0}")]
    InvalidTorrent(String),
    #[error("Not implemented: {0}")]
    NotImplemented(String),
}

/// Priority for torrent files
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilePriority {
    DoNotDownload = 0,
    Normal = 4,
    High = 6,
    Maximum = 7,
}

/// Wrapper around Arc that is Send-safe
///
/// SAFETY: This wraps Arc<RwLock<UniquePtr>> which isn't Send because UniquePtr isn't Send.
/// However, we guarantee that the session is only used from a single task context.
/// The Arc ensures reference-counting, and we only use it sequentially.
struct SendSafeArc<T>(Arc<T>);

unsafe impl<T> Send for SendSafeArc<T> {}

/// Wrapper around libtorrent session that is Send-safe
/// Uses our FFI Session type (custom storage backend)
struct SendSafeSession(SendSafeArc<RwLock<UniquePtr<Session>>>);

unsafe impl Send for SendSafeSession {}

/// Storage registry for mapping torrent IDs to BaeStorage instances
/// Used by sync callbacks to access async storage methods
type StorageRegistry = Arc<RwLock<HashMap<String, Arc<RwLock<BaeStorage>>>>>;

/// Mapping from libtorrent storage_index_t to torrent_id
/// libtorrent assigns a storage_index when a torrent is added, we need to map it to our torrent_id
type StorageIndexMap = Arc<RwLock<HashMap<i32, String>>>;

thread_local! {
    pub(crate) static STORAGE_REGISTRY: std::cell::RefCell<Option<StorageRegistry>> = std::cell::RefCell::new(None);
    static STORAGE_INDEX_MAP: std::cell::RefCell<Option<StorageIndexMap>> = std::cell::RefCell::new(None);
    static RUNTIME_HANDLE: std::cell::RefCell<Option<tokio::runtime::Handle>> = std::cell::RefCell::new(None);
}

/// Wrapper around libtorrent session
pub struct TorrentClient {
    session: SendSafeSession,
    runtime_handle: tokio::runtime::Handle,
    storage_registry: StorageRegistry,
    storage_index_map: StorageIndexMap,
}

// SAFETY: TorrentClient contains UniquePtr which isn't Send, but we only use it
// from a single task context. The Arc ensures the session is reference-counted
// and can be safely moved between tasks as long as we don't actually use it
// concurrently. In our use case, we create the client in one task and use it
// sequentially in that same task, so this is safe.
unsafe impl Send for TorrentClient {}

impl Clone for TorrentClient {
    fn clone(&self) -> Self {
        TorrentClient {
            session: SendSafeSession(SendSafeArc(Arc::clone(&self.session.0 .0))),
            runtime_handle: self.runtime_handle.clone(),
            storage_registry: Arc::clone(&self.storage_registry),
            storage_index_map: Arc::clone(&self.storage_index_map),
        }
    }
}

impl TorrentClient {
    /// Create a new torrent client with custom storage
    ///
    /// This creates a session with custom storage backend for use in the main import flow.
    /// Storage instances will be registered per-torrent when adding torrents.
    pub fn new(runtime_handle: tokio::runtime::Handle) -> Result<Self, TorrentError> {
        let storage_registry: StorageRegistry = Arc::new(RwLock::new(HashMap::new()));
        let storage_index_map: StorageIndexMap = Arc::new(RwLock::new(HashMap::new()));

        // Store registry and runtime handle in thread-local for callbacks to access
        let registry_clone = Arc::clone(&storage_registry);
        let index_map_clone = Arc::clone(&storage_index_map);
        let runtime_clone = runtime_handle.clone();

        STORAGE_REGISTRY.with(|tl| {
            *tl.borrow_mut() = Some(registry_clone);
        });
        STORAGE_INDEX_MAP.with(|tl| {
            *tl.borrow_mut() = Some(index_map_clone);
        });
        RUNTIME_HANDLE.with(|tl| {
            *tl.borrow_mut() = Some(runtime_clone);
        });

        // Create callback functions that will be called from C++
        // These are 'static fn' pointers that look up storage from thread-local registry
        fn read_callback(storage_index: i32, piece_index: i32, offset: i32, size: i32) -> Vec<u8> {
            STORAGE_REGISTRY.with(|registry_tl| {
                STORAGE_INDEX_MAP.with(|index_map_tl| {
                    RUNTIME_HANDLE.with(|runtime_tl| {
                        if let (Some(registry), Some(index_map), Some(runtime)) = (
                            registry_tl.borrow().as_ref(),
                            index_map_tl.borrow().as_ref(),
                            runtime_tl.borrow().as_ref(),
                        ) {
                            // Look up torrent_id and storage in a single async block
                            runtime.block_on(async {
                                // Look up torrent_id from storage_index
                                let torrent_id = {
                                    let map_guard = index_map.read().await;
                                    map_guard.get(&storage_index).cloned()
                                };

                                if let Some(torrent_id) = torrent_id {
                                    // Look up storage and call async method
                                    let registry_guard = registry.read().await;
                                    if let Some(storage) = registry_guard.get(&torrent_id) {
                                        let storage_guard = storage.read().await;
                                        storage_guard
                                            .read_piece(piece_index, offset, size)
                                            .await
                                            .unwrap_or_else(|e| {
                                                error!("Storage read error: {}", e);
                                                vec![]
                                            })
                                    } else {
                                        error!("Storage not found for torrent_id: {}", torrent_id);
                                        vec![]
                                    }
                                } else {
                                    error!(
                                        "No torrent_id mapped for storage_index: {}",
                                        storage_index
                                    );
                                    vec![]
                                }
                            })
                        } else {
                            error!("Thread-local storage not initialized");
                            vec![]
                        }
                    })
                })
            })
        }

        fn write_callback(storage_index: i32, piece_index: i32, offset: i32, data: &[u8]) -> bool {
            STORAGE_REGISTRY.with(|registry_tl| {
                STORAGE_INDEX_MAP.with(|index_map_tl| {
                    RUNTIME_HANDLE.with(|runtime_tl| {
                        if let (Some(registry), Some(index_map), Some(runtime)) = (
                            registry_tl.borrow().as_ref(),
                            index_map_tl.borrow().as_ref(),
                            runtime_tl.borrow().as_ref(),
                        ) {
                            // Look up torrent_id from storage_index
                            // Note: We can't use async here, so we use a blocking read
                            // This is safe because we're in a sync callback context
                            let torrent_id = {
                                // Use tokio's Handle::block_on for the read lock
                                // But we need to be careful - we're already in a block_on context
                                // So we use a blocking approach: create a new runtime or use the existing one
                                // Actually, we can't nest block_on, so we need a different approach
                                // For now, use a blocking mutex or try_lock
                                // TODO: Refactor to avoid nested block_on
                                let map_guard = runtime.block_on(index_map.read());
                                map_guard.get(&storage_index).cloned()
                            };

                            if let Some(torrent_id) = torrent_id {
                                // Look up storage and call async method
                                runtime.block_on(async {
                                    let registry_guard = registry.read().await;
                                    if let Some(storage) = registry_guard.get(&torrent_id) {
                                        let storage_guard = storage.read().await;
                                        storage_guard
                                            .write_piece(piece_index, offset, data)
                                            .await
                                            .map(|_| true)
                                            .unwrap_or_else(|e| {
                                                error!("Storage write error: {}", e);
                                                false
                                            })
                                    } else {
                                        error!("Storage not found for torrent_id: {}", torrent_id);
                                        false
                                    }
                                })
                            } else {
                                error!("No torrent_id mapped for storage_index: {}", storage_index);
                                false
                            }
                        } else {
                            error!("Thread-local storage not initialized");
                            false
                        }
                    })
                })
            })
        }

        fn hash_callback(storage_index: i32, piece_index: i32, hash: &[u8]) -> bool {
            STORAGE_REGISTRY.with(|registry_tl| {
                STORAGE_INDEX_MAP.with(|index_map_tl| {
                    RUNTIME_HANDLE.with(|runtime_tl| {
                        if let (Some(registry), Some(index_map), Some(runtime)) = (
                            registry_tl.borrow().as_ref(),
                            index_map_tl.borrow().as_ref(),
                            runtime_tl.borrow().as_ref(),
                        ) {
                            // Look up torrent_id from storage_index
                            // Note: We can't use async here, so we use a blocking read
                            // This is safe because we're in a sync callback context
                            let torrent_id = {
                                // Use tokio's Handle::block_on for the read lock
                                // But we need to be careful - we're already in a block_on context
                                // So we use a blocking approach: create a new runtime or use the existing one
                                // Actually, we can't nest block_on, so we need a different approach
                                // For now, use a blocking mutex or try_lock
                                // TODO: Refactor to avoid nested block_on
                                let map_guard = runtime.block_on(index_map.read());
                                map_guard.get(&storage_index).cloned()
                            };

                            if let Some(torrent_id) = torrent_id {
                                // Look up storage and call async method
                                runtime.block_on(async {
                                    let registry_guard = registry.read().await;
                                    if let Some(storage) = registry_guard.get(&torrent_id) {
                                        let storage_guard = storage.read().await;
                                        storage_guard
                                            .hash_piece(piece_index, hash)
                                            .await
                                            .unwrap_or_else(|e| {
                                                error!("Storage hash error: {}", e);
                                                false
                                            })
                                    } else {
                                        error!("Storage not found for torrent_id: {}", torrent_id);
                                        false
                                    }
                                })
                            } else {
                                error!("No torrent_id mapped for storage_index: {}", storage_index);
                                false
                            }
                        } else {
                            error!("Thread-local storage not initialized");
                            false
                        }
                    })
                })
            })
        }

        // Create storage constructor with callbacks
        let storage_constructor =
            create_bae_storage_constructor(read_callback, write_callback, hash_callback);

        // Create session with custom storage backend
        let session_params = create_session_params_with_storage(storage_constructor);
        let custom_session = create_session_with_params(session_params);

        if custom_session.is_null() {
            return Err(TorrentError::Libtorrent(
                "Failed to create libtorrent session with custom storage".to_string(),
            ));
        }

        Ok(TorrentClient {
            session: SendSafeSession(SendSafeArc(Arc::new(RwLock::new(custom_session)))),
            runtime_handle,
            storage_registry,
            storage_index_map,
        })
    }

    /// Create a new torrent client with default disk storage
    ///
    /// This creates a session that uses libtorrent's default disk storage,
    /// which writes files directly to disk. Useful for temporary operations
    /// like metadata detection where we don't need custom storage.
    pub fn new_with_default_storage(
        runtime_handle: tokio::runtime::Handle,
    ) -> Result<Self, TorrentError> {
        // Create session with default storage (no custom storage)
        let session_params = create_session_params_default();
        let default_session = create_session_with_params(session_params);

        if default_session.is_null() {
            return Err(TorrentError::Libtorrent(
                "Failed to create libtorrent session with default storage".to_string(),
            ));
        }

        // For default storage, we don't need storage registry or index map
        // but we still create empty ones to satisfy the struct
        let storage_registry: StorageRegistry = Arc::new(RwLock::new(HashMap::new()));
        let storage_index_map: StorageIndexMap = Arc::new(RwLock::new(HashMap::new()));

        Ok(TorrentClient {
            session: SendSafeSession(SendSafeArc(Arc::new(RwLock::new(default_session)))),
            runtime_handle,
            storage_registry,
            storage_index_map,
        })
    }

    /// Register a BaeStorage instance for a torrent
    ///
    /// This allows the storage callbacks to look up and use the storage instance
    /// when libtorrent requests piece reads/writes.
    ///
    /// `storage_index` is the libtorrent-assigned storage index for this torrent.
    /// `torrent_id` is our internal torrent identifier.
    pub async fn register_storage(
        &self,
        storage_index: i32,
        torrent_id: String,
        storage: BaeStorage,
    ) {
        // Register storage instance
        let mut registry = self.storage_registry.write().await;
        registry.insert(torrent_id.clone(), Arc::new(RwLock::new(storage)));

        // Map storage_index to torrent_id
        let mut index_map = self.storage_index_map.write().await;
        index_map.insert(storage_index, torrent_id);
    }

    /// Read a piece by torrent_id (for use by seeder)
    ///
    /// This allows reading pieces from storage without needing a TorrentHandle.
    pub async fn read_piece_by_torrent_id(
        &self,
        torrent_id: &str,
        piece_index: usize,
    ) -> Result<Vec<u8>, TorrentError> {
        // Look up storage from registry
        let registry_guard = self.storage_registry.read().await;
        let storage = registry_guard.get(torrent_id).ok_or_else(|| {
            TorrentError::Libtorrent(format!("Storage not found for torrent_id: {}", torrent_id))
        })?;

        // Read piece from storage
        let storage_guard = storage.read().await;
        storage_guard
            .read_piece(piece_index as i32, 0, 0)
            .await
            .map_err(|e| {
                TorrentError::Libtorrent(format!("Failed to read piece {}: {}", piece_index, e))
            })
    }

    /// Add a torrent from a file
    pub async fn add_torrent_file(&self, path: &Path) -> Result<TorrentHandle, TorrentError> {
        // Convert path to string
        let file_path = path
            .to_str()
            .ok_or_else(|| TorrentError::InvalidTorrent("Invalid file path encoding".to_string()))?
            .to_string();

        // Use a temporary save path - we'll handle data ourselves
        let temp_path = std::env::temp_dir().to_string_lossy().to_string();

        // Extract session reference to avoid capturing self across await
        let session = SendSafeArc(Arc::clone(&self.session.0 .0));

        // Get write lock first
        let mut session_guard = session.0.write().await;

        // Load torrent file using our wrapper function
        let mut params = load_torrent_file(&file_path, &temp_path);
        if params.is_null() {
            drop(session_guard);
            return Err(TorrentError::InvalidTorrent(
                "Failed to load torrent file".to_string(),
            ));
        }

        // Get raw session pointer for wrapper function
        let session_ptr = get_session_ptr(&mut *session_guard);
        if session_ptr.is_null() {
            drop(session_guard);
            drop(params);
            return Err(TorrentError::Libtorrent(
                "Failed to get session pointer".to_string(),
            ));
        }

        // Add torrent using our wrapper function
        let handle_ptr = unsafe { session_add_torrent(session_ptr, &mut params) };

        // Drop guard and params immediately
        drop(session_guard);
        drop(params);

        if handle_ptr.is_null() {
            return Err(TorrentError::Libtorrent(
                "Failed to add torrent to session".to_string(),
            ));
        }

        // Get storage_index for this torrent (needed for read_piece)
        let storage_index = unsafe { torrent_get_storage_index(handle_ptr) };

        // Store raw pointer directly (opaque type, can't use UniquePtr)
        // SAFETY: We own the handle_ptr returned from session_add_torrent
        // The handle will be valid as long as the session exists
        Ok(TorrentHandle {
            handle: SendSafeTorrentHandle(Arc::new(RwLock::new(handle_ptr))),
            storage_index,
        })
    }

    /// Add a torrent from magnet link
    pub async fn add_magnet_link(&self, magnet: &str) -> Result<TorrentHandle, TorrentError> {
        // Use a temporary save path - we'll handle data ourselves
        let temp_path = std::env::temp_dir().to_string_lossy().to_string();

        // Extract session reference to avoid capturing self across await
        // Keep it wrapped in SendSafeArc to maintain Send safety
        let session = SendSafeArc(Arc::clone(&self.session.0 .0));

        // Get write lock first
        let mut session_guard = session.0.write().await;

        // Parse magnet URI using our wrapper function
        let mut params = parse_magnet_uri(magnet, &temp_path);
        if params.is_null() {
            drop(session_guard);
            return Err(TorrentError::InvalidTorrent(
                "Failed to parse magnet URI".to_string(),
            ));
        }

        // Get raw session pointer for wrapper function
        let session_ptr = get_session_ptr(&mut *session_guard);
        if session_ptr.is_null() {
            drop(session_guard);
            drop(params);
            return Err(TorrentError::Libtorrent(
                "Failed to get session pointer".to_string(),
            ));
        }

        // Add torrent using our wrapper function
        let handle_ptr = unsafe { session_add_torrent(session_ptr, &mut params) };

        // Drop guard and params immediately
        drop(session_guard);
        drop(params);

        if handle_ptr.is_null() {
            return Err(TorrentError::Libtorrent(
                "Failed to add torrent to session".to_string(),
            ));
        }

        // Get storage_index for this torrent (needed for read_piece)
        let storage_index = unsafe { torrent_get_storage_index(handle_ptr) };

        // Store raw pointer directly (opaque type, can't use UniquePtr)
        // SAFETY: We own the handle_ptr returned from session_add_torrent
        // The handle will be valid as long as the session exists
        Ok(TorrentHandle {
            handle: SendSafeTorrentHandle(Arc::new(RwLock::new(handle_ptr))),
            storage_index,
            // torrent_id will be looked up in read_piece from storage_index_map
            // Don't store session reference - TorrentHandle doesn't need it
            // The handle is self-contained and can be used independently
        })
    }
}

/// Wrapper around raw TorrentHandle pointer that is Send/Sync-safe
/// Uses our FFI TorrentHandle type (opaque, so we use raw pointer)
struct SendSafeTorrentHandle(Arc<RwLock<*mut FfiTorrentHandle>>);

unsafe impl Send for SendSafeTorrentHandle {}
unsafe impl Sync for SendSafeTorrentHandle {}

/// Handle to a torrent in the session
pub struct TorrentHandle {
    handle: SendSafeTorrentHandle,
    storage_index: i32,
    // Note: We don't store the session here to avoid Send issues
    // The handle is self-contained and doesn't need the session reference
}

// SAFETY: TorrentHandle contains UniquePtr which isn't Send/Sync, but we only use it
// from a single task context. The Arc ensures the handle is reference-counted
// and can be safely moved/shared between tasks as long as we don't actually use it
// concurrently. In our use case, we create the handle in one task and use it
// sequentially in that same task, so this is safe.
unsafe impl Send for TorrentHandle {}
unsafe impl Sync for TorrentHandle {}

impl TorrentHandle {
    /// Get the info hash of this torrent
    ///
    /// NOTE: The libtorrent-rs crate doesn't expose info_hash directly.
    /// We would need to extract it from bencoded data or extend FFI.
    pub async fn info_hash(&self) -> String {
        // TODO: Extract from bencoded data or extend FFI
        // For now, return placeholder
        "0000000000000000000000000000000000000000".to_string()
    }

    /// Get the storage index for this torrent
    pub async fn storage_index(&self) -> Result<i32, TorrentError> {
        let handle_guard = self.handle.0.read().await;
        let handle_ptr = *handle_guard;
        if handle_ptr.is_null() {
            return Err(TorrentError::Libtorrent(
                "Invalid torrent handle".to_string(),
            ));
        }
        let storage_index = unsafe { torrent_get_storage_index(handle_ptr) };
        if storage_index < 0 {
            return Err(TorrentError::Libtorrent(
                "Failed to get storage index".to_string(),
            ));
        }
        Ok(storage_index)
    }

    /// Get the name of the torrent
    pub async fn name(&self) -> Result<String, TorrentError> {
        let handle_guard = self.handle.0.read().await;
        let handle_ptr = *handle_guard;
        if handle_ptr.is_null() {
            return Err(TorrentError::Libtorrent(
                "Invalid torrent handle".to_string(),
            ));
        }
        let name = unsafe { torrent_get_name(handle_ptr) };
        Ok(name)
    }

    /// Get the total size of the torrent
    pub async fn total_size(&self) -> Result<i64, TorrentError> {
        let handle_guard = self.handle.0.read().await;
        let handle_ptr = *handle_guard;
        if handle_ptr.is_null() {
            return Err(TorrentError::Libtorrent(
                "Invalid torrent handle".to_string(),
            ));
        }
        let size = unsafe { torrent_get_total_size(handle_ptr) };
        if size == 0 {
            return Err(TorrentError::Libtorrent(
                "Failed to get total size (metadata may not be available)".to_string(),
            ));
        }
        Ok(size)
    }

    /// Get the piece length
    pub async fn piece_length(&self) -> Result<i32, TorrentError> {
        let handle_guard = self.handle.0.read().await;
        let handle_ptr = *handle_guard;
        if handle_ptr.is_null() {
            return Err(TorrentError::Libtorrent(
                "Invalid torrent handle".to_string(),
            ));
        }
        let length = unsafe { torrent_get_piece_length(handle_ptr) };
        if length == 0 {
            return Err(TorrentError::Libtorrent(
                "Failed to get piece length (metadata may not be available)".to_string(),
            ));
        }
        Ok(length)
    }

    /// Get the number of pieces
    pub async fn num_pieces(&self) -> Result<i32, TorrentError> {
        let handle_guard = self.handle.0.read().await;
        let handle_ptr = *handle_guard;
        if handle_ptr.is_null() {
            return Err(TorrentError::Libtorrent(
                "Invalid torrent handle".to_string(),
            ));
        }
        let num = unsafe { torrent_get_num_pieces(handle_ptr) };
        if num == 0 {
            return Err(TorrentError::Libtorrent(
                "Failed to get num pieces (metadata may not be available)".to_string(),
            ));
        }
        Ok(num)
    }

    /// Get the list of files in the torrent
    pub async fn get_file_list(&self) -> Result<Vec<TorrentFile>, TorrentError> {
        let handle_guard = self.handle.0.read().await;
        let handle_ptr = *handle_guard;
        if handle_ptr.is_null() {
            return Err(TorrentError::Libtorrent(
                "Invalid torrent handle".to_string(),
            ));
        }
        let file_infos = unsafe { torrent_get_file_list(handle_ptr) };
        drop(handle_guard);

        let files: Vec<TorrentFile> = file_infos
            .into_iter()
            .map(|info: TorrentFileInfo| TorrentFile {
                index: info.index,
                path: PathBuf::from(info.path),
                size: info.size,
                priority: FilePriority::Normal,
            })
            .collect();

        Ok(files)
    }

    /// Set file priorities
    pub async fn set_file_priorities(
        &self,
        priorities: Vec<FilePriority>,
    ) -> Result<(), TorrentError> {
        let handle_guard = self.handle.0.read().await;
        let handle_ptr = *handle_guard;
        if handle_ptr.is_null() {
            return Err(TorrentError::Libtorrent(
                "Invalid torrent handle".to_string(),
            ));
        }

        // Convert FilePriority enum to u8 values
        let priority_values: Vec<u8> = priorities.iter().map(|p| *p as u8).collect();

        let success = unsafe { torrent_set_file_priorities(handle_ptr, priority_values) };
        drop(handle_guard);

        if !success {
            return Err(TorrentError::Libtorrent(
                "Failed to set file priorities".to_string(),
            ));
        }

        Ok(())
    }

    /// Check if torrent is complete
    pub async fn is_complete(&self) -> Result<bool, TorrentError> {
        // TODO: Implement actual completion check
        Err(TorrentError::NotImplemented(
            "is_complete() not yet implemented".to_string(),
        ))
    }

    /// Get download progress (0.0 to 1.0)
    pub async fn progress(&self) -> Result<f32, TorrentError> {
        let handle_guard = self.handle.0.read().await;
        let handle_ptr = *handle_guard;
        if handle_ptr.is_null() {
            return Err(TorrentError::Libtorrent(
                "Invalid torrent handle".to_string(),
            ));
        }

        let progress = unsafe { torrent_get_progress(handle_ptr) };
        drop(handle_guard);

        Ok(progress)
    }

    /// Read a piece of data from our custom storage
    ///
    /// This reads from BaeStorage via the storage registry, which libtorrent
    /// populated when pieces were downloaded.
    pub async fn read_piece(&self, piece_index: usize) -> Result<Vec<u8>, TorrentError> {
        // Access storage registry and index map from thread-local storage
        let (registry, index_map) = STORAGE_REGISTRY
            .with(|registry_tl| {
                STORAGE_INDEX_MAP.with(|index_map_tl| {
                    if let (Some(reg), Some(idx_map)) = (
                        registry_tl.borrow().as_ref(),
                        index_map_tl.borrow().as_ref(),
                    ) {
                        Some((Arc::clone(reg), Arc::clone(idx_map)))
                    } else {
                        None
                    }
                })
            })
            .ok_or_else(|| {
                TorrentError::Libtorrent("Storage registry not initialized".to_string())
            })?;

        // Look up torrent_id from storage_index
        let torrent_id = {
            let map_guard = index_map.read().await;
            map_guard
                .get(&self.storage_index)
                .ok_or_else(|| {
                    TorrentError::Libtorrent(format!(
                        "No torrent_id mapped for storage_index: {}",
                        self.storage_index
                    ))
                })?
                .clone()
        };

        // Look up storage and read piece asynchronously
        let registry_guard = registry.read().await;
        let storage = registry_guard.get(&torrent_id).ok_or_else(|| {
            TorrentError::Libtorrent(format!("Storage not found for torrent_id: {}", torrent_id))
        })?;

        let storage_guard = storage.read().await;
        storage_guard
            .read_piece(piece_index as i32, 0, 0)
            .await
            .map_err(|e| {
                TorrentError::Libtorrent(format!("Failed to read piece {}: {}", piece_index, e))
            })
    }

    /// Check if a piece is ready to be read
    pub async fn is_piece_ready(&self, piece_index: usize) -> Result<bool, TorrentError> {
        let handle_guard = self.handle.0.read().await;
        let handle_ptr = *handle_guard;
        if handle_ptr.is_null() {
            return Err(TorrentError::Libtorrent(
                "Invalid torrent handle".to_string(),
            ));
        }
        let available = unsafe { torrent_have_piece(handle_ptr, piece_index as i32) };
        Ok(available)
    }

    /// Read a specific file from the torrent by file index
    ///
    /// This reads the file data from torrent pieces. The file must be downloaded first.
    /// Returns the file contents as bytes.
    pub async fn read_file_by_index(&self, file_index: i32) -> Result<Vec<u8>, TorrentError> {
        // Get file list to find the file and calculate byte offsets
        let files = self.get_file_list().await?;

        // Find the file and calculate cumulative byte offset
        let mut cumulative_offset = 0i64;
        let mut target_file: Option<&TorrentFile> = None;

        for file in &files {
            if file.index == file_index {
                target_file = Some(file);
                break;
            }
            cumulative_offset += file.size;
        }

        let file = target_file.ok_or_else(|| {
            TorrentError::InvalidTorrent(format!("File index {} not found", file_index))
        })?;

        // Get torrent metadata for piece calculations
        let piece_length = self.piece_length().await? as usize;
        let total_size = self.total_size().await? as usize;

        // Calculate which pieces contain this file
        let file_start_byte = cumulative_offset as usize;
        let file_end_byte = (cumulative_offset + file.size) as usize;

        let start_piece = file_start_byte / piece_length;
        let end_piece = (file_end_byte - 1) / piece_length;

        // Read all pieces that contain this file
        let mut file_data = Vec::with_capacity(file.size as usize);

        for piece_index in start_piece..=end_piece {
            // Wait for piece to be ready
            loop {
                if self.is_piece_ready(piece_index).await? {
                    break;
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }

            // Read piece
            let piece_data = self.read_piece(piece_index).await?;

            // Calculate byte range within this piece that belongs to our file
            let piece_start_byte = piece_index * piece_length;
            let piece_end_byte = ((piece_index + 1) * piece_length).min(total_size);

            let overlap_start = file_start_byte.max(piece_start_byte);
            let overlap_end = file_end_byte.min(piece_end_byte);

            if overlap_start < overlap_end {
                // Extract bytes from piece data
                let piece_offset = overlap_start - piece_start_byte;
                let piece_length_needed = overlap_end - overlap_start;

                if piece_offset + piece_length_needed <= piece_data.len() {
                    file_data.extend_from_slice(
                        &piece_data[piece_offset..piece_offset + piece_length_needed],
                    );
                }
            }
        }

        Ok(file_data)
    }

    /// Read a specific file from the torrent by path
    ///
    /// Finds the file by path and reads it from torrent pieces.
    pub async fn read_file_by_path(&self, file_path: &Path) -> Result<Vec<u8>, TorrentError> {
        let files = self.get_file_list().await?;

        let file = files.iter().find(|f| f.path == file_path).ok_or_else(|| {
            TorrentError::InvalidTorrent(format!("File not found: {:?}", file_path))
        })?;

        self.read_file_by_index(file.index).await
    }

    /// Wait for metadata to be available
    pub async fn wait_for_metadata(&self) -> Result<(), TorrentError> {
        // Poll until metadata is available
        loop {
            // Check metadata in a block to ensure guard is dropped before await
            let has_metadata = {
                let handle_guard = self.handle.0.read().await;
                let handle_ptr = *handle_guard;
                if handle_ptr.is_null() {
                    return Err(TorrentError::Libtorrent(
                        "Invalid torrent handle".to_string(),
                    ));
                }
                unsafe { torrent_has_metadata(handle_ptr) }
            };

            if has_metadata {
                return Ok(());
            }

            // Wait a bit before checking again
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }
}

/// Represents a file in a torrent
#[derive(Debug, Clone)]
pub struct TorrentFile {
    pub index: i32,
    pub path: PathBuf,
    pub size: i64,
    pub priority: FilePriority,
}
