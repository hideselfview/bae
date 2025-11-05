// # Import Handle
//
// Handle for sending import requests and subscribing to progress updates.
// Provides the public API for interacting with the import service.

use crate::cue_flac::CueFlacProcessor;
use crate::db::{DbAlbum, DbRelease};
use crate::import::discogs_parser::parse_discogs_release;
use crate::import::progress::ImportProgressHandle;
use crate::import::track_to_file_mapper::map_tracks_to_files;
use crate::import::types::{
    CueFlacMetadata, DiscoveredFile, ImportProgress, ImportRequestParams, TrackFile,
};
use crate::library::SharedLibraryManager;
use crate::playback::symphonia_decoder::TrackDecoder;
use std::path::Path;
use tokio::sync::mpsc;
use tracing::{debug, warn};

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
    /// Pre-parsed CUE/FLAC metadata (for CUE/FLAC imports only).
    /// Validated during track mapping, passed through to avoid re-parsing.
    pub cue_flac_metadata: Option<std::collections::HashMap<std::path::PathBuf, CueFlacMetadata>>,
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
    /// Returns (album_id, release_id) for navigation and progress subscription.
    pub async fn send_request(
        &self,
        params: ImportRequestParams,
    ) -> Result<(String, String), String> {
        match params {
            ImportRequestParams::FromFolder {
                discogs_release: release,
                folder,
                master_year,
            } => {
                let library_manager = self.library_manager.get();

                // ========== VALIDATION (before queueing) ==========

                // 1. Parse Discogs release into database models
                let (db_album, db_release, db_tracks, artists, album_artists) =
                    parse_discogs_release(&release, master_year)?;

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

                // 3. Build track-to-file mapping (validates and parses CUE sheets if present)
                let mapping_result = map_tracks_to_files(&db_tracks, &discovered_files).await?;
                let tracks_to_files = mapping_result.track_files.clone();
                let cue_flac_metadata = mapping_result.cue_flac_metadata;

                // 4. Insert or lookup artists (deduplicate across imports)
                // Build a map from parsed artist ID to actual database artist ID
                let mut artist_id_map = std::collections::HashMap::new();
                for artist in &artists {
                    let parsed_id = artist.id.clone();

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
                    let actual_id = if let Some(existing_artist) = existing {
                        existing_artist.id
                    } else {
                        library_manager
                            .insert_artist(artist)
                            .await
                            .map_err(|e| format!("Failed to insert artist: {}", e))?;
                        artist.id.clone()
                    };

                    artist_id_map.insert(parsed_id, actual_id);
                }

                // 5. Insert album + release + tracks with status='queued'
                library_manager
                    .insert_album_with_release_and_tracks(&db_album, &db_release, &db_tracks)
                    .await
                    .map_err(|e| format!("Database error: {}", e))?;

                // 6. Extract and store durations early (before pipeline starts)
                extract_and_store_durations(library_manager, &tracks_to_files).await?;

                // 7. Insert album-artist relationships (using actual database artist IDs)
                for album_artist in &album_artists {
                    let actual_artist_id =
                        artist_id_map.get(&album_artist.artist_id).ok_or_else(|| {
                            format!(
                                "Artist ID {} not found in artist map",
                                album_artist.artist_id
                            )
                        })?;

                    let mut updated_album_artist = album_artist.clone();
                    updated_album_artist.artist_id = actual_artist_id.clone();

                    library_manager
                        .insert_album_artist(&updated_album_artist)
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

                let album_id = db_album.id.clone();
                let release_id = db_release.id.clone();

                self.requests_tx
                    .send(ImportRequest {
                        db_album,
                        db_release,
                        tracks_to_files,
                        discovered_files,
                        cue_flac_metadata,
                    })
                    .map_err(|_| "Failed to queue validated album for import".to_string())?;

                Ok((album_id, release_id))
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
        track_id: String,
    ) -> tokio::sync::mpsc::UnboundedReceiver<ImportProgress> {
        self.progress_handle.subscribe_track(track_id)
    }
}

/// Extract durations from audio files and update database immediately
async fn extract_and_store_durations(
    library_manager: &crate::library::LibraryManager,
    tracks_to_files: &[TrackFile],
) -> Result<(), String> {
    use std::collections::HashMap;
    use std::path::Path;

    // Group tracks by file path for CUE/FLAC handling
    let mut file_groups: HashMap<&Path, Vec<&TrackFile>> = HashMap::new();
    for mapping in tracks_to_files {
        file_groups
            .entry(mapping.file_path.as_path())
            .or_default()
            .push(mapping);
    }

    for (file_path, mappings) in file_groups {
        let is_cue_flac = mappings.len() > 1
            && file_path
                .extension()
                .and_then(|e| e.to_str())
                .map(|s| s.to_lowercase())
                == Some("flac".to_string());

        if is_cue_flac {
            // CUE/FLAC: extract durations from CUE sheet
            let cue_path = file_path.with_extension("cue");
            if cue_path.exists() {
                match CueFlacProcessor::parse_cue_sheet(&cue_path) {
                    Ok(cue_sheet) => {
                        for (mapping, cue_track) in mappings.iter().zip(cue_sheet.tracks.iter()) {
                            let duration_ms = if let Some(end_time) = cue_track.end_time_ms {
                                Some((end_time - cue_track.start_time_ms) as i64)
                            } else {
                                // Last track - extract from file
                                extract_duration_from_file(file_path).map(|file_duration_ms| {
                                    file_duration_ms - cue_track.start_time_ms as i64
                                })
                            };

                            library_manager
                                .update_track_duration(&mapping.db_track_id, duration_ms)
                                .await
                                .map_err(|e| format!("Failed to update track duration: {}", e))?;
                        }
                    }
                    Err(e) => {
                        warn!("Failed to parse CUE sheet for duration extraction: {:?}", e);
                    }
                }
            }
        } else {
            // Individual files: extract duration from each file
            for mapping in mappings {
                let duration_ms = extract_duration_from_file(&mapping.file_path);
                library_manager
                    .update_track_duration(&mapping.db_track_id, duration_ms)
                    .await
                    .map_err(|e| format!("Failed to update track duration: {}", e))?;
            }
        }
    }

    Ok(())
}

/// Extract duration from an audio file
fn extract_duration_from_file(file_path: &Path) -> Option<i64> {
    debug!("Extracting duration from file: {}", file_path.display());
    let file_data = match std::fs::read(file_path) {
        Ok(data) => {
            debug!("Read {} bytes from file", data.len());
            data
        }
        Err(e) => {
            warn!("Failed to read file for duration extraction: {}", e);
            return None;
        }
    };

    match TrackDecoder::new(file_data) {
        Ok(decoder) => {
            let duration = decoder.duration().map(|d| d.as_millis() as i64);
            if let Some(dur_ms) = duration {
                debug!(
                    "Extracted duration: {} ms from {}",
                    dur_ms,
                    file_path.display()
                );
            } else {
                warn!("Duration not available for file: {}", file_path.display());
            }
            duration
        }
        Err(e) => {
            warn!("Failed to decode file for duration extraction: {:?}", e);
            None
        }
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
