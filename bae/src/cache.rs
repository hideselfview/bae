use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use tokio::sync::RwLock;
use tokio::fs;
use thiserror::Error;

/// Errors that can occur during cache operations
#[derive(Error, Debug)]
pub enum CacheError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Cache configuration error: {0}")]
    Config(String),
}

/// Configuration for the cache manager
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// Directory where cached chunks are stored
    pub cache_dir: PathBuf,
    /// Maximum cache size in bytes (default: 1GB)
    pub max_size_bytes: u64,
    /// Maximum number of cached chunks (default: 10,000)
    pub max_chunks: usize,
}

impl Default for CacheConfig {
    fn default() -> Self {
        let home_dir = dirs::home_dir().expect("Failed to get home directory");
        CacheConfig {
            cache_dir: home_dir.join(".bae").join("cache"),
            max_size_bytes: 1024 * 1024 * 1024, // 1GB
            max_chunks: 10_000,
        }
    }
}

/// Metadata about a cached chunk
#[derive(Debug, Clone)]
struct CacheEntry {
    /// Chunk ID
    chunk_id: String,
    /// File path in cache
    file_path: PathBuf,
    /// Size in bytes
    size_bytes: u64,
    /// Last access time (for LRU)
    last_accessed: std::time::SystemTime,
}

/// LRU cache manager for encrypted chunks
pub struct CacheManager {
    config: CacheConfig,
    /// In-memory index of cached chunks (chunk_id -> CacheEntry)
    entries: Arc<RwLock<HashMap<String, CacheEntry>>>,
    /// Current cache size in bytes
    current_size: Arc<RwLock<u64>>,
}

// Global instance - created once and reused
static CACHE_MANAGER: OnceLock<CacheManager> = OnceLock::new();

impl CacheManager {
    /// Create a new cache manager with default configuration (private - use get_cache() instead)
    async fn new() -> Result<Self, CacheError> {
        let config = CacheConfig::default();
        Self::new_with_config(config).await
    }

    /// Create a new cache manager with custom configuration (private - use get_cache() instead)
    async fn new_with_config(config: CacheConfig) -> Result<Self, CacheError> {
        // Ensure cache directory exists
        fs::create_dir_all(&config.cache_dir).await?;

        let cache_manager = CacheManager {
            config,
            entries: Arc::new(RwLock::new(HashMap::new())),
            current_size: Arc::new(RwLock::new(0)),
        };

        // Load existing cache entries from disk
        cache_manager.load_existing_cache().await?;

        Ok(cache_manager)
    }

    /// Get a chunk from cache if it exists
    pub async fn get_chunk(&self, chunk_id: &str) -> Result<Option<Vec<u8>>, CacheError> {
        let mut entries = self.entries.write().await;
        
        if let Some(entry) = entries.get_mut(chunk_id) {
            // Update last accessed time for LRU
            entry.last_accessed = std::time::SystemTime::now();
            
            // Read chunk data from file
            match fs::read(&entry.file_path).await {
                Ok(data) => {
                    println!("CacheManager: Cache hit for chunk {}", chunk_id);
                    Ok(Some(data))
                }
                Err(e) => {
                    // File doesn't exist or can't be read - remove from cache
                    println!("CacheManager: Cache entry corrupted for chunk {}, removing: {}", chunk_id, e);
                    let mut current_size = self.current_size.write().await;
                    *current_size = current_size.saturating_sub(entry.size_bytes);
                    entries.remove(chunk_id);
                    Ok(None)
                }
            }
        } else {
            println!("CacheManager: Cache miss for chunk {}", chunk_id);
            Ok(None)
        }
    }

    /// Put a chunk into the cache
    pub async fn put_chunk(&self, chunk_id: &str, data: &[u8]) -> Result<(), CacheError> {
        let chunk_size = data.len() as u64;
        
        // Check if we need to evict chunks to make space
        self.ensure_space_available(chunk_size).await?;

        // Write chunk to cache file
        let cache_file_path = self.config.cache_dir.join(format!("{}.enc", chunk_id));
        fs::write(&cache_file_path, data).await?;

        // Update cache metadata
        let entry = CacheEntry {
            chunk_id: chunk_id.to_string(),
            file_path: cache_file_path,
            size_bytes: chunk_size,
            last_accessed: std::time::SystemTime::now(),
        };

        let mut entries = self.entries.write().await;
        let mut current_size = self.current_size.write().await;

        // If chunk already exists, remove old size
        if let Some(old_entry) = entries.get(chunk_id) {
            *current_size = current_size.saturating_sub(old_entry.size_bytes);
        }

        entries.insert(chunk_id.to_string(), entry);
        *current_size += chunk_size;

        println!("CacheManager: Cached chunk {} ({} bytes, total cache: {} bytes)", 
                chunk_id, chunk_size, *current_size);

        Ok(())
    }

    /// Get current cache statistics
    pub async fn get_stats(&self) -> CacheStats {
        let entries = self.entries.read().await;
        let current_size = self.current_size.read().await;
        
        CacheStats {
            total_chunks: entries.len(),
            total_size_bytes: *current_size,
            max_size_bytes: self.config.max_size_bytes,
            max_chunks: self.config.max_chunks,
            hit_rate: 0.0, // TODO: Track hit/miss ratio
        }
    }

    /// Clear all cached chunks
    pub async fn clear(&self) -> Result<(), CacheError> {
        let mut entries = self.entries.write().await;
        let mut current_size = self.current_size.write().await;

        // Remove all cache files
        for entry in entries.values() {
            if let Err(e) = fs::remove_file(&entry.file_path).await {
                println!("Warning: Failed to remove cache file {}: {}", entry.file_path.display(), e);
            }
        }

        entries.clear();
        *current_size = 0;

        println!("CacheManager: Cleared all cached chunks");
        Ok(())
    }

    /// Load existing cache entries from disk on startup
    async fn load_existing_cache(&self) -> Result<(), CacheError> {
        let mut entries = self.entries.write().await;
        let mut current_size = self.current_size.write().await;

        let mut dir_entries = fs::read_dir(&self.config.cache_dir).await?;
        while let Some(entry) = dir_entries.next_entry().await? {
            let path = entry.path();
            
            // Only process .enc files
            if path.extension().and_then(|s| s.to_str()) == Some("enc") {
                if let Some(file_stem) = path.file_stem().and_then(|s| s.to_str()) {
                    let chunk_id = file_stem.to_string();
                    
                    match entry.metadata().await {
                        Ok(metadata) => {
                            let cache_entry = CacheEntry {
                                chunk_id: chunk_id.clone(),
                                file_path: path,
                                size_bytes: metadata.len(),
                                last_accessed: metadata.accessed().unwrap_or(std::time::SystemTime::now()),
                            };
                            
                            *current_size += cache_entry.size_bytes;
                            entries.insert(chunk_id, cache_entry);
                        }
                        Err(e) => {
                            println!("Warning: Failed to read metadata for cache file {}: {}", path.display(), e);
                        }
                    }
                }
            }
        }

        println!("CacheManager: Loaded {} existing cache entries ({} bytes)", 
                entries.len(), *current_size);
        Ok(())
    }

    /// Ensure there's enough space for a new chunk, evicting old chunks if necessary
    async fn ensure_space_available(&self, needed_bytes: u64) -> Result<(), CacheError> {
        let mut entries = self.entries.write().await;
        let mut current_size = self.current_size.write().await;

        // Check if we need to evict by size
        while *current_size + needed_bytes > self.config.max_size_bytes && !entries.is_empty() {
            self.evict_lru_chunk(&mut entries, &mut current_size).await?;
        }

        // Check if we need to evict by count
        while entries.len() >= self.config.max_chunks && !entries.is_empty() {
            self.evict_lru_chunk(&mut entries, &mut current_size).await?;
        }

        Ok(())
    }

    /// Evict the least recently used chunk
    async fn evict_lru_chunk(
        &self,
        entries: &mut HashMap<String, CacheEntry>,
        current_size: &mut u64,
    ) -> Result<(), CacheError> {
        // Find the chunk with the oldest last_accessed time
        let lru_chunk_id = entries
            .iter()
            .min_by_key(|(_, entry)| entry.last_accessed)
            .map(|(id, _)| id.clone());

        if let Some(chunk_id) = lru_chunk_id {
            if let Some(entry) = entries.remove(&chunk_id) {
                // Remove the file
                if let Err(e) = fs::remove_file(&entry.file_path).await {
                    println!("Warning: Failed to remove evicted cache file {}: {}", entry.file_path.display(), e);
                }

                *current_size = current_size.saturating_sub(entry.size_bytes);
                println!("CacheManager: Evicted chunk {} ({} bytes)", chunk_id, entry.size_bytes);
            }
        }

        Ok(())
    }
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub total_chunks: usize,
    pub total_size_bytes: u64,
    pub max_size_bytes: u64,
    pub max_chunks: usize,
    pub hit_rate: f64,
}

/// Initialize the global cache manager (must be called at app startup)
pub async fn initialize_cache() -> Result<(), CacheError> {
    let manager = CacheManager::new().await?;
    CACHE_MANAGER.set(manager).map_err(|_| {
        CacheError::Config("Cache manager already initialized".to_string())
    })?;
    Ok(())
}

/// Initialize the global cache manager with custom config
pub async fn initialize_cache_with_config(config: CacheConfig) -> Result<(), CacheError> {
    let manager = CacheManager::new_with_config(config).await?;
    CACHE_MANAGER.set(manager).map_err(|_| {
        CacheError::Config("Cache manager already initialized".to_string())
    })?;
    Ok(())
}

/// Get the global cache manager instance
pub fn get_cache() -> &'static CacheManager {
    CACHE_MANAGER.get()
        .expect("Cache manager not initialized - call initialize_cache first")
}
