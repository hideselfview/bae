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
use crate::import::progress_service::ImportProgressService;
use crate::import::track_file_mapper::TrackFileMapper;
use crate::import::types::{ImportProgress, ImportRequest};
use crate::import::upload_pipeline::UploadPipeline;
use crate::library_context::SharedLibraryManager;
use crate::models::DiscogsAlbum;
use std::path::Path;
use tokio::sync::mpsc;

/// Handle for sending import requests and subscribing to progress updates
#[derive(Clone)]
pub struct ImportServiceHandle {
    request_tx: mpsc::UnboundedSender<ImportRequest>,
    progress_service: ImportProgressService,
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

/// Configuration for import service worker pools
pub struct ImportWorkerConfig {
    /// Number of parallel encryption workers (CPU-bound, typically 2x CPU cores)
    pub max_encrypt_workers: usize,
    /// Number of parallel upload workers (I/O-bound)
    pub max_upload_workers: usize,
}

/// Import service that orchestrates the import workflow on the shared runtime
pub struct ImportService {
    library_manager: SharedLibraryManager,
    upload_pipeline: UploadPipeline,
    progress_tx: mpsc::UnboundedSender<ImportProgress>,
    request_rx: mpsc::UnboundedReceiver<ImportRequest>,
    max_encrypt_workers: usize,
    max_upload_workers: usize,
}

impl ImportService {
    /// Start import service worker, returning handle for sending requests
    pub fn start(
        runtime_handle: tokio::runtime::Handle,
        library_manager: SharedLibraryManager,
        chunking_service: ChunkingService,
        cloud_storage: CloudStorageManager,
        worker_config: ImportWorkerConfig,
    ) -> ImportServiceHandle {
        let (request_tx, request_rx) = mpsc::unbounded_channel();
        let (progress_tx, progress_rx) = mpsc::unbounded_channel();

        let upload_pipeline = UploadPipeline::new(chunking_service, cloud_storage);

        // Create service instance for worker task
        let service = ImportService {
            library_manager,
            upload_pipeline,
            progress_tx,
            request_rx,
            max_encrypt_workers: worker_config.max_encrypt_workers,
            max_upload_workers: worker_config.max_upload_workers,
        };

        // Spawn import worker task on shared runtime
        runtime_handle.spawn(service.listen_for_import_requests());

        // Create ProgressService, used to broadcast progress updates to external subscribers
        let progress_service = ImportProgressService::new(progress_rx, runtime_handle.clone());

        ImportServiceHandle {
            request_tx,
            progress_service,
        }
    }

    async fn listen_for_import_requests(mut self) {
        println!("ImportService: Worker started");

        // Process import requests
        loop {
            match self.request_rx.recv().await {
                Some(request) => {
                    if let Err(e) = self.handle_import_request(request).await {
                        println!("ImportService: Import failed: {}", e);
                        // TODO: Handle error
                    }
                }
                None => {
                    println!("ImportService: Channel closed");
                    break;
                }
            }
        }
    }

    async fn handle_import_request(&self, request: ImportRequest) -> Result<(), String> {
        match request {
            ImportRequest::FromFolder { album, folder } => {
                println!(
                    "ImportService: Received import request for {}",
                    album.title()
                );

                self.import_from_folder(&album, &folder).await
            }
        }
    }

    async fn import_from_folder(
        &self,
        discogs_album: &DiscogsAlbum,
        folder: &Path,
    ) -> Result<(), String> {
        let library_manager = self.library_manager.get();

        println!(
            "ImportService: Starting import for {} from {}",
            discogs_album.title(),
            folder.display()
        );

        // 1. Create album + track records (in memory only)
        let album = create_album_record(discogs_album)?;
        let tracks = create_track_records(discogs_album, &album.id)?;

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
            album_id: album.id.clone(),
        });

        // 4. Start upload pipeline (returns event channel)
        let upload_config = crate::import::UploadConfig {
            max_encrypt_workers: self.max_encrypt_workers,
            max_upload_workers: self.max_upload_workers,
        };

        let mut upload_events = self
            .upload_pipeline
            .chunk_and_upload_album(track_files.clone(), upload_config);

        // 5. Process upload events and handle database persistence
        let mut file_mappings = None;
        let mut total_chunks = 0;
        let mut chunks_completed = 0;

        while let Some(event) = upload_events.recv().await {
            match event {
                crate::import::UploadEvent::Started {
                    total_chunks: total,
                } => {
                    total_chunks = total;
                }
                crate::import::UploadEvent::ChunkUploaded {
                    chunk_id,
                    chunk_index,
                    original_size,
                    encrypted_size,
                    checksum,
                    cloud_location,
                } => {
                    // Persist chunk to database
                    let db_chunk = crate::database::DbChunk::from_album_chunk(
                        &chunk_id,
                        &album.id.clone(),
                        chunk_index,
                        original_size,
                        encrypted_size,
                        &checksum,
                        &cloud_location,
                        false,
                    );
                    library_manager
                        .add_chunk(&db_chunk)
                        .await
                        .map_err(|e| format!("Failed to add chunk: {}", e))?;

                    // Send progress update
                    chunks_completed += 1;
                    if total_chunks > 0 {
                        let percent =
                            ((chunks_completed as f64 / total_chunks as f64) * 100.0) as u8;
                        let _ = self.progress_tx.send(ImportProgress::ProcessingProgress {
                            album_id: album.id.clone(),
                            percent,
                            current: chunks_completed,
                            total: total_chunks,
                        });
                    }
                }
                crate::import::UploadEvent::TrackCompleted { track_id } => {
                    // Mark track complete in database
                    library_manager
                        .mark_track_complete(&track_id)
                        .await
                        .map_err(|e| format!("Failed to mark track complete: {}", e))?;

                    // Send track completion progress event
                    let _ = self.progress_tx.send(ImportProgress::TrackComplete {
                        album_id: album.id.clone(),
                        track_id,
                    });
                }
                crate::import::UploadEvent::Completed {
                    file_mappings: mappings,
                } => {
                    // Store file mappings for metadata persistence
                    file_mappings = Some(mappings);
                    break;
                }
                crate::import::UploadEvent::Failed { error } => {
                    // Mark as failed
                    let _ = library_manager.mark_album_failed(&album.id).await;
                    for track in &tracks {
                        let _ = library_manager.mark_track_failed(&track.id).await;
                    }
                    return Err(format!("Upload failed: {}", error));
                }
            }
        }

        let file_mappings =
            file_mappings.ok_or_else(|| "Upload completed without file mappings".to_string())?;

        // 6. Persist file metadata
        let persister = MetadataPersister::new(library_manager);
        persister
            .persist_album_metadata(&track_files, &file_mappings)
            .await?;

        // 7. Mark album as complete
        library_manager
            .mark_album_complete(&album.id)
            .await
            .map_err(|e| format!("Failed to mark album complete: {}", e))?;

        let _ = self
            .progress_tx
            .send(ImportProgress::Complete { album_id: album.id });

        println!(
            "ImportService: Import completed successfully for {}",
            album.title
        );
        Ok(())
    }
}

/// Create album database record from Discogs data
fn create_album_record(import_item: &DiscogsAlbum) -> Result<DbAlbum, String> {
    let artist_name = extract_artist_name(import_item);

    let album = match import_item {
        DiscogsAlbum::Master(master) => DbAlbum::from_discogs_master(master, &artist_name),
        DiscogsAlbum::Release(release) => DbAlbum::from_discogs_release(release, &artist_name),
    };
    Ok(album)
}

/// Create track database records from Discogs tracklist
fn create_track_records(
    import_item: &DiscogsAlbum,
    album_id: &str,
) -> Result<Vec<DbTrack>, String> {
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
fn extract_artist_name(import_item: &DiscogsAlbum) -> String {
    let title = import_item.title();
    if let Some(dash_pos) = title.find(" - ") {
        title[..dash_pos].to_string()
    } else {
        "Unknown Artist".to_string()
    }
}
