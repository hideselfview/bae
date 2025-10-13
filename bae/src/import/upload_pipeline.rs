use crate::chunking::{AlbumChunkingResult, ChunkCallback, ChunkingService};
use crate::cloud_storage::CloudStorageManager;
use crate::database::DbChunk;
use crate::import::types::TrackSourceFile;
use crate::library::LibraryManager;
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

    /// Chunk and upload all album files in parallel.
    ///
    /// Steps:
    /// 1. Finds all files in album folder (audio + artwork + notes)
    /// 2. Chunks files with parallel encryption (CPU cores * 2)
    /// 3. Uploads chunks in parallel (20 concurrent uploads)
    /// 4. Stores chunk metadata in database
    ///
    /// Returns the chunking result with file-to-chunk mappings.
    pub async fn chunk_and_upload_album(
        &self,
        library_manager: &LibraryManager,
        track_files: &[TrackSourceFile],
        album_id: &str,
        progress_callback: Option<Box<dyn Fn(usize, usize) + Send + Sync>>,
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

        // Shared state for parallel uploads
        let cloud_storage = self.cloud_storage.clone();
        let library_manager_clone = library_manager.clone();
        let album_id = album_id.to_string();
        let chunks_completed = Arc::new(AtomicUsize::new(0));
        let total_chunks_ref = Arc::new(AtomicUsize::new(0));
        let progress_callback = Arc::new(progress_callback);
        let upload_handles = Arc::new(tokio::sync::Mutex::new(Vec::new()));

        // Limit concurrent uploads to prevent resource exhaustion
        let upload_semaphore = Arc::new(tokio::sync::Semaphore::new(20));

        // Create chunk callback for streaming pipeline with parallel uploads
        let chunk_callback: ChunkCallback = {
            let chunks_completed = chunks_completed.clone();
            let total_chunks_ref = total_chunks_ref.clone();
            let progress_callback = progress_callback.clone();
            let upload_handles = upload_handles.clone();
            let upload_semaphore = upload_semaphore.clone();

            Box::new(move |chunk: crate::chunking::AlbumChunk| {
                let cloud_storage = cloud_storage.clone();
                let library_manager = library_manager_clone.clone();
                let album_id = album_id.clone();
                let chunks_completed = chunks_completed.clone();
                let total_chunks_ref = total_chunks_ref.clone();
                let progress_callback = progress_callback.clone();
                let upload_handles = upload_handles.clone();
                let upload_semaphore = upload_semaphore.clone();

                Box::pin(async move {
                    // Spawn parallel upload task (semaphore limits concurrency)
                    let handle = tokio::spawn(async move {
                        // Acquire semaphore permit (blocks if 20 uploads already in progress)
                        let _permit = upload_semaphore.acquire().await.unwrap();

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
                        let total = total_chunks_ref.load(Ordering::SeqCst);

                        if total > 0 {
                            let progress = ((completed as f64 / total as f64) * 100.0) as u8;
                            println!(
                                "  Chunk progress: {}/{} ({:.0}%)",
                                completed, total, progress
                            );

                            if let Some(ref callback) = progress_callback.as_ref() {
                                callback(completed, total);
                            }
                        }

                        Ok::<(), String>(())
                    });

                    // Store handle for later awaiting
                    upload_handles.lock().await.push(handle);

                    Ok(())
                })
            })
        };

        // Calculate total chunks upfront so progress reporting works immediately
        let expected_total_chunks = self
            .chunking_service
            .calculate_total_chunks(&all_files)
            .await
            .map_err(|e| format!("Failed to calculate chunks: {}", e))?;
        total_chunks_ref.store(expected_total_chunks, Ordering::SeqCst);

        println!(
            "UploadPipeline: Expecting {} total chunks, starting parallel upload pipeline",
            expected_total_chunks
        );

        // Stream chunks through the pipeline (spawns uploads in parallel)
        let album_result = self
            .chunking_service
            .chunk_album_streaming(album_folder, &all_files, chunk_callback)
            .await
            .map_err(|e| format!("Chunking failed: {}", e))?;

        // Wait for all parallel uploads to complete
        let mut handles_vec = upload_handles.lock().await;
        println!(
            "UploadPipeline: Waiting for {} parallel uploads to complete...",
            handles_vec.len()
        );
        while let Some(handle) = handles_vec.pop() {
            handle
                .await
                .map_err(|e| format!("Task join failed: {}", e))??;
        }

        let final_completed = chunks_completed.load(Ordering::SeqCst);
        println!(
            "UploadPipeline: Completed {} chunks from {} files",
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
