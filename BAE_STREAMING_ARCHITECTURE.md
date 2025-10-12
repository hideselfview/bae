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
    Read files (8KB increments) → Fill chunk buffer (1MB)
         ↓                              ↓
    Continue next file      Encrypt (blocking pool) → Upload (parallel, max 20)
         ↓                              ↓
    [loop until done]           Update progress
    ↓
Primary Storage: S3 (encrypted chunks with hash-based partitioning)
    ↓
Local Cache: ~/.bae/cache/ (encrypted chunks, LRU eviction)
    ↓
Streaming: Cache → Decrypt → Reassemble → Stream
```

**Streaming Pipeline:** bae reads album files sequentially and streams data into chunk buffers. When a buffer fills (1MB), it's immediately encrypted on Tokio's blocking thread pool (enabling parallel CPU-bound encryption) then uploaded via semaphore-controlled concurrency (max 20 concurrent uploads). Reading continues while encryption and uploads happen in background. This eliminates temporary files, reduces memory usage, and maximizes throughput through parallel processing. See `BAE_IMPORT_WORKFLOW.md` for import details and `BAE_CUE_FLAC_SPEC.md` for CUE/FLAC handling.

**Database Schema:**
- `albums` → album metadata from Discogs
- `tracks` → individual track metadata  
- `files` → original file info (filename, size, format)
- `chunks` → encrypted album chunks (index, S3 location, checksum)
- `file_chunks` → file-to-chunk mapping (which chunks contain which files)

**Database Sync:**
- SQLite database backed up to S3 immediately after each import
- Manifest statistics updated every 5-10 minutes in background
- On shutdown to capture any pending changes
- Enables multi-device library access
- Database stored at `s3://bucket/bae-library.db`

**Chunk Format:**
```
[nonce_len(4)][nonce(12)][key_id_len(4)][key_id][encrypted_data]
```

### Storage Locations

**Primary Storage (S3):**
- Hash-partitioned chunks: `s3://bucket/chunks/ab/cd/chunk_abcd1234-5678-9abc-def0-123456789abc.enc`
- Uses first 4 UUID characters for even distribution across prefixes
- Encrypted with AES-256-GCM before upload
- Source of truth for all music data
- Scales to millions of chunks without S3 throttling
- Database backup: `s3://bucket/bae-library.db`
- Library manifest: `s3://bucket/bae-library.json` (indicates library presence)

**Local Storage (`~/.bae/`):**
- Cache directory: `~/.bae/cache/` (encrypted chunks, LRU eviction)
- Database: `~/.bae/libraries/{library_id}/library.db` (SQLite, synced to S3)
- Config file: `~/.bae/config.yaml` (library settings, S3 credentials)
- Encryption keys stored in system keyring

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

### Authentication

Currently no authentication - any username/password accepted.
TODO: Implement proper user management and token-based auth.

## Current Limitations

### File Format Support

**Supported Formats:**
- Individual audio files: `1 file = 1 track` (MP3, FLAC, etc.)
- CUE/FLAC albums: `1 file = entire album` with track boundaries

### CUE Sheet Support

**Implemented:**
- Parse `.cue` files for track boundaries using nom parser
- Seek to specific positions within large FLAC files using symphonia
- Store FLAC headers in database for streaming
- Chunk-range streaming reduces bandwidth
- Precise track extraction with audio processing

**Remaining:**
- AI-powered CUE-to-Discogs track mapping (currently uses simple filename matching)
- See `BAE_CUE_FLAC_SPEC.md` for implementation details

**Transcoding:**
- No format conversion (FLAC → MP3 for bandwidth)
- No quality adjustment for mobile clients
- No streaming optimization

**Advanced Streaming:**
- No HTTP range request support (seeking)
- No streaming buffers (entire file loaded to memory)
- No concurrent stream management

**Cloud Storage:**
- Chunk upload and download implemented
- Local cache management (CacheManager with LRU eviction)
- Database sync to S3 (after imports and periodically)

## Future Architecture

### CUE/FLAC Support (Completed)

CUE/FLAC albums are now fully supported:

```
album.flac + album.cue
    ↓
Parse CUE: track boundaries (00:00, 03:45, 07:22, ...)
    ↓
Chunk entire FLAC: 150 × 1MB encrypted chunks
    ↓
Stream with seeking: reassemble chunks + seek to track position
```

**Database Implementation:**
- `cue_sheets` table stores parsed CUE data
- `track_positions` table stores seek offsets within files
- `files` table extended with FLAC headers and CUE flags
- Streaming supports precise byte-range seeking with symphonia

### Transcoding Pipeline

```
Encrypted Chunks → Decrypt → Reassemble → Transcode → Stream
                                    ↓
                            FLAC → MP3/OGG/AAC
```

**Implementation:**
- Integrate FFmpeg for format conversion
- Add quality profiles (320k, 128k, 64k)
- Stream transcoded data without temp files

### Cloud Streaming

```
Track Request → Check Local Cache → Download Missing Chunks → Decrypt → Stream
                      ↓
              Cache Management (LRU, size limits)
```

## Security Model

### Encryption at Rest

- All chunks encrypted with AES-256-GCM
- Master keys stored in system keyring
- Unique nonces prevent cryptographic attacks
- Key ID verification prevents key confusion

### Network Security

- Local-only API server (localhost:4533)
- No external network exposure by default
- TODO: Add HTTPS for remote access
- TODO: Add proper authentication/authorization

## Performance Characteristics

### Memory Usage

- **Current**: Entire track loaded into memory before streaming
- **Target**: Streaming chunks with bounded memory usage

### Latency

- **Startup**: ~100-500ms (decrypt first chunks)
- **Seeking**: Not supported (would require re-decryption)
- **Network**: Local-only (no network latency)

### Throughput

- **Bottleneck**: AES decryption speed
- **Optimization**: Parallel chunk decryption
- **Caching**: Decrypted chunks could be cached temporarily

## Implementation Status

### Completed
- Chunk storage and encryption (AES-256-GCM)
- Subsonic API server foundation (axum-based)
- Streaming endpoint with chunk reassembly
- Cloud chunk upload and download (S3)
- Local chunk caching with LRU eviction (CacheManager)
- Database schema for tracks/files/chunks
- **CUE/FLAC support with precise seeking**
- **FLAC header storage for streaming**
- **Chunk-range streaming reduces bandwidth**

### In Progress
- Database sync to S3 (periodic backup)
- Library initialization and manifest detection
- First-launch setup wizard (S3 + Discogs configuration)

### Not Implemented
- Transcoding pipeline (FLAC → MP3/OGG for bandwidth)
- HTTP range request support for seeking
- Streaming optimization (buffering, concurrent streams)
- Authentication system (currently accepts any credentials)
- Advanced audio format conversion

This architecture provides encrypted chunk storage and Subsonic API compatibility.
