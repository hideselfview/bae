use crate::cache::CacheManager;
use crate::cloud_storage::CloudStorageManager;
use crate::database::{DbChunk, DbTrackPosition};
use crate::encryption::EncryptionService;
use crate::library::LibraryManager;
use futures::stream::{self, StreamExt};
use tracing::{debug, info, warn};

/// Reassemble chunks for a track into a continuous audio buffer
/// Handles both regular tracks (individual files) and CUE/FLAC tracks (single file, multiple tracks)
pub async fn reassemble_track(
    track_id: &str,
    library_manager: &LibraryManager,
    cloud_storage: &CloudStorageManager,
    cache: &CacheManager,
    encryption_service: &EncryptionService,
    chunk_size_bytes: usize,
) -> Result<Vec<u8>, String> {
    info!("Reassembling chunks for track: {}", track_id);

    // Check if this is a CUE/FLAC track with track positions
    if let Some(track_position) = library_manager
        .get_track_position(track_id)
        .await
        .map_err(|e| format!("Database error: {}", e))?
    {
        info!("CUE/FLAC track detected - using efficient chunk range streaming");
        return reassemble_cue_track(
            track_id,
            &track_position,
            library_manager,
            cloud_storage,
            cache,
            encryption_service,
            chunk_size_bytes,
        )
        .await;
    }

    // Fallback to regular file streaming for individual tracks
    info!("Regular track - reassembling full file chunks");

    // Get files for this track
    let files = library_manager
        .get_files_for_track(track_id)
        .await
        .map_err(|e| format!("Database error: {}", e))?;
    if files.is_empty() {
        return Err("No files found for track".to_string());
    }

    // Handle the first file (most tracks have one file)
    let file = &files[0];
    debug!(
        "Processing file: {} ({} bytes)",
        file.original_filename, file.file_size
    );

    // Get chunks for this file
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
/// This provides significant download reduction compared to downloading entire files
async fn reassemble_cue_track(
    track_id: &str,
    track_position: &DbTrackPosition,
    library_manager: &LibraryManager,
    cloud_storage: &CloudStorageManager,
    cache: &CacheManager,
    encryption_service: &EncryptionService,
    chunk_size_bytes: usize,
) -> Result<Vec<u8>, String> {
    info!(
        "Streaming CUE/FLAC track: chunks {}-{}",
        track_position.start_chunk_index, track_position.end_chunk_index
    );

    // Get the file for this track
    let files = library_manager
        .get_files_for_track(track_id)
        .await
        .map_err(|e| format!("Database error: {}", e))?;
    if files.is_empty() {
        return Err("No files found for CUE track".to_string());
    }

    let file = &files[0];

    // Check if this file has FLAC headers stored in database
    if !file.has_cue_sheet {
        return Err("File is not marked as CUE/FLAC".to_string());
    }

    let flac_headers = file
        .flac_headers
        .as_ref()
        .ok_or("No FLAC headers found in database")?;

    debug!("Using stored FLAC headers: {} bytes", flac_headers.len());

    // Get the album_id for this track
    let album_id = library_manager
        .get_album_id_for_track(track_id)
        .await
        .map_err(|e| format!("Failed to get album ID: {}", e))?;

    // Get only the chunks we need for this track (efficient!)
    let chunk_range = track_position.start_chunk_index..=track_position.end_chunk_index;
    let chunks = library_manager
        .get_chunks_in_range(&album_id, chunk_range)
        .await
        .map_err(|e| format!("Failed to get chunk range: {}", e))?;

    if chunks.is_empty() {
        return Err("No chunks found in track range".to_string());
    }

    let approximate_total_chunks = file.file_size / chunk_size_bytes as i64;
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
