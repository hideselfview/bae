# Local Playback Implementation Plan

## Goal

Add native audio playback to the desktop app using rodio. Users can play tracks directly without external Subsonic clients.

## Architecture

```
AlbumDetail (UI)
    ↓ click "Play"
PlaybackService
    ↓ fetch chunks
LibraryManager + CloudStorage + Cache
    ↓ decrypt + reassemble
rodio Sink
    → audio output
```

## Key Components

### PlaybackService (`src/playback/service.rs`)

Long-lived service in AppContext, similar to ImportService:

```rust
pub struct PlaybackService {
    sink: Sink,  // rodio audio sink
    queue: Arc<Mutex<VecDeque<String>>>,  // track IDs
    current_track: Arc<Mutex<Option<DbTrack>>>,
    is_playing: Arc<AtomicBool>,
    
    command_tx: mpsc::UnboundedSender<PlaybackCommand>,
}

enum PlaybackCommand {
    Play(String),  // track_id
    Pause,
    Resume,
    Stop,
    Next,
    Seek(Duration),
}
```

### Chunk Reassembly

Reuse the existing logic from `subsonic.rs`:
- Fetch chunks from cache/cloud (already works)
- Decrypt chunks (already works)
- Concatenate into continuous buffer
- For CUE/FLAC: seek to track position, prepend header (already works)

Extract into `src/playback/reassembly.rs` so both subsonic and local playback use it.

### UI Hook

```rust
pub fn use_playback_service() -> PlaybackHandle {
    let app_context = use_context::<AppContext>();
    app_context.playback_service.clone()
}
```

## Implementation Steps

### 1. Add rodio dependency
```toml
rodio = "0.21"
```

### 2. Create PlaybackService

Create `src/playback/mod.rs` and `src/playback/service.rs`:

- Initialize rodio `OutputStream` and `Sink`
- Create command channel
- Spawn command handler task on shared Tokio runtime
- Implement `play_track(track_id)` that:
  - Queries DB for track chunks
  - Calls chunk reassembly function
  - Loads buffer into rodio sink
  - Updates current_track state

### 3. Extract chunk reassembly

Move chunk reassembly logic from `subsonic.rs` into `src/playback/reassembly.rs`:

```rust
pub async fn reassemble_track(
    track_id: &str,
    library_manager: &LibraryManager,
    cloud_storage: &CloudStorageManager,
    cache: &CacheManager,
    encryption_service: &EncryptionService,
) -> Result<Vec<u8>, String> {
    // Existing logic from subsonic.rs stream_track()
}
```

Update `subsonic.rs` to call this function.

### 4. Wire up to AppContext

In `main.rs`:
```rust
let playback_service = PlaybackService::start(
    library_manager.clone(),
    cloud_storage.clone(),
    cache_manager.clone(),
    encryption_service.clone(),
    runtime_handle.clone(),
);

let app_context = AppContext {
    // ... existing fields
    playback_service,
};
```

### 5. Add NowPlayingBar UI

Create `src/components/now_playing_bar.rs`:

- Fixed position at bottom of screen
- Display current track title, artist, album
- Play/pause button
- Progress bar (update every second via `use_effect`)

Add to `App` component layout.

### 6. Add Play button to AlbumDetail

In `album_detail.rs`, add a "Play Album" button that calls:
```rust
playback_service.play_album(album_id)
```

This queues all tracks and starts playback.

### 7. Implement queue

Add queue management to PlaybackService:
- `play_album()` - load all album tracks into queue
- `next()` - pop next track from queue
- Auto-advance when track finishes (rodio callback)

### 8. Add seeking

- Update progress bar to be clickable
- Calculate target position in seconds
- Use rodio's `try_seek()` method
- For chunked files, may need to reassemble and seek within buffer

### 9. Handle CUE/FLAC

Reuse existing CUE/FLAC logic from subsonic.rs:
- Detect if track has `track_positions` record
- Reassemble full FLAC file chunks
- Seek to track start byte
- Prepend FLAC header from `files.flac_header`
- Feed to rodio (symphonia decoder handles FLAC)

## File Structure

```
src/
  playback/
    mod.rs         # Re-exports
    service.rs     # PlaybackService
    reassembly.rs  # Chunk reassembly (shared with subsonic)
  components/
    now_playing_bar.rs
```

## Testing

1. Play a regular FLAC album (multi-file)
2. Play a CUE/FLAC album (single file, multiple tracks)
3. Test queue navigation (next/previous)
4. Test seeking within tracks
5. Verify chunks are being cached and reused

## Open Questions

1. **Buffering**: Rodio handles buffering internally, so we just feed it the full track buffer. For large tracks, we might want to stream chunks incrementally - revisit if memory becomes an issue.

2. **Gapless playback**: Rodio supports this via `append()` on the sink. Queue next track buffer while current is playing.

3. **Volume**: Rodio `Sink` has `.set_volume(f32)`. Persist to config and restore on startup.
