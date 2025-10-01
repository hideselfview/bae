use std::path::Path;
use std::io::{Read, Write};
use thiserror::Error;
use sha2::{Sha256, Digest};
use crate::encryption::{EncryptionService, EncryptedChunk, EncryptionError};

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


/// Represents a single album chunk with metadata
#[derive(Debug, Clone)]
pub struct AlbumChunk {
    pub id: String,
    pub chunk_index: i32,
    pub original_size: usize,
    pub encrypted_size: usize,
    pub checksum: String,
    pub final_path: std::path::PathBuf,
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
    pub chunks: Vec<AlbumChunk>,
    pub file_mappings: Vec<FileChunkMapping>,
}

/// Main chunking service that handles file splitting and encryption
pub struct ChunkingService {
    config: ChunkingConfig,
    encryption_service: EncryptionService,
}

impl ChunkingService {
    /// Create a new chunking service with default configuration
    pub fn new() -> Result<Self, ChunkingError> {
        let config = ChunkingConfig::default();
        Self::new_with_config(config)
    }

    /// Create a new chunking service with custom configuration
    pub fn new_with_config(config: ChunkingConfig) -> Result<Self, ChunkingError> {
        // Ensure temp directory exists
        std::fs::create_dir_all(&config.temp_dir)?;
        
        // Initialize encryption service
        let encryption_service = EncryptionService::new()?;
        
        Ok(ChunkingService { 
            config,
            encryption_service,
        })
    }

    /// Chunk an entire album folder into uniform chunks (BitTorrent-style)
    /// Concatenates all files and creates file-to-chunk mappings
    pub async fn chunk_album(&self, album_folder: &Path, file_paths: &[std::path::PathBuf], output_dir: &Path) -> Result<AlbumChunkingResult, ChunkingError> {
        println!("ChunkingService: Starting album-level chunking for folder: {}", album_folder.display());
        
        let mut file_mappings = Vec::new();
        let mut total_bytes_processed = 0u64;
        let mut current_chunk_index = 0i32;
        let mut chunks = Vec::new();
        
        // Create a temporary concatenated file
        let temp_concat_path = output_dir.join("album_concat.tmp");
        let mut concat_file = tokio::fs::File::create(&temp_concat_path).await?;
        
        // Concatenate all files and track their positions
        for file_path in file_paths {
            let file_size = tokio::fs::metadata(file_path).await?.len();
            let start_byte = total_bytes_processed;
            
            // Copy file content to concatenated stream
            let mut source_file = tokio::fs::File::open(file_path).await?;
            tokio::io::copy(&mut source_file, &mut concat_file).await?;
            
            let end_byte = total_bytes_processed + file_size;
            
            // Calculate chunk boundaries for this file
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
        
        // Close the concatenated file
        drop(concat_file);
        
        println!("ChunkingService: Concatenated {} files, total size: {} bytes ({:.2} MB)", 
                 file_paths.len(), total_bytes_processed, total_bytes_processed as f64 / 1024.0 / 1024.0);
        
        // Calculate expected chunks for progress reporting
        let expected_chunks = ((total_bytes_processed + self.config.chunk_size as u64 - 1) / self.config.chunk_size as u64) as usize;
        println!("ChunkingService: Creating {} encrypted chunks (1MB each)...", expected_chunks);
        
        // Now chunk the concatenated file
        let concat_file = std::fs::File::open(&temp_concat_path)?;
        let mut reader = std::io::BufReader::new(concat_file);
        let mut buffer = vec![0u8; self.config.chunk_size];
        
        loop {
            let bytes_read = reader.read(&mut buffer)?;
            if bytes_read == 0 {
                break; // End of concatenated file
            }
            
            // Create album chunk
            let chunk_data = &buffer[..bytes_read];
            let chunk = self.create_album_chunk(current_chunk_index, chunk_data, output_dir).await?;
            
            chunks.push(chunk);
            current_chunk_index += 1;
            
            // Progress reporting every 100 chunks
            if current_chunk_index % 100 == 0 {
                let progress = (current_chunk_index as f64 / expected_chunks as f64) * 100.0;
                println!("ChunkingService: Progress: {}/{} chunks ({:.1}%)", 
                         current_chunk_index, expected_chunks, progress);
            }
        }
        
        // Clean up temporary concatenated file
        if let Err(e) = tokio::fs::remove_file(&temp_concat_path).await {
            println!("Warning: Failed to clean up temp concatenated file: {}", e);
        }
        
        println!("ChunkingService: Created {} album chunks from {} files", chunks.len(), file_paths.len());
        
        Ok(AlbumChunkingResult {
            chunks,
            file_mappings,
        })
    }

    /// Create a single encrypted album chunk from data, writing directly to output directory
    async fn create_album_chunk(&self, chunk_index: i32, data: &[u8], output_dir: &Path) -> Result<AlbumChunk, ChunkingError> {
        let chunk_id = uuid::Uuid::new_v4().to_string();
        let chunk_filename = format!("chunk_{:06}_{}.enc", chunk_index, chunk_id);
        let final_path = output_dir.join(&chunk_filename);
        
        // Encrypt with AES-256-GCM
        let (encrypted_data, nonce) = self.encryption_service.encrypt(data)?;
        
        // Create encrypted chunk with metadata
        let encrypted_chunk = EncryptedChunk::new(
            encrypted_data,
            nonce,
            self.encryption_service.key_id().to_string(),
        );
        
        // Calculate checksum of original data
        let mut hasher = Sha256::new();
        hasher.update(data);
        let checksum = format!("{:x}", hasher.finalize());
        
        // Write encrypted data directly to final location
        let chunk_file = std::fs::File::create(&final_path)?;
        let mut writer = std::io::BufWriter::new(chunk_file);
        writer.write_all(&encrypted_chunk.to_bytes())?;
        writer.flush()?;
        
        Ok(AlbumChunk {
            id: chunk_id,
            chunk_index,
            original_size: data.len(),
            encrypted_size: encrypted_chunk.to_bytes().len(),
            checksum,
            final_path,
        })
    }

}