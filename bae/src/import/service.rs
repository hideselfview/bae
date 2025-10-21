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
use crate::import::album_chunk_layout::AlbumDataLayout;
use crate::import::handle::{ImportHandle, ImportRequest};
use crate::import::metadata_persister::MetadataPersister;
use crate::import::pipeline;
use crate::import::progress_emitter::ImportProgressEmitter;
use crate::import::types::ImportProgress;
use crate::library_context::SharedLibraryManager;
use futures::stream::StreamExt;
use tokio::sync::mpsc;
use tracing::{error, info};

/// Configuration for import service
#[derive(Clone)]
pub struct ImportConfig {
    /// Number of parallel encryption workers (CPU-bound, typically 2x CPU cores)
    pub max_encrypt_workers: usize,
    /// Number of parallel upload workers (I/O-bound)
    pub max_upload_workers: usize,
    /// Size of each chunk in bytes
    pub chunk_size_bytes: usize,
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

        let service = ImportService {
            config,
            requests_rx,
            progress_tx,
            library_manager: library_manager.clone(),
            encryption_service,
            cloud_storage,
        };

        runtime_handle.spawn(service.run_import_worker());

        ImportHandle::new(requests_tx, progress_rx, library_manager, runtime_handle)
    }

    async fn run_import_worker(mut self) {
        info!("Worker started");

        // Import validated albums sequentially from the queue.
        loop {
            match self.requests_rx.recv().await {
                Some(request) => {
                    info!("Starting pipeline for '{}'", request.db_album.title);

                    if let Err(e) = self.import_from_folder(request).await {
                        error!("Pipeline failed: {}", e);
                        // TODO: Mark album as failed
                    }
                }
                None => {
                    info!("Channel closed");
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
    async fn import_from_folder(&self, request: ImportRequest) -> Result<(), String> {
        let library_manager = self.library_manager.get();

        let ImportRequest {
            db_album,
            tracks_to_files,
            discovered_files,
        } = request;

        // Mark album as importing now that pipeline is starting
        library_manager
            .mark_album_importing(&db_album.id)
            .await
            .map_err(|e| format!("Failed to mark album as importing: {}", e))?;

        info!("Marked album as 'importing' - starting pipeline");

        // Send started progress
        let _ = self.progress_tx.send(ImportProgress::Started {
            album_id: db_album.id.clone(),
        });

        // Analyze album layout: files → chunks → tracks
        let layout = AlbumDataLayout::build(
            discovered_files,
            &tracks_to_files,
            self.config.chunk_size_bytes,
        )?;

        info!(
            "Will stream {} chunks across {} files",
            layout.total_chunks,
            layout.file_mappings.len()
        );

        // Destructure layout to move ownership of each piece to where it's needed
        let AlbumDataLayout {
            file_mappings,
            total_chunks,
            chunk_to_track,
            track_chunk_counts,
        } = layout;

        // ========== STREAMING PIPELINE ==========
        // Read → Encrypt → Upload → Persist (bounded parallelism at each stage)

        let progress_emitter = ImportProgressEmitter::new(
            db_album.id.clone(),
            chunk_to_track,
            track_chunk_counts,
            self.progress_tx.clone(),
            total_chunks,
        );

        let (pipeline, chunk_tx) = pipeline::build_pipeline(
            self.config.clone(),
            db_album.id.clone(),
            self.encryption_service.clone(),
            self.cloud_storage.clone(),
            library_manager.clone(),
            progress_emitter,
        );

        // Spawn chunk producer task with file_mappings that tell it exactly what to read
        tokio::spawn(pipeline::chunk_producer::produce_chunk_stream(
            file_mappings.clone(), // Clone needed since we use it later for metadata
            self.config.chunk_size_bytes,
            chunk_tx,
        ));

        // Wait for the pipeline to complete
        let results: Vec<_> = pipeline.collect().await;

        // Check for errors (fail fast on first error)
        for result in results {
            result?;
        }

        info!("All {} chunks uploaded successfully", total_chunks);

        // ========== TEARDOWN ==========

        // Persist file metadata to database
        let persister = MetadataPersister::new(library_manager);
        persister
            .persist_album_metadata(
                &tracks_to_files,
                file_mappings,
                self.config.chunk_size_bytes,
            )
            .await?;

        // Mark album complete
        library_manager
            .mark_album_complete(&db_album.id)
            .await
            .map_err(|e| format!("Failed to mark album complete: {}", e))?;

        // Send completion event
        let _ = self.progress_tx.send(ImportProgress::Complete {
            album_id: db_album.id,
        });

        info!("Import completed successfully for {}", db_album.title);
        Ok(())
    }
}

// ============================================================================
// Validation Helper Functions
// ============================================================================
