use crate::cache::CacheManager;
use crate::db::{Database, DbTorrent, DbTorrentPieceMapping};
use crate::torrent::client::TorrentClient;
use crate::torrent::{BaeStorage, TorrentPieceMapper};
use thiserror::Error;
use tokio::sync::mpsc;
use tracing::{error, info};

#[derive(Error, Debug)]
pub enum SeederError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("Cache error: {0}")]
    Cache(#[from] crate::cache::CacheError),
    #[error("Torrent error: {0}")]
    Torrent(#[from] crate::torrent::client::TorrentError),
    #[error("Piece mapping error: {0}")]
    PieceMapping(String),
}

/// Commands sent to the seeder service
#[derive(Debug, Clone)]
pub enum SeederCommand {
    StartSeeding(String), // release_id
    StopSeeding(String),  // release_id
}

/// Handle to the seeder service for sending commands
#[derive(Clone)]
pub struct TorrentSeederHandle {
    command_tx: mpsc::UnboundedSender<SeederCommand>,
}

impl TorrentSeederHandle {
    pub fn start_seeding(&self, release_id: String) {
        let _ = self
            .command_tx
            .send(SeederCommand::StartSeeding(release_id));
    }

    pub fn stop_seeding(&self, release_id: String) {
        let _ = self.command_tx.send(SeederCommand::StopSeeding(release_id));
    }
}

/// Manages seeding torrents from cached chunks via custom storage
/// Runs on a dedicated thread with its own Tokio runtime
struct TorrentSeeder {
    command_rx: mpsc::UnboundedReceiver<SeederCommand>,
    client: TorrentClient,
    cache_manager: CacheManager,
    database: Database,
    chunk_size_bytes: usize,
}

/// Start the torrent seeder service
/// Returns a handle for sending commands to the seeder
pub fn start(
    cache_manager: CacheManager,
    database: Database,
    chunk_size_bytes: usize,
    _runtime_handle: tokio::runtime::Handle,
) -> TorrentSeederHandle {
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
            // Create TorrentClient on this thread
            let client = TorrentClient::new(rt_handle).expect("Failed to create torrent client");

            let service = TorrentSeeder {
                command_rx,
                client,
                cache_manager: cache_manager_for_worker,
                database: database_for_worker,
                chunk_size_bytes,
            };

            service.run_seeder_worker().await;
        });
    });

    TorrentSeederHandle { command_tx }
}

impl TorrentSeeder {
    async fn run_seeder_worker(mut self) {
        info!("TorrentSeeder worker started");

        loop {
            match self.command_rx.recv().await {
                Some(SeederCommand::StartSeeding(release_id)) => {
                    if let Err(e) = self.start_seeding(&release_id).await {
                        error!("Failed to start seeding for release {}: {}", release_id, e);
                    }
                }
                Some(SeederCommand::StopSeeding(release_id)) => {
                    if let Err(e) = self.stop_seeding(&release_id).await {
                        error!("Failed to stop seeding for release {}: {}", release_id, e);
                    }
                }
                None => {
                    info!("TorrentSeeder command channel closed");
                    break;
                }
            }
        }

        info!("TorrentSeeder worker stopped");
    }

    /// Start seeding a torrent for a release
    pub async fn start_seeding(&self, release_id: &str) -> Result<(), SeederError> {
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

        let torrent_handle = self
            .client
            .add_magnet_link(magnet_link)
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
        self.client
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
    pub async fn stop_seeding(&self, release_id: &str) -> Result<(), SeederError> {
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
