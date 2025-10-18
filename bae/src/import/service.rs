// # Import Service
//
// Single-instance queue-based service that imports albums sequentially.
// One worker task handles import requests one at a time from a queue.
//
// Flow:
// 1. Validation & Queueing (synchronous per request):
//    - Validate track-to-file mapping
//    - Insert album/tracks with status='queued'
//    - Returns immediately so next request can be validated
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
use crate::database::DbAlbum;
use crate::encryption::EncryptionService;
use crate::import::album_layout::AlbumLayout;
use crate::import::album_track_creator;
use crate::import::metadata_persister::MetadataPersister;
use crate::import::pipeline;
use crate::import::progress_service::ImportProgressService;
use crate::import::track_file_mapper;
use crate::import::types::{ImportProgress, ImportRequest, TrackSourceFile};
use crate::library_context::SharedLibraryManager;
use futures::stream::StreamExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc;

/// Handle for sending import requests and subscribing to progress updates
#[derive(Clone)]
pub struct ImportHandle {
    validated_tx: mpsc::UnboundedSender<ValidatedImport>,
    progress_service: ImportProgressService,
    library_manager: SharedLibraryManager,
}

impl ImportHandle {
    /// Validate and queue an import request.
    ///
    /// Performs validation (track-to-file mapping) and DB insertion synchronously.
    /// If validation fails, returns error immediately with no side effects.
    /// If successful, album is inserted with status='queued' and sent to import worker.
    /// Returns the database album ID for progress subscription.
    pub async fn send_request(&self, request: ImportRequest) -> Result<String, String> {
        match request {
            ImportRequest::FromFolder { album, folder } => {
                let library_manager = self.library_manager.get();

                // ========== VALIDATION (before queueing) ==========

                // 1. Parse Discogs album into database models
                let (db_album, db_tracks) = album_track_creator::parse_discogs_album(&album)?;

                // 2. Discover files
                let folder_files = discover_folder_files(&folder)?;

                // 3. Validate track-to-file mapping
                let tracks_to_files =
                    track_file_mapper::map_tracks_to_files(&db_tracks, &folder_files).await?;

                // 4. Insert album + tracks with status='queued'
                library_manager
                    .insert_album_with_tracks(&db_album, &db_tracks)
                    .await
                    .map_err(|e| format!("Database error: {}", e))?;

                println!(
                    "ImportHandle: Validated and queued album '{}' with {} tracks",
                    db_album.title,
                    db_tracks.len()
                );

                // ========== QUEUE FOR PIPELINE ==========

                let album_id = db_album.id.clone();

                self.validated_tx
                    .send(ValidatedImport {
                        db_album,
                        tracks_to_files,
                        folder_files,
                    })
                    .map_err(|_| "Failed to queue validated album for import".to_string())?;

                Ok(album_id)
            }
        }
    }

    /// Subscribe to progress updates for a specific album
    /// Returns a filtered receiver that yields only updates for the specified album
    pub fn subscribe_album(
        &self,
        album_id: String,
    ) -> tokio::sync::mpsc::UnboundedReceiver<ImportProgress> {
        self.progress_service.subscribe_album(album_id)
    }

    /// Subscribe to progress updates for a specific track
    /// Returns a filtered receiver that yields only updates for the specified track
    pub fn subscribe_track(
        &self,
        album_id: String,
        track_id: String,
    ) -> tokio::sync::mpsc::UnboundedReceiver<ImportProgress> {
        self.progress_service.subscribe_track(album_id, track_id)
    }
}

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

/// Validated import ready for pipeline execution
struct ValidatedImport {
    db_album: DbAlbum,
    tracks_to_files: Vec<TrackSourceFile>,
    folder_files: Vec<DiscoveredFile>,
}

/// Import service that orchestrates the import workflow on the shared runtime
pub struct ImportService {
    library_manager: SharedLibraryManager,
    encryption_service: EncryptionService,
    cloud_storage: CloudStorageManager,
    progress_tx: mpsc::UnboundedSender<ImportProgress>,
    validated_rx: mpsc::UnboundedReceiver<ValidatedImport>,
    config: ImportConfig,
}

impl ImportService {
    /// Start the single import service worker for the entire app.
    ///
    /// Creates one worker task that imports validated albums sequentially from a queue.
    /// Multiple imports will be queued and handled one at a time, not concurrently.
    /// Returns a handle that can be cloned and used throughout the app to submit import requests.
    pub fn start(
        runtime_handle: tokio::runtime::Handle,
        library_manager: SharedLibraryManager,
        encryption_service: EncryptionService,
        cloud_storage: CloudStorageManager,
        config: ImportConfig,
    ) -> ImportHandle {
        let (validated_tx, validated_rx) = mpsc::unbounded_channel();
        let (progress_tx, progress_rx) = mpsc::unbounded_channel();

        // Create the import service worker
        let service = ImportService {
            library_manager: library_manager.clone(),
            encryption_service,
            cloud_storage,
            progress_tx,
            validated_rx,
            config,
        };

        // Spawn worker task that imports validated albums sequentially
        runtime_handle.spawn(service.run_import_worker());

        // Create ProgressService, used to broadcast progress updates to external subscribers
        let progress_service = ImportProgressService::new(progress_rx, runtime_handle.clone());

        ImportHandle {
            validated_tx,
            progress_service,
            library_manager,
        }
    }

    async fn run_import_worker(mut self) {
        println!("ImportService: Worker started");

        // Import validated albums sequentially from the queue.
        loop {
            match self.validated_rx.recv().await {
                Some(validated) => {
                    println!(
                        "ImportService: Starting pipeline for '{}'",
                        validated.db_album.title
                    );

                    if let Err(e) = self.import_from_folder(validated).await {
                        println!("ImportService: Pipeline failed: {}", e);
                        // TODO: Mark album as failed
                    }
                }
                None => {
                    println!("ImportService: Channel closed");
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
    async fn import_from_folder(&self, validated: ValidatedImport) -> Result<(), String> {
        let library_manager = self.library_manager.get();

        let ValidatedImport {
            db_album,
            tracks_to_files,
            folder_files,
        } = validated;

        // Mark album as importing now that pipeline is starting
        library_manager
            .mark_album_importing(&db_album.id)
            .await
            .map_err(|e| format!("Failed to mark album as importing: {}", e))?;

        println!("ImportService: Marked album as 'importing' - starting pipeline");

        // Send started progress
        let _ = self.progress_tx.send(ImportProgress::Started {
            album_id: db_album.id.clone(),
        });

        // Analyze album layout: files → chunks → tracks
        let layout = AlbumLayout::analyze(
            &folder_files,
            &tracks_to_files,
            self.config.chunk_size_bytes,
        )?;

        println!(
            "ImportService: Will stream {} chunks across {} files",
            layout.total_chunks,
            layout.file_mappings.len()
        );

        // ========== STREAMING PIPELINE ==========
        // Read → Encrypt → Upload → Persist (bounded parallelism at each stage)

        let results: Vec<_> = pipeline::build_pipeline(
            folder_files.clone(),
            self.config.clone(),
            db_album.id.clone(),
            self.encryption_service.clone(),
            self.cloud_storage.clone(),
            library_manager.clone(),
            Arc::new(layout.progress_tracker),
            self.progress_tx.clone(),
            layout.total_chunks,
        )
        .collect()
        .await;

        // Check for errors (fail fast on first error)
        for result in results {
            result?;
        }

        let file_mappings = layout.file_mappings;
        let total_chunks = layout.total_chunks;

        println!(
            "ImportService: All {} chunks uploaded successfully",
            total_chunks
        );

        // ========== TEARDOWN ==========

        // Persist file metadata to database
        let persister = MetadataPersister::new(library_manager);
        persister
            .persist_album_metadata(
                &tracks_to_files,
                &file_mappings,
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

        println!(
            "ImportService: Import completed successfully for {}",
            db_album.title
        );
        Ok(())
    }
}

// ============================================================================
// Validation Helper Functions
// ============================================================================

/// File discovered during initial filesystem scan.
///
/// Created during validation phase when we traverse the album folder once.
/// Used to calculate chunk layout and feed the reader task.
///
/// Example: `{ path: "/music/album/track01.flac", size: 45_821_345 }`
#[derive(Clone)]
pub struct DiscoveredFile {
    pub path: PathBuf,
    pub size: u64,
}

/// Discover all files in folder with metadata.
///
/// Single filesystem traversal to gather file paths and sizes upfront.
/// This avoids redundant directory reads later for CUE detection and chunk calculations.
/// Files are sorted by path for consistent ordering across runs.
fn discover_folder_files(folder: &Path) -> Result<Vec<DiscoveredFile>, String> {
    let mut files = Vec::new();

    for entry in std::fs::read_dir(folder).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();

        if path.is_file() {
            let size = entry
                .metadata()
                .map_err(|e| format!("Failed to read metadata for {:?}: {}", path, e))?
                .len();

            files.push(DiscoveredFile { path, size });
        }
    }

    // Sort by path for consistent ordering
    files.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(files)
}
