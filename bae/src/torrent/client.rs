// NOTE: This module requires libtorrent C++ library to be installed on the system.
// On macOS: brew install libtorrent-rasterbar
// The libtorrent-rs crate provides Rust bindings to the C++ library.
//
// The libtorrent-rs crate (v0.1.1) provides a minimal API. We use what's available
// and parse bencoded data for additional torrent metadata.

use crate::torrent::ffi::{
    create_session_params_default, create_session_params_with_storage, create_session_with_params,
    get_session_ptr, load_torrent_file, parse_magnet_uri, session_add_torrent,
    session_remove_torrent, torrent_get_file_list, torrent_get_name, torrent_get_num_pieces,
    torrent_get_piece_length, torrent_get_progress, torrent_get_storage_index,
    torrent_get_total_size, torrent_has_metadata, torrent_set_file_priorities, Session,
    TorrentFileInfo, TorrentHandle as FfiTorrentHandle,
};
use crate::torrent::storage::{create_bae_storage_constructor, BaeStorage};
use cxx::UniquePtr;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

#[derive(Error, Debug)]
pub enum TorrentError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Libtorrent error: {0}")]
    Libtorrent(String),
    #[error("Invalid torrent: {0}")]
    InvalidTorrent(String),
}

/// Priority for torrent files
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilePriority {
    DoNotDownload = 0,
    Normal = 4,
    Maximum = 7,
}

/// Type alias for the libtorrent session
/// Uses our FFI Session type (custom storage backend)
/// Uses Rc instead of Arc since TorrentClient is not Send/Sync and only used on a single thread
type SessionArc = Rc<RwLock<UniquePtr<Session>>>;

/// Storage registry for mapping torrent IDs to BaeStorage instances
/// Used by sync callbacks to access async storage methods
type StorageRegistry = Arc<RwLock<HashMap<String, Arc<RwLock<BaeStorage>>>>>;

/// Mapping from libtorrent storage_index_t to torrent_id
/// libtorrent assigns a storage_index when a torrent is added, we need to map it to our torrent_id
type StorageIndexMap = Arc<RwLock<HashMap<i32, String>>>;

thread_local! {
    pub(crate) static STORAGE_REGISTRY: std::cell::RefCell<Option<StorageRegistry>> = const { std::cell::RefCell::new(None) };
    pub(crate) static STORAGE_INDEX_MAP: std::cell::RefCell<Option<StorageIndexMap>> = const { std::cell::RefCell::new(None) };
    pub(crate) static RUNTIME_HANDLE: std::cell::RefCell<Option<tokio::runtime::Handle>> = const { std::cell::RefCell::new(None) };
}

/// Wrapper around libtorrent session
pub struct TorrentClient {
    session: SessionArc,
    runtime_handle: tokio::runtime::Handle,
    storage_registry: StorageRegistry,
    storage_index_map: StorageIndexMap,
}

// NOTE: TorrentClient is NOT Send or Sync because:
//
// 1. It uses thread_local! storage (STORAGE_REGISTRY, STORAGE_INDEX_MAP, RUNTIME_HANDLE)
//    that is initialized in TorrentClient::new() on the creating thread.
//
// 2. The C++ storage callbacks access this thread-local storage. If TorrentClient is moved
//    to a different thread, the callbacks won't find the registry and will fail.
//
// 3. Therefore, TorrentClient must be created and used on a single dedicated thread.
//    ImportService and TorrentSeeder spawn dedicated threads for two reasons:
//    - TorrentClient cannot be moved between threads (thread-local storage constraint)
//    - They need separate instances with different storage types:
//      * ImportService/TorrentSeeder use custom storage (BaeStorage) via TorrentClient::new()
//      * ImportContext uses default storage for metadata detection via TorrentClient::new_with_default_storage()

impl Clone for TorrentClient {
    fn clone(&self) -> Self {
        TorrentClient {
            session: Rc::clone(&self.session),
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

        // Create storage constructor with callbacks
        let storage_constructor = create_bae_storage_constructor();

        // Create session with custom storage backend
        let session_params = create_session_params_with_storage(storage_constructor);
        let custom_session = create_session_with_params(session_params);

        if custom_session.is_null() {
            return Err(TorrentError::Libtorrent(
                "Failed to create libtorrent session with custom storage".to_string(),
            ));
        }

        Ok(TorrentClient {
            session: Rc::new(RwLock::new(custom_session)),
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
    pub fn new_with_default_storage() -> Result<Self, TorrentError> {
        let runtime_handle = tokio::runtime::Handle::current();
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
            session: Rc::new(RwLock::new(default_session)),
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
        let session = Rc::clone(&self.session);

        // Get write lock first
        let mut session_guard = session.write().await;

        // Load torrent file using our wrapper function
        let mut params = load_torrent_file(&file_path, &temp_path);
        if params.is_null() {
            drop(session_guard);
            return Err(TorrentError::InvalidTorrent(
                "Failed to load torrent file".to_string(),
            ));
        }

        // Get raw session pointer for wrapper function
        let session_ptr = get_session_ptr(&mut session_guard);
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

        // Store raw pointer directly (opaque type, can't use UniquePtr)
        // SAFETY: We own the handle_ptr returned from session_add_torrent
        // The handle will be valid as long as the session exists
        #[allow(clippy::arc_with_non_send_sync)]
        Ok(TorrentHandle {
            handle: SendSafeTorrentHandle(Arc::new(RwLock::new(handle_ptr))),
        })
    }

    /// Add a torrent from magnet link
    pub async fn add_magnet_link(&self, magnet: &str) -> Result<TorrentHandle, TorrentError> {
        // Use a temporary save path - we'll handle data ourselves
        let temp_path = std::env::temp_dir().to_string_lossy().to_string();

        // Extract session reference to avoid capturing self across await
        let session = Rc::clone(&self.session);

        // Get write lock first
        let mut session_guard = session.write().await;

        // Parse magnet URI using our wrapper function
        let mut params = parse_magnet_uri(magnet, &temp_path);
        if params.is_null() {
            drop(session_guard);
            return Err(TorrentError::InvalidTorrent(
                "Failed to parse magnet URI".to_string(),
            ));
        }

        // Get raw session pointer for wrapper function
        let session_ptr = get_session_ptr(&mut session_guard);
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

        // Store raw pointer directly (opaque type, can't use UniquePtr)
        // SAFETY: We own the handle_ptr returned from session_add_torrent
        // The handle will be valid as long as the session exists
        #[allow(clippy::arc_with_non_send_sync)]
        Ok(TorrentHandle {
            handle: SendSafeTorrentHandle(Arc::new(RwLock::new(handle_ptr))),
            // Don't store session reference - TorrentHandle doesn't need it
            // The handle is self-contained and can be used independently
        })
    }

    /// Remove a torrent from the session
    ///
    /// This removes the torrent from the libtorrent session, freeing resources.
    /// The downloaded files are kept on disk.
    /// After calling this, the TorrentHandle should not be used.
    pub async fn remove_torrent_and_keep_data(
        &self,
        handle: &TorrentHandle,
    ) -> Result<(), TorrentError> {
        self._remove_torrent(handle, false).await
    }

    /// Remove a torrent from the session and delete its files from disk
    ///
    /// This removes the torrent from the libtorrent session and deletes all downloaded files.
    /// After calling this, the TorrentHandle should not be used.
    pub async fn remove_torrent_and_delete_data(
        &self,
        handle: &TorrentHandle,
    ) -> Result<(), TorrentError> {
        self._remove_torrent(handle, true).await
    }

    /// Internal implementation for removing a torrent
    ///
    /// # Arguments
    /// * `handle` - The torrent handle to remove
    /// * `delete_files` - If true, also deletes the downloaded files from disk
    async fn _remove_torrent(
        &self,
        handle: &TorrentHandle,
        delete_files: bool,
    ) -> Result<(), TorrentError> {
        let session = Rc::clone(&self.session);
        let mut session_guard = session.write().await;
        let session_ptr = get_session_ptr(&mut session_guard);

        if session_ptr.is_null() {
            drop(session_guard);
            return Err(TorrentError::Libtorrent(
                "Failed to get session pointer".to_string(),
            ));
        }

        let handle_guard = handle.handle.0.read().await;
        let handle_ptr = *handle_guard;

        if handle_ptr.is_null() {
            drop(handle_guard);
            drop(session_guard);
            return Err(TorrentError::Libtorrent(
                "Invalid torrent handle".to_string(),
            ));
        }

        unsafe {
            session_remove_torrent(session_ptr, handle_ptr, delete_files);
        }

        drop(handle_guard);
        drop(session_guard);

        Ok(())
    }
}

/// Wrapper around raw TorrentHandle pointer that is Send/Sync-safe
/// Uses our FFI TorrentHandle type (opaque, so we use raw pointer)
#[allow(clippy::arc_with_non_send_sync)]
struct SendSafeTorrentHandle(Arc<RwLock<*mut FfiTorrentHandle>>);

unsafe impl Send for SendSafeTorrentHandle {}
unsafe impl Sync for SendSafeTorrentHandle {}

/// Handle to a torrent in the session
pub struct TorrentHandle {
    handle: SendSafeTorrentHandle,
    // Note: We don't store the session here to avoid Send issues
    // The handle is self-contained and doesn't need the session reference
}

// SAFETY: TorrentHandle must be Send because it's held across .await points in async functions.
// Even though TorrentHandle is only used on the same dedicated thread as TorrentClient,
// Rust requires Send for values held across .await. The raw pointer (*mut FfiTorrentHandle)
// is safe to send between threads because it's just a pointer to C++ memory, and all
// operations go through the same TorrentClient session on its dedicated thread.
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
                path: PathBuf::from(info.path),
                size: info.size,
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
    pub path: PathBuf,
    pub size: i64,
}
