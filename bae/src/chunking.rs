use crate::encryption::{EncryptedChunk, EncryptionError, EncryptionService};
use futures::stream::{FuturesUnordered, StreamExt};
use sha2::{Digest, Sha256};
use std::path::Path;
use thiserror::Error;
use tokio::io::{AsyncReadExt, BufReader};
use tokio::sync::mpsc;

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
}

/// Main chunking service that handles file splitting and encryption
///
/// # Concurrency Architecture
///
/// Files are read through `BufReader` and accumulated into buffers. When a full
/// chunk is ready (default 5MB), it's queued for encryption.
///
/// Encryption parallelism is bounded to `max_concurrent` tasks (CPU cores * 2) using
/// `FuturesUnordered`. When at capacity, file reading pauses until an encryption
/// completes. This creates backpressure - slow encryption pauses reading, slow I/O
/// leaves encryption slots idle.
///
/// Encryption runs on tokio's blocking thread pool (`spawn_blocking`) to prevent
/// CPU-intensive crypto from blocking async I/O tasks.
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

    /// Calculate total number of chunks without actually processing files
    pub async fn calculate_total_chunks(
        &self,
        file_paths: &[std::path::PathBuf],
    ) -> Result<usize, ChunkingError> {
        let mut total_size = 0u64;
        for file_path in file_paths {
            let metadata = tokio::fs::metadata(file_path).await?;
            total_size += metadata.len();
        }
        Ok(total_size.div_ceil(self.config.chunk_size as u64) as usize)
    }

    /// Chunk an entire album folder into uniform chunks (BitTorrent-style)
    /// Returns a channel of encrypted chunks for streaming upload pipeline
    pub async fn chunk_album_streaming(
        &self,
        album_folder: &Path,
        file_paths: &[std::path::PathBuf],
        max_encrypt_workers: usize,
    ) -> Result<(AlbumChunkingResult, mpsc::Receiver<AlbumChunk>), ChunkingError> {
        println!(
            "ChunkingService: Starting streaming pipeline for folder: {}",
            album_folder.display()
        );

        // Calculate file mappings and total size upfront
        let mut file_mappings = Vec::new();
        let mut total_bytes_processed = 0u64;

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
            "ChunkingService: Total size: {} bytes ({:.2} MB), {} chunks expected",
            total_bytes_processed,
            total_bytes_processed as f64 / 1024.0 / 1024.0,
            total_chunks
        );

        // Create bounded channels for pipeline stages (provides backpressure)
        // Capacity matches worker pool size to prevent unbounded memory growth
        let (raw_chunk_tx, raw_chunk_rx) = mpsc::channel(max_encrypt_workers);
        let (encrypted_chunk_tx, encrypted_chunk_rx) = mpsc::channel(max_encrypt_workers);

        // Stage 1: Spawn reader task (sequential file reading)
        let file_paths_clone = file_paths.to_vec();
        let chunk_size = self.config.chunk_size;
        tokio::spawn(async move {
            Self::reader_task(file_paths_clone, chunk_size, raw_chunk_tx).await;
        });

        // Stage 2: Spawn encryption coordinator (bounded parallel encryption)
        let chunking_service = self.clone();
        tokio::spawn(async move {
            chunking_service
                .encryption_coordinator_task(raw_chunk_rx, encrypted_chunk_tx, max_encrypt_workers)
                .await;
        });

        println!(
            "ChunkingService: Pipeline started with {} encryption workers",
            max_encrypt_workers
        );

        Ok((AlbumChunkingResult { file_mappings }, encrypted_chunk_rx))
    }

    /// Reader task: sequentially reads files and sends raw chunks to encryption stage
    async fn reader_task(
        file_paths: Vec<std::path::PathBuf>,
        chunk_size: usize,
        raw_chunk_tx: mpsc::Sender<(i32, Vec<u8>)>,
    ) {
        let mut chunk_buffer = Vec::with_capacity(chunk_size);
        let mut current_chunk_index = 0i32;
        let file_count = file_paths.len();

        for file_path in file_paths {
            let file = match tokio::fs::File::open(&file_path).await {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("Reader task: Failed to open file {:?}: {}", file_path, e);
                    return;
                }
            };

            let mut reader = BufReader::new(file);
            let mut read_buffer = vec![0u8; 8192];

            loop {
                let bytes_read = match reader.read(&mut read_buffer).await {
                    Ok(n) => n,
                    Err(e) => {
                        eprintln!("Reader task: Failed to read file {:?}: {}", file_path, e);
                        return;
                    }
                };

                if bytes_read == 0 {
                    break; // End of file
                }

                // Add to chunk buffer
                chunk_buffer.extend_from_slice(&read_buffer[..bytes_read]);

                // Process complete chunks
                while chunk_buffer.len() >= chunk_size {
                    let chunk_data: Vec<u8> = chunk_buffer.drain(..chunk_size).collect();

                    // Bounded channel: blocks here if encryption is behind (backpressure)
                    if raw_chunk_tx
                        .send((current_chunk_index, chunk_data))
                        .await
                        .is_err()
                    {
                        eprintln!("Reader task: Channel closed, stopping");
                        return;
                    }

                    current_chunk_index += 1;
                }
            }
        }

        // Process final partial chunk if any data remains
        if !chunk_buffer.is_empty() {
            if raw_chunk_tx
                .send((current_chunk_index, chunk_buffer))
                .await
                .is_err()
            {
                eprintln!("Reader task: Channel closed on final chunk");
                return;
            }
            current_chunk_index += 1;
        }

        println!(
            "Reader task: Completed reading {} chunks from {} files",
            current_chunk_index, file_count
        );
        // Channel sender is dropped here, closing the channel
    }

    /// Encryption coordinator task: maintains bounded pool of encryption workers
    async fn encryption_coordinator_task(
        self,
        mut raw_chunk_rx: mpsc::Receiver<(i32, Vec<u8>)>,
        encrypted_chunk_tx: mpsc::Sender<AlbumChunk>,
        max_workers: usize,
    ) {
        let mut encryption_tasks = FuturesUnordered::new();
        let mut chunks_processed = 0usize;

        loop {
            // If we have room for more tasks and there are chunks to process
            if encryption_tasks.len() < max_workers {
                match raw_chunk_rx.recv().await {
                    Some((chunk_index, chunk_data)) => {
                        // Spawn encryption task
                        let chunking_service = self.clone();
                        let task = tokio::spawn(async move {
                            // CPU-bound work isolation: Encryption runs on tokio's blocking thread pool
                            // to prevent CPU-intensive crypto from starving async I/O tasks on the main runtime.
                            tokio::task::spawn_blocking(move || {
                                chunking_service.create_encrypted_chunk(chunk_index, &chunk_data)
                            })
                            .await
                            .map_err(|e| format!("Encryption task panicked: {}", e))?
                            .map_err(|e| format!("Encryption failed: {}", e))
                        });

                        encryption_tasks.push(task);
                    }
                    None => {
                        // No more raw chunks, drain remaining tasks
                        break;
                    }
                }
            } else {
                // At capacity, wait for one to complete
                match encryption_tasks.next().await {
                    Some(Ok(Ok(encrypted_chunk))) => {
                        // Bounded channel: blocks here if upload is behind (backpressure)
                        if encrypted_chunk_tx.send(encrypted_chunk).await.is_err() {
                            eprintln!("Encryption coordinator: Output channel closed, stopping");
                            return;
                        }
                        chunks_processed += 1;
                    }
                    Some(Ok(Err(e))) => {
                        eprintln!("Encryption coordinator: Encryption error: {}", e);
                        return;
                    }
                    Some(Err(e)) => {
                        eprintln!("Encryption coordinator: Task join error: {}", e);
                        return;
                    }
                    None => break,
                }
            }
        }

        // Drain remaining encryption tasks
        while let Some(result) = encryption_tasks.next().await {
            match result {
                Ok(Ok(encrypted_chunk)) => {
                    if encrypted_chunk_tx.send(encrypted_chunk).await.is_err() {
                        eprintln!("Encryption coordinator: Output channel closed during drain");
                        return;
                    }
                    chunks_processed += 1;
                }
                Ok(Err(e)) => {
                    eprintln!(
                        "Encryption coordinator: Encryption error during drain: {}",
                        e
                    );
                    return;
                }
                Err(e) => {
                    eprintln!(
                        "Encryption coordinator: Task join error during drain: {}",
                        e
                    );
                    return;
                }
            }
        }

        println!(
            "Encryption coordinator: Completed {} encrypted chunks",
            chunks_processed
        );
        // Channel sender is dropped here, closing the channel
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
