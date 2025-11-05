# CUE/FLAC Track Splitting - Current Status

## Overview
We're implementing accurate track splitting for CUE/FLAC pairs using libFLAC's seektable generation to map sample positions to byte positions, enabling precise track boundary extraction.

## Current Implementation

### Seektable-Based Byte Position Calculation
- **Location**: `bae/src/import/album_chunk_layout.rs`
- **Function**: `build_flac_seektable()`
- **Approach**:
  1. Use libFLAC stream decoder to scan all frames in the FLAC file
  2. Extract sample numbers from frame headers and track byte positions via decode position callbacks
  3. Build a HashMap mapping sample positions to byte positions
  4. Use Symphonia for time-based seeking to get sample positions, then lookup in seektable for byte positions
  5. This provides accurate byte positions even for variable bitrate FLAC files

### Test Infrastructure
- **Test**: `bae/tests/cue_flac_import_test.rs`
- **Features**:
  - Creates unique S3 bucket per test run
  - Imports a CUE/FLAC album (configured via environment variables)
  - Extracts all tracks using `reassemble_track()`
  - Writes FLAC files for comparison with XLD output
  - Can run with `--release` for faster performance
  - Suppresses noisy cache/cloud_storage logs

## Architecture

### Import Flow
1. **CUE Sheet Parsing**: Extract track times (start_time_ms, end_time_ms)
2. **Seektable Building**: 
   - Use libFLAC stream decoder to scan all frames
   - Extract sample numbers and byte positions from frame callbacks
   - Build HashMap mapping samples → bytes
3. **Byte Position Lookup**: 
   - Use Symphonia to seek to track time positions (gets sample numbers)
   - Lookup sample numbers in seektable to get precise byte positions
   - Handle variable bitrate by using actual frame positions, not interpolation
4. **Byte Range Storage**: Store absolute byte positions in `track_byte_ranges` map
5. **Chunk Mapping**: Convert byte positions to chunk indices and offsets
6. **Database Storage**: Store chunk coordinates and original FLAC headers

### Playback Flow
1. **Download Chunks**: Get chunks in range from cloud storage
2. **Extract Byte Range**: Extract track's byte range from reassembled chunks
3. **Prepend Headers**: Add original album FLAC headers
4. **Decode/Re-encode**: Use Symphonia to decode and flacenc to re-encode
5. **Stream**: Send to audio player

## Status

### ✅ Implementation Complete
- libFLAC seektable generation working correctly
- Symphonia time-based seeking integrated
- Byte position lookup from seektable working
- Test passes successfully
- All tracks extracted and written to FLAC files

### Key Improvements Over Previous Approach
1. **No Linear Interpolation**: Previous approach used linear interpolation which was inaccurate for variable bitrate FLAC
2. **Frame-Based Accuracy**: New approach uses actual FLAC frame positions from libFLAC decoder
3. **Proper Callback Setup**: Fixed libFLAC initialization by providing required `length_callback` when `seek_callback` is used

## Files Modified

- `bae/src/import/album_chunk_layout.rs`: 
  - Added `build_flac_seektable()` using libFLAC
  - Added `find_track_byte_range()` using Symphonia + seektable lookup
  - Added `lookup_seektable()` helper function
  - Removed old frame boundary scanning code (find_byte_position_at_time, find_nearest_frame_boundary, etc.)
- `bae/src/import/metadata_persister.rs`: Store byte ranges in database
- `bae/src/import/types.rs`: Added `track_byte_ranges` to `CueFlacLayoutData`
- `bae/src/playback/mod.rs`: Made reassembly module public for tests
- `bae/tests/cue_flac_import_test.rs`: Extended test to extract and write FLAC files, suppressed noisy logs
- `bae/Cargo.toml`: Added `libflac-sys` and `libc` dependencies
- `bae/README.md`: Documented cmake requirement for libflac-sys

## Dependencies

- `libflac-sys`: FFI bindings to libFLAC for seektable generation
- `libc`: C types for FFI
- `cmake`: Required build tool for libflac-sys (compiles libFLAC from source)

## Test Commands

```bash
# Run test (release mode recommended for speed)
cargo test --test cue_flac_import_test --release -- --ignored --nocapture

# Extracted files location:
# /var/folders/.../bae_test_cue_flac/extracted_tracks/
# Compare with XLD output at: /Users/dima/Desktop/tmp/
```
