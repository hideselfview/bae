// Streaming Import Pipeline
//
// Three-stage pipeline for album import: Read → Encrypt → Upload
//
// Each stage runs with bounded parallelism via channels:
// - Reader: Sequential file reading, streaming chunks to encryption
// - Encryption: Parallel CPU-bound work via spawn_blocking
// - Upload: Parallel I/O-bound S3 uploads
// - Persistence: DB writes and progress tracking
//
// The pipeline ensures bounded memory usage and fail-fast error handling.

#[cfg(test)]
mod tests;

#[cfg(test)]
mod chunk_reader_test;

use crate::cloud_storage::CloudStorageManager;
use crate::database::DbChunk;
use crate::encryption::{EncryptedChunk, EncryptionService};
use crate::import::album_layout::TrackProgressTracker;
use crate::import::service::{DiscoveredFile, ImportConfig};
use crate::import::types::ImportProgress;
use crate::library::LibraryManager;
use futures::stream::{Stream, StreamExt};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, BufReader};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

// ============================================================================
// Public API
// ============================================================================

/// Build the complete streaming pipeline: read → encrypt → upload → persist.
///
/// Returns a stream of results, one per chunk processed. Caller drives the stream
/// by collecting or folding. This is the only public function in this module.
/// All pipeline stages and data structures are private implementation details.
///
/// Using `impl Stream` keeps everything stack-allocated with static dispatch,
/// avoiding heap allocation and virtual calls that `BoxStream` would require.
#[allow(clippy::too_many_arguments)]
pub(super) fn build_pipeline(
    folder_files: Vec<DiscoveredFile>,
    config: ImportConfig,
    album_id: String,
    encryption_service: EncryptionService,
    cloud_storage: CloudStorageManager,
    library_manager: LibraryManager,
    progress_tracker: Arc<TrackProgressTracker>,
    progress_tx: mpsc::UnboundedSender<ImportProgress>,
    total_chunks: usize,
) -> impl Stream<Item = Result<(), String>> {
    // Shared state for tracking progress
    let completed_chunks = Arc::new(Mutex::new(HashSet::new()));
    let completed_tracks = Arc::new(Mutex::new(HashSet::new()));

    // Stage 1: Read files and stream chunks (bounded channel for backpressure)
    let (chunk_tx, chunk_rx) = mpsc::channel::<Result<ChunkData, String>>(10);

    // Spawn reader task that streams chunks as they're produced
    tokio::spawn(produce_chunk_stream(
        folder_files,
        config.chunk_size_bytes,
        chunk_tx,
    ));

    // Stage 2: Consume chunks from channel and encrypt (bounded CPU via spawn_blocking)
    ReceiverStream::new(chunk_rx)
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
        .buffer_unordered(config.max_encrypt_workers)
        // Stage 3: Upload chunks (bounded I/O)
        .map(move |encrypted_result| {
            let cloud_storage = cloud_storage.clone();
            async move {
                let encrypted = encrypted_result?;
                upload_chunk(encrypted, &cloud_storage).await
            }
        })
        .buffer_unordered(config.max_upload_workers)
        // Stage 4: Persist to DB and handle progress
        .map(move |upload_result| {
            let album_id = album_id.clone();
            let library_manager = library_manager.clone();
            let progress_tracker = progress_tracker.clone();
            let completed_chunks = completed_chunks.clone();
            let completed_tracks = completed_tracks.clone();
            let progress_tx = progress_tx.clone();

            async move {
                persist_and_track_progress(
                    upload_result,
                    &album_id,
                    &library_manager,
                    &progress_tracker,
                    &completed_chunks,
                    &completed_tracks,
                    &progress_tx,
                    total_chunks,
                )
                .await
            }
        })
        .buffer_unordered(10) // Allow some parallelism for DB writes
}

// ============================================================================
// Pipeline Data Structures
// ============================================================================

/// Raw chunk data read from disk.
///
/// Stage 1 output: Reader task produces these by reading files sequentially
/// and packing bytes into fixed-size chunks.
///
/// Example: `{ chunk_id: "uuid-123", chunk_index: 0, data: [1MB of bytes] }`
pub(super) struct ChunkData {
    pub(super) chunk_id: String,
    pub(super) chunk_index: i32,
    pub(super) data: Vec<u8>,
}

/// Encrypted chunk data ready for upload.
///
/// Stage 2 output: Encryption workers produce these by encrypting ChunkData
/// via spawn_blocking. The encrypted_data includes AES-256-GCM ciphertext,
/// nonce, and authentication tag.
///
/// Example: `{ chunk_id: "uuid-123", chunk_index: 0, encrypted_data: [1.01MB encrypted bytes] }`
pub(super) struct EncryptedChunkData {
    pub(super) chunk_id: String,
    pub(super) chunk_index: i32,
    pub(super) encrypted_data: Vec<u8>,
}

/// Chunk successfully uploaded to cloud storage.
///
/// Stage 3 output: Upload workers produce these after successfully uploading
/// to S3. The cloud_location is the full S3 URI used to retrieve this chunk
/// during playback.
///
/// Example: `{ chunk_id: "abc123...", chunk_index: 0, encrypted_size: 1049000, cloud_location: "s3://bucket/chunks/ab/c1/abc123....enc" }`
pub(super) struct UploadedChunk {
    pub(super) chunk_id: String,
    pub(super) chunk_index: i32,
    pub(super) encrypted_size: usize,
    pub(super) cloud_location: String,
}

// ============================================================================
// Pipeline Stage Functions
// ============================================================================

/// Read files sequentially and stream chunks as they're produced.
///
/// Treats all files as a concatenated byte stream, dividing it into fixed-size chunks.
/// Chunks are sent to the channel as soon as they're full, allowing downstream
/// encryption and upload to start immediately without buffering the entire album.
///
/// Files don't align to chunk boundaries - a chunk may contain data from multiple files.
pub(super) async fn produce_chunk_stream(
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
pub(super) fn finalize_chunk(chunk_index: i32, data: Vec<u8>) -> ChunkData {
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
pub(super) fn encrypt_chunk_blocking(
    chunk_data: ChunkData,
    encryption_service: &EncryptionService,
) -> Result<EncryptedChunkData, String> {
    let (ciphertext, nonce) = encryption_service
        .encrypt(&chunk_data.data)
        .map_err(|e| format!("Encryption failed: {}", e))?;

    // Create EncryptedChunk and serialize to bytes (includes nonce and authentication tag)
    let encrypted_chunk = EncryptedChunk::new(ciphertext, nonce, "master".to_string());
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
pub(super) async fn upload_chunk(
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
pub(super) async fn persist_chunk(
    chunk: &UploadedChunk,
    album_id: &str,
    library_manager: &LibraryManager,
) -> Result<(), String> {
    let db_chunk = DbChunk::from_album_chunk(
        album_id,
        &chunk.chunk_id,
        chunk.chunk_index,
        chunk.encrypted_size,
        &chunk.cloud_location,
    );

    library_manager
        .add_chunk(&db_chunk)
        .await
        .map_err(|e| format!("Failed to add chunk: {}", e))
}

// ============================================================================
// Progress Tracking
// ============================================================================

/// Stage 4: Persist chunk to DB and handle progress tracking.
///
/// Final stage of the pipeline. Saves chunk metadata to DB and emits progress events.
/// Checks if this chunk completes a track, marking it playable and emitting TrackComplete.
/// This is where the streaming pipeline meets the database and UI.
#[allow(clippy::too_many_arguments)]
pub(super) async fn persist_and_track_progress(
    upload_result: Result<UploadedChunk, String>,
    album_id: &str,
    library_manager: &LibraryManager,
    progress_tracker: &TrackProgressTracker,
    completed_chunks: &Arc<Mutex<HashSet<i32>>>,
    completed_tracks: &Arc<Mutex<HashSet<String>>>,
    progress_tx: &mpsc::UnboundedSender<ImportProgress>,
    total_chunks: usize,
) -> Result<(), String> {
    let uploaded_chunk = upload_result?;

    // Persist chunk to database
    persist_chunk(&uploaded_chunk, album_id, library_manager).await?;

    // Track completion - check ALL tracks since buffer_unordered means any track could complete
    let (newly_completed_tracks, progress_update) = {
        let mut completed = completed_chunks.lock().unwrap();
        let mut already_completed = completed_tracks.lock().unwrap();

        completed.insert(uploaded_chunk.chunk_index);

        // Check all tracks for completion (not just the current chunk's track)
        let newly_completed =
            check_all_tracks_for_completion(progress_tracker, &completed, &already_completed);

        // Mark these tracks as completed so we don't check them again
        for track_id in &newly_completed {
            already_completed.insert(track_id.clone());
        }

        let percent = calculate_progress(completed.len(), total_chunks);
        (newly_completed, (completed.len(), percent))
    };

    // Mark newly completed tracks
    for track_id in newly_completed_tracks {
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

/// Check all tracks for completion and return newly completed ones.
///
/// Called after each chunk upload to detect any tracks that have all their chunks done.
/// Skips tracks that are already marked as complete.
fn check_all_tracks_for_completion(
    progress_tracker: &TrackProgressTracker,
    completed_chunks: &HashSet<i32>,
    already_completed: &HashSet<String>,
) -> Vec<String> {
    let mut newly_completed = Vec::new();

    for (track_id, &total_for_track) in &progress_tracker.track_chunk_counts {
        // Skip if already marked complete
        if already_completed.contains(track_id) {
            continue;
        }

        // Count how many of this track's chunks are complete
        let completed_for_track = progress_tracker
            .chunk_to_track
            .iter()
            .filter(|(idx, tid)| *tid == track_id && completed_chunks.contains(idx))
            .count();

        if completed_for_track == total_for_track {
            newly_completed.push(track_id.clone());
        }
    }

    newly_completed
}

/// Calculate progress percentage
pub(super) fn calculate_progress(completed: usize, total: usize) -> u8 {
    if total == 0 {
        100
    } else {
        ((completed as f64 / total as f64) * 100.0).min(100.0) as u8
    }
}
