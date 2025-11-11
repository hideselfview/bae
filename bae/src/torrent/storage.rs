use crate::cache::CacheManager;
use crate::db::{Database, DbTorrentPieceMapping};
use crate::torrent::piece_mapper::TorrentPieceMapper;
use thiserror::Error;
use tracing::error;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("Cache error: {0}")]
    Cache(#[from] crate::cache::CacheError),
    #[error("Piece mapping error: {0}")]
    PieceMapping(String),
    #[error("Invalid piece index: {0}")]
    InvalidPieceIndex(i32),
}

/// Storage backend for libtorrent that reads/writes directly to BAE chunks
///
/// Chunks are stored unencrypted in the local cache for performance.
/// Encryption happens when uploading to cloud storage (handled separately).
pub struct BaeStorage {
    cache_manager: CacheManager,
    database: Database,
    piece_mapper: TorrentPieceMapper,
    torrent_id: String,
    release_id: String,
}

impl BaeStorage {
    /// Create a new BAE storage backend for a torrent
    pub fn new(
        cache_manager: CacheManager,
        database: Database,
        piece_mapper: TorrentPieceMapper,
        torrent_id: String,
        release_id: String,
    ) -> Self {
        BaeStorage {
            cache_manager,
            database,
            piece_mapper,
            torrent_id,
            release_id,
        }
    }

    /// Read a piece of data by reconstructing from chunks
    pub async fn read_piece(
        &self,
        piece_index: i32,
        offset: i32,
        size: i32,
    ) -> Result<Vec<u8>, StorageError> {
        // Get piece mapping from database
        let mapping = self
            .database
            .get_torrent_piece_mapping(&self.torrent_id, piece_index)
            .await?
            .ok_or_else(|| {
                StorageError::PieceMapping(format!(
                    "Piece mapping not found for piece {}",
                    piece_index
                ))
            })?;

        // Parse chunk IDs
        let chunk_ids: Vec<String> = serde_json::from_str(&mapping.chunk_ids)
            .map_err(|e| StorageError::PieceMapping(format!("Failed to parse chunk IDs: {}", e)))?;

        if chunk_ids.is_empty() {
            return Err(StorageError::PieceMapping(
                "No chunks mapped to piece".to_string(),
            ));
        }

        // Read chunks from cache (stored unencrypted locally)
        let mut chunks = Vec::new();
        for chunk_id in &chunk_ids {
            let cached_data = self
                .cache_manager
                .get_chunk(chunk_id)
                .await?
                .ok_or_else(|| {
                    StorageError::Cache(crate::cache::CacheError::Io(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("Chunk {} not in cache", chunk_id),
                    )))
                })?;

            // Chunks are stored unencrypted in local cache
            chunks.push(cached_data);
        }

        // Extract piece bytes from chunks
        let piece_data = self.extract_piece_from_chunks(
            &chunks,
            mapping.start_byte_in_first_chunk as usize,
            mapping.end_byte_in_last_chunk as usize,
        )?;

        // Apply offset and size if specified
        let start = offset as usize;
        let end = if size > 0 {
            (offset + size) as usize
        } else {
            piece_data.len()
        };

        if start > piece_data.len() {
            return Err(StorageError::PieceMapping(format!(
                "Offset {} exceeds piece size {}",
                start,
                piece_data.len()
            )));
        }

        let end = end.min(piece_data.len());
        Ok(piece_data[start..end].to_vec())
    }

    /// Write a piece of data by chunking and storing
    pub async fn write_piece(
        &self,
        piece_index: i32,
        offset: i32,
        data: &[u8],
    ) -> Result<(), StorageError> {
        // Map piece to chunks
        let chunk_mappings = self.piece_mapper.map_piece_to_chunks(piece_index as usize);

        if chunk_mappings.is_empty() {
            return Err(StorageError::InvalidPieceIndex(piece_index));
        }

        // Calculate which chunks this write affects
        let piece_length = self.piece_mapper.piece_length();
        let piece_start_byte = (piece_index as usize) * piece_length;
        let write_start_byte = piece_start_byte + (offset as usize);
        let write_end_byte = write_start_byte + data.len();

        // Find which chunks this write spans
        let mut chunk_ids = Vec::new();

        for chunk_mapping in &chunk_mappings {
            let chunk_start_byte = chunk_mapping.chunk_index * self.piece_mapper.piece_length();
            let chunk_end_byte = chunk_start_byte + self.piece_mapper.piece_length();

            // Check if this chunk overlaps with the write
            if write_start_byte < chunk_end_byte && write_end_byte > chunk_start_byte {
                // Calculate overlap
                let overlap_start = write_start_byte.max(chunk_start_byte);
                let overlap_end = write_end_byte.min(chunk_end_byte);

                // Calculate byte range within the data slice
                let data_start = overlap_start - write_start_byte;
                let data_end = overlap_end - write_start_byte;

                // Get or create chunk data
                let chunk_data = if let Some(existing_mapping) = self
                    .database
                    .get_torrent_piece_mapping(&self.torrent_id, piece_index)
                    .await?
                {
                    // Load existing chunk data
                    let existing_chunk_ids: Vec<String> =
                        serde_json::from_str(&existing_mapping.chunk_ids).map_err(|e| {
                            StorageError::PieceMapping(format!(
                                "Failed to parse existing chunk IDs: {}",
                                e
                            ))
                        })?;

                    if chunk_mapping.chunk_index < existing_chunk_ids.len() {
                        // Load existing chunk (stored unencrypted in cache)
                        let chunk_id = &existing_chunk_ids[chunk_mapping.chunk_index];
                        self.cache_manager
                            .get_chunk(chunk_id)
                            .await?
                            .ok_or_else(|| {
                                StorageError::Cache(crate::cache::CacheError::Io(
                                    std::io::Error::new(
                                        std::io::ErrorKind::NotFound,
                                        format!("Chunk {} not found", chunk_id),
                                    ),
                                ))
                            })?
                    } else {
                        // New chunk, create empty
                        vec![0u8; self.piece_mapper.piece_length()]
                    }
                } else {
                    // No existing mapping, create new chunk
                    vec![0u8; self.piece_mapper.piece_length()]
                };

                // Update chunk data with new piece data
                let mut updated_chunk = chunk_data;
                let chunk_offset = overlap_start - chunk_start_byte;
                let chunk_end = chunk_offset + (data_end - data_start);
                updated_chunk[chunk_offset..chunk_end].copy_from_slice(&data[data_start..data_end]);

                // Store chunk unencrypted in local cache
                let chunk_id = uuid::Uuid::new_v4().to_string();
                self.cache_manager
                    .put_chunk(&chunk_id, &updated_chunk)
                    .await?;

                chunk_ids.push(chunk_id);
            }
        }

        // Update piece mapping in database
        let start_byte_in_first_chunk = chunk_mappings[0].start_byte as i64;
        let end_byte_in_last_chunk = chunk_mappings.last().unwrap().end_byte as i64;

        let mapping = DbTorrentPieceMapping::new(
            &self.torrent_id,
            piece_index,
            chunk_ids,
            start_byte_in_first_chunk,
            end_byte_in_last_chunk,
        )
        .map_err(|e| StorageError::PieceMapping(format!("Failed to create mapping: {}", e)))?;

        self.database.insert_torrent_piece_mapping(&mapping).await?;

        Ok(())
    }

    /// Verify piece hash (for libtorrent hash verification)
    pub async fn hash_piece(
        &self,
        piece_index: i32,
        expected_hash: &[u8],
    ) -> Result<bool, StorageError> {
        // Read full piece
        let piece_data = self.read_piece(piece_index, 0, 0).await?;

        // Calculate SHA-1 hash (libtorrent uses SHA-1 for piece verification)
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&piece_data);
        let calculated_hash = hasher.finalize();

        // Compare hashes
        Ok(calculated_hash.as_slice() == expected_hash)
    }

    /// Extract piece bytes from decrypted chunks
    fn extract_piece_from_chunks(
        &self,
        chunks: &[Vec<u8>],
        start_byte: usize,
        end_byte: usize,
    ) -> Result<Vec<u8>, StorageError> {
        if chunks.is_empty() {
            return Err(StorageError::PieceMapping("No chunks provided".to_string()));
        }

        // Concatenate all chunks
        let mut combined_data = Vec::new();
        for chunk_data in chunks {
            combined_data.extend_from_slice(chunk_data);
        }

        // Extract the relevant portion
        if end_byte > combined_data.len() {
            return Err(StorageError::PieceMapping(format!(
                "Piece data length mismatch: expected {} bytes, got {} bytes",
                end_byte,
                combined_data.len()
            )));
        }

        Ok(combined_data[start_byte..end_byte].to_vec())
    }
}
