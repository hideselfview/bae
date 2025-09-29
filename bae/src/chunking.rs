use std::path::Path;
use std::fs::File;
use std::io::{Read, Write, BufReader, BufWriter};
use thiserror::Error;
use sha2::{Sha256, Digest};
use uuid::Uuid;
use crate::encryption::{EncryptionService, EncryptedChunk, EncryptionError};

#[derive(Error, Debug)]
pub enum ChunkingError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Encryption error: {0}")]
    Encryption(#[from] EncryptionError),
    #[error("Chunk validation error: {0}")]
    Validation(String),
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

/// Represents a single file chunk with metadata
#[derive(Debug, Clone)]
pub struct FileChunk {
    pub id: String,
    pub file_id: String,
    pub chunk_index: i32,
    pub original_size: usize,
    pub encrypted_size: usize,
    pub checksum: String,
    pub final_path: std::path::PathBuf,
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
    pub file_id: String,
    pub file_path: std::path::PathBuf,
    pub start_chunk_index: i32,
    pub end_chunk_index: i32,
    pub start_byte_offset: i64,
    pub end_byte_offset: i64,
    pub file_size: u64,
}

/// Result of album-level chunking
#[derive(Debug)]
pub struct AlbumChunkingResult {
    pub chunks: Vec<AlbumChunk>,
    pub file_mappings: Vec<FileChunkMapping>,
    pub total_size: u64,
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

    /// Create a new chunking service for testing (uses in-memory encryption)
    pub fn new_for_testing() -> Result<Self, ChunkingError> {
        let config = ChunkingConfig::default();
        Self::new_for_testing_with_config(config)
    }

    /// Create a new chunking service for testing with custom configuration
    pub fn new_for_testing_with_config(config: ChunkingConfig) -> Result<Self, ChunkingError> {
        // Ensure temp directory exists
        std::fs::create_dir_all(&config.temp_dir)?;
        
        // Initialize encryption service with in-memory storage for testing
        let test_key_id = format!("chunking_test_key_{}", uuid::Uuid::new_v4());
        let encryption_service = EncryptionService::new_for_testing(test_key_id)?;
        
        Ok(ChunkingService { 
            config,
            encryption_service,
        })
    }

    /// Split a file into encrypted chunks, writing directly to output directory
    /// Returns a list of chunks that need to be uploaded to storage
    pub async fn chunk_file(&self, file_path: &Path, file_id: &str, output_dir: &Path) -> Result<Vec<FileChunk>, ChunkingError> {
        println!("ChunkingService: Starting to chunk file: {}", file_path.display());
        
        let file = File::open(file_path)?;
        let file_size = file.metadata()?.len() as usize;
        let mut reader = BufReader::new(file);
        
        let mut chunks = Vec::new();
        let mut buffer = vec![0u8; self.config.chunk_size];
        let mut chunk_index = 0;
        
        loop {
            let bytes_read = reader.read(&mut buffer)?;
            if bytes_read == 0 {
                break; // End of file
            }
            
            // Create chunk with actual data size
            let chunk_data = &buffer[..bytes_read];
            let chunk = self.create_chunk(file_id, chunk_index, chunk_data, output_dir).await?;
            
            chunks.push(chunk);
            chunk_index += 1;
        }
        
        println!("ChunkingService: Created {} chunks for file {} (total size: {} bytes)", 
                 chunks.len(), file_path.display(), file_size);
        
        Ok(chunks)
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
            
            // Generate file ID from path
            let file_id = uuid::Uuid::new_v4().to_string();
            
            file_mappings.push(FileChunkMapping {
                file_id,
                file_path: file_path.clone(),
                start_chunk_index,
                end_chunk_index,
                start_byte_offset: (start_byte % self.config.chunk_size as u64) as i64,
                end_byte_offset: ((end_byte - 1) % self.config.chunk_size as u64) as i64,
                file_size,
            });
            
            total_bytes_processed = end_byte;
        }
        
        // Close the concatenated file
        drop(concat_file);
        
        println!("ChunkingService: Concatenated {} files, total size: {} bytes", file_paths.len(), total_bytes_processed);
        
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
        }
        
        // Clean up temporary concatenated file
        if let Err(e) = tokio::fs::remove_file(&temp_concat_path).await {
            println!("Warning: Failed to clean up temp concatenated file: {}", e);
        }
        
        println!("ChunkingService: Created {} album chunks from {} files", chunks.len(), file_paths.len());
        
        Ok(AlbumChunkingResult {
            chunks,
            file_mappings,
            total_size: total_bytes_processed,
        })
    }

    /// Create a single encrypted chunk from data, writing directly to output directory
    async fn create_chunk(&self, file_id: &str, chunk_index: i32, data: &[u8], output_dir: &Path) -> Result<FileChunk, ChunkingError> {
        let chunk_id = Uuid::new_v4().to_string();
        
        // Calculate checksum of original data
        let mut hasher = Sha256::new();
        hasher.update(data);
        let checksum = format!("{:x}", hasher.finalize());
        
        // Encrypt with AES-256-GCM
        let (encrypted_data, nonce) = self.encryption_service.encrypt(data)?;
        
        // Create encrypted chunk with metadata
        let encrypted_chunk = EncryptedChunk::new(
            encrypted_data,
            nonce,
            self.encryption_service.key_id().to_string(),
        );
        
        // Serialize and write encrypted chunk directly to final location
        let chunk_filename = format!("{}.enc", chunk_id);
        let final_path = output_dir.join(chunk_filename);
        
        // Ensure output directory exists
        tokio::fs::create_dir_all(output_dir).await.map_err(|e| ChunkingError::Io(e))?;
        
        let chunk_file = File::create(&final_path)?;
        let mut writer = BufWriter::new(chunk_file);
        writer.write_all(&encrypted_chunk.to_bytes())?;
        writer.flush()?;
        
        Ok(FileChunk {
            id: chunk_id,
            file_id: file_id.to_string(),
            chunk_index,
            original_size: data.len(),
            encrypted_size: encrypted_chunk.to_bytes().len(),
            checksum,
            final_path,
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


    /// Reassemble chunks back into the original file
    /// This is used for playback and verification
    pub async fn reassemble_chunks(&self, chunks: &[FileChunk], output_path: &Path) -> Result<(), ChunkingError> {
        println!("ChunkingService: Reassembling {} chunks to {}", chunks.len(), output_path.display());
        
        // Sort chunks by index to ensure correct order
        let mut sorted_chunks = chunks.to_vec();
        sorted_chunks.sort_by_key(|c| c.chunk_index);
        
        let output_file = File::create(output_path)?;
        let mut writer = BufWriter::new(output_file);
        
        for chunk in &sorted_chunks {
            // Read encrypted chunk file
            let chunk_bytes = std::fs::read(&chunk.final_path)?;
            
            // Deserialize encrypted chunk
            let encrypted_chunk = EncryptedChunk::from_bytes(&chunk_bytes)?;
            
            // Decrypt chunk data
            let decrypted_data = self.encryption_service.decrypt(
                &encrypted_chunk.encrypted_data,
                &encrypted_chunk.nonce,
            )?;
            
            // Verify checksum
            let mut hasher = Sha256::new();
            hasher.update(&decrypted_data);
            let calculated_checksum = format!("{:x}", hasher.finalize());
            
            if calculated_checksum != chunk.checksum {
                return Err(ChunkingError::Validation(
                    format!("Checksum mismatch for chunk {}: expected {}, got {}", 
                           chunk.id, chunk.checksum, calculated_checksum)
                ));
            }
            
            // Write decrypted data to output file
            writer.write_all(&decrypted_data)?;
        }
        
        writer.flush()?;
        println!("ChunkingService: Successfully reassembled file to {}", output_path.display());
        Ok(())
    }

    /// Clean up chunk files (now removes final files, use with caution)
    pub fn cleanup_chunks(&self, chunks: &[FileChunk]) -> Result<(), ChunkingError> {
        for chunk in chunks {
            if chunk.final_path.exists() {
                std::fs::remove_file(&chunk.final_path)?;
            }
        }
        Ok(())
    }

    /// Get chunking statistics for a file
    pub fn calculate_chunk_stats(&self, file_size: usize) -> ChunkStats {
        let chunk_count = (file_size + self.config.chunk_size - 1) / self.config.chunk_size;
        let last_chunk_size = if file_size % self.config.chunk_size == 0 {
            self.config.chunk_size
        } else {
            file_size % self.config.chunk_size
        };
        
        ChunkStats {
            total_chunks: chunk_count,
            chunk_size: self.config.chunk_size,
            last_chunk_size,
            total_size: file_size,
        }
    }
}

/// Statistics about file chunking
#[derive(Debug, Clone)]
pub struct ChunkStats {
    pub total_chunks: usize,
    pub chunk_size: usize,
    pub last_chunk_size: usize,
    pub total_size: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_chunk_and_reassemble() {
        // Create a temporary test file
        let mut temp_file = NamedTempFile::new().unwrap();
        let test_data = b"Hello, world! This is a test file for chunking.";
        temp_file.write_all(test_data).unwrap();
        temp_file.flush().unwrap();
        
        // Create chunking service with small chunk size for testing
        let config = ChunkingConfig {
            chunk_size: 10, // Very small chunks for testing
            temp_dir: std::env::temp_dir().join("bae_test_chunks"),
        };
        let chunking_service = ChunkingService::new_for_testing_with_config(config).unwrap();
        
        // Chunk the file
        let file_id = "test_file_123";
        let output_dir = std::env::temp_dir().join("bae_test_output");
        std::fs::create_dir_all(&output_dir).unwrap();
        let chunks = chunking_service.chunk_file(temp_file.path(), file_id, &output_dir).await.unwrap();
        
        // Verify we got the expected number of chunks
        let expected_chunks = (test_data.len() + 9) / 10; // Ceiling division
        assert_eq!(chunks.len(), expected_chunks);
        
        // Reassemble the chunks
        let output_file = NamedTempFile::new().unwrap();
        chunking_service.reassemble_chunks(&chunks, output_file.path()).await.unwrap();
        
        // Verify the reassembled file matches the original
        let reassembled_data = std::fs::read(output_file.path()).unwrap();
        assert_eq!(reassembled_data, test_data);
        
        // Clean up
        chunking_service.cleanup_chunks(&chunks).unwrap();
    }

    #[test]
    fn test_chunk_stats() {
        let chunking_service = ChunkingService::new_for_testing().unwrap();
        
        // Test with exact multiple of chunk size
        let stats = chunking_service.calculate_chunk_stats(2048 * 1024); // 2MB
        assert_eq!(stats.total_chunks, 2);
        assert_eq!(stats.last_chunk_size, 1024 * 1024);
        
        // Test with partial last chunk
        let stats = chunking_service.calculate_chunk_stats(1536 * 1024); // 1.5MB
        assert_eq!(stats.total_chunks, 2);
        assert_eq!(stats.last_chunk_size, 512 * 1024);
    }
}
