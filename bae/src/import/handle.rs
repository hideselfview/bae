// # Import Handle
//
// Handle for sending import requests and subscribing to progress updates.
// Provides the public API for interacting with the import service.

use crate::db::{DbAlbum, DbRelease};
use crate::import::discogs_parser::parse_discogs_album;
use crate::import::progress::ImportProgressHandle;
use crate::import::track_to_file_mapper::map_tracks_to_files;
use crate::import::types::{DiscoveredFile, ImportProgress, ImportRequestParams, TrackFile};
use crate::library::SharedLibraryManager;
use std::path::Path;
use tokio::sync::mpsc;

/// Handle for sending import requests and subscribing to progress updates
#[derive(Clone)]
pub struct ImportHandle {
    pub requests_tx: mpsc::UnboundedSender<ImportRequest>,
    pub progress_handle: ImportProgressHandle,
    pub library_manager: SharedLibraryManager,
}

/// Validated import ready for pipeline execution
pub struct ImportRequest {
    pub db_album: DbAlbum,
    pub db_release: DbRelease,
    pub tracks_to_files: Vec<TrackFile>,
    pub discovered_files: Vec<DiscoveredFile>,
}

impl ImportHandle {
    /// Create a new ImportHandle with the given dependencies
    pub fn new(
        requests_tx: mpsc::UnboundedSender<ImportRequest>,
        progress_rx: mpsc::UnboundedReceiver<ImportProgress>,
        library_manager: SharedLibraryManager,
        runtime_handle: tokio::runtime::Handle,
    ) -> Self {
        let progress_handle = ImportProgressHandle::new(progress_rx, runtime_handle);

        Self {
            requests_tx,
            progress_handle,
            library_manager,
        }
    }

    /// Validate and queue an import request.
    ///
    /// Performs validation (track-to-file mapping) and DB insertion synchronously.
    /// If validation fails, returns error immediately with no side effects.
    /// If successful, album is inserted with status='queued' and an import
    /// request is sent to the import worker.  
    ///
    /// Returns the database release ID for progress subscription.
    pub async fn send_request(&self, params: ImportRequestParams) -> Result<String, String> {
        match params {
            ImportRequestParams::FromFolder {
                discogs_album: album,
                folder,
            } => {
                let library_manager = self.library_manager.get();

                // ========== VALIDATION (before queueing) ==========

                // 1. Parse Discogs album into database models
                let (db_album, db_release, db_tracks, artists, album_artists) =
                    parse_discogs_album(&album)?;

                tracing::info!(
                    "Parsed Discogs album into database models:\n{:#?}",
                    db_album
                );
                tracing::info!(
                    "Parsed Discogs release into database models:\n{:#?}",
                    db_release
                );
                tracing::info!(
                    "Parsed Discogs tracks into database models:\n{:#?}",
                    db_tracks
                );
                tracing::info!(
                    "Parsed {} artists and {} album-artist relationships",
                    artists.len(),
                    album_artists.len()
                );

                // 2. Discover files
                let discovered_files = discover_folder_files(&folder)?;

                // 3. Build track-to-file mapping
                let tracks_to_files = map_tracks_to_files(&db_tracks, &discovered_files).await?;

                // 4. Insert or lookup artists (deduplicate across imports)
                for artist in &artists {
                    // Check if artist already exists by Discogs ID
                    let existing = if let Some(ref discogs_id) = artist.discogs_artist_id {
                        library_manager
                            .get_artist_by_discogs_id(discogs_id)
                            .await
                            .map_err(|e| format!("Database error: {}", e))?
                    } else {
                        None
                    };

                    // Insert only if artist doesn't exist
                    if existing.is_none() {
                        library_manager
                            .insert_artist(artist)
                            .await
                            .map_err(|e| format!("Failed to insert artist: {}", e))?;
                    }
                }

                // 5. Insert album + release + tracks with status='queued'
                library_manager
                    .insert_album_with_release_and_tracks(&db_album, &db_release, &db_tracks)
                    .await
                    .map_err(|e| format!("Database error: {}", e))?;

                // 6. Insert album-artist relationships
                for album_artist in &album_artists {
                    library_manager
                        .insert_album_artist(album_artist)
                        .await
                        .map_err(|e| {
                            format!("Failed to insert album-artist relationship: {}", e)
                        })?;
                }

                tracing::info!(
                    "Validated and queued album '{}' (release: {}) with {} tracks",
                    db_album.title,
                    db_release.id,
                    db_tracks.len()
                );

                // ========== QUEUE FOR PIPELINE ==========

                let release_id = db_release.id.clone();

                self.requests_tx
                    .send(ImportRequest {
                        db_album,
                        db_release,
                        tracks_to_files,
                        discovered_files,
                    })
                    .map_err(|_| "Failed to queue validated album for import".to_string())?;

                Ok(release_id)
            }
        }
    }

    /// Subscribe to progress updates for a specific release
    /// Returns a filtered receiver that yields only updates for the specified release
    pub fn subscribe_release(
        &self,
        release_id: String,
    ) -> tokio::sync::mpsc::UnboundedReceiver<ImportProgress> {
        self.progress_handle.subscribe_release(release_id)
    }

    /// Subscribe to progress updates for a specific track
    /// Returns a filtered receiver that yields only updates for the specified track
    pub fn subscribe_track(
        &self,
        release_id: String,
        track_id: String,
    ) -> tokio::sync::mpsc::UnboundedReceiver<ImportProgress> {
        self.progress_handle.subscribe_track(release_id, track_id)
    }
}

/// Discover all files in folder with metadata.
///
/// Single filesystem traversal to gather file paths and sizes upfront.
/// This avoids redundant directory reads later for CUE detection and chunk calculations.
/// Files are sorted by path for consistent ordering across runs.
fn discover_folder_files(folder: &Path) -> Result<Vec<DiscoveredFile>, String> {
    let mut files = Vec::new();

    for entry in std::fs::read_dir(folder).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();

        if path.is_file() {
            let size = entry
                .metadata()
                .map_err(|e| format!("Failed to read metadata for {:?}: {}", path, e))?
                .len();

            files.push(DiscoveredFile { path, size });
        }
    }

    // Sort by path for consistent ordering
    files.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(files)
}
