# Custom Storage Backend Implementation Status

## ✅ Completed

1. **Rust Storage Backend** (`bae/src/torrent/storage.rs`)
   - `BaeStorage` struct that reads/writes pieces from/to BAE chunks
   - Chunks stored unencrypted in local cache (encryption happens on S3 upload)
   - Piece-to-chunk mapping via database
   - `read_piece()`, `write_piece()`, `hash_piece()` methods implemented

2. **FFI Bindings Structure** (`bae/src/torrent/ffi.rs`)
   - CXX bridge definition for custom storage constructor
   - Callback function signatures defined

3. **Build System** (`bae/build.rs`)
   - Added C++ compilation support via `cc` crate
   - Links against libtorrent-rasterbar

4. **C++ Skeleton** (`bae/cpp/bae_storage.h`, `bae/cpp/bae_storage.cpp`)
   - Basic structure for `disk_interface` implementation
   - ⚠️ Needs refinement to match actual libtorrent API

## ⚠️ Remaining Work

### Critical Blocker: libtorrent-rs Extension

The `libtorrent-rs` crate (v0.1.1) only exposes `lt_create_session()` which doesn't accept parameters. To use custom storage, we need:

1. **Extend libtorrent-rs FFI** to expose:
   - `session_params` struct
   - `session_params::set_disk_io_constructor()` method
   - `session::new(session_params)` constructor

2. **Complete C++ Implementation**:
   - Fix `BaeDiskInterface` to match actual libtorrent `disk_interface` API
   - Implement proper `disk_buffer_holder` handling
   - Handle async operations correctly
   - Test with actual libtorrent

3. **TorrentClient Integration**:
   - Add `TorrentClient::new_with_storage()` method
   - Create `BaeStorage` instance and pass callbacks to C++
   - Initialize session with custom storage constructor

### Current Workaround

Until libtorrent-rs is extended, the custom storage backend exists but cannot be integrated. The Rust side is complete and ready to use once the FFI bridge is in place.

## Next Steps

1. Extend libtorrent-rs FFI bindings (or fork/contribute upstream)
2. Complete C++ `disk_interface` implementation
3. Test integration end-to-end
4. Update `TorrentClient` to use custom storage
5. Remove `torrent_producer` (no longer needed - libtorrent handles chunking)

## Architecture

```
libtorrent → C++ disk_interface → FFI callbacks → Rust BaeStorage → CacheManager → Chunks
```

The custom storage eliminates double storage:
- ❌ Old: libtorrent files + BAE encrypted chunks in S3
- ✅ New: BAE chunks only (unencrypted in cache, encrypted in S3)

