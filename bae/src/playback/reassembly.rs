use crate::cache::CacheManager;
use crate::cloud_storage::CloudStorageManager;
use crate::db::{DbChunk, DbFile, DbTrackPosition};
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

    // Get file_chunk mapping to know byte offsets
    let file_chunk = library_manager
        .get_file_chunk_mapping(&file.id)
        .await
        .map_err(|e| format!("Database error: {}", e))?
        .ok_or_else(|| "No file_chunk mapping found for file".to_string())?;

    debug!(
        "File spans chunks {}-{} with byte offsets {}-{}",
        file_chunk.start_chunk_index,
        file_chunk.end_chunk_index,
        file_chunk.start_byte_offset,
        file_chunk.end_byte_offset
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

    // Extract only the chunk data (without the position index)
    let chunk_data: Vec<Vec<u8>> = indexed_chunks.into_iter().map(|(_, data)| data).collect();

    // Use byte offsets to extract exactly the file data
    debug!(
        "Extracting file data: {} chunks, start_offset={}, end_offset={}, chunk_size={}",
        chunk_data.len(),
        file_chunk.start_byte_offset,
        file_chunk.end_byte_offset,
        _chunk_size_bytes
    );
    let audio_data = extract_file_from_chunks(
        &chunk_data,
        file_chunk.start_byte_offset,
        file_chunk.end_byte_offset,
        _chunk_size_bytes,
    );

    info!(
        "Successfully reassembled {} bytes of audio data (expected: {} bytes)",
        audio_data.len(),
        file.file_size
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
    file: &DbFile,
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
        .get_chunks_in_range(&file.release_id, chunk_range)
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

/// Extract file data from chunks using byte offsets
///
/// Given a list of chunks and the file's byte offsets within those chunks,
/// this function extracts exactly the bytes that belong to the file.
///
/// # Arguments
/// * `chunks` - Decrypted chunk data in order (chunk 0, chunk 1, chunk 2, ...)
/// * `start_byte_offset` - Byte offset within the first chunk where the file starts
/// * `end_byte_offset` - Byte offset within the last chunk where the file ends (inclusive)
/// * `chunk_size` - Size of each chunk in bytes
///
/// # Returns
/// The extracted file data
fn extract_file_from_chunks(
    chunks: &[Vec<u8>],
    start_byte_offset: i64,
    end_byte_offset: i64,
    _chunk_size: usize,
) -> Vec<u8> {
    if chunks.is_empty() {
        return Vec::new();
    }

    let mut file_data = Vec::new();

    if chunks.len() == 1 {
        // File is entirely within a single chunk
        let start = start_byte_offset as usize;
        let end = (end_byte_offset + 1) as usize; // end_byte_offset is inclusive
        file_data.extend_from_slice(&chunks[0][start..end]);
    } else {
        // File spans multiple chunks
        // First chunk: from start_byte_offset to end of chunk
        let first_chunk_start = start_byte_offset as usize;
        file_data.extend_from_slice(&chunks[0][first_chunk_start..]);

        // Middle chunks: use entirely
        for chunk in &chunks[1..chunks.len() - 1] {
            file_data.extend_from_slice(chunk);
        }

        // Last chunk: from start to end_byte_offset
        let last_chunk_end = (end_byte_offset + 1) as usize; // end_byte_offset is inclusive
        file_data.extend_from_slice(&chunks[chunks.len() - 1][0..last_chunk_end]);
    }

    file_data
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::CacheConfig;
    use crate::cloud_storage::CloudStorageManager;
    use crate::db::{Database, DbChunk, DbFile, DbFileChunk, DbTrackPosition};
    use crate::encryption::EncryptionService;
    use crate::test_support::MockCloudStorage;
    use std::sync::Arc;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_reassemble_track_with_file_ending_mid_chunk() {
        // This test simulates the vinyl album scenario:
        // File 1 is 14,832,725 bytes (~14.14 MB)
        // With 1MB chunks, it spans chunks 0-14
        // Chunk 14 contains bytes 0-832,724 of file 1, then file 2 starts at byte 832,725

        let chunk_size = 1024 * 1024; // 1MB
        let file_size = 14_832_725;

        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let cache_dir = temp_dir.path().join("cache");
        std::fs::create_dir_all(&cache_dir).unwrap();

        let database = Database::new(db_path.to_str().unwrap()).await.unwrap();
        let library_manager = LibraryManager::new(database);
        let encryption_service = EncryptionService::new_with_key(vec![0u8; 32]);
        let mock_storage = Arc::new(MockCloudStorage::new());
        let cloud_storage = CloudStorageManager::from_storage(mock_storage);
        let cache_config = CacheConfig {
            cache_dir,
            max_size_bytes: 1024 * 1024 * 100,
            max_chunks: 1000,
        };
        let cache = CacheManager::with_config(cache_config).await.unwrap();

        // Create test data and chunks
        // Use a pattern that doesn't include 0xFF to avoid false positives
        let pattern: Vec<u8> = (0..=254).collect(); // 0-254, excluding 255 (0xFF)
        let file_data = pattern.repeat((file_size / 255) + 1);
        let file_data = &file_data[0..file_size];

        // Encrypt and upload 15 chunks
        for i in 0..15 {
            let start = i * chunk_size;
            let end = std::cmp::min(start + chunk_size, file_size);
            let mut chunk_data = vec![0u8; chunk_size];
            if start < file_size {
                let actual_len = end - start;
                chunk_data[0..actual_len].copy_from_slice(&file_data[start..end]);
                // Fill rest with data from "file 2" to simulate concatenation
                if actual_len < chunk_size {
                    chunk_data[actual_len..chunk_size].fill(0xFF);
                }
            }

            let (ciphertext, nonce) = encryption_service.encrypt(&chunk_data).unwrap();
            let encrypted_chunk =
                crate::encryption::EncryptedChunk::new(ciphertext, nonce, "master".to_string());
            cloud_storage
                .upload_chunk_data(&format!("test-chunk-{}", i), &encrypted_chunk.to_bytes())
                .await
                .unwrap();
        }

        // Setup database
        let album = crate::db::DbAlbum::new_test("Test Album");
        let release = crate::db::DbRelease::new_test(&album.id, "test-release");
        let track =
            crate::db::DbTrack::new_test("test-release", "test-track", "Test Track", Some(1));
        library_manager
            .insert_album_with_release_and_tracks(&album, &release, &[track])
            .await
            .unwrap();

        let file = DbFile::new("test-release", "test.flac", file_size as i64, "flac");
        library_manager.add_file(&file).await.unwrap();

        // Add chunks
        for i in 0..15 {
            let chunk_id = format!("test-chunk-{}", i);
            let location = format!(
                "s3://test-bucket/chunks/{}/{}/{}.enc",
                &chunk_id[0..2],
                &chunk_id[2..4],
                chunk_id
            );
            let chunk =
                DbChunk::from_release_chunk("test-release", &chunk_id, i, chunk_size, &location);
            library_manager.add_chunk(&chunk).await.unwrap();
        }

        // Add file_chunk mapping with byte offsets
        let file_chunk = DbFileChunk {
            id: uuid::Uuid::new_v4().to_string(),
            file_id: file.id.clone(),
            start_chunk_index: 0,
            end_chunk_index: 14,
            start_byte_offset: 0,
            end_byte_offset: 152_660, // Last byte of file in chunk 14 (14,832,725 - 14,680,064 - 1)
            created_at: chrono::Utc::now(),
        };
        library_manager
            .add_file_chunk_mapping(&file_chunk)
            .await
            .unwrap();

        // Add track_position
        let track_position = DbTrackPosition {
            id: uuid::Uuid::new_v4().to_string(),
            track_id: "test-track".to_string(),
            file_id: file.id.clone(),
            start_time_ms: 0,
            end_time_ms: 0,
            start_chunk_index: 0,
            end_chunk_index: 14,
            created_at: chrono::Utc::now(),
        };
        library_manager
            .add_track_position(&track_position)
            .await
            .unwrap();

        // THE TEST: Reassemble the track
        let reassembled = reassemble_track(
            "test-track",
            &library_manager,
            &cloud_storage,
            &cache,
            &encryption_service,
            chunk_size,
        )
        .await
        .unwrap();

        // Verify the fix works correctly
        assert_eq!(
            reassembled.len(),
            file_size,
            "File size should match exactly"
        );
        assert!(
            !reassembled.contains(&0xFF),
            "Should not contain bytes from next file"
        );
    }
}
