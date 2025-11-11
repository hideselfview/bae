use crate::cache::CacheManager;
use crate::db::{Database, DbTorrent, DbTorrentPieceMapping};
use crate::encryption::EncryptionService;
use crate::torrent::client::TorrentClient;
use thiserror::Error;
use tracing::{error, info};

#[derive(Error, Debug)]
pub enum SeederError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("Cache error: {0}")]
    Cache(#[from] crate::cache::CacheError),
    #[error("Encryption error: {0}")]
    Encryption(#[from] crate::encryption::EncryptionError),
    #[error("Torrent error: {0}")]
    Torrent(#[from] crate::torrent::client::TorrentError),
    #[error("Piece mapping error: {0}")]
    PieceMapping(String),
}

/// Manages seeding torrents from cached chunks
pub struct TorrentSeeder {
    client: TorrentClient,
    cache_manager: CacheManager,
    encryption_service: EncryptionService,
    database: Database,
}

impl TorrentSeeder {
    pub fn new(
        client: TorrentClient,
        cache_manager: CacheManager,
        encryption_service: EncryptionService,
        database: Database,
    ) -> Self {
        TorrentSeeder {
            client,
            cache_manager,
            encryption_service,
            database,
        }
    }

    /// Start seeding a torrent for a release
    pub async fn start_seeding(&self, release_id: &str) -> Result<(), SeederError> {
        // Load torrent metadata from database
        let torrent = self.get_torrent_by_release(release_id).await?;

        info!(
            "Starting seeding for release {} (torrent: {})",
            release_id, torrent.info_hash
        );

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

    /// Read a piece of data from cached chunks
    pub async fn read_piece(
        &self,
        torrent_id: &str,
        piece_index: i32,
    ) -> Result<Vec<u8>, SeederError> {
        // Get piece mapping
        let mapping = self.get_piece_mapping(torrent_id, piece_index).await?;

        // Parse chunk IDs
        let chunk_ids: Vec<String> = serde_json::from_str(&mapping.chunk_ids)
            .map_err(|e| SeederError::PieceMapping(format!("Failed to parse chunk IDs: {}", e)))?;

        if chunk_ids.is_empty() {
            return Err(SeederError::PieceMapping(
                "No chunks mapped to piece".to_string(),
            ));
        }

        // Read and decrypt chunks
        let mut decrypted_chunks = Vec::new();
        for chunk_id in &chunk_ids {
            let cached_data = self
                .cache_manager
                .get_chunk(chunk_id)
                .await?
                .ok_or_else(|| {
                    SeederError::Cache(crate::cache::CacheError::Io(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("Chunk {} not in cache", chunk_id),
                    )))
                })?;

            let decrypted = self.encryption_service.decrypt_chunk(&cached_data)?;
            decrypted_chunks.push(decrypted);
        }

        // Extract piece bytes from chunks
        // Piece may span multiple chunks, need to extract the right byte range
        let piece_data = self.extract_piece_from_chunks(
            &decrypted_chunks,
            mapping.start_byte_in_first_chunk as usize,
            mapping.end_byte_in_last_chunk as usize,
        )?;

        Ok(piece_data)
    }

    /// Extract piece bytes from decrypted chunks
    fn extract_piece_from_chunks(
        &self,
        chunks: &[Vec<u8>],
        start_byte: usize,
        end_byte: usize,
    ) -> Result<Vec<u8>, SeederError> {
        if chunks.is_empty() {
            return Err(SeederError::PieceMapping("No chunks provided".to_string()));
        }

        if chunks.len() == 1 {
            // Piece is entirely within one chunk
            let chunk = &chunks[0];
            if end_byte > chunk.len() {
                return Err(SeederError::PieceMapping(format!(
                    "Piece end byte {} exceeds chunk size {}",
                    end_byte,
                    chunk.len()
                )));
            }
            return Ok(chunk[start_byte..end_byte].to_vec());
        }

        // Piece spans multiple chunks
        let mut piece_data = Vec::new();
        let mut current_offset = 0;

        for (i, chunk) in chunks.iter().enumerate() {
            let chunk_start = if i == 0 { start_byte } else { 0 };
            let chunk_end = if i == chunks.len() - 1 {
                end_byte - current_offset
            } else {
                chunk.len()
            };

            if chunk_end > chunk.len() {
                return Err(SeederError::PieceMapping(format!(
                    "Invalid chunk range: end {} exceeds chunk size {}",
                    chunk_end,
                    chunk.len()
                )));
            }

            piece_data.extend_from_slice(&chunk[chunk_start..chunk_end]);
            current_offset += chunk.len();
        }

        Ok(piece_data)
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

    /// Get a specific piece mapping
    async fn get_piece_mapping(
        &self,
        torrent_id: &str,
        piece_index: i32,
    ) -> Result<DbTorrentPieceMapping, SeederError> {
        self.database
            .get_torrent_piece_mapping(torrent_id, piece_index)
            .await?
            .ok_or_else(|| SeederError::Database(sqlx::Error::RowNotFound))
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
