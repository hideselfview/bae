use crate::cache::CacheManager;
use crate::cloud_storage::CloudStorageManager;
use crate::db::DbChunk;
use crate::encryption::EncryptionService;
use crate::library::LibraryManager;
use futures::stream::{self, StreamExt};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// Number of chunks to prefetch for adjacent tracks during gapless playback
pub const PREFETCH_CHUNKS: usize = 5;

/// Manages fetching and buffering chunks for streaming playback
///
/// Maintains a sliding window of decrypted chunks that are fetched on-demand
/// and cached. Supports parallel fetching of multiple chunks and pre-fetching
/// for gapless playback.
pub struct ChunkBuffer {
    library_manager: LibraryManager,
    cloud_storage: CloudStorageManager,
    cache: CacheManager,
    encryption_service: EncryptionService,
    release_id: String,
    /// Map of chunk_index -> decrypted chunk data
    /// Chunks are stored in order and can be accessed by index
    loaded_chunks: Arc<RwLock<HashMap<i32, Vec<u8>>>>,
    /// Set of chunk indices that are currently being fetched
    pending_chunks: Arc<RwLock<std::collections::HashSet<i32>>>,
}

impl ChunkBuffer {
    /// Create a new chunk buffer for a release
    pub fn new(
        library_manager: LibraryManager,
        cloud_storage: CloudStorageManager,
        cache: CacheManager,
        encryption_service: EncryptionService,
        release_id: String,
    ) -> Self {
        Self {
            library_manager,
            cloud_storage,
            cache,
            encryption_service,
            release_id,
            loaded_chunks: Arc::new(RwLock::new(HashMap::new())),
            pending_chunks: Arc::new(RwLock::new(std::collections::HashSet::new())),
        }
    }

    /// Ensure chunks in the given range are loaded, with a minimum buffer size
    ///
    /// Fetches chunks starting from `start_chunk_index` up to at least `min_chunks` chunks.
    /// `should_cache` determines whether chunks should be cached. Only chunks for currently
    /// playing tracks should be cached; prefetched chunks should not be cached.
    /// Returns the number of chunks successfully loaded.
    pub async fn ensure_chunks_loaded(
        &self,
        start_chunk_index: i32,
        end_chunk_index: i32,
        min_chunks: usize,
        should_cache: bool,
    ) -> Result<usize, String> {
        let chunk_range = start_chunk_index..=end_chunk_index;
        let chunks = self
            .library_manager
            .get_chunks_in_range(&self.release_id, chunk_range)
            .await
            .map_err(|e| format!("Database error: {}", e))?;

        if chunks.is_empty() {
            return Err(format!(
                "No chunks found for range {}-{}",
                start_chunk_index, end_chunk_index
            ));
        }

        // Sort chunks by index
        let mut sorted_chunks = chunks;
        sorted_chunks.sort_by_key(|c| c.chunk_index);

        // Determine which chunks need to be fetched
        let chunks_to_fetch: Vec<_> = {
            let loaded = self.loaded_chunks.read().await;
            let pending = self.pending_chunks.read().await;
            sorted_chunks
                .iter()
                .take(min_chunks.min(sorted_chunks.len()))
                .filter(|chunk| {
                    !loaded.contains_key(&chunk.chunk_index)
                        && !pending.contains(&chunk.chunk_index)
                })
                .cloned()
                .collect()
        };

        // If chunks are already loaded but should be cached, cache them now
        // This handles "graduating" prefetched chunks when track starts playing
        // Only chunks already in buffer get cached; new chunks will be cached during fetch
        if should_cache {
            let loaded = self.loaded_chunks.read().await;
            let loaded_indices: std::collections::HashSet<i32> = loaded.keys().copied().collect();
            let chunks_to_cache: Vec<_> = sorted_chunks
                .iter()
                .filter(|chunk| loaded_indices.contains(&chunk.chunk_index))
                .cloned()
                .collect();
            drop(loaded);

            // Re-download and cache chunks that are in buffer but not cached yet
            // These are prefetched chunks that "graduate" to cache when track starts playing
            for chunk in chunks_to_cache {
                let cloud_storage = self.cloud_storage.clone();
                let cache = self.cache.clone();
                let chunk_id = chunk.id.clone();
                let storage_location = chunk.storage_location.clone();
                tokio::spawn(async move {
                    // Check if already cached (may have been cached by another spawn)
                    if let Ok(None) = cache.get_chunk(&chunk_id).await {
                        // Re-download to get encrypted data for caching
                        // This is the "graduation" - prefetched chunks become cached
                        if let Ok(data) = cloud_storage.download_chunk(&storage_location).await {
                            if let Err(e) = cache.put_chunk(&chunk_id, &data).await {
                                warn!("Failed to cache chunk {} (non-fatal): {}", chunk_id, e);
                            } else {
                                debug!("Graduated prefetched chunk {} to cache", chunk_id);
                            }
                        }
                    }
                });
            }
        }

        if chunks_to_fetch.is_empty() {
            // All requested chunks are already loaded or pending
            let loaded = self.loaded_chunks.read().await;
            return Ok(loaded.len());
        }

        // Mark chunks as pending
        {
            let mut pending = self.pending_chunks.write().await;
            for chunk in &chunks_to_fetch {
                pending.insert(chunk.chunk_index);
            }
        }

        debug!(
            "Fetching {} chunks for range {}-{}",
            chunks_to_fetch.len(),
            start_chunk_index,
            end_chunk_index
        );

        // Download and decrypt chunks in parallel (max 10 concurrent)
        let chunk_results: Vec<Result<(i32, Vec<u8>), String>> = stream::iter(chunks_to_fetch)
            .map(|chunk| {
                let cloud_storage = self.cloud_storage.clone();
                let cache = self.cache.clone();
                let encryption_service = self.encryption_service.clone();
                async move {
                    let chunk_data = Self::download_and_decrypt_chunk(
                        &chunk,
                        &cloud_storage,
                        &cache,
                        &encryption_service,
                        should_cache,
                    )
                    .await?;
                    Ok::<_, String>((chunk.chunk_index, chunk_data))
                }
            })
            .buffer_unordered(10) // Download up to 10 chunks concurrently
            .collect()
            .await;

        // Store successfully loaded chunks
        let mut loaded = self.loaded_chunks.write().await;
        let mut pending = self.pending_chunks.write().await;
        let mut loaded_count = 0;

        for result in chunk_results {
            match result {
                Ok((chunk_index, chunk_data)) => {
                    loaded.insert(chunk_index, chunk_data);
                    pending.remove(&chunk_index);
                    loaded_count += 1;
                }
                Err(e) => {
                    warn!("Failed to load chunk: {}", e);
                    // Remove from pending even on error
                    if let Some(chunk) =
                        sorted_chunks.iter().find(|c| c.chunk_index == loaded_count)
                    {
                        pending.remove(&chunk.chunk_index);
                    }
                }
            }
        }

        debug!("Loaded {} chunks into buffer", loaded_count);
        Ok(loaded.len())
    }

    /// Get decrypted chunk data by chunk index
    ///
    /// Returns None if the chunk is not yet loaded.
    pub async fn get_chunk_data(&self, chunk_index: i32) -> Option<Vec<u8>> {
        let loaded = self.loaded_chunks.read().await;
        loaded.get(&chunk_index).cloned()
    }

    /// Pre-fetch chunks for adjacent tracks for gapless playback
    ///
    /// Fetches the first few chunks of the next track and last few chunks of the previous track.
    /// These chunks are ephemeral and will be evicted when playback advances or stops.
    pub async fn prefetch_adjacent_tracks(
        &self,
        previous_track_coords: Option<&crate::db::DbTrackChunkCoords>,
        next_track_coords: Option<&crate::db::DbTrackChunkCoords>,
    ) -> Result<(), String> {
        // Pre-fetch last chunks of previous track (do not cache)
        if let Some(coords) = previous_track_coords {
            let start_chunk =
                (coords.end_chunk_index - i32::try_from(PREFETCH_CHUNKS).unwrap_or(0) + 1)
                    .max(coords.start_chunk_index);
            if start_chunk <= coords.end_chunk_index {
                let _ = self
                    .ensure_chunks_loaded(
                        start_chunk,
                        coords.end_chunk_index,
                        PREFETCH_CHUNKS,
                        false, // Do not cache prefetched chunks
                    )
                    .await;
            }
        }

        // Pre-fetch first chunks of next track (do not cache)
        if let Some(coords) = next_track_coords {
            let end_chunk =
                (coords.start_chunk_index + PREFETCH_CHUNKS as i32 - 1).min(coords.end_chunk_index);
            if coords.start_chunk_index <= end_chunk {
                let _ = self
                    .ensure_chunks_loaded(
                        coords.start_chunk_index,
                        end_chunk,
                        PREFETCH_CHUNKS,
                        false, // Do not cache prefetched chunks
                    )
                    .await;
            }
        }

        Ok(())
    }

    /// Download and decrypt a single chunk
    ///
    /// `should_cache` determines whether to cache the chunk. Prefetched chunks should not be cached
    /// until their track starts playing.
    async fn download_and_decrypt_chunk(
        chunk: &DbChunk,
        cloud_storage: &CloudStorageManager,
        cache: &CacheManager,
        encryption_service: &EncryptionService,
        should_cache: bool,
    ) -> Result<Vec<u8>, String> {
        // Check cache first
        let encrypted_data = match cache.get_chunk(&chunk.id).await {
            Ok(Some(cached_encrypted_data)) => {
                debug!("Cache hit for chunk: {}", chunk.id);
                cached_encrypted_data
            }
            Ok(None) => {
                debug!("Cache miss - downloading chunk from cloud: {}", chunk.id);
                // Download from cloud storage
                let data = cloud_storage
                    .download_chunk(&chunk.storage_location)
                    .await
                    .map_err(|e| format!("Failed to download chunk: {}", e))?;

                // Only cache if should_cache is true (for currently playing tracks)
                // Prefetched chunks are not cached until their track starts playing
                if should_cache {
                    if let Err(e) = cache.put_chunk(&chunk.id, &data).await {
                        warn!("Failed to cache chunk (non-fatal): {}", e);
                    }
                }
                data
            }
            Err(e) => {
                warn!("Cache error (continuing with download): {}", e);
                // Download from cloud storage
                let data = cloud_storage
                    .download_chunk(&chunk.storage_location)
                    .await
                    .map_err(|e| format!("Failed to download chunk: {}", e))?;

                // Only cache if should_cache is true
                if should_cache {
                    if let Err(e) = cache.put_chunk(&chunk.id, &data).await {
                        warn!("Failed to cache chunk (non-fatal): {}", e);
                    }
                }
                data
            }
        };

        // Decrypt in spawn_blocking to avoid blocking the async runtime
        let encryption_service = encryption_service.clone();
        let decrypted_data = tokio::task::spawn_blocking(move || {
            encryption_service
                .decrypt_chunk(&encrypted_data)
                .map_err(|e| format!("Failed to decrypt chunk: {}", e))
        })
        .await
        .map_err(|e| format!("Decryption task failed: {}", e))??;

        Ok(decrypted_data)
    }
}
