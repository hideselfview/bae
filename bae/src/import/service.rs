// # Import Service
//
// Single-instance queue-based service that imports albums.
// Runs on dedicated thread with own tokio runtime, handles import requests sequentially.
//
// Two-phase import model:
// 1. Acquire Phase: Get data ready (folder: no-op, torrent: download, CD: rip)
// 2. Chunk Phase: Upload and encrypt (same for all types)
//
// Flow:
// 1. Validation & Queueing (in ImportHandle, synchronous):
//    - Validate track-to-file mapping
//    - Insert album/tracks with status='queued'
//    - Send ImportCommand to service
//
// 2. Acquire Phase (async, in ImportService):
//    - Folder: Instant (no work)
//    - Torrent: Download torrent, emit progress with ImportPhase::Acquire
//    - CD: Rip tracks, emit progress with ImportPhase::Acquire
//
// 3. Chunk Phase (async, in ImportService::run_chunk_phase):
//    - Mark album as 'importing'
//    - Streaming pipeline: read → encrypt → upload → persist (bounded parallelism)
//    - Emit progress with ImportPhase::Chunk
//    - Mark album/tracks as 'complete'
//
// Architecture:
// - ImportHandle: Validates requests, inserts DB records, sends commands
// - ImportService: Executes acquire + chunk phases on dedicated thread
// - ImportProgressTracker: Tracks chunk completion, emits progress events
// - MetadataPersister: Saves file/chunk metadata to DB

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
    commands_rx: mpsc::UnboundedReceiver<ImportCommand>,
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
        let (commands_tx, commands_rx) = mpsc::unbounded_channel();
        let (progress_tx, progress_rx) = mpsc::unbounded_channel();

        // Clone progress_tx for the handle (before moving it into the service)
        let progress_tx_for_handle = progress_tx.clone();

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
                    commands_rx,
                    progress_tx,
                    library_manager: library_manager_for_worker,
                    encryption_service,
                    cloud_storage,
                    cache_manager: cache_manager_for_worker,
                    torrent_client,
                };

                service.receive_import_commands().await;
            });
        });

        ImportHandle::new(
            commands_tx,
            progress_tx_for_handle,
            progress_rx,
            library_manager,
            runtime_handle,
        )
    }

    async fn receive_import_commands(mut self) {
        info!("Worker started");

        // Import validated albums sequentially from the queue.
        loop {
            match self.commands_rx.recv().await {
                Some(command) => {
                    let result = match &command {
                        ImportCommand::Folder { db_album, .. } => {
                            info!("Starting folder import pipeline for '{}'", db_album.title);
                            self.import_album_from_folder(command).await
                        }
                        ImportCommand::Torrent { db_album, .. } => {
                            info!("Starting torrent import pipeline for '{}'", db_album.title);
                            self.import_album_from_torrent(command).await
                        }
                        ImportCommand::CD { db_album, .. } => {
                            info!("Starting CD import pipeline for '{}'", db_album.title);
                            self.import_album_from_cd(command).await
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

        let ImportCommand::Folder {
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

        // ========== CHUNK PHASE ==========
        // Folder import has no acquire phase (files already available)
        // Run chunk phase directly

        self.run_chunk_phase(
            &db_release,
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

        let ImportCommand::Torrent {
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

        // ========== ACQUIRE PHASE: TORRENT DOWNLOAD ==========

        info!("Starting torrent download (acquire phase)");

        // Use shared torrent client and create handle
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

        // Get storage_index and register storage (for BaeStorage)
        let storage_index = torrent_handle
            .storage_index()
            .await
            .map_err(|e| format!("Failed to get storage index: {}", e))?;

        use crate::torrent::TorrentPieceMapper;
        let piece_mapper = TorrentPieceMapper::new(
            torrent_metadata.piece_length as usize,
            self.config.chunk_size_bytes,
            torrent_metadata.num_pieces as usize,
            torrent_metadata.total_size_bytes as usize,
        );

        let bae_storage = BaeStorage::new(
            self.cache_manager.clone(),
            self.library_manager.database().clone(),
            piece_mapper,
            torrent_metadata.info_hash.clone(),
        );

        torrent_client
            .register_storage(
                storage_index,
                torrent_metadata.info_hash.clone(),
                bae_storage,
            )
            .await;

        // Download torrent and emit progress (Acquire phase)
        loop {
            let progress = torrent_handle
                .progress()
                .await
                .map_err(|e| format!("Failed to check torrent progress: {}", e))?;

            let percent = (progress * 100.0) as u8;
            let _ = self.progress_tx.send(ImportProgress::Progress {
                id: db_release.id.clone(),
                percent,
                phase: Some(crate::import::types::ImportPhase::Acquire),
            });

            if progress >= 1.0 {
                break;
            }

            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }

        // Wait a bit for libtorrent to finish writing files to disk
        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

        info!("Torrent download (acquire phase) complete, starting chunk phase");

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

        // Detect and parse CUE/FLAC files
        let file_paths: Vec<std::path::PathBuf> =
            discovered_files.iter().map(|f| f.path.clone()).collect();
        let cue_flac_pairs =
            crate::cue_flac::CueFlacProcessor::detect_cue_flac_from_paths(&file_paths)
                .map_err(|e| format!("Failed to detect CUE/FLAC files: {}", e))?;

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

        // ========== CHUNK PHASE ==========
        // Now that data is acquired, run chunk phase (same as folder import)

        self.run_chunk_phase(
            &db_release,
            &tracks_to_files,
            &discovered_files,
            Some(cue_flac_metadata),
        )
        .await?;

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

    /// Run the chunk phase: compute layout, stream chunks, upload, and persist metadata.
    ///
    /// This is the common chunk upload phase used by all import types after data acquisition.
    /// For folder imports, this runs immediately (no acquire phase).
    /// For CD/torrent imports, this runs after the acquire phase completes.
    async fn run_chunk_phase(
        &self,
        db_release: &crate::db::DbRelease,
        tracks_to_files: &[crate::import::types::TrackFile],
        discovered_files: &[crate::import::types::DiscoveredFile],
        cue_flac_metadata: Option<
            std::collections::HashMap<std::path::PathBuf, crate::import::types::CueFlacMetadata>,
        >,
    ) -> Result<(), String> {
        let library_manager = self.library_manager.get();

        // ========== COMPUTE LAYOUT FIRST ==========
        // Compute the layout before streaming so we have accurate progress tracking

        let chunk_layout = AlbumChunkLayout::build(
            discovered_files.to_vec(),
            tracks_to_files,
            self.config.chunk_size_bytes,
            cue_flac_metadata.clone(),
        )?;

        // ========== STREAMING PIPELINE ==========
        // Stream chunks with accurate progress tracking

        let progress_tracker = ImportProgressTracker::new(
            db_release.id.clone(),
            chunk_layout.total_chunks,
            chunk_layout.chunk_to_track.clone(),
            chunk_layout.track_chunk_counts.clone(),
            self.progress_tx.clone(),
        );

        let (pipeline, chunk_tx) = pipeline::build_import_pipeline(
            self.config.clone(),
            db_release.id.clone(),
            self.encryption_service.clone(),
            self.cloud_storage.clone(),
            library_manager.clone(),
            progress_tracker,
            tracks_to_files.to_vec(),
            chunk_layout.files_to_chunks.clone(),
            self.config.chunk_size_bytes,
            chunk_layout.cue_flac_data.clone(),
        );

        // Use file producer
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

        info!("All chunks uploaded successfully, persisting metadata...");

        // ========== PERSIST METADATA ==========
        // Layout already computed at the beginning, just persist it now

        self.persist_metadata_from_layout(
            library_manager,
            &db_release.id,
            tracks_to_files,
            &chunk_layout.files_to_chunks,
            &chunk_layout.cue_flac_data,
        )
        .await?;

        Ok(())
    }

    /// Persist metadata from an already-computed chunk layout.
    ///
    /// Used by folder imports where layout is computed upfront for accurate progress tracking.
    async fn persist_metadata_from_layout(
        &self,
        library_manager: &crate::library::LibraryManager,
        release_id: &str,
        tracks_to_files: &[crate::import::types::TrackFile],
        files_to_chunks: &[crate::import::types::FileToChunks],
        cue_flac_data: &std::collections::HashMap<
            std::path::PathBuf,
            crate::import::types::CueFlacLayoutData,
        >,
    ) -> Result<(), String> {
        // Persist track metadata for all tracks
        let persister = MetadataPersister::new(library_manager);
        for track_file in tracks_to_files {
            persister
                .persist_track_metadata(
                    release_id,
                    &track_file.db_track_id,
                    tracks_to_files,
                    files_to_chunks,
                    self.config.chunk_size_bytes,
                    cue_flac_data,
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
            .persist_release_metadata(release_id, tracks_to_files, files_to_chunks)
            .await?;

        // Mark release complete
        library_manager
            .mark_release_complete(release_id)
            .await
            .map_err(|e| format!("Failed to mark release complete: {}", e))?;

        Ok(())
    }

    /// Compute chunk layout and persist all metadata after chunk upload completes.
    ///
    /// Used by torrent imports where layout can't be computed upfront.
    /// Called after all chunks have been uploaded to cloud storage. Computes layout,
    /// persists track metadata, persists release metadata, and marks the release complete.
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

        // Persist using the computed layout
        self.persist_metadata_from_layout(
            library_manager,
            release_id,
            tracks_to_files,
            &chunk_layout.files_to_chunks,
            &chunk_layout.cue_flac_data,
        )
        .await
    }

    /// Executes the streaming import pipeline for a CD-based import.
    ///
    /// Orchestrates the entire import workflow:
    /// 1. Marks the album as 'importing'
    /// 2. **Acquire phase**: Rip CD tracks to FLAC files
    /// 3. **Chunk phase**: Stream ripped files → encrypts → uploads
    /// 4. After upload: persists metadata, marks complete
    /// 5. Cleans up temporary directory
    async fn import_album_from_cd(&self, command: ImportCommand) -> Result<(), String> {
        let library_manager = self.library_manager.get();

        let ImportCommand::CD {
            db_album,
            db_release,
            db_tracks,
            drive_path,
            toc,
        } = command
        else {
            return Err("Expected CdImport command".to_string());
        };

        // Mark release as importing now that pipeline is starting
        library_manager
            .mark_release_importing(&db_release.id)
            .await
            .map_err(|e| format!("Failed to mark release as importing: {}", e))?;

        info!("Marked release as 'importing' - starting CD import pipeline");

        // Send started progress
        let _ = self.progress_tx.send(ImportProgress::Started {
            id: db_release.id.clone(),
        });

        // ========== ACQUIRE PHASE: CD RIPPING ==========

        info!(
            "Starting CD ripping (acquire phase) for {} tracks",
            toc.last_track - toc.first_track + 1
        );

        // Create temporary directory for ripped files
        let temp_dir = std::env::temp_dir().join(format!("bae_cd_rip_{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp_dir)
            .await
            .map_err(|e| format!("Failed to create temp directory: {}", e))?;

        // Create CD drive and ripper
        use crate::cd::{CdDrive, CdRipper, CueGenerator, LogGenerator};
        let drive = CdDrive {
            device_path: drive_path.clone(),
            name: drive_path.to_str().unwrap_or("Unknown").to_string(),
        };
        let ripper = CdRipper::new(drive.clone(), toc.clone(), temp_dir.clone());

        // Create progress channel for ripping
        let (rip_progress_tx, mut rip_progress_rx) =
            tokio::sync::mpsc::unbounded_channel::<crate::cd::RipProgress>();

        // Map track numbers (1-indexed) to track IDs
        let track_number_to_id: std::collections::HashMap<u8, String> = db_tracks
            .iter()
            .enumerate()
            .map(|(idx, track)| {
                // Track numbers are 1-indexed, enumerate is 0-indexed
                let track_num = toc.first_track + idx as u8;
                (track_num, track.id.clone())
            })
            .collect();

        // Spawn task to forward ripping progress to UI (Acquire phase)
        let release_id_for_progress = db_release.id.clone();
        let progress_tx_for_ripping = self.progress_tx.clone();
        let track_number_to_id_for_progress = track_number_to_id.clone();
        tokio::spawn(async move {
            while let Some(rip_progress) = rip_progress_rx.recv().await {
                use crate::import::types::ImportPhase;

                // Send release-level progress (Acquire phase)
                let _ = progress_tx_for_ripping.send(ImportProgress::Progress {
                    id: release_id_for_progress.clone(),
                    percent: rip_progress.percent,
                    phase: Some(ImportPhase::Acquire),
                });

                // Send track-level progress (Acquire phase) for the current track
                if let Some(track_id) = track_number_to_id_for_progress.get(&rip_progress.track) {
                    let _ = progress_tx_for_ripping.send(ImportProgress::Progress {
                        id: track_id.clone(),
                        percent: rip_progress.track_percent,
                        phase: Some(ImportPhase::Acquire),
                    });
                }
            }
        });

        // Rip all tracks
        let rip_results = ripper
            .rip_all_tracks(Some(rip_progress_tx))
            .await
            .map_err(|e| format!("Failed to rip CD: {}", e))?;

        info!("CD ripping completed, {} tracks ripped", rip_results.len());

        // Generate CUE sheet and log file
        // Note: Artist name is just for CUE metadata, use placeholder if not available
        let artist_name = "Unknown Artist".to_string();
        let flac_filename = format!("{}.flac", db_album.title.replace("/", "_"));
        let cue_sheet = CueGenerator::generate_cue_sheet(
            &toc,
            &rip_results,
            &flac_filename,
            &artist_name,
            &db_album.title,
        );

        let cue_path = temp_dir.join(format!("{}.cue", db_album.title.replace("/", "_")));
        CueGenerator::write_cue_file(&cue_sheet, &toc.disc_id, &flac_filename, &cue_path)
            .map_err(|e| format!("Failed to write CUE file: {}", e))?;

        let log_path = temp_dir.join(format!("{}.log", db_album.title.replace("/", "_")));
        LogGenerator::write_log_file(&toc, &rip_results, &drive.name, &log_path)
            .map_err(|e| format!("Failed to write log file: {}", e))?;

        // Discover files after ripping
        let mut discovered_files = Vec::new();
        for result in &rip_results {
            let metadata = tokio::fs::metadata(&result.output_path)
                .await
                .map_err(|e| format!("Failed to get file size: {}", e))?;
            discovered_files.push(crate::import::types::DiscoveredFile {
                path: result.output_path.clone(),
                size: metadata.len(),
            });
        }

        // Add CUE and log files
        let cue_metadata = tokio::fs::metadata(&cue_path)
            .await
            .map_err(|e| format!("Failed to get CUE file size: {}", e))?;
        discovered_files.push(crate::import::types::DiscoveredFile {
            path: cue_path.clone(),
            size: cue_metadata.len(),
        });

        let log_metadata = tokio::fs::metadata(&log_path)
            .await
            .map_err(|e| format!("Failed to get log file size: {}", e))?;
        discovered_files.push(crate::import::types::DiscoveredFile {
            path: log_path.clone(),
            size: log_metadata.len(),
        });

        // Map tracks to files
        use crate::import::track_to_file_mapper::map_tracks_to_files;
        let mapping_result = map_tracks_to_files(&db_tracks, &discovered_files)
            .await
            .map_err(|e| format!("Failed to map tracks to files: {}", e))?;
        let tracks_to_files = mapping_result.track_files.clone();
        let cue_flac_metadata = mapping_result.cue_flac_metadata.clone();

        // Extract and store durations
        crate::import::handle::extract_and_store_durations(library_manager, &tracks_to_files)
            .await
            .map_err(|e| format!("Failed to extract durations: {}", e))?;

        info!("CD ripping (acquire phase) complete, starting chunk phase");

        // ========== CHUNK PHASE ==========
        // Now that data is acquired, run chunk phase (same as folder import)

        self.run_chunk_phase(
            &db_release,
            &tracks_to_files,
            &discovered_files,
            cue_flac_metadata,
        )
        .await?;

        // ========== CLEANUP TEMP DIRECTORY ==========
        // Remove temporary directory with ripped files
        if let Err(e) = tokio::fs::remove_dir_all(&temp_dir).await {
            warn!("Failed to remove temp directory {:?}: {}", temp_dir, e);
            // Don't fail the import if cleanup fails
        } else {
            info!("Cleaned up temp directory: {:?}", temp_dir);
        }

        // Send completion event
        let _ = self
            .progress_tx
            .send(ImportProgress::Complete { id: db_release.id });

        info!("CD import completed successfully for {}", db_album.title);
        Ok(())
    }
}

// ============================================================================
// Validation Helper Functions
// ============================================================================
