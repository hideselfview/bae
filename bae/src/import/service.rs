// # Import Service - Orchestrator
//
// This module contains the thin orchestrator that coordinates specialized services:
// - TrackFileMapper: Validates track-to-file mapping
// - UploadPipeline: Chunks and uploads to cloud
// - MetadataPersister: Saves file/chunk metadata to DB
//
// The orchestrator's job is to call these services in the right order and handle
// progress reporting to the UI.

use crate::chunking::{ChunkingService, FileChunkMapping};
use crate::cloud_storage::CloudStorageManager;
use crate::database::{DbAlbum, DbTrack};
use crate::encryption::EncryptionService;
use crate::import::metadata_persister::MetadataPersister;
use crate::import::progress_service::ImportProgressService;
use crate::import::track_file_mapper::TrackFileMapper;
use crate::import::types::{ImportProgress, ImportRequest, TrackSourceFile};
use crate::import::upload_pipeline::UploadPipeline;
use crate::library::LibraryManager;
use crate::library_context::SharedLibraryManager;
use crate::models::DiscogsAlbum;
use futures::stream::{self, StreamExt};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncSeekExt, BufReader};
use tokio::sync::mpsc;
use uuid::Uuid;

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
    #[allow(dead_code)]
    chunking_service: ChunkingService,
    #[allow(dead_code)]
    encryption_service: EncryptionService,
    #[allow(dead_code)]
    cloud_storage: CloudStorageManager,
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
        encryption_service: EncryptionService,
        cloud_storage: CloudStorageManager,
        worker_config: ImportWorkerConfig,
    ) -> ImportServiceHandle {
        let (request_tx, request_rx) = mpsc::unbounded_channel();
        let (progress_tx, progress_rx) = mpsc::unbounded_channel();

        let upload_pipeline = UploadPipeline::new(chunking_service.clone(), cloud_storage.clone());

        // Create service instance for worker task
        let service = ImportService {
            library_manager,
            upload_pipeline,
            chunking_service,
            encryption_service,
            cloud_storage,
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

    /// Alternative import implementation using direct stream pipeline
    #[allow(dead_code)]
    async fn import_from_folder_2(
        &self,
        discogs_album: &DiscogsAlbum,
        folder: &Path,
    ) -> Result<(), String> {
        let library_manager = self.library_manager.get();

        println!(
            "ImportService: Starting stream-based import for {} from {}",
            discogs_album.title(),
            folder.display()
        );

        // ========== SETUP ==========

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

        // 4. Send started progress
        let _ = self.progress_tx.send(ImportProgress::Started {
            album_id: album.id.clone(),
        });

        // 5. Find all files and calculate chunk layout
        let all_files = find_album_files(folder)?;
        let chunk_size = 1024 * 1024; // 1MB chunks (default)
        let (file_mappings, chunk_specs) = calculate_chunk_layout(&all_files, chunk_size)?;

        println!(
            "ImportService: Calculated {} chunks across {} files",
            chunk_specs.len(),
            file_mappings.len()
        );

        // 6. Build track mapping for progress tracking
        let track_chunk_map = build_track_chunk_mapping(&file_mappings, &track_files);

        // ========== PIPELINE ==========

        // Shared state for tracking progress
        let completed_chunks = Arc::new(Mutex::new(HashSet::new()));
        let total_chunks = chunk_specs.len();

        let album_id = album.id.clone();
        let encryption_service = self.encryption_service.clone();
        let cloud_storage = self.cloud_storage.clone();
        let max_encrypt_workers = self.max_encrypt_workers;
        let max_upload_workers = self.max_upload_workers;
        let progress_tx = self.progress_tx.clone();
        let track_chunk_map = Arc::new(track_chunk_map);

        let results = stream::iter(chunk_specs)
            // Stage 1: Read chunks (bounded I/O)
            .map(|spec| async move { read_chunk(spec).await })
            .buffer_unordered(5)
            // Stage 2: Encrypt chunks (bounded CPU via spawn_blocking)
            .map(move |chunk_data_result| {
                let encryption_service = encryption_service.clone();
                async move {
                    let chunk_data = chunk_data_result?;
                    let join_result = tokio::task::spawn_blocking(move || {
                        encrypt_chunk_blocking(chunk_data, &encryption_service)
                    })
                    .await
                    .map_err(|e| format!("Encryption task panicked: {}", e))?;
                    join_result
                }
            })
            .buffer_unordered(max_encrypt_workers)
            // Stage 3: Upload chunks (bounded I/O)
            .map(move |encrypted_result| {
                let cloud_storage = cloud_storage.clone();
                async move {
                    let encrypted = encrypted_result?;
                    upload_chunk(encrypted, &cloud_storage).await
                }
            })
            .buffer_unordered(max_upload_workers)
            // Stage 4: Persist to DB and handle progress
            .map(move |upload_result| {
                let album_id = album_id.clone();
                let library_manager = library_manager.clone();
                let track_chunk_map = track_chunk_map.clone();
                let completed_chunks = completed_chunks.clone();
                let progress_tx = progress_tx.clone();

                async move {
                    persist_and_track_progress(
                        upload_result,
                        &album_id,
                        &library_manager,
                        &track_chunk_map,
                        &completed_chunks,
                        &progress_tx,
                        total_chunks,
                    )
                    .await
                }
            })
            .buffer_unordered(10) // Allow some parallelism for DB writes
            // Collect to drive the stream to completion
            .collect::<Vec<_>>()
            .await;

        // Check for errors (fail fast on first error)
        for result in results {
            result?;
        }

        println!(
            "ImportService: All {} chunks processed successfully",
            total_chunks
        );

        // ========== TEARDOWN ==========

        // Persist file metadata to database
        let persister = MetadataPersister::new(library_manager);
        persister
            .persist_album_metadata(&track_files, &file_mappings)
            .await?;

        // Mark album complete
        library_manager
            .mark_album_complete(&album.id)
            .await
            .map_err(|e| format!("Failed to mark album complete: {}", e))?;

        // Send completion event
        let _ = self
            .progress_tx
            .send(ImportProgress::Complete { album_id: album.id });

        println!(
            "ImportService: Stream-based import completed successfully for {}",
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

// ============================================================================
// Stream-based Pipeline Data Structures
// (Used by import_from_folder_2 - new implementation, not yet active)
// ============================================================================

/// Specification for a single chunk to be read from disk
#[allow(dead_code)]
struct ChunkSpec {
    chunk_id: String,
    chunk_index: i32,
    file_path: PathBuf,
    offset: u64,
    size: usize,
}

/// Raw chunk data read from disk with checksum
#[allow(dead_code)]
struct ChunkData {
    chunk_id: String,
    chunk_index: i32,
    data: Vec<u8>,
    checksum: String,
}

/// Encrypted chunk data ready for upload
#[allow(dead_code)]
struct EncryptedChunkData {
    chunk_id: String,
    chunk_index: i32,
    original_size: usize,
    encrypted_data: Vec<u8>,
    checksum: String,
}

/// Chunk successfully uploaded to cloud storage
#[allow(dead_code)]
struct UploadedChunk {
    chunk_id: String,
    chunk_index: i32,
    original_size: usize,
    encrypted_size: usize,
    checksum: String,
    cloud_location: String,
}

/// Mapping of chunks to tracks for progress tracking
#[allow(dead_code)]
struct TrackChunkMap {
    chunk_to_track: HashMap<i32, String>,
    track_chunk_counts: HashMap<String, usize>,
}

// ============================================================================
// Pipeline Helper Functions
// ============================================================================

/// Find all files in an album folder (sorted for consistent ordering)
#[allow(dead_code)]
fn find_album_files(folder: &Path) -> Result<Vec<PathBuf>, String> {
    let mut all_files = Vec::new();

    for entry in std::fs::read_dir(folder).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();

        if path.is_file() {
            all_files.push(path);
        }
    }

    all_files.sort();
    Ok(all_files)
}

/// Calculate chunk layout for a list of files
#[allow(dead_code)]
fn calculate_chunk_layout(
    files: &[PathBuf],
    chunk_size: usize,
) -> Result<(Vec<FileChunkMapping>, Vec<ChunkSpec>), String> {
    let mut file_mappings = Vec::new();
    let mut chunk_specs = Vec::new();
    let mut total_bytes_processed = 0u64;

    for file_path in files {
        let file_size = std::fs::metadata(file_path)
            .map_err(|e| format!("Failed to read file metadata: {}", e))?
            .len();

        let start_byte = total_bytes_processed;
        let end_byte = total_bytes_processed + file_size;

        let start_chunk_index = (start_byte / chunk_size as u64) as i32;
        let end_chunk_index = ((end_byte - 1) / chunk_size as u64) as i32;

        file_mappings.push(FileChunkMapping {
            file_path: file_path.clone(),
            start_chunk_index,
            end_chunk_index,
            start_byte_offset: (start_byte % chunk_size as u64) as i64,
            end_byte_offset: ((end_byte - 1) % chunk_size as u64) as i64,
        });

        total_bytes_processed = end_byte;
    }

    // Generate chunk specs for all chunks across all files
    let total_chunks = if total_bytes_processed == 0 {
        0
    } else {
        ((total_bytes_processed - 1) / chunk_size as u64) as i32 + 1
    };

    for chunk_index in 0..total_chunks {
        let chunk_start_byte = chunk_index as u64 * chunk_size as u64;
        let chunk_end_byte =
            ((chunk_index + 1) as u64 * chunk_size as u64).min(total_bytes_processed);
        let this_chunk_size = (chunk_end_byte - chunk_start_byte) as usize;

        // Find which file this chunk starts in
        let mut file_start_byte = 0u64;
        let mut chunk_file_path = None;
        let mut offset_in_file = 0u64;

        for mapping in &file_mappings {
            let file_meta = std::fs::metadata(&mapping.file_path)
                .map_err(|e| format!("Failed to read file metadata: {}", e))?;
            let file_size = file_meta.len();
            let file_end_byte = file_start_byte + file_size;

            if chunk_start_byte >= file_start_byte && chunk_start_byte < file_end_byte {
                chunk_file_path = Some(mapping.file_path.clone());
                offset_in_file = chunk_start_byte - file_start_byte;
                break;
            }

            file_start_byte = file_end_byte;
        }

        if let Some(file_path) = chunk_file_path {
            chunk_specs.push(ChunkSpec {
                chunk_id: Uuid::new_v4().to_string(),
                chunk_index,
                file_path,
                offset: offset_in_file,
                size: this_chunk_size,
            });
        }
    }

    Ok((file_mappings, chunk_specs))
}

/// Build mapping from chunks to tracks for progress tracking
#[allow(dead_code)]
fn build_track_chunk_mapping(
    file_mappings: &[FileChunkMapping],
    track_files: &[TrackSourceFile],
) -> TrackChunkMap {
    let mut file_to_track: HashMap<PathBuf, String> = HashMap::new();
    for track_file in track_files {
        file_to_track.insert(track_file.file_path.clone(), track_file.db_track_id.clone());
    }

    let mut chunk_to_track: HashMap<i32, String> = HashMap::new();
    let mut track_chunk_counts: HashMap<String, usize> = HashMap::new();

    for file_mapping in file_mappings {
        if let Some(track_id) = file_to_track.get(&file_mapping.file_path) {
            let chunk_count =
                (file_mapping.end_chunk_index - file_mapping.start_chunk_index + 1) as usize;

            for chunk_idx in file_mapping.start_chunk_index..=file_mapping.end_chunk_index {
                chunk_to_track.insert(chunk_idx, track_id.clone());
            }

            *track_chunk_counts.entry(track_id.clone()).or_insert(0) += chunk_count;
        }
    }

    TrackChunkMap {
        chunk_to_track,
        track_chunk_counts,
    }
}

/// Check if completing a chunk triggers track completion
#[allow(dead_code)]
fn check_track_completion(
    chunk_index: i32,
    track_chunk_map: &TrackChunkMap,
    completed_chunks: &HashSet<i32>,
) -> Option<String> {
    let track_id = track_chunk_map.chunk_to_track.get(&chunk_index)?;
    let total_for_track = track_chunk_map.track_chunk_counts.get(track_id).copied()?;

    let completed_for_track = track_chunk_map
        .chunk_to_track
        .iter()
        .filter(|(idx, tid)| *tid == track_id && completed_chunks.contains(idx))
        .count();

    if completed_for_track == total_for_track {
        Some(track_id.clone())
    } else {
        None
    }
}

/// Calculate progress percentage
#[allow(dead_code)]
fn calculate_progress(completed: usize, total: usize) -> u8 {
    if total == 0 {
        100
    } else {
        ((completed as f64 / total as f64) * 100.0).min(100.0) as u8
    }
}

// ============================================================================
// Pipeline Stage Functions
// ============================================================================

/// Stage 4: Persist chunk to DB and handle progress tracking
#[allow(dead_code)]
async fn persist_and_track_progress(
    upload_result: Result<UploadedChunk, String>,
    album_id: &str,
    library_manager: &LibraryManager,
    track_chunk_map: &TrackChunkMap,
    completed_chunks: &Arc<Mutex<HashSet<i32>>>,
    progress_tx: &mpsc::UnboundedSender<ImportProgress>,
    total_chunks: usize,
) -> Result<(), String> {
    let uploaded_chunk = upload_result?;

    // Persist chunk to database
    persist_chunk(&uploaded_chunk, album_id, library_manager).await?;

    // Track completion
    let (track_id_opt, progress_update) = {
        let mut completed = completed_chunks.lock().unwrap();
        completed.insert(uploaded_chunk.chunk_index);

        let track_id =
            check_track_completion(uploaded_chunk.chunk_index, track_chunk_map, &completed);

        let percent = calculate_progress(completed.len(), total_chunks);
        (track_id, (completed.len(), percent))
    };

    // Mark track complete if needed
    if let Some(track_id) = track_id_opt {
        library_manager
            .mark_track_complete(&track_id)
            .await
            .map_err(|e| format!("Failed to mark track complete: {}", e))?;
        let _ = progress_tx.send(ImportProgress::TrackComplete {
            album_id: album_id.to_string(),
            track_id,
        });
    }

    // Send progress update
    let _ = progress_tx.send(ImportProgress::ProcessingProgress {
        album_id: album_id.to_string(),
        percent: progress_update.1,
        current: progress_update.0,
        total: total_chunks,
    });

    Ok(())
}

/// Read a chunk from disk
#[allow(dead_code)]
async fn read_chunk(spec: ChunkSpec) -> Result<ChunkData, String> {
    let file = tokio::fs::File::open(&spec.file_path)
        .await
        .map_err(|e| format!("Failed to open file: {}", e))?;

    let mut reader = BufReader::new(file);
    reader
        .seek(std::io::SeekFrom::Start(spec.offset))
        .await
        .map_err(|e| format!("Failed to seek: {}", e))?;

    let mut buffer = vec![0u8; spec.size];
    reader
        .read_exact(&mut buffer)
        .await
        .map_err(|e| format!("Failed to read chunk: {}", e))?;

    let mut hasher = Sha256::new();
    hasher.update(&buffer);
    let checksum = format!("{:x}", hasher.finalize());

    Ok(ChunkData {
        chunk_id: spec.chunk_id,
        chunk_index: spec.chunk_index,
        data: buffer,
        checksum,
    })
}

/// Encrypt a chunk (CPU-bound, should be called from spawn_blocking)
#[allow(dead_code)]
fn encrypt_chunk_blocking(
    chunk_data: ChunkData,
    encryption_service: &EncryptionService,
) -> Result<EncryptedChunkData, String> {
    let (ciphertext, nonce) = encryption_service
        .encrypt(&chunk_data.data)
        .map_err(|e| format!("Encryption failed: {}", e))?;

    // Create EncryptedChunk and serialize to bytes (includes nonce and metadata)
    let encrypted_chunk =
        crate::encryption::EncryptedChunk::new(ciphertext, nonce, "master".to_string());
    let encrypted_bytes = encrypted_chunk.to_bytes();

    Ok(EncryptedChunkData {
        chunk_id: chunk_data.chunk_id,
        chunk_index: chunk_data.chunk_index,
        original_size: chunk_data.data.len(),
        encrypted_data: encrypted_bytes,
        checksum: chunk_data.checksum,
    })
}

/// Upload encrypted chunk to cloud storage
#[allow(dead_code)]
async fn upload_chunk(
    encrypted_chunk: EncryptedChunkData,
    cloud_storage: &CloudStorageManager,
) -> Result<UploadedChunk, String> {
    let cloud_location = cloud_storage
        .upload_chunk_data(&encrypted_chunk.chunk_id, &encrypted_chunk.encrypted_data)
        .await
        .map_err(|e| format!("Upload failed: {}", e))?;

    Ok(UploadedChunk {
        chunk_id: encrypted_chunk.chunk_id,
        chunk_index: encrypted_chunk.chunk_index,
        original_size: encrypted_chunk.original_size,
        encrypted_size: encrypted_chunk.encrypted_data.len(),
        checksum: encrypted_chunk.checksum,
        cloud_location,
    })
}

/// Persist chunk to database
#[allow(dead_code)]
async fn persist_chunk(
    chunk: &UploadedChunk,
    album_id: &str,
    library_manager: &LibraryManager,
) -> Result<(), String> {
    let db_chunk = crate::database::DbChunk::from_album_chunk(
        &chunk.chunk_id,
        album_id,
        chunk.chunk_index,
        chunk.original_size,
        chunk.encrypted_size,
        &chunk.checksum,
        &chunk.cloud_location,
        false,
    );

    library_manager
        .add_chunk(&db_chunk)
        .await
        .map_err(|e| format!("Failed to add chunk: {}", e))
}
