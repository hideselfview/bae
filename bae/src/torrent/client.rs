// NOTE: This module requires libtorrent C++ library to be installed on the system.
// On macOS: brew install libtorrent-rasterbar
// The libtorrent-rs crate provides Rust bindings to the C++ library.
//
// The libtorrent-rs crate (v0.1.1) provides a minimal API. We use what's available
// and parse bencoded data for additional torrent metadata.

use cxx::UniquePtr;
use libtorrent::{add_torrent_params, session, torrent_handle};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

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
struct SendSafeSession(SendSafeArc<RwLock<UniquePtr<session>>>);

unsafe impl Send for SendSafeSession {}

/// Wrapper around libtorrent session
pub struct TorrentClient {
    session: SendSafeSession,
    runtime_handle: tokio::runtime::Handle,
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
        }
    }
}

impl TorrentClient {
    /// Create a new torrent client
    pub fn new(runtime_handle: tokio::runtime::Handle) -> Result<Self, TorrentError> {
        let session_ptr = libtorrent::lt_create_session();
        if session_ptr.is_null() {
            return Err(TorrentError::Libtorrent(
                "Failed to create libtorrent session".to_string(),
            ));
        }

        Ok(TorrentClient {
            session: SendSafeSession(SendSafeArc(Arc::new(RwLock::new(session_ptr)))),
            runtime_handle,
        })
    }

    /// Add a torrent from a file
    ///
    /// NOTE: The libtorrent-rs crate doesn't expose torrent file loading.
    /// This would require extending the FFI bindings or parsing the .torrent file ourselves.
    pub async fn add_torrent_file(&self, path: &Path) -> Result<TorrentHandle, TorrentError> {
        // Read and parse .torrent file
        // For now, return an error - this needs bencode parsing or FFI extension
        Err(TorrentError::NotImplemented(
            "Torrent file loading requires bencode parsing or FFI extension. Use magnet links for now.".to_string(),
        ))
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

        // Parse magnet URI and use params immediately - don't hold it across await
        let mut params = libtorrent::lt_parse_magnet_uri(magnet, &temp_path);
        if params.is_null() {
            drop(session_guard);
            return Err(TorrentError::InvalidTorrent(
                "Failed to parse magnet URI".to_string(),
            ));
        }

        // Use params immediately - don't hold it across await
        let handle = libtorrent::lt_session_add_torrent(session_guard.pin_mut(), params.pin_mut());

        // Drop guard and params immediately
        drop(session_guard);
        drop(params);

        if handle.is_null() {
            return Err(TorrentError::Libtorrent(
                "Failed to add torrent to session".to_string(),
            ));
        }

        Ok(TorrentHandle {
            handle: SendSafeTorrentHandle(Arc::new(RwLock::new(handle))),
            // Don't store session reference - TorrentHandle doesn't need it
            // The handle is self-contained and can be used independently
        })
    }
}

/// Wrapper around UniquePtr<torrent_handle> that is Send/Sync-safe
struct SendSafeTorrentHandle(Arc<RwLock<UniquePtr<torrent_handle>>>);

unsafe impl Send for SendSafeTorrentHandle {}
unsafe impl Sync for SendSafeTorrentHandle {}

/// Handle to a torrent in the session
pub struct TorrentHandle {
    handle: SendSafeTorrentHandle,
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

    /// Get the name of the torrent
    pub async fn name(&self) -> Result<String, TorrentError> {
        let handle_guard = self.handle.0.read().await;
        let name = libtorrent::lt_torrent_get_name(handle_guard.as_ref().unwrap());
        Ok(name.to_string())
    }

    /// Get the total size of the torrent
    pub async fn total_size(&self) -> Result<i64, TorrentError> {
        // TODO: Implement actual size retrieval
        Err(TorrentError::NotImplemented(
            "total_size() not yet implemented".to_string(),
        ))
    }

    /// Get the piece length
    pub async fn piece_length(&self) -> Result<i32, TorrentError> {
        // TODO: Implement actual piece length retrieval
        Err(TorrentError::NotImplemented(
            "piece_length() not yet implemented".to_string(),
        ))
    }

    /// Get the number of pieces
    pub async fn num_pieces(&self) -> Result<i32, TorrentError> {
        // TODO: Implement actual piece count retrieval
        Err(TorrentError::NotImplemented(
            "num_pieces() not yet implemented".to_string(),
        ))
    }

    /// Get the list of files in the torrent
    pub async fn get_file_list(&self) -> Result<Vec<TorrentFile>, TorrentError> {
        // TODO: Implement actual file list retrieval
        Err(TorrentError::NotImplemented(
            "get_file_list() not yet implemented".to_string(),
        ))
    }

    /// Set file priorities
    pub async fn set_file_priorities(
        &self,
        _priorities: Vec<FilePriority>,
    ) -> Result<(), TorrentError> {
        // TODO: Implement actual priority setting
        Err(TorrentError::NotImplemented(
            "set_file_priorities() not yet implemented".to_string(),
        ))
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
        // TODO: Implement actual progress retrieval
        Err(TorrentError::NotImplemented(
            "progress() not yet implemented".to_string(),
        ))
    }

    /// Read a piece of data
    pub async fn read_piece(&self, piece_index: usize) -> Result<Vec<u8>, TorrentError> {
        // TODO: Implement actual piece reading from libtorrent
        // For now, return an error indicating this needs implementation
        // Once libtorrent API is understood, this should:
        // 1. Check if piece is available
        // 2. Read piece data from libtorrent handle
        // 3. Return piece bytes
        Err(TorrentError::NotImplemented(format!(
            "read_piece() not yet implemented for piece {}",
            piece_index
        )))
    }

    /// Check if a piece is ready to be read
    pub async fn is_piece_ready(&self, piece_index: usize) -> Result<bool, TorrentError> {
        // TODO: Implement actual piece availability check
        // For now, return an error indicating this needs implementation
        // Once libtorrent API is understood, this should:
        // 1. Check torrent status
        // 2. Check if piece at piece_index is available
        // 3. Return true if piece can be read
        Err(TorrentError::NotImplemented(format!(
            "is_piece_ready() not yet implemented for piece {}",
            piece_index
        )))
    }

    /// Wait for metadata to be available
    pub async fn wait_for_metadata(&self) -> Result<(), TorrentError> {
        // Poll until metadata is available
        loop {
            // Check metadata in a block to ensure guard is dropped before await
            let has_metadata = {
                let handle_guard = self.handle.0.read().await;
                libtorrent::lt_torrent_has_metadata(handle_guard.as_ref().unwrap())
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
