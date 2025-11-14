// # Import Handle
//
// Handle for sending import requests and subscribing to progress updates.
// Provides the public API for interacting with the import service.

use crate::cue_flac::CueFlacProcessor;
use crate::db::DbTorrent;
use crate::discogs::DiscogsRelease;
use crate::import::progress::ImportProgressHandle;
use crate::import::track_to_file_mapper::map_tracks_to_files;
use crate::import::types::{
    DiscoveredFile, ImportCommand, ImportProgress, ImportRequest, TorrentSource, TrackFile,
};
use crate::library::{LibraryManager, SharedLibraryManager};
use crate::musicbrainz::MbRelease;
use crate::playback::symphonia_decoder::TrackDecoder;
use crate::torrent::{SelectiveDownloader, TorrentClient};
use std::path::Path;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

/// Handle for sending import requests and subscribing to progress updates
#[derive(Clone)]
pub struct ImportHandle {
    pub requests_tx: mpsc::UnboundedSender<ImportCommand>,
    pub progress_handle: ImportProgressHandle,
    pub library_manager: SharedLibraryManager,
    pub runtime_handle: tokio::runtime::Handle,
    torrent_client: TorrentClient,
}

/// Torrent-specific metadata for import
#[derive(Debug, Clone)]
pub struct TorrentImportMetadata {
    pub info_hash: String,
    pub magnet_link: Option<String>,
    pub torrent_name: String,
    pub total_size_bytes: i64,
    pub piece_length: i32,
    pub num_pieces: i32,
    pub seed_after_download: bool,
}

impl ImportHandle {
    /// Create a new ImportHandle with the given dependencies
    pub fn new(
        requests_tx: mpsc::UnboundedSender<ImportCommand>,
        progress_rx: mpsc::UnboundedReceiver<ImportProgress>,
        library_manager: SharedLibraryManager,
        runtime_handle: tokio::runtime::Handle,
    ) -> Self {
        let progress_handle = ImportProgressHandle::new(progress_rx, runtime_handle.clone());
        let torrent_client = TorrentClient::new(runtime_handle.clone())
            .expect("Failed to create shared torrent client");

        Self {
            requests_tx,
            progress_handle,
            library_manager,
            runtime_handle,
            torrent_client,
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
    pub async fn send_request(&self, request: ImportRequest) -> Result<(String, String), String> {
        match request {
            ImportRequest::Folder {
                discogs_release,
                mb_release,
                folder,
                master_year,
            } => {
                self.handle_folder_request(discogs_release, mb_release, folder, master_year)
                    .await
            }
            ImportRequest::Torrent {
                torrent_source,
                discogs_release,
                mb_release,
                master_year,
                seed_after_download,
            } => {
                self.handle_torrent_request(
                    torrent_source,
                    discogs_release,
                    mb_release,
                    master_year,
                    seed_after_download,
                )
                .await
            }
            ImportRequest::CD {
                discogs_release,
                mb_release,
                drive_path,
                master_year,
            } => {
                self.handle_cd_request(discogs_release, mb_release, drive_path, master_year)
                    .await
            }
        }
    }

    async fn handle_folder_request(
        &self,
        discogs_release: Option<DiscogsRelease>,
        mb_release: Option<MbRelease>,
        folder: std::path::PathBuf,
        master_year: u32,
    ) -> Result<(String, String), String> {
        // Validate that at least one release is provided
        if discogs_release.is_none() && mb_release.is_none() {
            return Err("Either discogs_release or mb_release must be provided".to_string());
        }

        let library_manager = self.library_manager.get();

        // ========== VALIDATION (before queueing) ==========

        // 1. Parse release into database models (Discogs or MusicBrainz)
        let (db_album, db_release, db_tracks, artists, album_artists) =
            if let Some(ref discogs_rel) = discogs_release {
                use crate::import::discogs_parser::parse_discogs_release;
                parse_discogs_release(discogs_rel, master_year)?
            } else if let Some(ref mb_rel) = mb_release {
                use crate::import::musicbrainz_parser::fetch_and_parse_mb_release;
                fetch_and_parse_mb_release(&mb_rel.release_id, master_year).await?
            } else {
                return Err("No release provided".to_string());
            };

        // 2. Discover files
        let discovered_files = discover_folder_files(&folder)?;

        // 3. Build track-to-file mapping (validates and parses CUE sheets if present)
        let mapping_result = map_tracks_to_files(&db_tracks, &discovered_files).await?;
        let tracks_to_files = mapping_result.track_files.clone();
        let cue_flac_metadata = mapping_result.cue_flac_metadata.clone();

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
            let actual_artist_id = artist_id_map.get(&album_artist.artist_id).ok_or_else(|| {
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
                .map_err(|e| format!("Failed to insert album-artist relationship: {}", e))?;
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
            .send(ImportCommand::Folder {
                db_album,
                db_release,
                tracks_to_files,
                discovered_files,
                cue_flac_metadata,
            })
            .map_err(|_| "Failed to queue validated album for import".to_string())?;

        Ok((album_id, release_id))
    }

    async fn handle_torrent_request(
        &self,
        torrent_source: TorrentSource,
        discogs_release: Option<DiscogsRelease>,
        mb_release: Option<MbRelease>,
        master_year: u32,
        seed_after_download: bool,
    ) -> Result<(String, String), String> {
        // Validate that at least one release is provided
        if discogs_release.is_none() && mb_release.is_none() {
            return Err("Either discogs_release or mb_release must be provided".to_string());
        }

        let library_manager = self.library_manager.get();

        // ========== TORRENT SETUP ==========

        // Use shared torrent client
        let torrent_client = self.torrent_client.clone();

        // Extract magnet link before moving torrent_source
        let magnet_link_opt = match &torrent_source {
            TorrentSource::MagnetLink(m) => Some(m.clone()),
            _ => None,
        };

        // Clone torrent_source for storing in ImportRequest
        let torrent_source_for_request = torrent_source.clone();

        // Add torrent to client
        let torrent_handle = match &torrent_source {
            TorrentSource::File(path) => torrent_client
                .add_torrent_file(path)
                .await
                .map_err(|e| format!("Failed to add torrent file: {}", e))?,
            TorrentSource::MagnetLink(magnet) => torrent_client
                .add_magnet_link(magnet)
                .await
                .map_err(|e| format!("Failed to add magnet link: {}", e))?,
        };

        // Wait for metadata to be available (for magnet links)
        torrent_handle
            .wait_for_metadata()
            .await
            .map_err(|e| format!("Failed to get torrent metadata: {}", e))?;

        // Get torrent info
        let info_hash = torrent_handle.info_hash().await;
        let torrent_name = torrent_handle
            .name()
            .await
            .map_err(|e| format!("Failed to get torrent name: {}", e))?;
        let total_size = torrent_handle
            .total_size()
            .await
            .map_err(|e| format!("Failed to get torrent size: {}", e))?;
        let piece_length = torrent_handle
            .piece_length()
            .await
            .map_err(|e| format!("Failed to get piece length: {}", e))?;
        let num_pieces = torrent_handle
            .num_pieces()
            .await
            .map_err(|e| format!("Failed to get piece count: {}", e))?;

        info!(
            "Torrent added: {} ({} pieces, {} bytes)",
            torrent_name, num_pieces, total_size
        );

        // ========== METADATA FILE PRIORITIZATION ==========

        let selective_downloader = SelectiveDownloader::new(torrent_client.clone());
        let metadata_files = selective_downloader
            .prioritize_metadata_files(&torrent_handle)
            .await
            .map_err(|e| format!("Failed to prioritize metadata files: {}", e))?;

        if !metadata_files.is_empty() {
            info!(
                "Waiting for {} metadata files to download...",
                metadata_files.len()
            );
            selective_downloader
                .wait_for_metadata_files(&torrent_handle, &metadata_files)
                .await
                .map_err(|e| format!("Failed to wait for metadata files: {}", e))?;
        }

        // ========== PARSE RELEASE INTO DATABASE MODELS ==========

        let (db_album, db_release, db_tracks, artists, album_artists) =
            if let Some(ref discogs_rel) = discogs_release {
                use crate::import::discogs_parser::parse_discogs_release;
                parse_discogs_release(discogs_rel, master_year)?
            } else if let Some(ref mb_rel) = mb_release {
                use crate::import::musicbrainz_parser::fetch_and_parse_mb_release;
                fetch_and_parse_mb_release(&mb_rel.release_id, master_year).await?
            } else {
                return Err("No release provided".to_string());
            };

        // ========== ENABLE ALL FILES FOR DOWNLOAD ==========

        selective_downloader
            .enable_remaining_files(&torrent_handle)
            .await
            .map_err(|e| format!("Failed to enable remaining files: {}", e))?;

        // ========== MAP TRACKS TO TORRENT FILES ==========

        // Get file list from torrent
        let torrent_files = torrent_handle
            .get_file_list()
            .await
            .map_err(|e| format!("Failed to get torrent file list: {}", e))?;

        // Convert torrent files to DiscoveredFile format
        // Torrent file paths from libtorrent already include the torrent name directory,
        // so we just need to prepend the temp directory
        let temp_dir = std::env::temp_dir();
        let discovered_files: Vec<DiscoveredFile> = torrent_files
            .iter()
            .map(|tf| DiscoveredFile {
                // libtorrent's file_path() returns paths that already include the torrent name,
                // so we only need to prepend the temp directory
                path: temp_dir.join(&tf.path),
                size: tf.size as u64,
            })
            .collect();

        // Build track-to-file mapping
        // Note: For torrents, we'll need to wait for files to download or map based on filenames
        // For now, this is a simplified version that assumes files match by name
        let mapping_result = map_tracks_to_files(&db_tracks, &discovered_files).await?;
        let tracks_to_files = mapping_result.track_files.clone();
        // Note: cue_flac_metadata will be re-parsed after torrent download completes

        // ========== INSERT ARTISTS ==========

        let mut artist_id_map = std::collections::HashMap::new();
        for artist in &artists {
            let parsed_id = artist.id.clone();

            let existing = if let Some(ref discogs_id) = artist.discogs_artist_id {
                library_manager
                    .get_artist_by_discogs_id(discogs_id)
                    .await
                    .map_err(|e| format!("Database error: {}", e))?
            } else {
                None
            };

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

        // ========== INSERT ALBUM + RELEASE + TRACKS ==========

        library_manager
            .insert_album_with_release_and_tracks(&db_album, &db_release, &db_tracks)
            .await
            .map_err(|e| format!("Database error: {}", e))?;

        // Extract and store durations
        extract_and_store_durations(library_manager, &tracks_to_files).await?;

        // Insert album-artist relationships
        for album_artist in &album_artists {
            let actual_artist_id = artist_id_map.get(&album_artist.artist_id).ok_or_else(|| {
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
                .map_err(|e| format!("Failed to insert album-artist relationship: {}", e))?;
        }

        // ========== SAVE TORRENT METADATA TO DATABASE ==========

        let torrent_metadata = TorrentImportMetadata {
            info_hash: info_hash.clone(),
            magnet_link: magnet_link_opt,
            torrent_name: torrent_name.clone(),
            total_size_bytes: total_size,
            piece_length,
            num_pieces,
            seed_after_download,
        };

        // Save torrent record (will be used for seeding later)
        let db_torrent = DbTorrent::new(
            &db_release.id,
            &info_hash,
            torrent_metadata.magnet_link.clone(),
            &torrent_name,
            total_size,
            piece_length,
            num_pieces,
        );

        // Save torrent metadata to database
        library_manager
            .insert_torrent(&db_torrent)
            .await
            .map_err(|e| format!("Failed to save torrent metadata: {}", e))?;

        tracing::info!(
            "Validated and queued torrent import '{}' (release: {}) with {} tracks",
            db_album.title,
            db_release.id,
            db_tracks.len()
        );

        // ========== QUEUE FOR PIPELINE ==========

        let album_id = db_album.id.clone();
        let release_id = db_release.id.clone();

        self.requests_tx
            .send(ImportCommand::Torrent {
                db_album,
                db_release,
                tracks_to_files,
                torrent_source: torrent_source_for_request,
                torrent_metadata,
                seed_after_download,
            })
            .map_err(|_| "Failed to queue validated torrent for import".to_string())?;

        Ok((album_id, release_id))
    }

    async fn handle_cd_request(
        &self,
        discogs_release: Option<DiscogsRelease>,
        mb_release: Option<MbRelease>,
        drive_path: std::path::PathBuf,
        master_year: u32,
    ) -> Result<(String, String), String> {
        // Validate that at least one release is provided
        if discogs_release.is_none() && mb_release.is_none() {
            return Err("Either discogs_release or mb_release must be provided".to_string());
        }

        let library_manager = self.library_manager.get();

        // ========== READ CD TOC ==========

        use crate::cd::CdDrive;

        // Create CD drive instance
        let drive = CdDrive {
            device_path: drive_path.clone(),
            name: drive_path.to_str().unwrap_or("Unknown").to_string(),
        };

        // Read TOC from CD
        let toc = drive
            .read_toc()
            .map_err(|e| format!("Failed to read CD TOC: {}", e))?;

        // ========== PARSE RELEASE INTO DATABASE MODELS ==========
        // Parse release BEFORE ripping so we have release_id for progress updates

        let (db_album, db_release, db_tracks, artists, album_artists) =
            if let Some(ref discogs_rel) = discogs_release {
                use crate::import::discogs_parser::parse_discogs_release;
                parse_discogs_release(discogs_rel, master_year)?
            } else if let Some(ref mb_rel) = mb_release {
                use crate::import::musicbrainz_parser::fetch_and_parse_mb_release;
                fetch_and_parse_mb_release(&mb_rel.release_id, master_year).await?
            } else {
                return Err("No release provided".to_string());
            };

        // ========== INSERT ARTISTS, ALBUM, RELEASE, TRACKS ==========
        // Insert into database BEFORE ripping so UI can navigate and show progress

        // Insert or lookup artists (deduplicate across imports)
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

        // Insert album + release + tracks with status='queued'
        library_manager
            .insert_album_with_release_and_tracks(&db_album, &db_release, &db_tracks)
            .await
            .map_err(|e| format!("Database error: {}", e))?;

        // Insert album-artist relationships (using actual database artist IDs)
        for album_artist in &album_artists {
            let actual_artist_id = artist_id_map.get(&album_artist.artist_id).ok_or_else(|| {
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
                .map_err(|e| format!("Failed to insert album-artist relationship: {}", e))?;
        }

        // ========== QUEUE FOR PIPELINE ==========
        // Service will handle CD ripping (acquire phase) and chunk upload

        let album_id = db_album.id.clone();
        let release_id = db_release.id.clone();

        self.requests_tx
            .send(ImportCommand::CD {
                db_album,
                db_release,
                db_tracks,
                drive_path: drive.device_path,
                toc,
            })
            .map_err(|_| "Failed to queue validated CD import".to_string())?;

        Ok((album_id, release_id))
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
pub async fn extract_and_store_durations(
    library_manager: &LibraryManager,
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
