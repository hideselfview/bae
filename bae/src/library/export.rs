use crate::cache::CacheManager;
use crate::cloud_storage::CloudStorageManager;
use crate::db::DbChunk;
use crate::encryption::EncryptionService;
use crate::library::LibraryManager;
use crate::playback::reassembly::reassemble_track;
use futures::stream::{self, StreamExt};
use std::path::Path;
use tracing::{debug, info, warn};

/// Export service for reconstructing files and tracks from chunks
pub struct ExportService;

impl ExportService {
    /// Export all files for a release to a directory
    ///
    /// Reconstructs files sequentially from chunks in the order they were imported.
    /// Files are written with their original filenames to the target directory.
    pub async fn export_release(
        release_id: &str,
        target_dir: &Path,
        library_manager: &LibraryManager,
        cloud_storage: &CloudStorageManager,
        cache: &CacheManager,
        encryption_service: &EncryptionService,
        _chunk_size_bytes: usize,
    ) -> Result<(), String> {
        info!(
            "Exporting release {} to {}",
            release_id,
            target_dir.display()
        );

        // Get all files for the release, sorted by filename (matches import order)
        let mut files = library_manager
            .get_files_for_release(release_id)
            .await
            .map_err(|e| format!("Failed to get files: {}", e))?;

        files.sort_by(|a, b| a.original_filename.cmp(&b.original_filename));

        if files.is_empty() {
            return Err("No files found for release".to_string());
        }

        // Get all chunks for the release, sorted by index
        let mut chunks = library_manager
            .get_chunks_for_release(release_id)
            .await
            .map_err(|e| format!("Failed to get chunks: {}", e))?;

        chunks.sort_by_key(|c| c.chunk_index);

        if chunks.is_empty() {
            return Err("No chunks found for release".to_string());
        }

        // Download and decrypt all chunks in parallel (max 10 concurrent)
        info!("Downloading {} chunks...", chunks.len());
        let chunk_results: Vec<Result<(i32, Vec<u8>), String>> = stream::iter(chunks.iter())
            .map(|chunk| {
                let cloud_storage = cloud_storage.clone();
                let cache = cache.clone();
                let encryption_service = encryption_service.clone();
                async move {
                    let chunk_data = download_and_decrypt_chunk(
                        chunk,
                        &cloud_storage,
                        &cache,
                        &encryption_service,
                    )
                    .await?;
                    Ok::<_, String>((chunk.chunk_index, chunk_data))
                }
            })
            .buffer_unordered(10)
            .collect()
            .await;

        // Check for errors and collect indexed chunks
        let mut indexed_chunks: Vec<(i32, Vec<u8>)> = Vec::new();
        for result in chunk_results {
            indexed_chunks.push(result?);
        }

        // Sort by chunk index to ensure correct order
        indexed_chunks.sort_by_key(|(idx, _)| *idx);
        let chunk_data: Vec<Vec<u8>> = indexed_chunks.into_iter().map(|(_, data)| data).collect();

        // Reconstruct files sequentially
        // Files are stored sequentially in chunks, so we iterate through chunks
        // and extract file_size bytes for each file
        let mut chunk_offset = 0usize;
        let mut byte_offset_in_chunk = 0usize;

        for file in &files {
            let file_size = file.file_size as usize;
            let mut file_data = Vec::with_capacity(file_size);
            let mut remaining_bytes = file_size;

            // Extract file data from chunks
            while remaining_bytes > 0 && chunk_offset < chunk_data.len() {
                let current_chunk = &chunk_data[chunk_offset];
                let available_in_chunk = current_chunk.len() - byte_offset_in_chunk;
                let bytes_to_take = remaining_bytes.min(available_in_chunk);

                file_data.extend_from_slice(
                    &current_chunk[byte_offset_in_chunk..byte_offset_in_chunk + bytes_to_take],
                );

                remaining_bytes -= bytes_to_take;
                byte_offset_in_chunk += bytes_to_take;

                // Move to next chunk if we've consumed this one
                if byte_offset_in_chunk >= current_chunk.len() {
                    chunk_offset += 1;
                    byte_offset_in_chunk = 0;
                }
            }

            if remaining_bytes > 0 {
                return Err(format!(
                    "Not enough data to reconstruct file {} (needed {} bytes, got {})",
                    file.original_filename,
                    file_size,
                    file_size - remaining_bytes
                ));
            }

            // Write file to target directory
            let file_path = target_dir.join(&file.original_filename);
            std::fs::write(&file_path, &file_data)
                .map_err(|e| format!("Failed to write file {}: {}", file.original_filename, e))?;

            debug!(
                "Exported file {} ({} bytes)",
                file.original_filename,
                file_data.len()
            );
        }

        info!(
            "Successfully exported {} files to {}",
            files.len(),
            target_dir.display()
        );
        Ok(())
    }

    /// Export a single track as a FLAC file
    ///
    /// For one-file-per-track: extracts the original file.
    /// For CUE/FLAC: extracts and re-encodes as a standalone FLAC.
    pub async fn export_track(
        track_id: &str,
        output_path: &Path,
        library_manager: &LibraryManager,
        cloud_storage: &CloudStorageManager,
        cache: &CacheManager,
        encryption_service: &EncryptionService,
        chunk_size_bytes: usize,
    ) -> Result<(), String> {
        info!("Exporting track {} to {}", track_id, output_path.display());

        // Use existing reassemble_track function
        let audio_data = reassemble_track(
            track_id,
            library_manager,
            cloud_storage,
            cache,
            encryption_service,
            chunk_size_bytes,
        )
        .await?;

        // Write to file
        std::fs::write(output_path, &audio_data)
            .map_err(|e| format!("Failed to write track file: {}", e))?;

        info!(
            "Successfully exported track {} ({} bytes)",
            track_id,
            audio_data.len()
        );
        Ok(())
    }
}

/// Download and decrypt a single chunk with caching
///
/// This is a copy of the function from playback/reassembly.rs
/// to avoid making it public there.
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
