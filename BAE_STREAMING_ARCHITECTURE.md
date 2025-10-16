# bae Streaming Architecture

This document specifies how bae serves music through encrypted chunk reassembly and Subsonic API compatibility.

## Overview

bae streaming architecture combines:
- **Encrypted chunk storage** for security and cloud distribution
- **Subsonic API compatibility** for client support
- **Real-time decryption and reassembly** for playback

## Storage Architecture

### Cloud-First Chunk Storage

bae uses a cloud-first storage model with local caching for streaming. For library configuration and initialization details, see [BAE_LIBRARY_CONFIGURATION.md](BAE_LIBRARY_CONFIGURATION.md).

```
Source Folder: /Users/user/Downloads/Album/
    ↓
Import Pipeline: Scan files → Match to Discogs → Streaming Chunk Pipeline
    ↓
Stream Processing:
    Read files sequentially → Fill chunk buffer (configurable size, default 1MB)
         ↓                              ↓
    Continue next file      Encrypt (spawn_blocking) → Upload (parallel, configurable workers)
         ↓                              ↓
    [loop until done]           Update progress
    ↓
Primary Storage: S3 (encrypted chunks with hash-based partitioning)
    ↓
Local Cache: ~/.bae/cache/ (encrypted chunks, LRU eviction)
    ↓
Streaming: Cache → Decrypt → Reassemble → Stream
```

**Streaming Pipeline:** bae reads album files sequentially into chunk buffers using `tokio::io::BufReader`. When a buffer fills (configurable, default 1MB), it's encrypted via `tokio::task::spawn_blocking` (CPU-bound work on blocking thread pool), then uploaded with bounded parallelism controlled by `.buffer_unordered()` from the futures crate. Reading continues while encryption and uploads happen concurrently. This eliminates temporary files, reduces memory usage, and maximizes throughput through parallel processing. See `BAE_IMPORT_WORKFLOW.md` for import details and `BAE_CUE_FLAC_SPEC.md` for CUE/FLAC handling.

**Import Pipeline Architecture:**
- `import/service.rs` - Orchestrates validation, queueing, and pipeline execution
- `import/pipeline/` - Stream-based pipeline with `impl Stream` return type for composition
- `import/album_layout.rs` - Analyzes file-to-chunk and chunk-to-track mappings
- `import/track_file_mapper.rs` - Validates track-to-file mapping before DB insertion
- `import/metadata_persister.rs` - Persists file/chunk metadata to database after upload

**Import Status Flow:**
1. User initiates import → Validation runs synchronously
2. If valid → Album/tracks inserted with `ImportStatus::Queued`
3. Pipeline starts → Album marked `ImportStatus::Importing`
4. Chunks upload → Track marked `ImportStatus::Complete` when all its chunks finish
5. All tracks done → Album marked `ImportStatus::Complete`

**Progress Tracking:**
- Chunk-to-track mapping built during layout analysis
- Each chunk completion checked against mapping
- Tracks marked complete as soon as all their chunks upload
- Real-time progress events via `ImportProgressService`

**Database Schema:**
- `albums` → album metadata from Discogs
- `tracks` → individual track metadata
- `files` → original file info (filename, size, format)
- `chunks` → encrypted album chunks (chunk_index, encrypted_size, S3 location)
- `file_chunks` → file-to-chunk mapping (which chunks contain which files)
- `cue_sheets` → parsed CUE sheet data for CUE/FLAC albums
- `track_positions` → seek offsets within files for CUE/FLAC streaming

**Chunk Format:**
```
[nonce_len(4)][nonce(12)][key_id_len(4)][key_id][encrypted_data]
```

Chunks use AES-256-GCM encryption. Integrity is guaranteed by the GCM authentication tag, so no separate checksum is stored.

### Storage Locations

**Primary Storage (S3):**
- Hash-partitioned chunks: `s3://bucket/chunks/ab/cd/chunk_abcd1234-5678-9abc-def0-123456789abc.enc`
- Uses first 4 UUID characters for even distribution across prefixes
- Encrypted with AES-256-GCM before upload
- Source of truth for all music data
- Scales to millions of chunks without S3 throttling

**Local Storage (`~/.bae/`):**
- Cache directory: `~/.bae/cache/` (encrypted chunks, LRU eviction)
- Database: `~/.bae/libraries/{library_id}/library.db` (SQLite)
- Config file: `~/.bae/config.yaml` (library settings, S3 credentials)
- Encryption keys stored in system keyring (production) or `.env` file (dev mode)

## Subsonic API Implementation

### Server Foundation

bae runs a Subsonic 1.16.1 compatible API server on `localhost:4533` alongside the desktop app:

```rust
// Desktop App (Dioxus) + API Server (Axum)
tokio::spawn(start_subsonic_server());  // Background API
LaunchBuilder::desktop().launch(App);   // Desktop UI
```

### Implemented Endpoints

**System Endpoints:**
- `GET /rest/ping` → connectivity test
- `GET /rest/getLicense` → always valid (open source)

**Browsing Endpoints:**
- `GET /rest/getArtists` → artists grouped alphabetically
- `GET /rest/getAlbumList` → all albums with metadata
- `GET /rest/getAlbum?id={album_id}` → album with tracklist

**Streaming Endpoint:**
- `GET /rest/stream?id={track_id}` → reassembled audio stream

### Response Format

All responses use standard Subsonic JSON envelope:

```json
{
  "subsonic-response": {
    "status": "ok",
    "version": "1.16.1",
    "artists": { ... }
  }
}
```

### Authentication

No authentication - any username/password accepted. This allows any Subsonic client to connect to the local server.

## Streaming Pipeline

### Track-to-Chunks Mapping

For streaming, bae maps tracks to their constituent chunks via file-to-chunk mapping:

```
Track ID → Files → File-Chunk Mapping → Album Chunks → Decrypted Data → Audio Stream
```

**Example Flow:**
1. Client requests: `GET /rest/stream?id=track_123`
2. bae queries: `SELECT * FROM files WHERE track_id = 'track_123'`
3. bae queries: `SELECT * FROM file_chunks WHERE file_id = 'file_456'`
4. bae queries: `SELECT * FROM chunks WHERE album_id = 'album_789' AND chunk_index BETWEEN start_chunk AND end_chunk`
5. bae downloads/decrypts chunks and extracts file portion using byte offsets
6. bae streams reassembled audio with HTTP headers

### Chunk Reassembly Process

```rust
async fn stream_track_chunks(track_id: &str) -> Result<Vec<u8>, Error> {
    // 1. Get files for track
    let files = library_manager.get_files_for_track(track_id).await?;
    
    // 2. Get chunks for first file (most tracks = 1 file)
    let chunks = library_manager.get_chunks_for_file(&files[0].id).await?;
    
    // 3. Sort chunks by index
    chunks.sort_by_key(|c| c.chunk_index);
    
    // 4. Download and decrypt each chunk
    let mut audio_data = Vec::new();
    for chunk in chunks {
        let decrypted = download_and_decrypt_chunk(&chunk).await?;
        audio_data.extend_from_slice(&decrypted);
    }
    
    // 5. Return complete audio
    Ok(audio_data)
}
```

### Cache-First Decryption Process

Each chunk is retrieved through the cache layer:

```rust
async fn download_and_decrypt_chunk(chunk: &DbChunk, cache: &CacheManager) -> Result<Vec<u8>, Error> {
    // 1. Check local cache first
    if let Some(cached_data) = cache.get_chunk(&chunk.id).await? {
        let encryption_service = EncryptionService::new()?;
        return Ok(encryption_service.decrypt_chunk(&cached_data)?);
    }
    
    // 2. Download from S3 and cache
    let cloud_storage = CloudStorageManager::new()?;
    let encrypted_data = cloud_storage.download_chunk(&chunk.storage_location).await?;
    
    // 3. Cache for future requests
    cache.put_chunk(&chunk.id, &encrypted_data).await?;
    
    // 4. Decrypt and return
    let encryption_service = EncryptionService::new()?;
    Ok(encryption_service.decrypt_chunk(&encrypted_data)?)
}
```

## Client Compatibility

### Supported Clients

Any Subsonic-compatible client can connect to `http://localhost:4533`:

**Mobile:**
- DSub (Android)
- Ultrasonic (Android)
- play:Sub (iOS)
- substreamer (iOS)

**Desktop:**
- Clementine
- Strawberry
- Subsonic Web UI

**Web:**
- Jamstash
- Aurial
- Subfire

## File Format Support

bae supports:
- Individual audio files: `1 file = 1 track` (MP3, FLAC, WAV, M4A, AAC, OGG)
- CUE/FLAC albums: `1 file = entire album` with track boundaries

### CUE/FLAC Support

CUE/FLAC albums are fully supported:

```
album.flac + album.cue
    ↓
Parse CUE: track boundaries (00:00, 03:45, 07:22, ...)
    ↓
Chunk entire FLAC: N × (configurable MB) encrypted chunks
    ↓
Stream with seeking: reassemble chunks + seek to track position
```

**Implementation:**
- Parse `.cue` files for track boundaries using nom parser
- Seek to specific positions within large FLAC files using symphonia
- Store FLAC headers in database for streaming
- Chunk-range streaming reduces bandwidth
- Precise track extraction with audio processing
- `cue_sheets` table stores parsed CUE data
- `track_positions` table stores seek offsets within files
- `files` table extended with FLAC headers and CUE flags

See `BAE_CUE_FLAC_SPEC.md` for complete implementation details.

## Security Model

### Encryption at Rest

- All chunks encrypted with AES-256-GCM
- Master keys stored in system keyring (production) or `.env` file (dev mode)
- Unique nonces prevent cryptographic attacks
- Key ID verification prevents key confusion
- AES-GCM authentication tag ensures integrity (no separate checksum needed)

### Network Security

- Local-only API server (localhost:4533)
- No external network exposure by default
- No authentication (any client can connect to local server)

## Performance Characteristics

### Memory Usage

Entire track loaded into memory before streaming. Bounded channel backpressure during import limits memory usage during chunk processing.

### Latency

- **Startup**: ~100-500ms (decrypt first chunks)
- **Seeking**: Not supported (would require re-decryption)
- **Network**: Local-only (no network latency)

### Throughput

- **Bottleneck**: AES decryption speed
- **Optimization**: Parallel chunk decryption possible
- **Caching**: Encrypted chunks cached locally with LRU eviction

This architecture provides encrypted chunk storage with Subsonic API compatibility.
