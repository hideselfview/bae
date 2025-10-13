// # Import Service - Orchestrator
//
// This module contains the thin orchestrator that coordinates specialized services:
// - TrackFileMapper: Validates track-to-file mapping
// - UploadPipeline: Chunks and uploads to cloud
// - MetadataPersister: Saves file/chunk metadata to DB
//
// The orchestrator's job is to call these services in the right order and handle
// progress reporting to the UI.

use crate::chunking::ChunkingService;
use crate::cloud_storage::CloudStorageManager;
use crate::database::{DbAlbum, DbTrack};
use crate::import::metadata_persister::MetadataPersister;
use crate::import::progress_service::ProgressService;
use crate::import::track_file_mapper::TrackFileMapper;
use crate::import::types::{ImportProgress, ImportRequest};
use crate::import::upload_pipeline::UploadPipeline;
use crate::library_context::SharedLibraryManager;
use crate::models::ImportItem;
use std::path::Path;
use tokio::sync::mpsc;

/// Handle for sending import requests and subscribing to progress updates
#[derive(Clone)]
pub struct ImportServiceHandle {
    request_tx: mpsc::UnboundedSender<ImportRequest>,
    progress_service: ProgressService,
}

impl ImportServiceHandle {
    /// Send an import request to the import service
    pub fn send_request(&self, request: ImportRequest) -> Result<(), String> {
        self.request_tx
            .send(request)
            .map_err(|e| format!("Failed to send import request: {}", e))
    }

    /// Subscribe to progress updates for a specific album
    /// Returns a filtered receiver that yields only updates for the specified album
    pub fn subscribe_album(
        &self,
        album_id: String,
    ) -> tokio::sync::mpsc::UnboundedReceiver<ImportProgress> {
        self.progress_service.subscribe_album(album_id)
    }

    /// Subscribe to progress updates for a specific track
    /// Returns a filtered receiver that yields only updates for the specified track
    pub fn subscribe_track(
        &self,
        album_id: String,
        track_id: String,
    ) -> tokio::sync::mpsc::UnboundedReceiver<ImportProgress> {
        self.progress_service.subscribe_track(album_id, track_id)
    }
}

/// Import service that orchestrates the import workflow on the shared runtime
pub struct ImportService {
    library_manager: SharedLibraryManager,
    chunking_service: ChunkingService,
    cloud_storage: CloudStorageManager,
    progress_tx: mpsc::UnboundedSender<ImportProgress>,
}

impl ImportService {
    /// Start import service worker, returning handle for sending requests
    pub fn start(
        library_manager: SharedLibraryManager,
        chunking_service: ChunkingService,
        cloud_storage: CloudStorageManager,
        runtime_handle: tokio::runtime::Handle,
    ) -> ImportServiceHandle {
        let (request_tx, mut request_rx) = mpsc::unbounded_channel();
        let (progress_tx, progress_rx) = mpsc::unbounded_channel();

        // Create ProgressService
        let progress_service = ProgressService::new(progress_rx, runtime_handle.clone());

        // Create service instance for worker task
        let service = ImportService {
            library_manager,
            chunking_service,
            cloud_storage,
            progress_tx,
        };

        // Spawn import worker task on shared runtime
        runtime_handle.spawn(async move {
            println!("ImportService: Worker started");

            // Process import requests
            loop {
                match request_rx.recv().await {
                    Some(ImportRequest::ImportAlbum { item, folder }) => {
                        println!(
                            "ImportService: Received import request for {}",
                            item.title()
                        );

                        if let Err(e) = service.handle_import(&item, &folder).await {
                            println!("ImportService: Import failed: {}", e);
                        }
                    }
                    Some(ImportRequest::Shutdown) => {
                        println!("ImportService: Shutdown requested");
                        break;
                    }
                    None => {
                        println!("ImportService: Channel closed");
                        break;
                    }
                }
            }

            println!("ImportService: Worker exiting");
        });

        ImportServiceHandle {
            request_tx,
            progress_service,
        }
    }

    /// Handle a single import request - orchestrates the import workflow
    async fn handle_import(&self, item: &ImportItem, folder: &Path) -> Result<(), String> {
        let library_manager = self.library_manager.get();
        println!(
            "ImportService: Starting import for {} from {}",
            item.title(),
            folder.display()
        );

        // 1. Create album + track records (in memory only)
        let artist_name = extract_artist_name(item);
        let album = create_album_record(
            item,
            &artist_name,
            Some(folder.to_string_lossy().to_string()),
        )?;
        let album_id = album.id.clone();
        let album_title = album.title.clone();
        let tracks = create_track_records(item, &album_id)?;

        println!(
            "ImportService: Created album record with {} tracks (not inserted yet)",
            tracks.len()
        );

        // 2. Validate track-to-file mapping
        let track_files = TrackFileMapper::map_tracks_to_files(folder, &tracks).await?;

        println!(
            "ImportService: Successfully mapped {} tracks to source files",
            track_files.len()
        );

        // 3. Insert album + tracks into database (status='importing')
        library_manager
            .insert_album_with_tracks(&album, &tracks)
            .await
            .map_err(|e| format!("Database error: {}", e))?;

        println!(
            "ImportService: Inserted album and {} tracks into database with status='importing'",
            tracks.len()
        );

        // Send started progress
        let _ = self.progress_tx.send(ImportProgress::Started {
            album_id: album_id.clone(),
            album_title: album_title.clone(),
        });

        // 4. Chunk and upload album files
        let progress_tx_clone = self.progress_tx.clone();
        let album_id_clone = album_id.clone();
        let progress_callback = Box::new(move |current, total| {
            let percent = ((current as f64 / total as f64) * 100.0) as u8;
            let _ = progress_tx_clone.send(ImportProgress::ProcessingProgress {
                album_id: album_id_clone.clone(),
                current,
                total,
                percent,
            });
        });

        // Configure worker pool sizes
        let max_encrypt_workers = std::thread::available_parallelism()
            .map(|n| n.get() * 2)
            .unwrap_or(4);
        let max_upload_workers = 20;

        let upload_pipeline =
            UploadPipeline::new(self.chunking_service.clone(), self.cloud_storage.clone());
        let album_result = upload_pipeline
            .chunk_and_upload_album(
                library_manager,
                &track_files,
                &album_id,
                max_encrypt_workers,
                max_upload_workers,
                progress_callback,
            )
            .await
            .map_err(|e| {
                // Mark as failed
                let _ = tokio::runtime::Handle::current()
                    .block_on(library_manager.mark_album_failed(&album_id));
                for track in &tracks {
                    let _ = tokio::runtime::Handle::current()
                        .block_on(library_manager.mark_track_failed(&track.id));
                }
                format!("Import failed: {}", e)
            })?;

        // 5. Persist file metadata
        let persister = MetadataPersister::new(library_manager);
        persister
            .persist_album_metadata(&track_files, &album_result.file_mappings)
            .await?;

        // 6. Mark tracks and album as complete
        for track in &tracks {
            library_manager
                .mark_track_complete(&track.id)
                .await
                .map_err(|e| format!("Failed to mark track complete: {}", e))?;

            let _ = self.progress_tx.send(ImportProgress::TrackComplete {
                album_id: album_id.clone(),
                track_id: track.id.clone(),
            });
        }

        library_manager
            .mark_album_complete(&album_id)
            .await
            .map_err(|e| format!("Failed to mark album complete: {}", e))?;

        let _ = self.progress_tx.send(ImportProgress::Complete {
            album_id: album_id.clone(),
        });

        println!(
            "ImportService: Import completed successfully for {}",
            album_title
        );
        Ok(())
    }
}

/// Create album database record from Discogs data
fn create_album_record(
    import_item: &ImportItem,
    artist_name: &str,
    source_folder_path: Option<String>,
) -> Result<DbAlbum, String> {
    let album = match import_item {
        ImportItem::Master(master) => {
            DbAlbum::from_discogs_master(master, artist_name, source_folder_path)
        }
        ImportItem::Release(release) => {
            DbAlbum::from_discogs_release(release, artist_name, source_folder_path)
        }
    };
    Ok(album)
}

/// Create track database records from Discogs tracklist
fn create_track_records(import_item: &ImportItem, album_id: &str) -> Result<Vec<DbTrack>, String> {
    let discogs_tracks = import_item.tracklist();
    let mut tracks = Vec::new();

    for (index, discogs_track) in discogs_tracks.iter().enumerate() {
        let track_number = parse_track_number(&discogs_track.position, index);
        let track = DbTrack::from_discogs_track(discogs_track, album_id, track_number);
        tracks.push(track);
    }

    Ok(tracks)
}

/// Parse track number from Discogs position string
/// Discogs positions can be like "1", "A1", "1-1", etc.
fn parse_track_number(position: &str, fallback_index: usize) -> Option<i32> {
    // Try to extract number from position string
    let numbers: String = position.chars().filter(|c| c.is_numeric()).collect();

    if let Ok(num) = numbers.parse::<i32>() {
        Some(num)
    } else {
        // Fallback to index + 1
        Some((fallback_index + 1) as i32)
    }
}

/// Extract artist name from import item
fn extract_artist_name(import_item: &ImportItem) -> String {
    let title = import_item.title();
    if let Some(dash_pos) = title.find(" - ") {
        title[..dash_pos].to_string()
    } else {
        "Unknown Artist".to_string()
    }
}
