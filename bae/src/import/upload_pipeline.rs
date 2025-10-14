use crate::chunking::ChunkingService;
use crate::cloud_storage::CloudStorageManager;
use crate::import::types::TrackSourceFile;
use futures::stream::{FuturesUnordered, StreamExt};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

/// Events emitted by the upload pipeline as chunks are processed
#[derive(Debug)]
pub enum UploadEvent {
    /// Upload started, includes total chunk count for progress tracking
    Started { total_chunks: usize },
    /// A chunk was successfully uploaded to cloud storage
    ChunkUploaded {
        chunk_id: String,
        chunk_index: i32,
        original_size: usize,
        encrypted_size: usize,
        checksum: String,
        cloud_location: String,
    },
    /// All chunks for a track have been uploaded
    TrackCompleted { track_id: String },
    /// All chunks for the album have been uploaded
    Completed {
        file_mappings: Vec<crate::chunking::FileChunkMapping>,
    },
    /// Upload failed
    Failed { error: String },
}

/// Configuration for upload pipeline execution
pub struct UploadConfig {
    pub max_encrypt_workers: usize,
    pub max_upload_workers: usize,
}

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
    /// Returns a channel receiver that emits UploadEvents as chunks are processed.
    /// The pipeline runs in the background; consume events to track progress and handle persistence.
    ///
    /// Events:
    /// - Started: Pipeline started with total chunk count
    /// - ChunkUploaded: Chunk successfully uploaded (caller persists to database)
    /// - TrackCompleted: All chunks for a track uploaded (caller marks track complete)
    /// - Completed: All chunks uploaded (includes file_mappings for metadata persistence)
    /// - Failed: Upload failed (caller handles cleanup)
    pub fn chunk_and_upload_album(
        &self,
        track_files: Vec<TrackSourceFile>,
        config: UploadConfig,
    ) -> tokio::sync::mpsc::UnboundedReceiver<UploadEvent> {
        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();

        // Early validation
        if track_files.is_empty() {
            let _ = event_tx.send(UploadEvent::Failed {
                error: "No track files to upload".to_string(),
            });
            return event_rx;
        }

        // Clone what we need for the spawned task
        let chunking_service = self.chunking_service.clone();
        let cloud_storage = self.cloud_storage.clone();

        // Spawn the upload pipeline as a background task
        tokio::spawn(async move {
            // Run the pipeline and send result
            if let Err(e) = Self::run_upload_pipeline(
                chunking_service,
                cloud_storage,
                track_files,
                config,
                event_tx.clone(),
            )
            .await
            {
                let _ = event_tx.send(UploadEvent::Failed { error: e });
            }
        });

        event_rx
    }

    /// Internal async function that runs the actual upload pipeline
    async fn run_upload_pipeline(
        chunking_service: ChunkingService,
        cloud_storage: CloudStorageManager,
        track_files: Vec<TrackSourceFile>,
        config: UploadConfig,
        event_tx: tokio::sync::mpsc::UnboundedSender<UploadEvent>,
    ) -> Result<(), String> {
        let UploadConfig {
            max_encrypt_workers,
            max_upload_workers,
        } = config;

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
        let total_chunks = chunking_service
            .calculate_total_chunks(&all_files)
            .await
            .map_err(|e| format!("Failed to calculate chunks: {}", e))?;

        println!(
            "UploadPipeline: Expecting {} total chunks, starting pipeline with {} encryption workers and {} upload workers",
            total_chunks, max_encrypt_workers, max_upload_workers
        );

        // Send started event with total chunk count
        let _ = event_tx.send(UploadEvent::Started { total_chunks });

        // Start chunking/encryption pipeline (returns channel of encrypted chunks)
        let (album_result, mut encrypted_chunks) = chunking_service
            .chunk_album_streaming(album_folder, &all_files, max_encrypt_workers)
            .await
            .map_err(|e| format!("Failed to start chunking pipeline: {}", e))?;

        // Build chunk → track mapping for progressive track completion
        let mut file_to_track: HashMap<PathBuf, String> = HashMap::new();
        for track_file in track_files {
            file_to_track.insert(track_file.file_path.clone(), track_file.db_track_id.clone());
        }

        let mut chunk_to_track: HashMap<i32, String> = HashMap::new();
        let mut track_chunk_counts: HashMap<String, usize> = HashMap::new();

        for file_mapping in &album_result.file_mappings {
            if let Some(track_id) = file_to_track.get(&file_mapping.file_path) {
                let chunk_count =
                    (file_mapping.end_chunk_index - file_mapping.start_chunk_index + 1) as usize;

                // Map each chunk to its track
                for chunk_idx in file_mapping.start_chunk_index..=file_mapping.end_chunk_index {
                    chunk_to_track.insert(chunk_idx, track_id.clone());
                }

                // Track total chunks per track
                *track_chunk_counts.entry(track_id.clone()).or_insert(0) += chunk_count;
            }
        }

        // Track completion state per track
        let track_chunks_uploaded: Arc<Mutex<HashMap<String, usize>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // Stage 3: Upload coordinator (bounded parallel uploads)
        let chunks_completed = Arc::new(AtomicUsize::new(0));
        let chunk_to_track = Arc::new(chunk_to_track);
        let track_chunk_counts = Arc::new(track_chunk_counts);
        let event_tx = Arc::new(event_tx);

        let mut upload_tasks = FuturesUnordered::new();

        loop {
            // If we have room for more tasks and there are chunks to process
            if upload_tasks.len() < max_upload_workers {
                match encrypted_chunks.recv().await {
                    Some(chunk) => {
                        // Spawn upload task
                        let cloud_storage = cloud_storage.clone();
                        let chunks_completed = chunks_completed.clone();
                        let chunk_to_track = chunk_to_track.clone();
                        let track_chunk_counts = track_chunk_counts.clone();
                        let track_chunks_uploaded = track_chunks_uploaded.clone();
                        let event_tx = event_tx.clone();

                        let task = tokio::spawn(async move {
                            // Upload chunk data directly from memory
                            let cloud_location = cloud_storage
                                .upload_chunk_data(&chunk.id, &chunk.encrypted_data)
                                .await
                                .map_err(|e| format!("Upload failed: {}", e))?;

                            // Send chunk uploaded event (caller will persist to database)
                            let _ = event_tx.send(UploadEvent::ChunkUploaded {
                                chunk_id: chunk.id.clone(),
                                chunk_index: chunk.chunk_index,
                                original_size: chunk.original_size,
                                encrypted_size: chunk.encrypted_size,
                                checksum: chunk.checksum.clone(),
                                cloud_location,
                            });

                            // Update progress counter
                            let completed = chunks_completed.fetch_add(1, Ordering::SeqCst) + 1;
                            let total = total_chunks;

                            if total > 0 {
                                let percent = ((completed as f64 / total as f64) * 100.0) as u8;
                                println!(
                                    "  Upload progress: {}/{} ({:.0}%)",
                                    completed, total, percent
                                );
                            }

                            // Check if this chunk completes a track
                            if let Some(track_id) = chunk_to_track.get(&chunk.chunk_index) {
                                let track_complete = {
                                    let mut uploaded = track_chunks_uploaded.lock().unwrap();
                                    let track_uploaded =
                                        uploaded.entry(track_id.clone()).or_insert(0);
                                    *track_uploaded += 1;

                                    let total_for_track =
                                        track_chunk_counts.get(track_id).copied().unwrap_or(0);
                                    *track_uploaded == total_for_track
                                };

                                if track_complete {
                                    // This was the last chunk for this track - send completion event
                                    let _ = event_tx.send(UploadEvent::TrackCompleted {
                                        track_id: track_id.clone(),
                                    });

                                    println!("  Track {} upload complete!", track_id);
                                }
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

        // Send completion event with file mappings for metadata persistence
        let _ = event_tx.send(UploadEvent::Completed {
            file_mappings: album_result.file_mappings,
        });

        Ok(())
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
