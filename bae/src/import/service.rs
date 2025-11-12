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

use crate::cache::CacheManager;
use crate::cloud_storage::CloudStorageManager;
use crate::encryption::EncryptionService;
use crate::import::album_chunk_layout::AlbumChunkLayout;
use crate::import::handle::ImportHandle;
use crate::import::metadata_persister::MetadataPersister;
use crate::import::pipeline;
use crate::import::progress::ImportProgressTracker;
use crate::import::types::{ImportCommand, ImportProgress};
use crate::library::SharedLibraryManager;
use crate::torrent::{BaeStorage, TorrentClient};
use futures::stream::StreamExt;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

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
    /// Channel for receiving import commands from clients
    requests_rx: mpsc::UnboundedReceiver<ImportCommand>,
    /// Channel for sending progress updates to subscribers
    progress_tx: mpsc::UnboundedSender<ImportProgress>,
    /// Service for encrypting files before upload
    encryption_service: EncryptionService,
    /// Service for uploading encrypted chunks to cloud storage
    cloud_storage: CloudStorageManager,
    /// Shared manager for library database operations
    library_manager: SharedLibraryManager,
    /// Cache manager for chunk storage
    cache_manager: CacheManager,
    /// Shared torrent client for reuse across imports
    torrent_client: TorrentClient,
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
        cache_manager: CacheManager,
    ) -> ImportHandle {
        let (requests_tx, requests_rx) = mpsc::unbounded_channel();
        let (progress_tx, progress_rx) = mpsc::unbounded_channel();

        // Clone library_manager and cache_manager for the thread
        let library_manager_for_worker = library_manager.clone();
        let cache_manager_for_worker = cache_manager.clone();

        // Spawn the service task on a dedicated thread (TorrentClient isn't Send-safe due to FFI)
        // This follows the same pattern as PlaybackService for handling non-Send types
        std::thread::spawn(move || {
            // Create a new tokio runtime for this thread
            let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");
            let worker_runtime_handle = rt.handle().clone();

            rt.block_on(async move {
                let torrent_client = TorrentClient::new(worker_runtime_handle)
                    .expect("Failed to create shared torrent client for import service");

                let service = ImportService {
                    config,
                    requests_rx,
                    progress_tx,
                    library_manager: library_manager_for_worker,
                    encryption_service,
                    cloud_storage,
                    cache_manager: cache_manager_for_worker,
                    torrent_client,
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
                Some(command) => {
                    let result = match &command {
                        ImportCommand::FolderImport { db_album, .. } => {
                            info!("Starting folder import pipeline for '{}'", db_album.title);
                            self.import_album_from_folder(command).await
                        }
                        ImportCommand::TorrentImport { db_album, .. } => {
                            info!("Starting torrent import pipeline for '{}'", db_album.title);
                            self.import_album_from_torrent(command).await
                        }
                    };

                    if let Err(e) = result {
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

    /// Executes the streaming import pipeline for a folder-based import.
    ///
    /// Orchestrates the entire import workflow:
    /// 1. Marks the album as 'importing'
    /// 2. Streams files → encrypts → uploads (no upfront layout computation)
    /// 3. After upload: computes layout, persists metadata, marks complete
    async fn import_album_from_folder(&self, command: ImportCommand) -> Result<(), String> {
        let library_manager = self.library_manager.get();

        let ImportCommand::FolderImport {
            db_album,
            db_release,
            tracks_to_files,
            discovered_files,
            cue_flac_metadata,
        } = command
        else {
            return Err("Expected FolderImport command".to_string());
        };

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

        // Estimate chunk count for progress tracking
        let total_size: usize = discovered_files.iter().map(|f| f.size as usize).sum();
        let estimated_total_chunks = total_size.div_ceil(self.config.chunk_size_bytes);

        // ========== STREAMING PIPELINE ==========
        // Stream chunks first, compute layout after upload completes

        let progress_tracker = ImportProgressTracker::new(
            db_release.id.clone(),
            estimated_total_chunks,
            std::collections::HashMap::new(), // Empty chunk_to_track mapping for now
            std::collections::HashMap::new(), // Empty track_chunk_counts for now
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
            Vec::new(), // Empty files_to_chunks for now - we'll compute after upload
            self.config.chunk_size_bytes,
            std::collections::HashMap::new(), // Empty CUE/FLAC data for now
        );

        // Folder import: use file producer
        // Create minimal FileToChunks - the producer only needs file_path, other fields are unused
        let files_to_chunks_for_producer: Vec<crate::import::types::FileToChunks> =
            discovered_files
                .iter()
                .map(|f| crate::import::types::FileToChunks {
                    file_path: f.path.clone(),
                    start_chunk_index: 0, // Unused by producer
                    end_chunk_index: 0,   // Unused by producer
                    start_byte_offset: 0, // Unused by producer
                    end_byte_offset: 0,   // Unused by producer
                })
                .collect();
        tokio::spawn(pipeline::chunk_producer::produce_chunk_stream_from_files(
            files_to_chunks_for_producer,
            self.config.chunk_size_bytes,
            chunk_tx,
        ));

        // Wait for the pipeline to complete
        let results: Vec<_> = pipeline.collect().await;

        // Check for errors
        for result in results {
            result?;
        }

        info!("All chunks uploaded successfully, computing layout...");

        // ========== COMPUTE LAYOUT AND PERSIST METADATA ==========

        self.compute_layout_and_persist_metadata(
            library_manager,
            &db_release.id,
            &tracks_to_files,
            &discovered_files,
            cue_flac_metadata,
        )
        .await?;

        // Send completion event
        let _ = self
            .progress_tx
            .send(ImportProgress::Complete { id: db_release.id });

        info!("Import completed successfully for {}", db_album.title);
        Ok(())
    }

    /// Executes the streaming import pipeline for a torrent-based import.
    ///
    /// Orchestrates the entire import workflow:
    /// 1. Marks the album as 'importing'
    /// 2. Streams torrent pieces → chunks → encrypts → uploads (no upfront layout computation)
    /// 3. After torrent completes: extracts FLAC headers, builds seektable, computes layout
    /// 4. Persists metadata and marks album complete.
    async fn import_album_from_torrent(&self, command: ImportCommand) -> Result<(), String> {
        let library_manager = self.library_manager.get();

        let ImportCommand::TorrentImport {
            db_album,
            db_release,
            tracks_to_files,
            torrent_source,
            torrent_metadata,
            seed_after_download: _,
        } = command
        else {
            return Err("Expected TorrentImport command".to_string());
        };

        // Mark release as importing now that pipeline is starting
        library_manager
            .mark_release_importing(&db_release.id)
            .await
            .map_err(|e| format!("Failed to mark release as importing: {}", e))?;

        info!("Marked release as 'importing' - starting torrent pipeline");

        // Send started progress
        let _ = self.progress_tx.send(ImportProgress::Started {
            id: db_release.id.clone(),
        });

        // ========== TORRENT SETUP ==========

        use crate::torrent::TorrentPieceMapper;

        // Use shared torrent client and create handle, then register storage
        let torrent_handle = {
            let torrent_client = self.torrent_client.clone();

            let torrent_handle = match torrent_source {
                crate::import::types::TorrentSource::File(path) => torrent_client
                    .add_torrent_file(&path)
                    .await
                    .map_err(|e| format!("Failed to add torrent file: {}", e))?,
                crate::import::types::TorrentSource::MagnetLink(magnet) => torrent_client
                    .add_magnet_link(&magnet)
                    .await
                    .map_err(|e| format!("Failed to add magnet link: {}", e))?,
            };

            // Wait for metadata if needed
            torrent_handle
                .wait_for_metadata()
                .await
                .map_err(|e| format!("Failed to wait for metadata: {}", e))?;

            // Get storage_index and register storage
            let storage_index = torrent_handle
                .storage_index()
                .await
                .map_err(|e| format!("Failed to get storage index: {}", e))?;

            // Create piece mapper for BaeStorage
            let piece_mapper = TorrentPieceMapper::new(
                torrent_metadata.piece_length as usize,
                self.config.chunk_size_bytes,
                torrent_metadata.num_pieces as usize,
                torrent_metadata.total_size_bytes as usize,
            );

            // Create BaeStorage instance
            let bae_storage = BaeStorage::new(
                self.cache_manager.clone(),
                self.library_manager.database().clone(),
                piece_mapper,
                torrent_metadata.info_hash.clone(),
                db_release.id.clone(),
            );

            // Register storage with torrent client
            torrent_client
                .register_storage(
                    storage_index,
                    torrent_metadata.info_hash.clone(),
                    bae_storage,
                )
                .await;

            torrent_handle
        };

        // ========== STREAMING PIPELINE ==========
        // Stream torrent pieces → chunks → encrypt → upload (no layout computation yet)

        // For torrent imports, we don't know the exact chunk count upfront.
        // We'll use a placeholder and update as we go, or calculate from total size.
        let estimated_total_chunks =
            (torrent_metadata.total_size_bytes as usize).div_ceil(self.config.chunk_size_bytes);

        // Create a minimal progress tracker (we'll update it properly after layout computation)
        // Note: For torrent imports, tracks won't be marked complete during streaming
        // because we don't have layout data yet. We'll persist track metadata after layout computation.
        let progress_tracker = ImportProgressTracker::new(
            db_release.id.clone(),
            estimated_total_chunks,           // usize, not i32
            std::collections::HashMap::new(), // Empty chunk_to_track mapping for now
            std::collections::HashMap::new(), // Empty track_chunk_counts for now
            self.progress_tx.clone(),
        );

        // Build pipeline (without CUE/FLAC data - we'll add that later)
        let (pipeline, chunk_tx) = pipeline::build_import_pipeline(
            self.config.clone(),
            db_release.id.clone(),
            self.encryption_service.clone(),
            self.cloud_storage.clone(),
            library_manager.clone(),
            progress_tracker,
            tracks_to_files.clone(),
            Vec::new(), // Empty files_to_chunks for now - we'll compute after download
            self.config.chunk_size_bytes,
            std::collections::HashMap::new(), // Empty CUE/FLAC data for now
        );

        // Create piece mapper for torrent producer
        let piece_mapper = TorrentPieceMapper::new(
            torrent_metadata.piece_length as usize,
            self.config.chunk_size_bytes,
            torrent_metadata.num_pieces as usize,
            torrent_metadata.total_size_bytes as usize,
        );

        // Run torrent producer (streams pieces → chunks)
        let piece_mappings = pipeline::torrent_producer::produce_chunk_stream_from_torrent(
            &torrent_handle,
            piece_mapper,
            self.config.chunk_size_bytes,
            chunk_tx,
        )
        .await;

        // Wait for the pipeline to complete
        let results: Vec<_> = pipeline.collect().await;

        // Check for errors
        for result in results {
            result?;
        }

        info!("All chunks uploaded successfully, waiting for torrent to complete...");

        // ========== WAIT FOR TORRENT COMPLETION ==========

        // Wait for torrent to finish downloading
        loop {
            let progress = torrent_handle
                .progress()
                .await
                .map_err(|e| format!("Failed to check torrent progress: {}", e))?;
            if progress >= 1.0 {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }

        // Wait a bit for libtorrent to finish writing files to disk
        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

        info!("Torrent download complete, computing layout...");

        // ========== COMPUTE LAYOUT AFTER DOWNLOAD ==========

        // Get file list from torrent to construct discovered_files
        let torrent_files = torrent_handle
            .get_file_list()
            .await
            .map_err(|e| format!("Failed to get torrent file list: {}", e))?;

        // Convert torrent files to DiscoveredFile format
        let temp_dir = std::env::temp_dir();
        let discovered_files: Vec<crate::import::types::DiscoveredFile> = torrent_files
            .iter()
            .map(|tf| crate::import::types::DiscoveredFile {
                path: temp_dir.join(&tf.path),
                size: tf.size as u64,
            })
            .collect();

        // Re-detect and parse CUE/FLAC files now that they're on disk
        // This will extract FLAC headers and build seektables
        let file_paths: Vec<std::path::PathBuf> =
            discovered_files.iter().map(|f| f.path.clone()).collect();
        let cue_flac_pairs =
            crate::cue_flac::CueFlacProcessor::detect_cue_flac_from_paths(&file_paths)
                .map_err(|e| format!("Failed to detect CUE/FLAC files: {}", e))?;

        // Parse CUE files to get metadata (FLAC headers and seektables will be extracted in layout computation)
        let mut cue_flac_metadata = std::collections::HashMap::new();
        for pair in cue_flac_pairs {
            let flac_path = pair.flac_path.clone();
            let cue_sheet = crate::cue_flac::CueFlacProcessor::parse_cue_sheet(&pair.cue_path)
                .map_err(|e| format!("Failed to parse CUE sheet: {}", e))?;
            let metadata = crate::import::types::CueFlacMetadata {
                cue_sheet,
                cue_path: pair.cue_path,
                flac_path: flac_path.clone(),
            };
            cue_flac_metadata.insert(flac_path, metadata);
        }

        info!("Layout computed, persisting metadata...");

        // ========== COMPUTE LAYOUT AND PERSIST METADATA ==========

        // Re-detect and parse CUE/FLAC files now that they're on disk
        let file_paths: Vec<std::path::PathBuf> =
            discovered_files.iter().map(|f| f.path.clone()).collect();
        let cue_flac_pairs =
            crate::cue_flac::CueFlacProcessor::detect_cue_flac_from_paths(&file_paths)
                .map_err(|e| format!("Failed to detect CUE/FLAC files: {}", e))?;

        // Parse CUE files to get metadata
        let mut cue_flac_metadata_map = std::collections::HashMap::new();
        for pair in cue_flac_pairs {
            let flac_path = pair.flac_path.clone();
            let cue_sheet = crate::cue_flac::CueFlacProcessor::parse_cue_sheet(&pair.cue_path)
                .map_err(|e| format!("Failed to parse CUE sheet: {}", e))?;
            let metadata = crate::import::types::CueFlacMetadata {
                cue_sheet,
                cue_path: pair.cue_path,
                flac_path: flac_path.clone(),
            };
            cue_flac_metadata_map.insert(flac_path, metadata);
        }

        self.compute_layout_and_persist_metadata(
            library_manager,
            &db_release.id,
            &tracks_to_files,
            &discovered_files,
            Some(cue_flac_metadata_map),
        )
        .await?;

        // ========== SAVE PIECE MAPPINGS ==========

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

        // ========== CLEANUP TEMPORARY FILES ==========

        // Clean up temporary downloaded files
        let temp_dir = std::env::temp_dir();
        let torrent_save_dir = temp_dir.join(&torrent_metadata.torrent_name);
        if torrent_save_dir.exists() {
            match tokio::fs::remove_dir_all(&torrent_save_dir).await {
                Ok(_) => {
                    info!("Cleaned up temporary torrent files: {:?}", torrent_save_dir);
                }
                Err(e) => {
                    warn!(
                        "Failed to clean up temporary torrent files {:?}: {}",
                        torrent_save_dir, e
                    );
                    // Don't fail the import if cleanup fails
                }
            }
        }

        // Send completion event
        let _ = self
            .progress_tx
            .send(ImportProgress::Complete { id: db_release.id });

        info!(
            "Torrent import completed successfully for {}",
            db_album.title
        );
        Ok(())
    }

    /// Compute chunk layout and persist all metadata after chunk upload completes.
    ///
    /// Shared helper for both folder and torrent imports. Called after all chunks
    /// have been uploaded to cloud storage. Computes layout, persists track metadata,
    /// persists release metadata, and marks the release complete.
    async fn compute_layout_and_persist_metadata(
        &self,
        library_manager: &crate::library::LibraryManager,
        release_id: &str,
        tracks_to_files: &[crate::import::types::TrackFile],
        discovered_files: &[crate::import::types::DiscoveredFile],
        cue_flac_metadata: Option<
            std::collections::HashMap<std::path::PathBuf, crate::import::types::CueFlacMetadata>,
        >,
    ) -> Result<(), String> {
        // Compute chunk layout
        let chunk_layout = AlbumChunkLayout::build(
            discovered_files.to_vec(),
            tracks_to_files,
            self.config.chunk_size_bytes,
            cue_flac_metadata,
        )?;

        let AlbumChunkLayout {
            files_to_chunks,
            total_chunks: _,
            chunk_to_track: _,
            track_chunk_counts: _,
            cue_flac_data,
        } = chunk_layout;

        // Persist track metadata for all tracks
        let persister = MetadataPersister::new(library_manager);
        for track_file in tracks_to_files {
            persister
                .persist_track_metadata(
                    release_id,
                    &track_file.db_track_id,
                    tracks_to_files,
                    &files_to_chunks,
                    self.config.chunk_size_bytes,
                    &cue_flac_data,
                )
                .await
                .map_err(|e| format!("Failed to persist track metadata: {}", e))?;

            // Mark track complete
            library_manager
                .mark_track_complete(&track_file.db_track_id)
                .await
                .map_err(|e| format!("Failed to mark track complete: {}", e))?;
        }

        // Persist release-level metadata
        persister
            .persist_release_metadata(release_id, tracks_to_files, &files_to_chunks)
            .await?;

        // Mark release complete
        library_manager
            .mark_release_complete(release_id)
            .await
            .map_err(|e| format!("Failed to mark release complete: {}", e))?;

        Ok(())
    }
}

// ============================================================================
// Validation Helper Functions
// ============================================================================
