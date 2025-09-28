use std::path::Path;
use std::fs::File;
use std::io::{Read, Write, BufReader, BufWriter};
use thiserror::Error;
use sha2::{Sha256, Digest};
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum ChunkingError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Encryption error: {0}")]
    Encryption(String),
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
    pub temp_path: std::path::PathBuf,
}

/// Main chunking service that handles file splitting and encryption
pub struct ChunkingService {
    config: ChunkingConfig,
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
        
        Ok(ChunkingService { config })
    }

    /// Split a file into encrypted chunks
    /// Returns a list of chunks that need to be uploaded to storage
    pub async fn chunk_file(&self, file_path: &Path, file_id: &str) -> Result<Vec<FileChunk>, ChunkingError> {
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
            let chunk = self.create_chunk(file_id, chunk_index, chunk_data).await?;
            
            chunks.push(chunk);
            chunk_index += 1;
        }
        
        println!("ChunkingService: Created {} chunks for file {} (total size: {} bytes)", 
                chunks.len(), file_path.display(), file_size);
        
        Ok(chunks)
    }

    /// Create a single encrypted chunk from data
    async fn create_chunk(&self, file_id: &str, chunk_index: i32, data: &[u8]) -> Result<FileChunk, ChunkingError> {
        let chunk_id = Uuid::new_v4().to_string();
        
        // Calculate checksum of original data
        let mut hasher = Sha256::new();
        hasher.update(data);
        let checksum = format!("{:x}", hasher.finalize());
        
        // For now, we'll do simple "encryption" (XOR with a key)
        // TODO: Replace with proper AES encryption
        let encrypted_data = self.simple_encrypt(data);
        
        // Write encrypted chunk to temp file
        let temp_filename = format!("chunk_{}_{}.enc", file_id, chunk_index);
        let temp_path = self.config.temp_dir.join(temp_filename);
        
        let temp_file = File::create(&temp_path)?;
        let mut writer = BufWriter::new(temp_file);
        writer.write_all(&encrypted_data)?;
        writer.flush()?;
        
        Ok(FileChunk {
            id: chunk_id,
            file_id: file_id.to_string(),
            chunk_index,
            original_size: data.len(),
            encrypted_size: encrypted_data.len(),
            checksum,
            temp_path,
        })
    }

    /// Simple encryption placeholder (XOR with fixed key)
    /// TODO: Replace with proper AES-256-GCM encryption
    fn simple_encrypt(&self, data: &[u8]) -> Vec<u8> {
        // Simple XOR encryption with a repeating key
        // This is NOT secure - just a placeholder for the real encryption
        let key = b"bae_temp_key_123"; // 16 bytes
        let mut encrypted = Vec::with_capacity(data.len());
        
        for (i, &byte) in data.iter().enumerate() {
            let key_byte = key[i % key.len()];
            encrypted.push(byte ^ key_byte);
        }
        
        encrypted
    }

    /// Simple decryption placeholder (reverse XOR)
    /// TODO: Replace with proper AES-256-GCM decryption
    pub fn simple_decrypt(&self, encrypted_data: &[u8]) -> Vec<u8> {
        // XOR is symmetric, so decryption is the same as encryption
        self.simple_encrypt(encrypted_data)
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
            // Read encrypted chunk data
            let encrypted_data = std::fs::read(&chunk.temp_path)?;
            
            // Decrypt chunk data
            let decrypted_data = self.simple_decrypt(&encrypted_data);
            
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

    /// Clean up temporary chunk files
    pub fn cleanup_temp_chunks(&self, chunks: &[FileChunk]) -> Result<(), ChunkingError> {
        for chunk in chunks {
            if chunk.temp_path.exists() {
                std::fs::remove_file(&chunk.temp_path)?;
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
        let chunking_service = ChunkingService::new_with_config(config).unwrap();
        
        // Chunk the file
        let file_id = "test_file_123";
        let chunks = chunking_service.chunk_file(temp_file.path(), file_id).await.unwrap();
        
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
        chunking_service.cleanup_temp_chunks(&chunks).unwrap();
    }

    #[test]
    fn test_chunk_stats() {
        let chunking_service = ChunkingService::new().unwrap();
        
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
