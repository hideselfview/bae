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

pub(super) mod chunk_producer;

use crate::cloud_storage::CloudStorageManager;
use crate::db::DbChunk;
use crate::encryption::{EncryptedChunk, EncryptionService};
use crate::import::progress_emitter::ImportProgressEmitter;
use crate::import::service::ImportConfig;
use crate::library::LibraryManager;
use futures::stream::{Stream, StreamExt};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

// ============================================================================
// Public API
// ============================================================================

/// Build the complete streaming pipeline: read → encrypt → upload → persist.
///
/// Returns a tuple of (chunk_tx, stream) where:
/// - chunk_tx: Sender for chunk data that should be spawned separately
/// - stream: Stream of results, one per chunk processed
///
/// Caller is responsible for spawning the chunk producer task using the returned chunk_tx.
/// This separation allows for better control over the producer lifecycle.
///
/// Using `impl Stream` keeps everything stack-allocated with static dispatch,
/// avoiding heap allocation and virtual calls that `BoxStream` would require.
pub(super) fn build_import_pipeline(
    config: ImportConfig,
    release_id: String,
    encryption_service: EncryptionService,
    cloud_storage: CloudStorageManager,
    library_manager: LibraryManager,
    progress_emitter: ImportProgressEmitter,
) -> (
    impl Stream<Item = Result<(), String>>,
    mpsc::Sender<Result<ChunkData, String>>,
) {
    // Stage 1: Read files and stream chunks (bounded channel for backpressure)
    let (chunk_tx, chunk_rx) = mpsc::channel::<Result<ChunkData, String>>(10);

    // Stage 2: Consume chunks from channel and encrypt (bounded CPU via spawn_blocking)
    let stream = ReceiverStream::new(chunk_rx)
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
            let release_id = release_id.clone();
            let library_manager = library_manager.clone();
            let progress_emitter = progress_emitter.clone();

            async move {
                match upload_result {
                    Ok(uploaded_chunk) => {
                        persist_and_track_progress(
                            &release_id,
                            uploaded_chunk,
                            &library_manager,
                            &progress_emitter,
                        )
                        .await
                    }
                    Err(error) => {
                        eprintln!("Upload failed: {}", error);
                        Err(error)
                    }
                }
            }
        })
        .buffer_unordered(10); // Allow some parallelism for DB writes

    (stream, chunk_tx)
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
async fn persist_chunk(
    chunk: &UploadedChunk,
    release_id: &str,
    library_manager: &LibraryManager,
) -> Result<(), String> {
    let db_chunk = DbChunk::from_release_chunk(
        release_id,
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
async fn persist_and_track_progress(
    release_id: &str,
    uploaded_chunk: UploadedChunk,
    library_manager: &LibraryManager,
    progress_emitter: &ImportProgressEmitter,
) -> Result<(), String> {
    // Persist chunk to database
    persist_chunk(&uploaded_chunk, release_id, library_manager).await?;

    // Track progress and get newly completed tracks
    let newly_completed_tracks = progress_emitter.on_chunk_complete(uploaded_chunk.chunk_index);

    // Mark newly completed tracks in the database
    for track_id in &newly_completed_tracks {
        library_manager
            .mark_track_complete(track_id)
            .await
            .map_err(|e| format!("Failed to mark track complete: {}", e))?;
    }

    Ok(())
}
