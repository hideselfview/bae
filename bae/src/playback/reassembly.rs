use crate::cache::CacheManager;
use crate::cloud_storage::CloudStorageManager;
use crate::db::DbChunk;
use crate::encryption::EncryptionService;
use crate::library::LibraryManager;
use futures::stream::{self, StreamExt};
use tracing::{debug, info, warn};

/// Reassemble chunks for a track into a continuous audio buffer
///
/// Unified streaming logic for all tracks using TrackChunkCoords:
/// 1. Get track chunk coordinates (has all location info)
/// 2. Get audio format (has FLAC headers if needed)
/// 3. Download chunks in range and extract byte ranges
/// 4. Prepend FLAC headers if needed (CUE/FLAC tracks)
///
/// Key insight: Both import types produce identical TrackChunkCoords records.
/// The only difference is whether we need to prepend FLAC headers.
pub async fn reassemble_track(
    track_id: &str,
    library_manager: &LibraryManager,
    cloud_storage: &CloudStorageManager,
    cache: &CacheManager,
    encryption_service: &EncryptionService,
    chunk_size_bytes: usize,
) -> Result<Vec<u8>, String> {
    info!("Reassembling chunks for track: {}", track_id);

    // Step 1: Get track chunk coordinates (has all location info)
    let coords = library_manager
        .get_track_chunk_coords(track_id)
        .await
        .map_err(|e| format!("Database error: {}", e))?
        .ok_or_else(|| format!("No chunk coordinates found for track {}", track_id))?;

    // Step 2: Get audio format (has FLAC headers if needed)
    let audio_format = library_manager
        .get_audio_format_by_track_id(track_id)
        .await
        .map_err(|e| format!("Database error: {}", e))?
        .ok_or_else(|| format!("No audio format found for track {}", track_id))?;

    debug!(
        "Track spans chunks {}-{} with byte offsets {}-{}",
        coords.start_chunk_index,
        coords.end_chunk_index,
        coords.start_byte_offset,
        coords.end_byte_offset
    );

    // Step 3: Get track to find release_id
    let track = library_manager
        .get_track(track_id)
        .await
        .map_err(|e| format!("Database error: {}", e))?
        .ok_or_else(|| format!("Track not found: {}", track_id))?;

    // Step 4: Get all chunks in range
    let chunk_range = coords.start_chunk_index..=coords.end_chunk_index;
    let chunks = library_manager
        .get_chunks_in_range(&track.release_id, chunk_range)
        .await
        .map_err(|e| format!("Database error: {}", e))?;

    if chunks.is_empty() {
        return Err(format!("No chunks found for track {}", track_id));
    }

    debug!("Found {} chunks to reassemble", chunks.len());

    // Sort chunks by index to ensure correct order
    let mut sorted_chunks = chunks;
    sorted_chunks.sort_by_key(|c| c.chunk_index);

    // Download and decrypt all chunks in parallel (max 10 concurrent)
    let chunk_results: Vec<Result<(i32, Vec<u8>), String>> = stream::iter(sorted_chunks)
        .map(move |chunk| {
            let cloud_storage = cloud_storage.clone();
            let cache = cache.clone();
            let encryption_service = encryption_service.clone();
            async move {
                let chunk_data =
                    download_and_decrypt_chunk(&chunk, &cloud_storage, &cache, &encryption_service)
                        .await?;
                Ok::<_, String>((chunk.chunk_index, chunk_data))
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

    // Sort by chunk index to ensure correct order (parallel downloads may complete out of order)
    indexed_chunks.sort_by_key(|(idx, _)| *idx);

    // Extract only the chunk data (without the index)
    let chunk_data: Vec<Vec<u8>> = indexed_chunks.into_iter().map(|(_, data)| data).collect();

    // Use byte offsets from coordinates to extract exactly the track data
    debug!(
        "Extracting track data: {} chunks, start_offset={}, end_offset={}, chunk_size={}",
        chunk_data.len(),
        coords.start_byte_offset,
        coords.end_byte_offset,
        chunk_size_bytes
    );
    let mut audio_data = extract_file_from_chunks(
        &chunk_data,
        coords.start_byte_offset,
        coords.end_byte_offset,
        chunk_size_bytes,
    );

    debug!(
        "Extracted {} bytes of audio data ({}MB)",
        audio_data.len(),
        audio_data.len() / 1_000_000
    );

    // For CUE/FLAC tracks, decode and re-encode (like split_cue_flac.rs)
    if audio_format.needs_headers {
        if let Some(ref headers) = audio_format.flac_headers {
            debug!("CUE/FLAC track: prepending headers and decode/re-encode");

            // Prepend original album headers to make valid FLAC file
            let mut temp_flac = headers.clone();
            temp_flac.extend_from_slice(&audio_data);

            // Decode with Symphonia and re-encode with flacenc
            // Note: audio_data already contains only this track's byte range,
            // so we decode from the start (time 0), not from the track's original position
            audio_data = decode_and_reencode_track(
                &temp_flac, 0,    // Start from beginning of extracted audio
                None, // Decode until end
            )
            .await?;

            debug!("Decode/re-encode complete: {} bytes", audio_data.len());
        } else {
            warn!("Audio format needs headers but none provided");
        }
    }

    info!(
        "Successfully reassembled {} bytes of audio data for track {}",
        audio_data.len(),
        track_id
    );
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

/// Decode and re-encode a track from FLAC data using Symphonia + flacenc
///
/// This matches the approach in split_cue_flac.rs binary.
/// The input is a complete FLAC file (with headers), and we seek to the
/// specified time range, decode, and re-encode.
async fn decode_and_reencode_track(
    flac_data: &[u8],
    start_ms: u64,
    end_ms: Option<u64>,
) -> Result<Vec<u8>, String> {
    use std::io::Cursor;
    use symphonia::core::audio::{AudioBufferRef, Signal};
    use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_FLAC};
    use symphonia::core::errors::Error as SymphoniaError;
    use symphonia::core::formats::{FormatOptions, SeekMode, SeekTo};
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;
    use symphonia::core::units::Time;

    // Run decode/encode in spawn_blocking since it's CPU-intensive
    let flac_data = flac_data.to_vec();
    tokio::task::spawn_blocking(move || {
        // Open FLAC data with Symphonia
        let cursor = Cursor::new(flac_data);
        let mss = MediaSourceStream::new(Box::new(cursor), Default::default());

        let mut hint = Hint::new();
        hint.with_extension("flac");

        let format_opts = FormatOptions::default();
        let metadata_opts = MetadataOptions::default();

        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &format_opts, &metadata_opts)
            .map_err(|e| format!("Failed to probe FLAC: {}", e))?;

        let mut format = probed.format;

        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec == CODEC_TYPE_FLAC)
            .ok_or_else(|| "No FLAC track found".to_string())?;

        let track_id = track.id;
        let codec_params = track.codec_params.clone();

        let sample_rate = codec_params
            .sample_rate
            .ok_or_else(|| "No sample rate found".to_string())?;
        let channels = codec_params
            .channels
            .ok_or_else(|| "No channel info found".to_string())?;
        let bits_per_sample = codec_params
            .bits_per_sample
            .ok_or_else(|| "No bits per sample found".to_string())?;

        // Create decoder
        let mut decoder = symphonia::default::get_codecs()
            .make(&codec_params, &DecoderOptions::default())
            .map_err(|e| format!("Failed to create decoder: {}", e))?;

        // Seek to start position
        let start_time = Time::from(start_ms as f64 / 1000.0);
        format
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time: start_time,
                    track_id: Some(track_id),
                },
            )
            .map_err(|e| format!("Failed to seek to start: {}", e))?;

        // Calculate end sample
        let end_sample = end_ms.map(|ms| (ms * sample_rate as u64) / 1000);

        // Collect decoded samples (interleaved for all channels)
        let num_channels = channels.count();
        let mut all_samples: Vec<i32> = Vec::new();
        let mut current_sample = (start_ms * sample_rate as u64) / 1000;

        loop {
            let packet = match format.next_packet() {
                Ok(packet) => packet,
                Err(SymphoniaError::IoError(e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    break;
                }
                Err(e) => return Err(format!("Failed to read packet: {}", e)),
            };

            if packet.track_id() != track_id {
                continue;
            }

            let decoded = decoder
                .decode(&packet)
                .map_err(|e| format!("Failed to decode packet: {}", e))?;

            // Extract samples from the decoded audio buffer (interleave channels)
            let num_frames = decoded.frames();

            for frame_idx in 0..num_frames {
                if let Some(end) = end_sample {
                    if current_sample >= end {
                        break;
                    }
                }

                // Interleave channels
                for ch_idx in 0..num_channels {
                    let sample = match &decoded {
                        AudioBufferRef::S16(buf) => buf.chan(ch_idx)[frame_idx] as i32,
                        AudioBufferRef::S32(buf) => {
                            // S32 samples from Symphonia are in full 32-bit range
                            // Scale down to the target bits_per_sample range
                            let s32_sample = buf.chan(ch_idx)[frame_idx];
                            s32_sample >> (32 - bits_per_sample)
                        }
                        _ => return Err("Unsupported sample format".to_string()),
                    };
                    all_samples.push(sample);
                }

                current_sample += 1;
            }

            if let Some(end) = end_sample {
                if current_sample >= end {
                    break;
                }
            }
        }

        // Encode to FLAC using flacenc
        encode_to_flac(
            &all_samples,
            sample_rate,
            num_channels as u32,
            bits_per_sample,
        )
    })
    .await
    .map_err(|e| format!("Decode/encode task failed: {}", e))?
}

/// Encode samples to FLAC using flacenc
fn encode_to_flac(
    samples: &[i32],
    sample_rate: u32,
    channels: u32,
    bits_per_sample: u32,
) -> Result<Vec<u8>, String> {
    use flacenc::bitsink::ByteSink;
    use flacenc::component::BitRepr;
    use flacenc::config;
    use flacenc::error::Verify;
    use flacenc::source::MemSource;

    // Convert samples to the format flacenc expects (interleaved i32)
    let source = MemSource::from_samples(
        samples,
        channels as usize,
        bits_per_sample as usize,
        sample_rate as usize,
    );

    // Create and verify encoder config
    let config = config::Encoder::default();
    let config = config
        .into_verified()
        .map_err(|(_, e)| format!("Failed to verify encoder config: {:?}", e))?;

    // Encode with default block size (4096)
    let flac_stream = flacenc::encode_with_fixed_block_size(&config, source, 4096)
        .map_err(|e| format!("Failed to encode FLAC: {:?}", e))?;

    // Write stream to a ByteSink
    let mut sink = ByteSink::new();
    flac_stream
        .write(&mut sink)
        .map_err(|e| format!("Failed to write stream to sink: {:?}", e))?;

    Ok(sink.as_slice().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::CacheConfig;
    use crate::cloud_storage::CloudStorageManager;
    use crate::db::{Database, DbAudioFormat, DbChunk, DbFile, DbTrackChunkCoords};
    use crate::encryption::EncryptionService;
    #[cfg(feature = "test-utils")]
    use crate::test_support::MockCloudStorage;
    use std::sync::Arc;
    use tempfile::TempDir;

    #[tokio::test]
    #[cfg(feature = "test-utils")]
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
        let encryption_service = EncryptionService::new_with_key(vec![0u8; 32]);
        let mock_storage = Arc::new(MockCloudStorage::new());
        let cloud_storage = CloudStorageManager::from_storage(mock_storage);
        let library_manager = LibraryManager::new(database, cloud_storage.clone());
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

        // Add audio format
        let audio_format = DbAudioFormat::new("test-track", "flac", None, false);
        library_manager
            .add_audio_format(&audio_format)
            .await
            .unwrap();

        // Add track chunk coordinates
        let coords = DbTrackChunkCoords::new(
            "test-track",
            0,
            14,
            0,
            152_660, // Last byte of file in chunk 14 (14,832,725 - 14,680,064 - 1)
            0,
            0,
        );
        library_manager
            .add_track_chunk_coords(&coords)
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
