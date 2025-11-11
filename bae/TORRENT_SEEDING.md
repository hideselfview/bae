# Torrent Seeding in BAE

## Overview

BAE supports seeding torrents after import, allowing you to share imported music with the BitTorrent swarm while keeping data encrypted in cloud storage.

## Architecture

### Components

1. **TorrentSeeder** (`src/torrent/seeder.rs`): Manages torrent seeding
   - Reads piece data from cached chunks
   - Pins chunks to prevent LRU eviction
   - Marks torrents as seeding in database

2. **Cache Pinning** (`src/cache.rs`): Prevents active torrent chunks from being evicted
   - `pin_chunks()`: Pins chunks for seeding
   - `unpin_chunks()`: Releases chunks when seeding stops

3. **Database**: Tracks seeding status
   - `torrents` table: Stores torrent metadata and `is_seeding` flag
   - `torrent_piece_mappings`: Maps torrent pieces to BAE chunks

4. **LibraryManager**: Provides API for seeding operations
   - `get_seeding_torrents()`: Get all actively seeding torrents
   - `set_torrent_seeding()`: Mark torrent as seeding/not seeding

## How It Works

### Import Flow
1. Torrent is added to import queue (file or magnet link)
2. Metadata files (CUE/log) are prioritized for download
3. Metadata is matched against Discogs/MusicBrainz
4. Full torrent content is downloaded
5. Audio is decoded and split into BAE chunks (1MB)
6. Chunks are encrypted and uploaded to S3
7. Piece-to-chunk mappings are saved to database
8. Torrent is kept in libtorrent session for seeding

### Seeding Flow
1. `TorrentSeeder::start_seeding(release_id)` is called
2. Load torrent metadata and piece mappings from database
3. Pin all chunks in cache to prevent eviction
4. Mark torrent as seeding in database
5. Libtorrent session continues seeding automatically

### Piece Reading
When a peer requests a piece:
1. `TorrentSeeder::read_piece(torrent_id, piece_index)` retrieves mapping
2. Load required chunks from cache
3. Decrypt chunks using encryption service
4. Extract piece bytes from chunks (pieces may span multiple chunks)
5. Return piece data to libtorrent

## API Usage

```rust
// Get library manager
let library_manager = use_library_manager();

// Start seeding a torrent
let release_id = "...";
library_manager.set_torrent_seeding(&torrent_id, true).await?;

// Stop seeding
library_manager.set_torrent_seeding(&torrent_id, false).await?;

// Get all seeding torrents
let seeding_torrents = library_manager.get_seeding_torrents().await?;
```

## Current Status

### ✅ Implemented
- Torrent import pipeline
- Piece-to-chunk mapping
- Cache pinning for active torrents
- Database schema for torrents and mappings
- TorrentSeeder with piece reading from chunks
- Library manager API for seeding control

### ⚠️ Partially Implemented
- **Libtorrent Integration**: The minimal `libtorrent-rs` API (v0.1.1) doesn't expose all functionality needed for full seeding control
- **Piece Serving**: Libtorrent naturally seeds torrents in the session, but custom piece serving (reading from BAE chunks) requires FFI extensions

### 🔄 Future Work
- Implement custom storage backend for libtorrent to serve pieces from BAE chunks
- Add UI controls for starting/stopping seeding per torrent
- Add seeding statistics (upload/download ratios, peer counts)
- Implement bandwidth limits for seeding
- Add automatic seeding on import completion

## Limitations

1. **FFI Boundary**: The `libtorrent-rs` crate provides minimal bindings. Full piece serving requires extending FFI or using a different approach.

2. **Encryption**: Chunks are currently encrypted for cloud storage. For seeding to work, either:
   - Keep decrypted pieces in cache (current approach)
   - Store pieces separately for seeding
   - Stream decryption on-the-fly

3. **Cache Capacity**: Pinned chunks remain in cache indefinitely while seeding. Ensure adequate cache size for active torrents.

## Notes

- Import service runs on dedicated thread to handle non-Send libtorrent types
- Torrent session persists across imports
- Chunks for seeding torrents are "pinned" to prevent LRU eviction
- All piece mappings are stored in database for future seeding

