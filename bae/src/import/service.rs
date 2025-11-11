// # Import Service
//
// Single-instance queue-based service that imports albums.
// One worker task handles import requests sequentially from a queue.
//
// Flow:
// 1. Validation & Queueing (synchronous per request):
//    - Validate track-to-file mapping
//    - Insert album/tracks with status='queued'
//
// 2. Pipeline Execution (async per import):
//    - Mark album as 'importing'
//    - Streaming pipeline: read → encrypt → upload → persist (bounded parallelism)
//    - Mark album/tracks as 'complete'
//
// Architecture:
// - TrackFileMapper: Validates track-to-file mapping before DB insertion
// - MetadataPersister: Saves file/chunk metadata to DB
// - ProgressService: Broadcasts real-time progress updates to UI subscribers

use crate::cloud_storage::CloudStorageManager;
use crate::encryption::EncryptionService;
use crate::import::album_chunk_layout::AlbumChunkLayout;
use crate::import::handle::{ImportHandle, ImportRequest};
use crate::import::metadata_persister::MetadataPersister;
use crate::import::pipeline;
use crate::import::progress::ImportProgressTracker;
use crate::import::types::ImportProgress;
use crate::library::SharedLibraryManager;
use futures::stream::StreamExt;
use tokio::sync::mpsc;
use tracing::{error, info};

/// Configuration for import service
#[derive(Clone)]
pub struct ImportConfig {
    /// Size of each chunk in bytes
    pub chunk_size_bytes: usize,
    /// Number of parallel encryption workers (CPU-bound, typically 2x CPU cores)
    pub max_encrypt_workers: usize,
    /// Number of parallel upload workers (I/O-bound)
    pub max_upload_workers: usize,
    /// Number of parallel DB write workers (I/O-bound)
    pub max_db_write_workers: usize,
}

/// Import service that orchestrates the album import workflow
pub struct ImportService {
    /// Configuration for the import service
    config: ImportConfig,
    /// Channel for receiving import requests from clients
    requests_rx: mpsc::UnboundedReceiver<ImportRequest>,
    /// Channel for sending progress updates to subscribers
    progress_tx: mpsc::UnboundedSender<ImportProgress>,
    /// Service for encrypting files before upload
    encryption_service: EncryptionService,
    /// Service for uploading encrypted chunks to cloud storage
    cloud_storage: CloudStorageManager,
    /// Shared manager for library database operations
    library_manager: SharedLibraryManager,
}

impl ImportService {
    /// Start the single import service worker for the entire app.
    ///
    /// Creates one worker task that imports validated albums sequentially from a queue.
    /// Multiple imports will be queued and handled one at a time, not concurrently.
    /// Returns a handle that can be cloned and used throughout the app to submit import requests.
    pub fn start(
        config: ImportConfig,
        runtime_handle: tokio::runtime::Handle,
        library_manager: SharedLibraryManager,
        encryption_service: EncryptionService,
        cloud_storage: CloudStorageManager,
    ) -> ImportHandle {
        let (requests_tx, requests_rx) = mpsc::unbounded_channel();
        let (progress_tx, progress_rx) = mpsc::unbounded_channel();

        // Clone library_manager for the thread
        let library_manager_for_worker = library_manager.clone();

        // Spawn the service task on a dedicated thread (TorrentClient isn't Send-safe due to FFI)
        // This follows the same pattern as PlaybackService for handling non-Send types
        std::thread::spawn(move || {
            // Create a new tokio runtime for this thread
            let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");

            rt.block_on(async move {
                let service = ImportService {
                    config,
                    requests_rx,
                    progress_tx,
                    library_manager: library_manager_for_worker,
                    encryption_service,
                    cloud_storage,
                };

                service.run_import_worker().await;
            });
        });

        ImportHandle::new(requests_tx, progress_rx, library_manager, runtime_handle)
    }

    async fn run_import_worker(mut self) {
        info!("Worker started");

        // Import validated albums sequentially from the queue.
        loop {
            match self.requests_rx.recv().await {
                Some(request) => {
                    info!("Starting pipeline for '{}'", request.db_album.title);

                    if let Err(e) = self.import_album_from_folder(request).await {
                        error!("Pipeline failed: {}", e);
                        // TODO: Mark album as failed
                    }
                }
                None => {
                    info!("Worker receive channel closed");
                    break;
                }
            }
        }
    }

    /// Executes the streaming import pipeline for a validated album.
    ///
    /// Orchestrates the entire import workflow:
    /// 1. Marks the album as 'importing'
    /// 2. Calculates chunk layout and track progress
    /// 3. Streams files → encrypts → uploads → persists
    /// 4. Persists metadata and marks album complete.
    async fn import_album_from_folder(&self, request: ImportRequest) -> Result<(), String> {
        let library_manager = self.library_manager.get();

        let ImportRequest {
            db_album,
            db_release,
            tracks_to_files,
            discovered_files,
            cue_flac_metadata,
            torrent_metadata,
            torrent_source,
        } = request;

        // Mark release as importing now that pipeline is starting
        library_manager
            .mark_release_importing(&db_release.id)
            .await
            .map_err(|e| format!("Failed to mark release as importing: {}", e))?;

        info!("Marked release as 'importing' - starting pipeline");

        // Send started progress
        let _ = self.progress_tx.send(ImportProgress::Started {
            id: db_release.id.clone(),
        });

        // Analyze album layout: tracks → files → chunks
        // Uses pre-parsed CUE metadata (if present) to avoid re-parsing
        let chunk_layout = AlbumChunkLayout::build(
            discovered_files,
            &tracks_to_files,
            self.config.chunk_size_bytes,
            cue_flac_metadata,
        )?;

        // Destructure layout to move ownership of each piece to where it's needed
        let AlbumChunkLayout {
            files_to_chunks,
            total_chunks,
            chunk_to_track,
            track_chunk_counts,
            cue_flac_data,
        } = chunk_layout;

        info!(
            "Will stream {} chunks across {} files",
            total_chunks,
            files_to_chunks.len()
        );

        // ========== STREAMING PIPELINE ==========
        // Read → Encrypt → Upload → Persist → Track (bounded parallelism at each stage)

        let progress_tracker = ImportProgressTracker::new(
            db_release.id.clone(),
            total_chunks,
            chunk_to_track,
            track_chunk_counts,
            self.progress_tx.clone(),
        );

        let (pipeline, chunk_tx) = pipeline::build_import_pipeline(
            self.config.clone(),
            db_release.id.clone(),
            self.encryption_service.clone(),
            self.cloud_storage.clone(),
            library_manager.clone(),
            progress_tracker,
            tracks_to_files.clone(),
            files_to_chunks.clone(),
            self.config.chunk_size_bytes,
            cue_flac_data.clone(),
        );

        // Spawn chunk producer task - use torrent producer if this is a torrent import
        use std::collections::HashMap;
        let piece_mappings_result: Option<Result<HashMap<usize, (Vec<String>, i64, i64)>, String>> =
            if let (Some(torrent_source), Some(torrent_meta)) = (torrent_source, &torrent_metadata)
            {
                // Torrent import: recreate torrent client and handle
                use crate::torrent::{TorrentClient, TorrentPieceMapper};
                use tokio::runtime::Handle;

                // Create torrent handle in a separate block to ensure torrent_client is dropped
                // before any awaits that might capture it
                let torrent_handle = {
                    let runtime_handle = Handle::current();
                    let torrent_client = TorrentClient::new(runtime_handle)
                        .map_err(|e| format!("Failed to create torrent client: {}", e))?;

                    match torrent_source {
                        crate::import::types::TorrentSource::File(path) => torrent_client
                            .add_torrent_file(&path)
                            .await
                            .map_err(|e| format!("Failed to add torrent file: {}", e))?,
                        crate::import::types::TorrentSource::MagnetLink(magnet) => torrent_client
                            .add_magnet_link(&magnet)
                            .await
                            .map_err(|e| format!("Failed to add magnet link: {}", e))?,
                    }
                    // torrent_client is dropped here at the end of the block
                };

                // Wait for metadata if needed
                torrent_handle
                    .wait_for_metadata()
                    .await
                    .map_err(|e| format!("Failed to wait for metadata: {}", e))?;

                // Torrent import: use torrent producer
                // Note: We can't spawn a task with TorrentHandle as it's not Send.
                // Instead, we'll run it in the current task.
                let piece_mapper = TorrentPieceMapper::new(
                    torrent_meta.piece_length as usize,
                    self.config.chunk_size_bytes,
                    torrent_meta.num_pieces as usize,
                    torrent_meta.total_size_bytes as usize,
                );

                // Run torrent producer in current task (can't spawn due to Send requirement)
                // This will block until all pieces are processed, but that's acceptable
                // as the import service processes imports sequentially anyway
                let piece_mappings = pipeline::torrent_producer::produce_chunk_stream_from_torrent(
                    &torrent_handle,
                    piece_mapper,
                    self.config.chunk_size_bytes,
                    chunk_tx,
                )
                .await;

                Some(Ok(piece_mappings))
            } else {
                // Regular import: use file producer
                tokio::spawn(pipeline::chunk_producer::produce_chunk_stream_from_files(
                    files_to_chunks.clone(),
                    self.config.chunk_size_bytes,
                    chunk_tx,
                ));
                None
            };

        // Wait for the pipeline to complete
        let results: Vec<_> = pipeline.collect().await;

        // Check for errors (fail on first error found)
        for result in results {
            result?;
        }

        info!("All {} chunks uploaded successfully", total_chunks);

        // ========== TEARDOWN ==========

        // Get piece mappings if this was a torrent import
        if let Some(Ok(piece_mappings)) = piece_mappings_result {
            if let Some(torrent_meta) = &torrent_metadata {
                // Get torrent ID from database
                let torrent = library_manager
                    .get_torrent_by_release(&db_release.id)
                    .await
                    .map_err(|e| format!("Failed to get torrent: {}", e))?
                    .ok_or_else(|| "Torrent not found in database".to_string())?;

                // Save piece mappings to database
                for (piece_index, (chunk_ids, start_byte, end_byte)) in &piece_mappings {
                    let mapping = crate::db::DbTorrentPieceMapping::new(
                        &torrent.id,
                        *piece_index as i32,
                        chunk_ids.clone(),
                        *start_byte,
                        *end_byte,
                    )
                    .map_err(|e| format!("Failed to create piece mapping: {}", e))?;

                    library_manager
                        .insert_torrent_piece_mapping(&mapping)
                        .await
                        .map_err(|e| format!("Failed to save piece mapping: {}", e))?;
                }

                info!("Saved {} piece mappings to database", piece_mappings.len());
            }
        }

        // Persist release-level metadata to database
        let persister = MetadataPersister::new(library_manager);
        persister
            .persist_release_metadata(&db_release.id, &tracks_to_files, &files_to_chunks)
            .await?;

        // Mark release complete
        library_manager
            .mark_release_complete(&db_release.id)
            .await
            .map_err(|e| format!("Failed to mark release complete: {}", e))?;

        // Send completion event
        let _ = self
            .progress_tx
            .send(ImportProgress::Complete { id: db_release.id });

        info!("Import completed successfully for {}", db_album.title);
        Ok(())
    }
}

// ============================================================================
// Validation Helper Functions
// ============================================================================
