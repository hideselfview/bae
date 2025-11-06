# Streaming Seek Test Plan

This document describes the comprehensive test harness for streaming playback and seeking.

## Test Scenarios

### 1. Sequential Seeks
- Seek to 30s, 60s, 120s, middle, near end
- Wait for each seek to complete before starting next
- **Expected**: Each seek completes within 30s, position is within 5s of requested

### 2. Rapid Consecutive Seeks (Seeking While Seeking)
- Fire off 3 seeks immediately: 100s → 200s → 300s
- Don't wait between seeks
- **Expected**: Final position stabilizes at 300s, no crashes

### 3. Backward Seeks
- Seek backward: 200s → 100s → 50s → 10s
- **Expected**: Backward seeking works, chunks are loaded correctly

### 4. Duplicate Seeks
- Seek to 150s twice in a row
- **Expected**: Both complete successfully

### 5. Seek During Initial Buffering
- Start track, immediately seek before playback begins
- **Expected**: Chunks at seek position load, playback starts there

## Current Issues

From logs, seeking is failing because:

1. **Seek fails with ForwardOnly error** - This is expected when stream isn't seekable yet
2. **Chunks aren't loading at seek position** - The decoder tries to seek, fails, but doesn't trigger loading the right chunks
3. **Seeking state not emitted** - UI doesn't show loading indicator

## Fix in Progress

The code now:
1. Returns error from `TrackDecoder::seek` when Symphonia's seek fails
2. Service catches error and loads chunks at target position
3. Emits `Seeking` state to UI
4. Creates new decoder with loaded chunks
5. Begins playback from target position

## To Run Tests

```bash
# Set environment variables
export BAE_TEST_FOLDER="/path/to/album/with/cuesheet"
export BAE_TEST_DISCOGS_RELEASE_ID="123456"

# Run test
cargo test test_streaming_seek -- --nocapture
```

## Next Steps

1. ✅ Create test harness with multiple scenarios
2. ✅ Handle seeking-while-seeking gracefully
3. ⏳ Fix chunk loading at seek position (current iteration)
4. ⏳ Verify UI shows loading state during seek
5. ⏳ Test with real album data

The test file `test_streaming_seek.rs` is currently being fixed to work with the actual API.

