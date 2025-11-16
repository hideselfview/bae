use crate::cache::CacheManager;
use crate::db::{Database, DbTorrent, DbTorrentPieceMapping};
use crate::import::TorrentSource;
use crate::torrent::client::{TorrentClient, TorrentError, TorrentHandle};
use crate::torrent::{BaeStorage, TorrentPieceMapper};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};
use tracing::{error, info};

#[derive(Error, Debug)]
pub enum SeederError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("Cache error: {0}")]
    Cache(#[from] crate::cache::CacheError),
    #[error("Torrent error: {0}")]
    Torrent(#[from] TorrentError),
    #[error("Piece mapping error: {0}")]
    PieceMapping(String),
}

/// Commands sent to the torrent manager service
pub enum TorrentManagerCommand {
    // Download/metadata operations (use download_client)
    AddTorrent {
        source: TorrentSource,
        response_tx: oneshot::Sender<Result<TorrentHandle, TorrentError>>,
    },
    RemoveTorrent {
        handle: TorrentHandle,
        delete_files: bool,
        response_tx: oneshot::Sender<Result<(), TorrentError>>,
    },

    // Seeding operations (use seeding_client)
    StartSeeding {
        release_id: String,
        response_tx: oneshot::Sender<Result<(), SeederError>>,
    },
    StopSeeding {
        release_id: String,
        response_tx: oneshot::Sender<Result<(), SeederError>>,
    },
}

/// Handle to the torrent manager service for sending commands
#[derive(Clone)]
pub struct TorrentManagerHandle {
    command_tx: mpsc::UnboundedSender<TorrentManagerCommand>,
}

impl TorrentManagerHandle {
    /// Add a torrent for download/metadata detection
    pub async fn add_torrent(&self, source: TorrentSource) -> Result<TorrentHandle, TorrentError> {
        let (tx, rx) = oneshot::channel();
        self.command_tx
            .send(TorrentManagerCommand::AddTorrent {
                source,
                response_tx: tx,
            })
            .map_err(|_| TorrentError::Libtorrent("TorrentManager channel closed".to_string()))?;
        rx.await.map_err(|_| {
            TorrentError::Libtorrent("TorrentManager response channel closed".to_string())
        })?
    }

    /// Remove a torrent
    pub async fn remove_torrent(
        &self,
        handle: TorrentHandle,
        delete_files: bool,
    ) -> Result<(), TorrentError> {
        let (tx, rx) = oneshot::channel();
        self.command_tx
            .send(TorrentManagerCommand::RemoveTorrent {
                handle,
                delete_files,
                response_tx: tx,
            })
            .map_err(|_| TorrentError::Libtorrent("TorrentManager channel closed".to_string()))?;
        rx.await.map_err(|_| {
            TorrentError::Libtorrent("TorrentManager response channel closed".to_string())
        })?
    }

    /// Start seeding a torrent for a release
    pub async fn start_seeding(&self, release_id: String) -> Result<(), SeederError> {
        let (tx, rx) = oneshot::channel();
        self.command_tx
            .send(TorrentManagerCommand::StartSeeding {
                release_id,
                response_tx: tx,
            })
            .map_err(|_| {
                SeederError::Torrent(TorrentError::Libtorrent(
                    "TorrentManager channel closed".to_string(),
                ))
            })?;
        rx.await.map_err(|_| {
            SeederError::Torrent(TorrentError::Libtorrent(
                "TorrentManager response channel closed".to_string(),
            ))
        })?
    }

    /// Stop seeding a torrent for a release
    pub async fn stop_seeding(&self, release_id: String) -> Result<(), SeederError> {
        let (tx, rx) = oneshot::channel();
        self.command_tx
            .send(TorrentManagerCommand::StopSeeding {
                release_id,
                response_tx: tx,
            })
            .map_err(|_| {
                SeederError::Torrent(TorrentError::Libtorrent(
                    "TorrentManager channel closed".to_string(),
                ))
            })?;
        rx.await.map_err(|_| {
            SeederError::Torrent(TorrentError::Libtorrent(
                "TorrentManager response channel closed".to_string(),
            ))
        })?
    }
}

/// Manages all torrent operations (downloads, metadata detection, seeding)
/// Runs on a dedicated thread with its own Tokio runtime
struct TorrentManager {
    command_rx: mpsc::UnboundedReceiver<TorrentManagerCommand>,
    download_client: TorrentClient, // default storage
    seeding_client: TorrentClient,  // custom storage (BaeStorage)
    cache_manager: CacheManager,
    database: Database,
    chunk_size_bytes: usize,
}

/// Start the torrent manager service
/// Returns a handle for sending commands to the manager
pub fn start_torrent_manager(
    cache_manager: CacheManager,
    database: Database,
    chunk_size_bytes: usize,
) -> TorrentManagerHandle {
    let (command_tx, command_rx) = mpsc::unbounded_channel();

    // Clone for the thread
    let cache_manager_for_worker = cache_manager.clone();
    let database_for_worker = database.clone();

    // Spawn the service task on a dedicated thread (TorrentClient isn't Send-safe due to FFI)
    std::thread::spawn(move || {
        // Create a new tokio runtime for this thread
        let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");

        let rt_handle = rt.handle().clone();
        rt.block_on(async move {
            // Create both TorrentClient instances on this thread
            let download_client = TorrentClient::new_with_default_storage(rt_handle.clone())
                .expect("Failed to create download torrent client");
            let seeding_client =
                TorrentClient::new(rt_handle).expect("Failed to create seeding torrent client");

            let service = TorrentManager {
                command_rx,
                download_client,
                seeding_client,
                cache_manager: cache_manager_for_worker,
                database: database_for_worker,
                chunk_size_bytes,
            };

            service.run_manager_worker().await;
        });
    });

    TorrentManagerHandle { command_tx }
}

impl TorrentManager {
    async fn run_manager_worker(mut self) {
        info!("TorrentManager worker started");

        loop {
            match self.command_rx.recv().await {
                Some(TorrentManagerCommand::AddTorrent {
                    source,
                    response_tx,
                }) => {
                    let result = match source {
                        TorrentSource::File(path) => {
                            self.download_client.add_torrent_file(&path).await
                        }
                        TorrentSource::MagnetLink(magnet) => {
                            self.download_client.add_magnet_link(&magnet).await
                        }
                    };
                    let _ = response_tx.send(result);
                }
                Some(TorrentManagerCommand::RemoveTorrent {
                    handle,
                    delete_files,
                    response_tx,
                }) => {
                    let result = if delete_files {
                        self.download_client
                            .remove_torrent_and_delete_data(&handle)
                            .await
                    } else {
                        self.download_client
                            .remove_torrent_and_keep_data(&handle)
                            .await
                    };
                    let _ = response_tx.send(result);
                }
                Some(TorrentManagerCommand::StartSeeding {
                    release_id,
                    response_tx,
                }) => {
                    let result = self.start_seeding(&release_id).await;
                    let _ = response_tx.send(result);
                }
                Some(TorrentManagerCommand::StopSeeding {
                    release_id,
                    response_tx,
                }) => {
                    let result = self.stop_seeding(&release_id).await;
                    let _ = response_tx.send(result);
                }
                None => {
                    info!("TorrentManager command channel closed");
                    break;
                }
            }
        }

        info!("TorrentManager worker stopped");
    }

    /// Start seeding a torrent for a release
    async fn start_seeding(&self, release_id: &str) -> Result<(), SeederError> {
        // Load torrent metadata from database
        let torrent = self.get_torrent_by_release(release_id).await?;

        info!(
            "Starting seeding for release {} (torrent: {})",
            release_id, torrent.info_hash
        );

        // Re-add torrent to libtorrent session using stored magnet link
        let magnet_link = torrent.magnet_link.as_ref().ok_or_else(|| {
            SeederError::PieceMapping("Torrent has no magnet link stored".to_string())
        })?;

        // Parse magnet link and enable seed_mode to skip hash verification
        // (we already have valid chunks in the chunk store)
        use crate::torrent::ffi::{parse_magnet_uri, set_seed_mode};

        let temp_path = std::env::temp_dir().to_string_lossy().to_string();
        let mut params = parse_magnet_uri(magnet_link, &temp_path);
        if params.is_null() {
            return Err(SeederError::Torrent(TorrentError::InvalidTorrent(
                "Failed to parse magnet URI".to_string(),
            )));
        }

        // Enable seed mode to skip hash verification
        unsafe {
            if let Some(pinned_params) = params.as_mut() {
                let params_ptr = std::pin::Pin::get_unchecked_mut(pinned_params) as *mut _;
                set_seed_mode(params_ptr, true);
            }
        }

        let torrent_handle = self
            .seeding_client
            .add_torrent_with_params(params)
            .await
            .map_err(SeederError::Torrent)?;

        // Wait for metadata
        torrent_handle
            .wait_for_metadata()
            .await
            .map_err(SeederError::Torrent)?;

        // Get storage_index from handle
        let storage_index = torrent_handle
            .storage_index()
            .await
            .map_err(SeederError::Torrent)?;

        // Create piece mapper
        let piece_mapper = TorrentPieceMapper::new(
            torrent.piece_length as usize,
            self.chunk_size_bytes,
            torrent.num_pieces as usize,
            torrent.total_size_bytes as usize,
        );

        // Create BaeStorage instance
        let bae_storage = BaeStorage::new(
            self.cache_manager.clone(),
            self.database.clone(),
            piece_mapper,
            torrent.info_hash.clone(),
        );

        // Register storage with torrent client
        self.seeding_client
            .register_storage(storage_index, torrent.info_hash.clone(), bae_storage)
            .await;

        // Load piece mappings
        let piece_mappings = self.get_piece_mappings(&torrent.id).await?;

        // Get all chunk IDs that need to be pinned
        let mut chunk_ids = Vec::new();
        for mapping in &piece_mappings {
            let ids: Vec<String> = serde_json::from_str(&mapping.chunk_ids).map_err(|e| {
                SeederError::PieceMapping(format!("Failed to parse chunk IDs: {}", e))
            })?;
            chunk_ids.extend(ids);
        }

        // Pin all chunks in cache
        self.cache_manager.pin_chunks(&chunk_ids).await;
        info!("Pinned {} chunks for seeding", chunk_ids.len());

        // Mark torrent as seeding in database
        self.mark_torrent_seeding(&torrent.id, true).await?;

        info!("Successfully started seeding for release {}", release_id);

        Ok(())
    }

    /// Stop seeding a torrent
    async fn stop_seeding(&self, release_id: &str) -> Result<(), SeederError> {
        let torrent = self.get_torrent_by_release(release_id).await?;

        info!(
            "Stopping seeding for release {} (torrent: {})",
            release_id, torrent.info_hash
        );

        // Load piece mappings to get chunk IDs
        let piece_mappings = self.get_piece_mappings(&torrent.id).await?;

        // Get all chunk IDs that were pinned
        let mut chunk_ids = Vec::new();
        for mapping in &piece_mappings {
            let ids: Vec<String> = serde_json::from_str(&mapping.chunk_ids).map_err(|e| {
                SeederError::PieceMapping(format!("Failed to parse chunk IDs: {}", e))
            })?;
            chunk_ids.extend(ids);
        }

        // Unpin chunks
        self.cache_manager.unpin_chunks(&chunk_ids).await;
        info!("Unpinned {} chunks", chunk_ids.len());

        // Mark torrent as not seeding
        self.mark_torrent_seeding(&torrent.id, false).await?;

        Ok(())
    }

    /// Get torrent by release ID
    async fn get_torrent_by_release(&self, release_id: &str) -> Result<DbTorrent, SeederError> {
        self.database
            .get_torrent_by_release(release_id)
            .await?
            .ok_or_else(|| SeederError::Database(sqlx::Error::RowNotFound))
    }

    /// Get piece mappings for a torrent
    async fn get_piece_mappings(
        &self,
        torrent_id: &str,
    ) -> Result<Vec<DbTorrentPieceMapping>, SeederError> {
        Ok(self.database.get_torrent_piece_mappings(torrent_id).await?)
    }

    /// Mark torrent as seeding or not
    async fn mark_torrent_seeding(
        &self,
        torrent_id: &str,
        is_seeding: bool,
    ) -> Result<(), SeederError> {
        Ok(self
            .database
            .update_torrent_seeding(torrent_id, is_seeding)
            .await?)
    }
}
