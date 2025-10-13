use crate::chunking::{AlbumChunkingResult, ChunkingService};
use crate::cloud_storage::CloudStorageManager;
use crate::database::DbChunk;
use crate::import::types::TrackSourceFile;
use crate::library::LibraryManager;
use futures::stream::{FuturesUnordered, StreamExt};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

/// Service responsible for chunking album files and uploading to cloud storage.
/// Handles parallel encryption (CPU-bound) and parallel uploads (I/O-bound).
pub struct UploadPipeline {
    chunking_service: ChunkingService,
    cloud_storage: CloudStorageManager,
}

impl UploadPipeline {
    /// Create a new upload pipeline
    pub fn new(chunking_service: ChunkingService, cloud_storage: CloudStorageManager) -> Self {
        Self {
            chunking_service,
            cloud_storage,
        }
    }

    /// Chunk and upload all album files using streaming pipeline.
    ///
    /// Steps:
    /// 1. Finds all files in album folder (audio + artwork + notes)
    /// 2. Starts chunking/encryption pipeline (returns channel of encrypted chunks)
    /// 3. Consumes encrypted chunks from channel and uploads in parallel
    /// 4. Stores chunk metadata in database
    ///
    /// Returns the chunking result with file-to-chunk mappings.
    pub async fn chunk_and_upload_album(
        &self,
        library_manager: &LibraryManager,
        track_files: &[TrackSourceFile],
        album_id: &str,
        max_encrypt_workers: usize,
        max_upload_workers: usize,
        progress_callback: Box<dyn Fn(usize, usize) + Send + Sync>,
    ) -> Result<AlbumChunkingResult, String> {
        if track_files.is_empty() {
            return Err("No track files to upload".to_string());
        }

        // Get the album folder from the first mapping
        let album_folder = track_files[0]
            .file_path
            .parent()
            .ok_or_else(|| "Invalid source path".to_string())?;

        println!(
            "UploadPipeline: Processing album folder {} with streaming pipeline",
            album_folder.display()
        );

        // Find ALL files in the album folder (audio, artwork, notes, etc.)
        let all_files = Self::find_all_files_in_folder(album_folder)?;

        // Calculate total chunks upfront for progress reporting
        let total_chunks = self
            .chunking_service
            .calculate_total_chunks(&all_files)
            .await
            .map_err(|e| format!("Failed to calculate chunks: {}", e))?;

        println!(
            "UploadPipeline: Expecting {} total chunks, starting pipeline with {} encryption workers and {} upload workers",
            total_chunks, max_encrypt_workers, max_upload_workers
        );

        // Start chunking/encryption pipeline (returns channel of encrypted chunks)
        let (album_result, mut encrypted_chunks) = self
            .chunking_service
            .chunk_album_streaming(album_folder, &all_files, max_encrypt_workers)
            .await
            .map_err(|e| format!("Failed to start chunking pipeline: {}", e))?;

        // Stage 3: Upload coordinator (bounded parallel uploads)
        let library_manager = library_manager.clone();
        let cloud_storage = self.cloud_storage.clone();
        let album_id = album_id.to_string();
        let chunks_completed = Arc::new(AtomicUsize::new(0));
        let progress_callback = Arc::new(progress_callback);

        let mut upload_tasks = FuturesUnordered::new();

        loop {
            // If we have room for more tasks and there are chunks to process
            if upload_tasks.len() < max_upload_workers {
                match encrypted_chunks.recv().await {
                    Some(chunk) => {
                        // Spawn upload task
                        let library_manager = library_manager.clone();
                        let cloud_storage = cloud_storage.clone();
                        let album_id = album_id.clone();
                        let chunks_completed = chunks_completed.clone();
                        let progress_callback = progress_callback.clone();

                        let task = tokio::spawn(async move {
                            // Upload chunk data directly from memory
                            let cloud_location = cloud_storage
                                .upload_chunk_data(&chunk.id, &chunk.encrypted_data)
                                .await
                                .map_err(|e| format!("Upload failed: {}", e))?;

                            // Store chunk in database
                            let db_chunk = DbChunk::from_album_chunk(
                                &chunk.id,
                                &album_id,
                                chunk.chunk_index,
                                chunk.original_size,
                                chunk.encrypted_size,
                                &chunk.checksum,
                                &cloud_location,
                                false,
                            );
                            library_manager
                                .add_chunk(&db_chunk)
                                .await
                                .map_err(|e| format!("Database insert failed: {}", e))?;

                            // Update progress
                            let completed = chunks_completed.fetch_add(1, Ordering::SeqCst) + 1;
                            let total = total_chunks;

                            if total > 0 {
                                let progress = ((completed as f64 / total as f64) * 100.0) as u8;
                                println!(
                                    "  Upload progress: {}/{} ({:.0}%)",
                                    completed, total, progress
                                );

                                progress_callback(completed, total);
                            }

                            Ok::<(), String>(())
                        });

                        upload_tasks.push(task);
                    }
                    None => {
                        // No more encrypted chunks, drain remaining uploads
                        break;
                    }
                }
            } else {
                // At capacity, wait for one to complete
                match upload_tasks.next().await {
                    Some(Ok(Ok(()))) => {
                        // Upload completed successfully
                    }
                    Some(Ok(Err(e))) => {
                        return Err(format!("Upload failed: {}", e));
                    }
                    Some(Err(e)) => {
                        return Err(format!("Upload task panicked: {}", e));
                    }
                    None => break,
                }
            }
        }

        // Drain remaining upload tasks
        while let Some(result) = upload_tasks.next().await {
            match result {
                Ok(Ok(())) => {
                    // Upload completed successfully
                }
                Ok(Err(e)) => {
                    return Err(format!("Upload failed during drain: {}", e));
                }
                Err(e) => {
                    return Err(format!("Upload task panicked during drain: {}", e));
                }
            }
        }

        let final_completed = chunks_completed.load(Ordering::SeqCst);
        println!(
            "UploadPipeline: Completed uploading {} chunks from {} files",
            final_completed,
            album_result.file_mappings.len()
        );

        Ok(album_result)
    }

    /// Find ALL files in a folder (for album-level chunking)
    fn find_all_files_in_folder(dir: &Path) -> Result<Vec<PathBuf>, String> {
        let mut all_files = Vec::new();

        for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();

            if path.is_file() {
                all_files.push(path);
            }
        }

        // Sort files by name for consistent ordering (important for BitTorrent compatibility)
        all_files.sort();

        println!(
            "UploadPipeline: Found {} total files in folder",
            all_files.len()
        );
        Ok(all_files)
    }
}
