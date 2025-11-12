# Custom Storage Integration Status

## ✅ Completed

1. **C++ disk_interface implementation** (`cpp/bae_storage.*`)
   - All required methods implemented
   - Callbacks to Rust for read/write/hash operations
   - Proper buffer handling with custom allocator

2. **FFI bindings** (`src/torrent/ffi.rs`)
   - CXX bridge for storage constructor
   - Session creation helpers
   - Callback function signatures

3. **Rust storage backend** (`src/torrent/storage.rs`)
   - `BaeStorage` with async read/write/hash methods
   - Piece-to-chunk mapping logic
   - Database integration for piece mappings

4. **Callback infrastructure** (`src/torrent/client.rs`)
   - Storage registry (torrent_id → BaeStorage)
   - Storage index map (storage_index → torrent_id)
   - Thread-local storage for sync callbacks
   - Async-to-sync bridge using `Handle::block_on()`
   - All three callbacks implemented (read/write/hash)

## 🚧 Remaining Work

### 1. Session Integration (BLOCKER)

**Problem**: libtorrent-rs functions expect `libtorrent::session` type, but our FFI returns `Session` (opaque type). Even though they're the same C++ type, Rust treats them as incompatible.

**Solutions**:
- **Option A**: Create wrapper FFI functions for all libtorrent operations we need
  - `add_torrent`, `get_torrent_status`, etc.
  - Store custom session and use wrappers instead of libtorrent-rs API
- **Option B**: Extend libtorrent-rs to accept our Session type
  - Requires modifying libtorrent-rs crate
- **Option C**: Use unsafe casting (risky, not recommended)

**Current State**: Callbacks are created but not used because we're still using default session (`lt_create_session()`).

### 2. Storage Index Mapping

**Problem**: libtorrent assigns `storage_index` when calling `new_torrent()` on disk_interface, but we don't capture it.

**Solution**: 
- Modify C++ `new_torrent()` to capture storage_index
- Expose it via FFI callback or return value
- Map it to torrent_id when registering storage

**Current State**: `register_storage()` exists but can't be called because we don't have storage_index.

### 3. Import Flow Integration

**Problem**: Import service needs to:
- Create `BaeStorage` instance when adding torrent
- Get `storage_index` from libtorrent
- Register storage with `TorrentClient`
- Create piece mapper based on torrent metadata

**Solution**: Update `ImportService` to:
1. Create `BaeStorage` with cache_manager, database, piece_mapper
2. Call `torrent_client.register_storage(storage_index, torrent_id, storage)`
3. Remove old `torrent_producer` logic (pieces come through custom storage now)

## Architecture Summary

```
libtorrent C++ → BaeDiskInterface → Rust callbacks → Storage Registry → BaeStorage → CacheManager/Database
                                                      ↑
                                              (lookup by storage_index → torrent_id)
```

**Flow**:
1. libtorrent calls `async_read(storage_index, ...)` on C++ side
2. C++ calls Rust `read_callback(storage_index, ...)`
3. Rust callback looks up `torrent_id` from `storage_index`
4. Rust callback looks up `BaeStorage` from `torrent_id`
5. Rust callback calls `storage.read_piece(...).await` using `block_on()`
6. `BaeStorage` reads chunks from cache/database and reconstructs piece

## Next Steps

1. **Implement wrapper FFI functions** for libtorrent operations (Option A above)
2. **Capture storage_index** in `new_torrent()` and expose via FFI
3. **Update import flow** to create and register storage instances
4. **Test end-to-end**: Add torrent → pieces download → chunks stored → verify

## Files Modified

- `cpp/bae_storage.h` - C++ disk_interface header
- `cpp/bae_storage.cpp` - C++ disk_interface implementation
- `cpp/bae_storage_helpers.h/.cpp` - Session creation helpers
- `src/torrent/ffi.rs` - CXX FFI bindings
- `src/torrent/storage.rs` - Rust storage backend
- `src/torrent/client.rs` - TorrentClient with callbacks and registry

