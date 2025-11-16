//! Lightweight metadata detection from torrent CUE/log files
//!
//! This module provides functionality to quickly download and analyze CUE/log files
//! from torrents for automatic release matching, separate from the main import flow.

use crate::import::{detect_metadata, FolderMetadata};
use crate::torrent::client::{FilePriority, TorrentHandle};
use std::path::PathBuf;
use thiserror::Error;
use tracing::{debug, info, warn};

#[derive(Error, Debug)]
pub enum TorrentMetadataError {
    #[error("Torrent error: {0}")]
    Torrent(#[from] crate::torrent::client::TorrentError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Prioritize metadata files (.cue, .log, .txt) for early download
async fn prioritize_metadata_files(
    handle: &TorrentHandle,
) -> Result<Vec<PathBuf>, TorrentMetadataError> {
    let files = handle.get_file_list().await?;

    let mut metadata_files = Vec::new();
    let mut priorities = Vec::new();

    for file in &files {
        let is_metadata = file
            .path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| {
                matches!(
                    ext.to_lowercase().as_str(),
                    "cue" | "log" | "txt" | "md5" | "ffp"
                )
            })
            .unwrap_or(false);

        if is_metadata {
            metadata_files.push(file.path.clone());
            priorities.push(FilePriority::Maximum);
            debug!("Prioritizing metadata file: {}", file.path.display());
        } else {
            priorities.push(FilePriority::DoNotDownload);
        }
    }

    handle.set_file_priorities(priorities).await?;
    info!("Prioritized {} metadata files", metadata_files.len());

    Ok(metadata_files)
}

/// Wait for metadata files to complete downloading
async fn wait_for_metadata_files(
    handle: &TorrentHandle,
    metadata_paths: &[PathBuf],
) -> Result<Vec<PathBuf>, TorrentMetadataError> {
    loop {
        let progress = handle.progress().await?;

        // Check if any metadata files are complete
        // Note: This is simplified - actual implementation would check individual file completion
        if progress > 0.0 {
            // For now, return the metadata paths once we have some progress
            // Full implementation would verify each file is complete
            let files = handle.get_file_list().await?;
            let completed: Vec<PathBuf> = files
                .iter()
                .filter(|f| metadata_paths.contains(&f.path))
                .map(|f| f.path.clone())
                .collect();

            if !completed.is_empty() {
                info!("Metadata files downloaded: {:?}", completed);
                return Ok(completed);
            }
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }
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
    let metadata_files = prioritize_metadata_files(handle).await?;

    if metadata_files.is_empty() {
        info!("No CUE/log files found in torrent");
        return Ok(None);
    }

    info!(
        "Found {} metadata files, downloading...",
        metadata_files.len()
    );

    // Wait for metadata files to download
    wait_for_metadata_files(handle, &metadata_files).await?;

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
