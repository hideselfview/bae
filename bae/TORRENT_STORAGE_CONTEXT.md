# Torrent Custom Storage Implementation - Context Summary

## Current Status

**✅ Completed:**
- C++ `disk_interface` implementation (`bae_storage.h/.cpp`) with all required methods
- FFI bindings via CXX (`torrent/ffi.rs`) for custom storage constructor
- Build system compiles C++ code and links libtorrent
- Rust `BaeStorage` struct (`torrent/storage.rs`) with read/write/hash methods
- Chunks stored unencrypted locally (encryption only for S3 upload)
- Database schema for torrents and piece-to-chunk mappings
- Cache pinning mechanism for active torrent chunks

**🚧 In Progress:**
- Wiring up Rust callbacks to C++ disk_interface
- Integrating custom storage into TorrentClient session initialization

## Architecture

### C++ Side (`cpp/bae_storage.*`)
- `BaeDiskInterface`: Implements libtorrent's `disk_interface` virtual class
- `BaeBufferAllocator`: Manages buffer lifecycle for `disk_buffer_holder`
- Callbacks to Rust: `read_callback_`, `write_callback_`, `hash_callback_`
- Factory function: `create_bae_disk_io_constructor()` returns `std::function` for libtorrent

### Rust Side (`src/torrent/storage.rs`)
- `BaeStorage`: Manages piece-to-chunk mapping and storage
- Methods: `read_piece()`, `write_piece()`, `hash_piece()`
- Uses `CacheManager` for local unencrypted storage
- Uses `Database` for piece-to-chunk mapping persistence

### FFI Bridge (`src/torrent/ffi.rs`)
- CXX bridge exposing `BaeStorageConstructor`, `SessionParams`, `Session`
- Functions: `create_bae_storage_constructor()`, `create_session_params_with_storage()`, `create_session_with_params()`

## Key Design Decisions

1. **Unencrypted Local Cache**: Chunks stored unencrypted locally for seeding performance. Encryption only when uploading to S3.

2. **Piece-to-Chunk Mapping**: Torrent pieces (256KB-4MB) mapped to BAE chunks (1MB) independently. Database stores mappings for reconstruction.

3. **Custom Storage Backend**: Direct integration with libtorrent's `disk_interface` to avoid double storage (libtorrent files + BAE chunks).

4. **Dedicated Thread**: `ImportService` runs on dedicated OS thread due to `UniquePtr` not being `Send`/`Sync`.

## Next Steps

1. **Wire Callbacks**: Create callback functions in Rust that call `BaeStorage` methods
2. **Session Initialization**: Update `TorrentClient::new()` to create session with custom storage
3. **Test Integration**: Verify pieces are read/written through custom storage

## Files

- `cpp/bae_storage.h` - C++ header
- `cpp/bae_storage.cpp` - C++ implementation
- `cpp/bae_storage_helpers.h/.cpp` - Session creation helpers
- `src/torrent/storage.rs` - Rust storage backend
- `src/torrent/ffi.rs` - CXX FFI bindings
- `src/torrent/client.rs` - TorrentClient (needs custom storage integration)
- `build.rs` - C++ compilation setup

