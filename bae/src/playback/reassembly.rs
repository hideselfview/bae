use crate::cache::CacheManager;
use crate::cloud_storage::CloudStorageManager;
use crate::database::{DbChunk, DbTrackPosition};
use crate::encryption::EncryptionService;
use crate::library::LibraryManager;
use futures::stream::{self, StreamExt};
use tracing::{debug, info, warn};

/// Reassemble chunks for a track into a continuous audio buffer
///
/// Unified streaming logic for all tracks:
/// 1. Look up track_position to find which file this track uses
/// 2. Get the file record
/// 3. Branch based on file type:
///    - CUE/FLAC: Use chunk range + prepend FLAC headers
///    - Regular: Download all file chunks and reassemble
///
/// Key insight: ALL tracks now have track_position entries (not just CUE/FLAC).
/// This unifies the streaming code path and eliminates special cases.
pub async fn reassemble_track(
    track_id: &str,
    library_manager: &LibraryManager,
    cloud_storage: &CloudStorageManager,
    cache: &CacheManager,
    encryption_service: &EncryptionService,
    _chunk_size_bytes: usize,
) -> Result<Vec<u8>, String> {
    info!("Reassembling chunks for track: {}", track_id);

    // Step 1: Get track position (links track→file with time ranges)
    // This now exists for ALL tracks, not just CUE/FLAC
    let track_position = library_manager
        .get_track_position(track_id)
        .await
        .map_err(|e| format!("Database error: {}", e))?
        .ok_or_else(|| format!("No track position found for track {}", track_id))?;

    // Step 2: Get the file record
    let file = library_manager
        .get_file_by_id(&track_position.file_id)
        .await
        .map_err(|e| format!("Database error: {}", e))?
        .ok_or_else(|| "File not found for track".to_string())?;

    // Step 3: Branch based on file type
    if file.has_cue_sheet {
        // CUE/FLAC: Download only the chunk range needed, prepend FLAC headers
        info!("CUE/FLAC track detected - using efficient chunk range streaming with FLAC headers");
        return reassemble_cue_track(
            &track_position,
            &file,
            library_manager,
            cloud_storage,
            cache,
            encryption_service,
        )
        .await;
    }

    // Regular file: Download all chunks for this file
    info!("Regular track - reassembling full file chunks");
    debug!(
        "Processing file: {} ({} bytes)",
        file.original_filename, file.file_size
    );

    // Get chunks for this track's file
    let chunks = library_manager
        .get_chunks_for_file(&file.id)
        .await
        .map_err(|e| format!("Database error: {}", e))?;
    if chunks.is_empty() {
        return Err("No chunks found for file".to_string());
    }

    debug!("Found {} chunks to reassemble", chunks.len());

    // Sort chunks by index to ensure correct order
    let mut sorted_chunks = chunks;
    sorted_chunks.sort_by_key(|c| c.chunk_index);

    // Calculate the base chunk index (minimum) so we can compute file-relative positions
    let base_chunk_index = sorted_chunks.first().map(|c| c.chunk_index).unwrap_or(0);

    // Download and decrypt all chunks in parallel (max 10 concurrent)
    let chunk_results: Vec<Result<(i32, Vec<u8>), String>> = stream::iter(sorted_chunks)
        .map(move |chunk| {
            let cloud_storage = cloud_storage.clone();
            let cache = cache.clone();
            let encryption_service = encryption_service.clone();
            async move {
                // Use file-relative position (0, 1, 2, ...) instead of album-level index
                let file_position = chunk.chunk_index - base_chunk_index;
                let chunk_data =
                    download_and_decrypt_chunk(&chunk, &cloud_storage, &cache, &encryption_service)
                        .await?;
                Ok::<_, String>((file_position, chunk_data))
            }
        })
        .buffer_unordered(10) // Download up to 10 chunks concurrently
        .collect()
        .await;

    // Check for errors and collect indexed chunks
    let mut indexed_chunks: Vec<(i32, Vec<u8>)> = Vec::new();
    for result in chunk_results {
        indexed_chunks.push(result?);
    }

    // Sort by file position to ensure correct order (parallel downloads may complete out of order)
    indexed_chunks.sort_by_key(|(position, _)| *position);

    // Reassemble chunks into audio data
    let mut audio_data = Vec::new();
    for (index, chunk_data) in indexed_chunks {
        debug!("Assembling chunk at index {}", index);
        audio_data.extend_from_slice(&chunk_data);
    }

    info!(
        "Successfully reassembled {} bytes of audio data",
        audio_data.len()
    );
    Ok(audio_data)
}

/// Reassemble a CUE/FLAC track efficiently using chunk ranges and header prepending
///
/// CUE/FLAC optimization:
/// - Instead of downloading the entire FLAC file, we only download the chunks
///   needed for this specific track's time range
/// - We prepend the FLAC headers (stored in database) to make it a valid FLAC file
/// - This provides ~85% download reduction for typical CUE/FLAC albums
///
/// How it works:
/// 1. Get the chunk range for this track from track_position
/// 2. Download and decrypt only those chunks
/// 3. Prepend FLAC headers from database
/// 4. Return as valid FLAC audio data
async fn reassemble_cue_track(
    track_position: &DbTrackPosition,
    file: &crate::database::DbFile,
    library_manager: &LibraryManager,
    cloud_storage: &CloudStorageManager,
    cache: &CacheManager,
    encryption_service: &EncryptionService,
) -> Result<Vec<u8>, String> {
    info!(
        "Streaming CUE/FLAC track: chunks {}-{}",
        track_position.start_chunk_index, track_position.end_chunk_index
    );

    // Sanity check
    if !file.has_cue_sheet {
        return Err("File is not marked as CUE/FLAC".to_string());
    }

    let flac_headers = file
        .flac_headers
        .as_ref()
        .ok_or("No FLAC headers found in database")?;

    debug!("Using stored FLAC headers: {} bytes", flac_headers.len());

    // Get only the chunks we need for this track (efficient!)
    let chunk_range = track_position.start_chunk_index..=track_position.end_chunk_index;
    let chunks = library_manager
        .get_chunks_in_range(&file.album_id, chunk_range)
        .await
        .map_err(|e| format!("Failed to get chunk range: {}", e))?;

    if chunks.is_empty() {
        return Err("No chunks found in track range".to_string());
    }

    // Calculate approximate reduction (for logging purposes only)
    let chunk_size_estimate = 1024 * 1024; // 1MB default estimate
    let approximate_total_chunks = file.file_size / chunk_size_estimate;
    info!(
        "Downloading {} chunks instead of {} total chunks ({}% reduction)",
        chunks.len(),
        approximate_total_chunks,
        100 - (chunks.len() * 100) / approximate_total_chunks as usize
    );

    // Sort chunks by index to ensure correct order
    let mut sorted_chunks = chunks;
    sorted_chunks.sort_by_key(|c| c.chunk_index);

    let chunk_count = sorted_chunks.len();

    // Calculate the base chunk index (minimum) so we can compute file-relative positions
    let base_chunk_index = sorted_chunks.first().map(|c| c.chunk_index).unwrap_or(0);

    // Download and decrypt all chunks in parallel (max 10 concurrent)
    let chunk_results: Vec<Result<(i32, Vec<u8>), String>> = stream::iter(sorted_chunks)
        .map(move |chunk| {
            let cloud_storage = cloud_storage.clone();
            let cache = cache.clone();
            let encryption_service = encryption_service.clone();
            async move {
                // Use file-relative position (0, 1, 2, ...) instead of album-level index
                let file_position = chunk.chunk_index - base_chunk_index;
                let chunk_data =
                    download_and_decrypt_chunk(&chunk, &cloud_storage, &cache, &encryption_service)
                        .await?;
                Ok::<_, String>((file_position, chunk_data))
            }
        })
        .buffer_unordered(10) // Download up to 10 chunks concurrently
        .collect()
        .await;

    // Check for errors and collect indexed chunks
    let mut indexed_chunks: Vec<(i32, Vec<u8>)> = Vec::new();
    for result in chunk_results {
        indexed_chunks.push(result?);
    }

    // Sort by file position to ensure correct order (parallel downloads may complete out of order)
    indexed_chunks.sort_by_key(|(position, _)| *position);

    // Start with FLAC headers
    let mut audio_data = flac_headers.clone();

    // Append track chunks in order
    for (index, chunk_data) in indexed_chunks {
        debug!("Assembling CUE track chunk at index {}", index);
        audio_data.extend_from_slice(&chunk_data);
    }

    info!(
        "Successfully assembled CUE track: {} bytes (headers + {} chunks)",
        audio_data.len(),
        chunk_count
    );

    // For now, return the reassembled chunks with headers
    // TODO: Use audio processing to extract precise track boundaries based on start_time_ms/end_time_ms
    Ok(audio_data)
}

/// Download and decrypt a single chunk with caching
async fn download_and_decrypt_chunk(
    chunk: &DbChunk,
    cloud_storage: &CloudStorageManager,
    cache: &CacheManager,
    encryption_service: &EncryptionService,
) -> Result<Vec<u8>, String> {
    // Check cache first
    let encrypted_data = match cache.get_chunk(&chunk.id).await {
        Ok(Some(cached_encrypted_data)) => {
            debug!("Cache hit for chunk: {}", chunk.id);
            cached_encrypted_data
        }
        Ok(None) => {
            debug!("Cache miss - downloading chunk from cloud: {}", chunk.id);
            // Download from cloud storage
            let data = cloud_storage
                .download_chunk(&chunk.storage_location)
                .await
                .map_err(|e| format!("Failed to download chunk: {}", e))?;

            // Cache the encrypted data for future use
            if let Err(e) = cache.put_chunk(&chunk.id, &data).await {
                warn!("Failed to cache chunk (non-fatal): {}", e);
            }
            data
        }
        Err(e) => {
            warn!("Cache error (continuing with download): {}", e);
            // Download from cloud storage
            cloud_storage
                .download_chunk(&chunk.storage_location)
                .await
                .map_err(|e| format!("Failed to download chunk: {}", e))?
        }
    };

    // Decrypt in spawn_blocking to avoid blocking the async runtime
    let encryption_service = encryption_service.clone();
    let decrypted_data = tokio::task::spawn_blocking(move || {
        encryption_service
            .decrypt_chunk(&encrypted_data)
            .map_err(|e| format!("Failed to decrypt chunk: {}", e))
    })
    .await
    .map_err(|e| format!("Decryption task failed: {}", e))??;

    Ok(decrypted_data)
}
