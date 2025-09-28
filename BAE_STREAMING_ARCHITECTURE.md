# bae Streaming Architecture

This document specifies how bae serves music through encrypted chunk reassembly and Subsonic API compatibility.

## Overview

bae implements a unique streaming architecture that combines:
- **Encrypted chunk storage** for security and cloud distribution
- **Subsonic API compatibility** for universal client support
- **Real-time decryption and reassembly** for seamless playback

## Storage Architecture

### Cloud-First Chunk Storage

bae uses a cloud-first storage model with local caching for streaming:

```
Source Folder: /Users/user/Downloads/Album/
    ↓
Import Process: Scan files → Match to Discogs → Chunk & Encrypt
    ↓
Primary Storage: S3 (encrypted chunks)
    ↓
Local Cache: ~/.bae/cache/ (encrypted chunks, LRU eviction)
    ↓
Streaming: Cache → Decrypt → Reassemble → Stream
```

**Database Schema:**
- `albums` → album metadata from Discogs + source folder path
- `tracks` → individual track metadata  
- `files` → original file info (filename, size, format)
- `chunks` → encrypted chunk info (index, S3 location, checksum)

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

**Local Cache (`~/.bae/cache/`):**
- Recently accessed chunks cached locally
- Same encrypted format as S3
- LRU eviction with configurable size limits
- Transparent to streaming layer

**Source Checkouts (Optional):**
- Original folder remains on disk: `/path/to/album/`
- Unencrypted original files for seeding/backup
- Can be deleted after successful import
- Can be recreated by downloading and reassembling chunks

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

For streaming, bae maps tracks to their constituent chunks:

```
Track ID → Files → Chunks → Decrypted Data → Audio Stream
```

**Example Flow:**
1. Client requests: `GET /rest/stream?id=track_123`
2. bae queries: `SELECT * FROM files WHERE track_id = 'track_123'`
3. bae queries: `SELECT * FROM chunks WHERE file_id = 'file_456' ORDER BY chunk_index`
4. bae downloads/decrypts chunks in sequence
5. bae streams reassembled audio with HTTP headers

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

### File Format Assumptions

**Current Model:** `1 file = 1 track`
- Works for: separate MP3/FLAC files per track
- Fails for: CUE/FLAC albums (1 file = entire album)

### Missing Features

**CUE Sheet Support:**
- Cannot parse `.cue` files for track boundaries
- Cannot seek to specific positions within large files
- Cannot map CUE tracks to Discogs tracklists

**Transcoding:**
- No format conversion (FLAC → MP3 for bandwidth)
- No quality adjustment for mobile clients
- No streaming optimization

**Advanced Streaming:**
- No HTTP range request support (seeking)
- No streaming buffers (entire file loaded to memory)
- No concurrent stream management

**Cloud Storage:**
- Chunk upload works, download not implemented
- No local cache management (need CacheManager)
- No checkout management (need CheckoutManager)

## Future Architecture

### CUE/FLAC Support

For albums with single FLAC + CUE sheet:

```
album.flac + album.cue
    ↓
Parse CUE: track boundaries (00:00, 03:45, 07:22, ...)
    ↓
Chunk entire FLAC: 150 × 1MB encrypted chunks
    ↓
Stream with seeking: reassemble chunks + seek to track position
```

**Database Changes:**
- Add `cue_sheets` table for parsed CUE data
- Add `track_positions` for seek offsets within files
- Modify streaming to support byte-range seeking

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

### ✅ Completed
- Chunk storage and encryption
- Subsonic API server foundation
- Basic streaming endpoint
- Local chunk decryption and reassembly
- Database schema for tracks/files/chunks

### 🔄 In Progress
- Cloud chunk download
- HTTP range request support

### ❌ Not Implemented
- CacheManager for local chunk caching with LRU eviction
- CheckoutManager for source folder lifecycle
- Cloud chunk download in streaming pipeline
- CUE sheet parsing and support
- Transcoding pipeline
- Streaming optimization
- Authentication system
- Concurrent stream management

This architecture provides a solid foundation for secure, distributed music streaming while maintaining compatibility with existing Subsonic clients.
