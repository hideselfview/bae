//! Lightweight metadata detection from torrent CUE/log files
//!
//! This module provides functionality to quickly download and analyze CUE/log files
//! from torrents for automatic release matching, separate from the main import flow.

use crate::import::{detect_metadata, FolderMetadata};
use crate::torrent::client::{FilePriority, TorrentHandle};
use crate::torrent::progress::TorrentProgress;
use std::path::PathBuf;
use thiserror::Error;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

#[derive(Error, Debug)]
pub enum TorrentMetadataError {
    #[error("Torrent error: {0}")]
    Torrent(#[from] crate::torrent::client::TorrentError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Disable all files in the torrent
async fn disable_all_files(handle: &TorrentHandle) -> Result<(), TorrentMetadataError> {
    let files = handle.get_file_list().await?;
    let priorities: Vec<FilePriority> = vec![FilePriority::DoNotDownload; files.len()];
    handle.set_file_priorities(priorities).await?;
    Ok(())
}

/// Enable only metadata files (.cue, .log, .txt) for download
async fn enable_metadata_files(
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
            debug!("Enabling metadata file: {}", file.path.display());
        } else {
            priorities.push(FilePriority::DoNotDownload);
        }
    }

    handle.set_file_priorities(priorities).await?;
    info!(
        "Enabled {} metadata files for download",
        metadata_files.len()
    );

    Ok(metadata_files)
}

/// Wait for metadata files to complete downloading
async fn wait_for_metadata_files(
    handle: &TorrentHandle,
    metadata_paths: &[PathBuf],
    progress_tx: &mpsc::UnboundedSender<TorrentProgress>,
    info_hash: &str,
) -> Result<Vec<PathBuf>, TorrentMetadataError> {
    loop {
        let progress = handle.progress().await?;

        // Wait for 100% completion (progress >= 1.0) before checking files
        if progress >= 1.0 {
            let files = handle.get_file_list().await?;
            let completed: Vec<PathBuf> = files
                .iter()
                .filter(|f| metadata_paths.contains(&f.path))
                .map(|f| f.path.clone())
                .collect();

            if !completed.is_empty() {
                return Ok(completed);
            }
        }

        // Emit progress for individual files
        for metadata_path in metadata_paths {
            // Check file progress (simplified - in real implementation would track per-file)
            let file_progress = progress; // Use overall progress as approximation
            let _ = progress_tx.send(TorrentProgress::MetadataProgress {
                info_hash: info_hash.to_string(),
                file: metadata_path.to_string_lossy().to_string(),
                progress: file_progress,
            });
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
    progress_tx: &mpsc::UnboundedSender<TorrentProgress>,
) -> Result<Option<FolderMetadata>, TorrentMetadataError> {
    info!("Starting metadata detection from torrent file");

    let info_hash = handle.info_hash().await;

    // Use system temp directory for downloads
    let temp_path = std::env::temp_dir();
    info!("Using temp directory: {:?}", temp_path);

    // Disable all files first, then enable only metadata files
    info!("Disabling all files...");
    disable_all_files(handle).await?;

    info!("Enabling metadata files...");
    let metadata_files = enable_metadata_files(handle).await?;

    if metadata_files.is_empty() {
        info!("No CUE/log files found in torrent");
        return Ok(None);
    }

    // Emit metadata files detected event
    let metadata_file_names: Vec<String> = metadata_files
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    let _ = progress_tx.send(TorrentProgress::MetadataFilesDetected {
        info_hash: info_hash.clone(),
        files: metadata_file_names.clone(),
    });

    info!(
        "Found {} metadata files, downloading...",
        metadata_files.len()
    );

    // Resume the torrent now that priorities are set
    info!("Resuming torrent download...");
    handle
        .resume()
        .await
        .map_err(TorrentMetadataError::Torrent)?;

    // Wait for metadata files to download (no timeout - user can cancel via UI)
    match wait_for_metadata_files(handle, &metadata_files, progress_tx, &info_hash).await {
        Ok(_files) => {
            // Metadata files downloaded
        }
        Err(e) => {
            warn!("Failed to download metadata files: {}", e);
            let _ = progress_tx.send(TorrentProgress::Error {
                info_hash: info_hash.clone(),
                message: format!("Failed to download metadata files: {}", e),
            });
            return Ok(None);
        }
    }

    // Get torrent name to construct the full save directory path
    // With default storage, files are written to temp_path/torrent_name/
    let torrent_name = handle.name().await?;
    let save_dir = temp_path.join(&torrent_name);

    // Try to detect metadata from the save directory
    if save_dir.exists() {
        match detect_metadata(save_dir.clone()) {
            Ok(metadata) => {
                info!("Successfully detected metadata from torrent CUE/log files");
                Ok(Some(metadata))
            }
            Err(e) => {
                warn!("Failed to detect metadata from {:?}: {}", save_dir, e);
                let _ = progress_tx.send(TorrentProgress::Error {
                    info_hash: info_hash.clone(),
                    message: format!("Failed to detect metadata: {}", e),
                });
                Ok(None)
            }
        }
    } else {
        warn!("Save directory does not exist: {:?}", save_dir);
        Ok(None)
    }
}
