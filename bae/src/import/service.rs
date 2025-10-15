// # Import Service - Orchestrator
//
// This module contains the thin orchestrator that coordinates specialized services:
// - TrackFileMapper: Validates track-to-file mapping
// - UploadPipeline: Chunks and uploads to cloud
// - MetadataPersister: Saves file/chunk metadata to DB
//
// The orchestrator's job is to call these services in the right order and handle
// progress reporting to the UI.

use crate::chunking::FileChunkMapping;
use crate::cloud_storage::CloudStorageManager;
use crate::database::{DbAlbum, DbTrack};
use crate::encryption::EncryptionService;
use crate::import::metadata_persister::MetadataPersister;
use crate::import::progress_service::ImportProgressService;
use crate::import::track_file_mapper::TrackFileMapper;
use crate::import::types::{ImportProgress, ImportRequest, TrackSourceFile};
use crate::library::LibraryManager;
use crate::library_context::SharedLibraryManager;
use crate::models::DiscogsAlbum;
use futures::stream::{self, StreamExt};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, BufReader};
use tokio::sync::mpsc;
use uuid::Uuid;

/// Handle for sending import requests and subscribing to progress updates
#[derive(Clone)]
pub struct ImportHandle {
    request_tx: mpsc::UnboundedSender<ImportRequest>,
    progress_service: ImportProgressService,
}

impl ImportHandle {
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

/// Configuration for import service
pub struct ImportConfig {
    /// Number of parallel encryption workers (CPU-bound, typically 2x CPU cores)
    pub max_encrypt_workers: usize,
    /// Number of parallel upload workers (I/O-bound)
    pub max_upload_workers: usize,
    /// Size of each chunk in bytes
    pub chunk_size_bytes: usize,
}

/// Import service that orchestrates the import workflow on the shared runtime
pub struct ImportService {
    library_manager: SharedLibraryManager,
    encryption_service: EncryptionService,
    cloud_storage: CloudStorageManager,
    progress_tx: mpsc::UnboundedSender<ImportProgress>,
    request_rx: mpsc::UnboundedReceiver<ImportRequest>,
    config: ImportConfig,
}

impl ImportService {
    /// Start import service worker, returning handle for sending requests
    pub fn start(
        runtime_handle: tokio::runtime::Handle,
        library_manager: SharedLibraryManager,
        encryption_service: EncryptionService,
        cloud_storage: CloudStorageManager,
        config: ImportConfig,
    ) -> ImportHandle {
        let (request_tx, request_rx) = mpsc::unbounded_channel();
        let (progress_tx, progress_rx) = mpsc::unbounded_channel();

        // Create service instance for worker task
        let service = ImportService {
            library_manager,
            encryption_service,
            cloud_storage,
            progress_tx,
            request_rx,
            config,
        };

        // Spawn import worker task on shared runtime
        runtime_handle.spawn(service.listen_for_import_requests());

        // Create ProgressService, used to broadcast progress updates to external subscribers
        let progress_service = ImportProgressService::new(progress_rx, runtime_handle.clone());

        ImportHandle {
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

        // ========== SETUP ==========

        // 1. Create album + track records (in memory only)
        let db_album = create_album_record(discogs_album)?;
        let db_tracks = create_track_records(discogs_album, &db_album.id)?;

        println!(
            "ImportService: Created album record with {} tracks (not inserted yet)",
            db_tracks.len()
        );

        // 2. Discover all files in folder (single filesystem traversal with metadata)
        let folder_files = discover_folder_files(folder)?;

        println!(
            "ImportService: Found {} files in album folder",
            folder_files.len()
        );

        // 3. Build track-to-file mapping using already-discovered files.
        // We compute this early as a validation step to ensure we have
        // source audio data for all tracks before proceeding.
        let tracks_to_files =
            TrackFileMapper::map_tracks_to_files(&db_tracks, &folder_files).await?;

        println!(
            "ImportService: Successfully mapped {} tracks to source files",
            tracks_to_files.len()
        );

        // 4. Insert album + tracks into database (status='importing')
        library_manager
            .insert_album_with_tracks(&db_album, &db_tracks)
            .await
            .map_err(|e| format!("Database error: {}", e))?;

        println!(
            "ImportService: Inserted album and {} tracks into database with status='importing'",
            db_tracks.len()
        );

        // 5. Send started progress
        let _ = self.progress_tx.send(ImportProgress::Started {
            album_id: db_album.id.clone(),
        });

        // 6. Calculate chunk layout from discovered files (no additional filesystem calls)
        let (file_mappings, total_chunks) =
            calculate_file_mappings_from_discovered(&folder_files, self.config.chunk_size_bytes)?;

        println!(
            "ImportService: Will process {} chunks across {} files",
            total_chunks,
            file_mappings.len()
        );

        // 6. Build track mapping for progress tracking
        let track_chunk_map = build_track_chunk_mapping(&file_mappings, &tracks_to_files);

        // ========== PIPELINE ==========

        // Shared state for tracking progress
        let completed_chunks = Arc::new(Mutex::new(HashSet::new()));

        let db_album_id = db_album.id.clone();
        let encryption_service = self.encryption_service.clone();
        let cloud_storage = self.cloud_storage.clone();
        let max_encrypt_workers = self.config.max_encrypt_workers;
        let max_upload_workers = self.config.max_upload_workers;
        let progress_tx = self.progress_tx.clone();
        let track_chunk_map = Arc::new(track_chunk_map);

        // Stage 1: Read chunks from files (each file opened once, chunks read sequentially)
        let chunk_size_bytes = self.config.chunk_size_bytes;
        let results = stream::iter(file_mappings.clone())
            .then(move |file_mapping| async move {
                read_chunks_from_file(file_mapping, chunk_size_bytes).await
            })
            .flat_map(|chunks_result| {
                stream::iter(match chunks_result {
                    Ok(chunks) => chunks.into_iter().map(Ok).collect(),
                    Err(e) => vec![Err(e)],
                })
            })
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
                let album_id = db_album_id.clone();
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
            .persist_album_metadata(
                &tracks_to_files,
                &file_mappings,
                self.config.chunk_size_bytes,
            )
            .await?;

        // Mark album complete
        library_manager
            .mark_album_complete(&db_album.id)
            .await
            .map_err(|e| format!("Failed to mark album complete: {}", e))?;

        // Send completion event
        let _ = self.progress_tx.send(ImportProgress::Complete {
            album_id: db_album.id,
        });

        println!(
            "ImportService: Import completed successfully for {}",
            db_album.title
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
// ============================================================================

/// File discovered during initial filesystem scan
pub struct DiscoveredFile {
    pub path: PathBuf,
    pub size: u64,
}

/// Raw chunk data read from disk with checksum
struct ChunkData {
    chunk_id: String,
    chunk_index: i32,
    data: Vec<u8>,
    checksum: String,
}

/// Encrypted chunk data ready for upload
struct EncryptedChunkData {
    chunk_id: String,
    chunk_index: i32,
    original_size: usize,
    encrypted_data: Vec<u8>,
    checksum: String,
}

/// Chunk successfully uploaded to cloud storage
struct UploadedChunk {
    chunk_id: String,
    chunk_index: i32,
    original_size: usize,
    encrypted_size: usize,
    checksum: String,
    cloud_location: String,
}

/// Mapping of chunks to tracks for progress tracking
struct TrackChunkMap {
    chunk_to_track: HashMap<i32, String>,
    track_chunk_counts: HashMap<String, usize>,
}

// ============================================================================
// Pipeline Helper Functions
// ============================================================================

/// Discover all files in folder with metadata (single filesystem traversal)
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

/// Calculate file mappings and total chunk count from already-discovered files
fn calculate_file_mappings_from_discovered(
    files: &[DiscoveredFile],
    chunk_size: usize,
) -> Result<(Vec<FileChunkMapping>, usize), String> {
    let mut file_mappings = Vec::new();
    let mut total_bytes_processed = 0u64;

    for file in files {
        let start_byte = total_bytes_processed;
        let end_byte = total_bytes_processed + file.size;

        let start_chunk_index = (start_byte / chunk_size as u64) as i32;
        let end_chunk_index = ((end_byte - 1) / chunk_size as u64) as i32;

        file_mappings.push(FileChunkMapping {
            file_path: file.path.clone(),
            start_chunk_index,
            end_chunk_index,
            start_byte_offset: (start_byte % chunk_size as u64) as i64,
            end_byte_offset: ((end_byte - 1) % chunk_size as u64) as i64,
        });

        total_bytes_processed = end_byte;
    }

    let total_chunks = if total_bytes_processed == 0 {
        0
    } else {
        ((total_bytes_processed - 1) / chunk_size as u64) as usize + 1
    };

    Ok((file_mappings, total_chunks))
}

/// Build mapping from chunks to tracks for progress tracking
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

/// Read all chunks from a single file (open once, read sequentially)
async fn read_chunks_from_file(
    file_mapping: FileChunkMapping,
    chunk_size: usize,
) -> Result<Vec<ChunkData>, String> {
    let file = tokio::fs::File::open(&file_mapping.file_path)
        .await
        .map_err(|e| format!("Failed to open file {:?}: {}", file_mapping.file_path, e))?;

    let mut reader = BufReader::new(file);
    let mut chunks = Vec::new();

    for chunk_index in file_mapping.start_chunk_index..=file_mapping.end_chunk_index {
        let mut buffer = vec![0u8; chunk_size];
        let bytes_read = reader
            .read(&mut buffer)
            .await
            .map_err(|e| format!("Failed to read chunk: {}", e))?;

        buffer.truncate(bytes_read);

        let mut hasher = Sha256::new();
        hasher.update(&buffer);
        let checksum = format!("{:x}", hasher.finalize());

        chunks.push(ChunkData {
            chunk_id: Uuid::new_v4().to_string(),
            chunk_index,
            data: buffer,
            checksum,
        });
    }

    Ok(chunks)
}

/// Encrypt a chunk (CPU-bound, should be called from spawn_blocking)
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
