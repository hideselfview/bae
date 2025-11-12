# Action Plan: Unblocking Custom Storage

## What We Just Did ✅

1. **Created FFI Helper Functions** (`bae/cpp/bae_storage_helpers.*`)
   - Functions to create `session_params` with custom storage
   - Function to create `session` from `session_params`
   - This extends libtorrent-rs without forking it

2. **Extended FFI Bindings** (`bae/src/torrent/ffi.rs`)
   - Added bindings for `SessionParams` and `Session` types
   - Added functions to create session with custom storage
   - Ready to use once C++ implementation is fixed

## Current Blocker: C++ API Mismatches

The C++ `disk_interface` implementation has API signature mismatches with libtorrent's actual API. This is expected - we need to fix the method signatures to match libtorrent's headers.

## Next Steps (In Order)

### 1. Fix C++ API Signatures (IMMEDIATE)

**Problem:** Method signatures in `bae_storage.h` don't match libtorrent's `disk_interface`

**Solution:** 
- Check actual libtorrent headers for correct signatures
- Fix method parameter types (e.g., `sha256_hash` vs `sha1_hash`)
- Remove `override` keywords where signatures don't match
- Start with minimal implementation (just `async_read` and `async_write`)

**Files:** `bae/cpp/bae_storage.h`, `bae/cpp/bae_storage.cpp`

**Estimated time:** 2-3 hours

### 2. Implement Minimal disk_interface (NEXT)

**Goal:** Get basic read/write working first

**Tasks:**
- Implement `async_read` properly (allocate buffer, call Rust callback, return)
- Implement `async_write` properly (receive data, call Rust callback)
- Stub out other methods (return success/no-op)
- Test compilation

**Files:** `bae/cpp/bae_storage.cpp`

**Estimated time:** 2-3 hours

### 3. Wire Up Rust Callbacks (THEN)

**Goal:** Connect Rust `BaeStorage` to C++ callbacks

**Tasks:**
- Create `BaeStorage` instance in `TorrentClient`
- Create callback functions that call `BaeStorage` methods
- Pass callbacks to C++ via FFI
- Create session with custom storage

**Files:** `bae/src/torrent/client.rs`

**Estimated time:** 1-2 hours

### 4. Test End-to-End (FINALLY)

**Goal:** Verify custom storage works

**Tasks:**
- Import small torrent
- Verify chunks created in cache
- Verify no temp files created
- Test seeding

**Estimated time:** 2-4 hours

## Strategy: Incremental Approach

**Don't try to implement everything at once.** 

1. ✅ FFI bindings structure (DONE)
2. Fix C++ compilation errors (NEXT)
3. Get minimal read/write working
4. Add remaining methods incrementally
5. Test and iterate

## Alternative: Skip C++ For Now?

If C++ integration proves too complex, we could:
- Keep using `torrent_producer` (works, but double storage)
- Document C++ work as "future enhancement"
- Focus on other features

But given storage costs, it's worth pushing through.

## Recommendation

**Start with Step 1:** Fix the C++ API signatures. Once it compiles, we can iterate on the implementation. The FFI bridge is ready - we just need the C++ side to match libtorrent's API.

