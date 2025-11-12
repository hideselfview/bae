# Next Steps: Unblocking Custom Storage Backend

## Strategy: Extend FFI Bindings Ourselves

Instead of waiting for libtorrent-rs to expose `session_params`, we'll add minimal FFI bindings ourselves using `cxx` (which we already have). This is simpler than forking libtorrent-rs.

## Step-by-Step Plan

### Step 1: Add Minimal FFI Bindings for session_params

**File:** `bae/src/torrent/ffi.rs` (extend existing)

Add CXX bindings to expose:
- `session_params` struct creation
- `session_params::set_disk_io_constructor()` 
- `session::new(session_params)` constructor

**Approach:**
- Use `cxx` to create thin wrappers around libtorrent C++ API
- Add C++ helper functions in `bae/cpp/bae_storage.cpp` that libtorrent-rs doesn't expose
- Keep it minimal - only what we need

**Estimated effort:** 2-3 hours

### Step 2: Complete C++ disk_interface Implementation

**Files:** `bae/cpp/bae_storage.h`, `bae/cpp/bae_storage.cpp`

**Tasks:**
1. Fix API mismatches (check libtorrent headers for exact signatures)
2. Implement proper `disk_buffer_holder` allocation/deallocation
3. Handle async callbacks correctly
4. Test compilation against libtorrent

**Approach:**
- Start with minimal implementation (just read/write)
- Add other methods incrementally
- Use libtorrent's in-memory storage example as reference

**Estimated effort:** 4-6 hours

### Step 3: Wire Up Callbacks

**File:** `bae/src/torrent/client.rs`

**Tasks:**
1. Create `TorrentClient::new_with_storage()` method
2. Instantiate `BaeStorage` with callbacks
3. Pass callbacks to C++ via FFI
4. Create session with custom storage

**Estimated effort:** 1-2 hours

### Step 4: Test & Iterate

**Tasks:**
1. Test with small torrent (metadata only)
2. Verify chunks are created in cache
3. Test piece reads during seeding
4. Fix bugs as they appear

**Estimated effort:** 2-4 hours

## Implementation Order

**Option A: Bottom-Up (Recommended)**
1. ✅ Rust storage backend (DONE)
2. Add FFI bindings for session_params
3. Complete C++ disk_interface (minimal first)
4. Wire up callbacks
5. Test end-to-end

**Option B: Top-Down**
1. ✅ Rust storage backend (DONE)  
2. Complete C++ disk_interface fully
3. Add FFI bindings
4. Wire up callbacks
5. Test

## Immediate Next Action

**Start with Step 1:** Add FFI bindings for `session_params`

We'll create a new CXX bridge module that extends libtorrent-sys with just the functions we need. This is the critical blocker - once we can create a session with custom storage, everything else falls into place.

## Alternative: Simpler Approach?

If the C++ integration proves too complex, we could:
- Keep using `torrent_producer` for now (works, but double storage)
- Implement custom storage later when libtorrent-rs matures
- Focus on other features first

But given the storage cost issue, custom storage is worth the effort.

