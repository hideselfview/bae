use crate::encryption::{EncryptedChunk, EncryptionError, EncryptionService};
use sha2::{Digest, Sha256};
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ChunkingError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Encryption error: {0}")]
    Encryption(#[from] EncryptionError),
}

/// Configuration for chunking operations
#[derive(Debug, Clone)]
pub struct ChunkingConfig {
    /// Size of each chunk in bytes (default: 1MB)
    pub chunk_size: usize,
    /// Directory to store chunks temporarily before upload
    pub temp_dir: std::path::PathBuf,
}

impl Default for ChunkingConfig {
    fn default() -> Self {
        ChunkingConfig {
            chunk_size: 1024 * 1024, // 1MB chunks
            temp_dir: std::env::temp_dir().join("bae_chunks"),
        }
    }
}

/// Represents a single album chunk with metadata and encrypted data
#[derive(Debug, Clone)]
pub struct AlbumChunk {
    pub id: String,
    pub chunk_index: i32,
    pub original_size: usize,
    pub encrypted_size: usize,
    pub checksum: String,
    pub encrypted_data: Vec<u8>,
}

/// Represents the mapping of a file to chunks within an album
#[derive(Debug, Clone)]
pub struct FileChunkMapping {
    pub file_path: std::path::PathBuf,
    pub start_chunk_index: i32,
    pub end_chunk_index: i32,
    pub start_byte_offset: i64,
    pub end_byte_offset: i64,
}

/// Result of album-level chunking
#[derive(Debug)]
pub struct AlbumChunkingResult {
    pub file_mappings: Vec<FileChunkMapping>,
    pub total_chunks: usize,
}

/// Callback type for streaming chunks as they're created
pub type ChunkCallback = Box<
    dyn Fn(
            AlbumChunk,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send>>
        + Send
        + Sync,
>;

/// Main chunking service that handles file splitting and encryption
#[derive(Debug, Clone)]
pub struct ChunkingService {
    config: ChunkingConfig,
    encryption_service: EncryptionService,
}

impl ChunkingService {
    /// Create a new chunking service with default configuration and encryption service
    pub fn new(encryption_service: EncryptionService) -> Result<Self, ChunkingError> {
        let config = ChunkingConfig::default();
        Self::new_with_config(config, encryption_service)
    }

    /// Create a new chunking service with custom configuration and encryption service
    fn new_with_config(
        config: ChunkingConfig,
        encryption_service: EncryptionService,
    ) -> Result<Self, ChunkingError> {
        // Ensure temp directory exists
        std::fs::create_dir_all(&config.temp_dir)?;

        Ok(ChunkingService {
            config,
            encryption_service,
        })
    }

    /// Chunk an entire album folder into uniform chunks (BitTorrent-style)
    /// Streams chunks via callback as they're created for immediate upload
    pub async fn chunk_album_streaming(
        &self,
        album_folder: &Path,
        file_paths: &[std::path::PathBuf],
        chunk_callback: ChunkCallback,
    ) -> Result<AlbumChunkingResult, ChunkingError> {
        println!(
            "ChunkingService: Starting streaming chunking for folder: {}",
            album_folder.display()
        );

        let mut file_mappings = Vec::new();
        let mut total_bytes_processed = 0u64;

        // First pass: calculate file positions and total size
        for file_path in file_paths {
            let file_size = tokio::fs::metadata(file_path).await?.len();
            let start_byte = total_bytes_processed;
            let end_byte = total_bytes_processed + file_size;

            let start_chunk_index = (start_byte / self.config.chunk_size as u64) as i32;
            let end_chunk_index = ((end_byte - 1) / self.config.chunk_size as u64) as i32;

            file_mappings.push(FileChunkMapping {
                file_path: file_path.clone(),
                start_chunk_index,
                end_chunk_index,
                start_byte_offset: (start_byte % self.config.chunk_size as u64) as i64,
                end_byte_offset: ((end_byte - 1) % self.config.chunk_size as u64) as i64,
            });

            total_bytes_processed = end_byte;
        }

        let total_chunks = total_bytes_processed.div_ceil(self.config.chunk_size as u64) as usize;

        println!(
            "ChunkingService: Total size: {} bytes ({:.2} MB), {} chunks",
            total_bytes_processed,
            total_bytes_processed as f64 / 1024.0 / 1024.0,
            total_chunks
        );

        // Second pass: stream through files, encrypt and upload chunks as we go
        let mut chunk_buffer = Vec::with_capacity(self.config.chunk_size);
        let mut current_chunk_index = 0i32;

        for file_path in file_paths {
            let mut file = tokio::fs::File::open(file_path).await?;
            let mut file_buffer = vec![0u8; 8192]; // Read in 8KB increments

            loop {
                let bytes_read = tokio::io::AsyncReadExt::read(&mut file, &mut file_buffer).await?;
                if bytes_read == 0 {
                    break; // End of file
                }

                // Add to chunk buffer
                chunk_buffer.extend_from_slice(&file_buffer[..bytes_read]);

                // Process complete chunks
                while chunk_buffer.len() >= self.config.chunk_size {
                    let chunk_data: Vec<u8> =
                        chunk_buffer.drain(..self.config.chunk_size).collect();

                    // Encrypt chunk
                    let chunk = self.create_encrypted_chunk(current_chunk_index, &chunk_data)?;

                    // Call callback for immediate upload
                    chunk_callback(chunk).await.map_err(|e| {
                        ChunkingError::Io(std::io::Error::other(format!(
                            "Chunk callback failed: {}",
                            e
                        )))
                    })?;

                    current_chunk_index += 1;
                }
            }
        }

        // Process final partial chunk if any data remains
        if !chunk_buffer.is_empty() {
            let chunk = self.create_encrypted_chunk(current_chunk_index, &chunk_buffer)?;
            chunk_callback(chunk).await.map_err(|e| {
                ChunkingError::Io(std::io::Error::other(format!(
                    "Chunk callback failed: {}",
                    e
                )))
            })?;
            current_chunk_index += 1;
        }

        println!(
            "ChunkingService: Completed {} chunks from {} files",
            current_chunk_index,
            file_paths.len()
        );

        Ok(AlbumChunkingResult {
            file_mappings,
            total_chunks: current_chunk_index as usize,
        })
    }

    /// Create a single encrypted album chunk from data (in-memory, no disk write)
    fn create_encrypted_chunk(
        &self,
        chunk_index: i32,
        data: &[u8],
    ) -> Result<AlbumChunk, ChunkingError> {
        let chunk_id = uuid::Uuid::new_v4().to_string();

        // Encrypt with AES-256-GCM
        let (encrypted_data, nonce) = self.encryption_service.encrypt(data)?;

        // Create encrypted chunk with metadata
        let encrypted_chunk =
            EncryptedChunk::new(encrypted_data, nonce, "encryption_master_key".to_string());

        // Calculate checksum of original data
        let mut hasher = Sha256::new();
        hasher.update(data);
        let checksum = format!("{:x}", hasher.finalize());

        let encrypted_bytes = encrypted_chunk.to_bytes();

        Ok(AlbumChunk {
            id: chunk_id,
            chunk_index,
            original_size: data.len(),
            encrypted_size: encrypted_bytes.len(),
            checksum,
            encrypted_data: encrypted_bytes,
        })
    }
}
