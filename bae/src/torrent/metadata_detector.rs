//! Lightweight metadata detection from torrent CUE/log files
//!
//! This module provides functionality to quickly download and analyze CUE/log files
//! from torrents for automatic release matching, separate from the main import flow.

use crate::import::{detect_metadata, FolderMetadata};
use crate::torrent::client::TorrentHandle;
use crate::torrent::selective_downloader::SelectiveDownloader;
use thiserror::Error;
use tracing::{info, warn};

#[derive(Error, Debug)]
pub enum TorrentMetadataError {
    #[error("Torrent error: {0}")]
    Torrent(#[from] crate::torrent::client::TorrentError),
    #[error("Selective download error: {0}")]
    SelectiveDownload(#[from] crate::torrent::selective_downloader::SelectiveDownloadError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Detect metadata from CUE/log files in a torrent
///
/// Uses the provided torrent handle (already added to client),
/// downloads only CUE/log files, then runs metadata detection on them.
///
/// This is separate from the main import flow and doesn't use custom storage.
pub async fn detect_metadata_from_torrent_file(
    handle: &TorrentHandle,
) -> Result<Option<FolderMetadata>, TorrentMetadataError> {
    // Use system temp directory for downloads
    let temp_path = std::env::temp_dir();

    // Prioritize and download metadata files
    let selective_downloader = SelectiveDownloader::new();
    let metadata_files = selective_downloader
        .prioritize_metadata_files(handle)
        .await?;

    if metadata_files.is_empty() {
        info!("No CUE/log files found in torrent");
        return Ok(None);
    }

    info!(
        "Found {} metadata files, downloading...",
        metadata_files.len()
    );

    // Wait for metadata files to download
    selective_downloader
        .wait_for_metadata_files(handle, &metadata_files)
        .await?;

    info!("Metadata files downloaded, extracting...");

    // Get torrent name to construct the full save directory path
    // With default storage, files are written to temp_path/torrent_name/
    let torrent_name = handle.name().await?;
    let save_dir = temp_path.join(&torrent_name);

    // Wait a bit for libtorrent to finish writing files
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Try to detect metadata from the save directory
    if save_dir.exists() {
        match detect_metadata(save_dir) {
            Ok(metadata) => {
                info!("Successfully detected metadata from torrent CUE/log files");
                Ok(Some(metadata))
            }
            Err(e) => {
                warn!("Failed to detect metadata: {}", e);
                Ok(None)
            }
        }
    } else {
        warn!("Save directory does not exist: {:?}", save_dir);
        Ok(None)
    }
}
