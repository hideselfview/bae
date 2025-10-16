// # Import Service
//
// Single-instance queue-based service that imports albums sequentially.
// One worker task handles import requests one at a time from a queue.
//
// Flow:
// 1. Validation & Queueing (synchronous per request):
//    - Validate track-to-file mapping
//    - Insert album/tracks with status='queued'
//    - Returns immediately so next request can be validated
//
// 2. Pipeline Execution (async per import):
//    - Mark album as 'importing'
//    - Streaming pipeline: read → encrypt → upload → persist (bounded parallelism)
//    - Mark album/tracks as 'complete'
//
// Architecture:
// - TrackFileMapper: Validates track-to-file mapping before DB insertion
// - MetadataPersister: Saves file/chunk metadata to DB
// - ProgressService: Broadcasts real-time progress updates to UI subscribers

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
use futures::stream::StreamExt;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, BufReader};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

/// Handle for sending import requests and subscribing to progress updates
#[derive(Clone)]
pub struct ImportHandle {
    validated_tx: mpsc::UnboundedSender<ValidatedImport>,
    progress_service: ImportProgressService,
    library_manager: SharedLibraryManager,
}

impl ImportHandle {
    /// Validate and queue an import request.
    ///
    /// Performs validation (track-to-file mapping) and DB insertion synchronously.
    /// If validation fails, returns error immediately with no side effects.
    /// If successful, album is inserted with status='queued' and sent to import worker.
    pub async fn send_request(&self, request: ImportRequest) -> Result<(), String> {
        match request {
            ImportRequest::FromFolder { album, folder } => {
                let library_manager = self.library_manager.get();

                // ========== VALIDATION (before queueing) ==========

                // 1. Create album + track records
                let (db_album, db_tracks) = create_album_and_tracks(&album)?;

                // 2. Discover files
                let folder_files = discover_folder_files(&folder)?;

                // 3. Validate track-to-file mapping
                let tracks_to_files =
                    TrackFileMapper::map_tracks_to_files(&db_tracks, &folder_files).await?;

                // 4. Insert album + tracks with status='queued'
                library_manager
                    .insert_album_with_tracks(&db_album, &db_tracks)
                    .await
                    .map_err(|e| format!("Database error: {}", e))?;

                println!(
                    "ImportHandle: Validated and queued album '{}' with {} tracks",
                    db_album.title,
                    db_tracks.len()
                );

                // ========== QUEUE FOR PIPELINE ==========

                self.validated_tx
                    .send(ValidatedImport {
                        db_album,
                        tracks_to_files,
                        folder_files,
                    })
                    .map_err(|_| "Failed to queue validated album for import".to_string())?;

                Ok(())
            }
        }
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
#[derive(Clone)]
pub struct ImportConfig {
    /// Number of parallel encryption workers (CPU-bound, typically 2x CPU cores)
    pub max_encrypt_workers: usize,
    /// Number of parallel upload workers (I/O-bound)
    pub max_upload_workers: usize,
    /// Size of each chunk in bytes
    pub chunk_size_bytes: usize,
}

/// Validated import ready for pipeline execution
struct ValidatedImport {
    db_album: DbAlbum,
    tracks_to_files: Vec<TrackSourceFile>,
    folder_files: Vec<DiscoveredFile>,
}

/// Import service that orchestrates the import workflow on the shared runtime
pub struct ImportService {
    library_manager: SharedLibraryManager,
    encryption_service: EncryptionService,
    cloud_storage: CloudStorageManager,
    progress_tx: mpsc::UnboundedSender<ImportProgress>,
    validated_rx: mpsc::UnboundedReceiver<ValidatedImport>,
    config: ImportConfig,
}

impl ImportService {
    /// Start the single import service worker for the entire app.
    ///
    /// Creates one worker task that imports validated albums sequentially from a queue.
    /// Multiple imports will be queued and handled one at a time, not concurrently.
    /// Returns a handle that can be cloned and used throughout the app to submit import requests.
    pub fn start(
        runtime_handle: tokio::runtime::Handle,
        library_manager: SharedLibraryManager,
        encryption_service: EncryptionService,
        cloud_storage: CloudStorageManager,
        config: ImportConfig,
    ) -> ImportHandle {
        let (validated_tx, validated_rx) = mpsc::unbounded_channel();
        let (progress_tx, progress_rx) = mpsc::unbounded_channel();

        // Create the import service worker
        let service = ImportService {
            library_manager: library_manager.clone(),
            encryption_service,
            cloud_storage,
            progress_tx,
            validated_rx,
            config,
        };

        // Spawn worker task that imports validated albums sequentially
        runtime_handle.spawn(service.run_import_worker());

        // Create ProgressService, used to broadcast progress updates to external subscribers
        let progress_service = ImportProgressService::new(progress_rx, runtime_handle.clone());

        ImportHandle {
            validated_tx,
            progress_service,
            library_manager,
        }
    }

    async fn run_import_worker(mut self) {
        println!("ImportService: Worker started");

        // Import validated albums sequentially from the queue.
        loop {
            match self.validated_rx.recv().await {
                Some(validated) => {
                    println!(
                        "ImportService: Starting pipeline for '{}'",
                        validated.db_album.title
                    );

                    if let Err(e) = self.import_from_folder(validated).await {
                        println!("ImportService: Pipeline failed: {}", e);
                        // TODO: Mark album as failed
                    }
                }
                None => {
                    println!("ImportService: Channel closed");
                    break;
                }
            }
        }
    }

    /// Executes the streaming import pipeline for a validated album.
    ///
    /// Orchestrates the entire import workflow:
    /// 1. Marks the album as 'importing'
    /// 2. Calculates chunk layout and track progress
    /// 3. Streams files → encrypts → uploads → persists
    /// 4. Persists metadata and marks album complete.
    async fn import_from_folder(&self, validated: ValidatedImport) -> Result<(), String> {
        let library_manager = self.library_manager.get();

        let ValidatedImport {
            db_album,
            tracks_to_files,
            folder_files,
        } = validated;

        // Mark album as importing now that pipeline is starting
        library_manager
            .mark_album_importing(&db_album.id)
            .await
            .map_err(|e| format!("Failed to mark album as importing: {}", e))?;

        println!("ImportService: Marked album as 'importing' - starting pipeline");

        // Send started progress
        let _ = self.progress_tx.send(ImportProgress::Started {
            album_id: db_album.id.clone(),
        });

        // Calculate chunk layout from discovered files (no additional filesystem calls)
        let (file_mappings, total_chunks) =
            calculate_file_mappings_from_discovered(&folder_files, self.config.chunk_size_bytes)?;

        println!(
            "ImportService: Will stream {} chunks across {} files",
            total_chunks,
            file_mappings.len()
        );

        // Build track mapping for progress tracking
        let progress_tracker = build_track_progress_tracker(&file_mappings, &tracks_to_files);

        // ========== STREAMING PIPELINE ==========
        // Read → Encrypt → Upload → Persist (bounded parallelism at each stage)

        // Shared state for tracking progress
        let completed_chunks = Arc::new(Mutex::new(HashSet::new()));

        let db_album_id = db_album.id.clone();
        let encryption_service = self.encryption_service.clone();
        let cloud_storage = self.cloud_storage.clone();
        let max_encrypt_workers = self.config.max_encrypt_workers;
        let max_upload_workers = self.config.max_upload_workers;
        let progress_tx = self.progress_tx.clone();
        let progress_tracker = Arc::new(progress_tracker);

        // Stage 1: Read files and stream chunks (bounded channel for backpressure)
        let chunk_size_bytes = self.config.chunk_size_bytes;
        let (chunk_tx, chunk_rx) = mpsc::channel::<Result<ChunkData, String>>(10);

        // Spawn reader task that streams chunks as they're produced
        tokio::spawn(stream_files_into_chunks(
            folder_files.clone(),
            chunk_size_bytes,
            chunk_tx,
        ));

        // Stage 2: Consume chunks from channel and encrypt (bounded CPU via spawn_blocking)
        let results = ReceiverStream::new(chunk_rx)
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
                let progress_tracker = progress_tracker.clone();
                let completed_chunks = completed_chunks.clone();
                let progress_tx = progress_tx.clone();

                async move {
                    persist_and_track_progress(
                        upload_result,
                        &album_id,
                        &library_manager,
                        &progress_tracker,
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
            "ImportService: All {} chunks uploaded successfully",
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

/// Create album and track records from Discogs metadata.
///
/// Combines album creation (extracting artist name) with track creation,
/// ensuring the album_id is available for track linkage.
/// All records start with status='queued'.
fn create_album_and_tracks(import_item: &DiscogsAlbum) -> Result<(DbAlbum, Vec<DbTrack>), String> {
    let artist_name = import_item.extract_artist_name();

    // Create album record
    let album = match import_item {
        DiscogsAlbum::Master(master) => DbAlbum::from_discogs_master(master, &artist_name),
        DiscogsAlbum::Release(release) => DbAlbum::from_discogs_release(release, &artist_name),
    };

    // Create track records linked to this album
    let discogs_tracks = import_item.tracklist();
    let mut tracks = Vec::new();

    for discogs_track in discogs_tracks.iter() {
        let track = DbTrack::from_discogs_track(discogs_track, &album.id)?;
        tracks.push(track);
    }

    Ok((album, tracks))
}

// ============================================================================
// Stream-based Pipeline Data Structures
// ============================================================================

/// File discovered during initial filesystem scan.
///
/// Created during validation phase when we traverse the album folder once.
/// Used to calculate chunk layout and feed the reader task.
///
/// Example: `{ path: "/music/album/track01.flac", size: 45_821_345 }`
#[derive(Clone)]
pub struct DiscoveredFile {
    pub path: PathBuf,
    pub size: u64,
}

/// Raw chunk data read from disk.
///
/// Stage 1 output: Reader task produces these by reading files sequentially
/// and packing bytes into fixed-size chunks.
///
/// Example: `{ chunk_id: "uuid-123", chunk_index: 0, data: [1MB of bytes] }`
struct ChunkData {
    chunk_id: String,
    chunk_index: i32,
    data: Vec<u8>,
}

/// Encrypted chunk data ready for upload.
///
/// Stage 2 output: Encryption workers produce these by encrypting ChunkData
/// via spawn_blocking. The encrypted_data includes AES-256-GCM ciphertext,
/// nonce, and authentication tag.
///
/// Example: `{ chunk_id: "uuid-123", chunk_index: 0, encrypted_data: [1.01MB encrypted bytes] }`
struct EncryptedChunkData {
    chunk_id: String,
    chunk_index: i32,
    encrypted_data: Vec<u8>,
}

/// Chunk successfully uploaded to cloud storage.
///
/// Stage 3 output: Upload workers produce these after successfully uploading
/// to S3. The cloud_location is the full S3 URI used to retrieve this chunk
/// during playback.
///
/// Example: `{ chunk_id: "abc123...", chunk_index: 0, encrypted_size: 1049000, cloud_location: "s3://bucket/chunks/ab/c1/abc123....enc" }`
struct UploadedChunk {
    chunk_id: String,
    chunk_index: i32,
    encrypted_size: usize,
    cloud_location: String,
}

/// Tracks which chunks belong to which tracks for progress updates.
///
/// Built before pipeline starts by mapping file ranges to chunk indices.
/// Used in Stage 4 to determine when a track is complete (all its chunks uploaded)
/// so we can send TrackComplete progress events and mark tracks as complete in DB.
///
/// Example:
/// ```
/// chunk_to_track: { 0 -> "track-id-1", 1 -> "track-id-1", 2 -> "track-id-2", ... }
/// track_chunk_counts: { "track-id-1" -> 2, "track-id-2" -> 3, ... }
/// ```
struct TrackProgressTracker {
    chunk_to_track: HashMap<i32, String>,
    track_chunk_counts: HashMap<String, usize>,
}

// ============================================================================
// Pipeline Helper Functions
// ============================================================================

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

/// Calculate file mappings and total chunk count from already-discovered files.
///
/// Treats all files as a single concatenated byte stream, divided into fixed-size chunks.
/// Each file mapping records which chunks it spans and byte offsets within those chunks.
/// This enables efficient streaming: open each file once, read its chunks sequentially.
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

/// Build progress tracker for tracks during import.
///
/// Creates reverse mappings from chunks to tracks so we can:
/// 1. Identify which track a chunk belongs to when it completes
/// 2. Count how many chunks each track needs to mark it complete
///
/// This enables progressive UI updates as tracks finish, rather than waiting for the entire album.
fn build_track_progress_tracker(
    file_mappings: &[FileChunkMapping],
    track_files: &[TrackSourceFile],
) -> TrackProgressTracker {
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

    TrackProgressTracker {
        chunk_to_track,
        track_chunk_counts,
    }
}

/// Check if completing a chunk triggers track completion.
///
/// Called after each chunk upload to see if all chunks for a track are done.
/// Returns the track_id if complete, allowing us to mark it playable immediately.
fn check_track_completion(
    chunk_index: i32,
    progress_tracker: &TrackProgressTracker,
    completed_chunks: &HashSet<i32>,
) -> Option<String> {
    let track_id = progress_tracker.chunk_to_track.get(&chunk_index)?;
    let total_for_track = progress_tracker.track_chunk_counts.get(track_id).copied()?;

    let completed_for_track = progress_tracker
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

/// Stage 4: Persist chunk to DB and handle progress tracking.
///
/// Final stage of the pipeline. Saves chunk metadata to DB and emits progress events.
/// Checks if this chunk completes a track, marking it playable and emitting TrackComplete.
/// This is where the streaming pipeline meets the database and UI.
async fn persist_and_track_progress(
    upload_result: Result<UploadedChunk, String>,
    album_id: &str,
    library_manager: &LibraryManager,
    progress_tracker: &TrackProgressTracker,
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
            check_track_completion(uploaded_chunk.chunk_index, progress_tracker, &completed);

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

/// Read files sequentially and stream chunks as they're produced.
///
/// Treats all files as a concatenated byte stream, dividing it into fixed-size chunks.
/// Chunks are sent to the channel as soon as they're full, allowing downstream
/// encryption and upload to start immediately without buffering the entire album.
///
/// Files don't align to chunk boundaries - a chunk may contain data from multiple files.
async fn stream_files_into_chunks(
    files: Vec<DiscoveredFile>,
    chunk_size: usize,
    chunk_tx: mpsc::Sender<Result<ChunkData, String>>,
) {
    let mut current_chunk_buffer = Vec::with_capacity(chunk_size);
    let mut current_chunk_index = 0i32;

    for file in files {
        let file_handle = match tokio::fs::File::open(&file.path).await {
            Ok(f) => f,
            Err(e) => {
                let _ = chunk_tx
                    .send(Err(format!("Failed to open file {:?}: {}", file.path, e)))
                    .await;
                return;
            }
        };

        let mut reader = BufReader::new(file_handle);

        loop {
            let space_remaining = chunk_size - current_chunk_buffer.len();
            let mut temp_buffer = vec![0u8; space_remaining];

            let bytes_read = match reader.read(&mut temp_buffer).await {
                Ok(n) => n,
                Err(e) => {
                    let _ = chunk_tx
                        .send(Err(format!("Failed to read from file: {}", e)))
                        .await;
                    return;
                }
            };

            if bytes_read == 0 {
                // EOF - move to next file
                break;
            }

            // Add the bytes we read to current chunk
            current_chunk_buffer.extend_from_slice(&temp_buffer[..bytes_read]);

            // If chunk is full, send it and start a new one
            if current_chunk_buffer.len() == chunk_size {
                let chunk = finalize_chunk(current_chunk_index, current_chunk_buffer);
                if chunk_tx.send(Ok(chunk)).await.is_err() {
                    // Receiver dropped, stop reading
                    return;
                }
                current_chunk_index += 1;
                current_chunk_buffer = Vec::with_capacity(chunk_size);
            }
        }
    }

    // Send final partial chunk if any data remains
    if !current_chunk_buffer.is_empty() {
        let chunk = finalize_chunk(current_chunk_index, current_chunk_buffer);
        let _ = chunk_tx.send(Ok(chunk)).await;
    }
}

/// Finalize a chunk by creating ChunkData with a unique ID.
fn finalize_chunk(chunk_index: i32, data: Vec<u8>) -> ChunkData {
    ChunkData {
        chunk_id: Uuid::new_v4().to_string(),
        chunk_index,
        data,
    }
}

/// Encrypt a chunk using AES-256-GCM.
///
/// CPU-bound operation called from spawn_blocking to avoid starving async I/O.
/// Wraps encrypted data with nonce and authentication tag, ready for cloud upload.
fn encrypt_chunk_blocking(
    chunk_data: ChunkData,
    encryption_service: &EncryptionService,
) -> Result<EncryptedChunkData, String> {
    let (ciphertext, nonce) = encryption_service
        .encrypt(&chunk_data.data)
        .map_err(|e| format!("Encryption failed: {}", e))?;

    // Create EncryptedChunk and serialize to bytes (includes nonce and authentication tag)
    let encrypted_chunk =
        crate::encryption::EncryptedChunk::new(ciphertext, nonce, "master".to_string());
    let encrypted_bytes = encrypted_chunk.to_bytes();

    Ok(EncryptedChunkData {
        chunk_id: chunk_data.chunk_id,
        chunk_index: chunk_data.chunk_index,
        encrypted_data: encrypted_bytes,
    })
}

/// Upload encrypted chunk to cloud storage.
///
/// I/O-bound operation that sends encrypted data to S3 or equivalent.
/// Returns cloud location for database storage and later retrieval.
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
        encrypted_size: encrypted_chunk.encrypted_data.len(),
        cloud_location,
    })
}

/// Persist chunk metadata to database.
///
/// Stores chunk ID, encrypted size, and cloud location for later retrieval.
/// This creates the link between our database and cloud storage.
/// Integrity is guaranteed by AES-GCM's authentication tag - no separate checksum needed.
async fn persist_chunk(
    chunk: &UploadedChunk,
    album_id: &str,
    library_manager: &LibraryManager,
) -> Result<(), String> {
    let db_chunk = crate::database::DbChunk::from_album_chunk(
        &chunk.chunk_id,
        album_id,
        chunk.chunk_index,
        chunk.encrypted_size,
        &chunk.cloud_location,
        false,
    );

    library_manager
        .add_chunk(&db_chunk)
        .await
        .map_err(|e| format!("Failed to add chunk: {}", e))
}
