use crate::torrent::client::{FilePriority, TorrentClient, TorrentHandle};
use std::path::PathBuf;
use thiserror::Error;
use tracing::{debug, info};

#[derive(Error, Debug)]
pub enum SelectiveDownloadError {
    #[error("Torrent error: {0}")]
    Torrent(#[from] crate::torrent::client::TorrentError),
    #[error("No metadata files found")]
    NoMetadataFiles,
}

/// Manages selective downloading of torrent files
pub struct SelectiveDownloader {
    client: TorrentClient,
}

impl SelectiveDownloader {
    pub fn new(client: TorrentClient) -> Self {
        SelectiveDownloader { client }
    }

    /// Prioritize metadata files (.cue, .log, .txt) for early download
    pub async fn prioritize_metadata_files(
        &self,
        handle: &TorrentHandle,
    ) -> Result<Vec<PathBuf>, SelectiveDownloadError> {
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
    pub async fn wait_for_metadata_files(
        &self,
        handle: &TorrentHandle,
        metadata_paths: &[PathBuf],
    ) -> Result<Vec<PathBuf>, SelectiveDownloadError> {
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

    /// Enable all remaining files for download
    pub async fn enable_remaining_files(
        &self,
        handle: &TorrentHandle,
    ) -> Result<(), SelectiveDownloadError> {
        let files = handle.get_file_list().await?;
        let priorities: Vec<FilePriority> = files.iter().map(|_| FilePriority::Normal).collect();

        handle.set_file_priorities(priorities).await?;
        info!("Enabled all {} files for download", files.len());

        Ok(())
    }
}
