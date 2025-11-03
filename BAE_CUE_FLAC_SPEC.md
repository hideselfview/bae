# bae CUE/FLAC Support Specification

This document specifies how bae handles CUE sheet + FLAC albums. For overall import process see `BAE_IMPORT_WORKFLOW.md` and for streaming details see `BAE_STREAMING_ARCHITECTURE.md`.

## Problem Statement

**Design Requirement:** In addition to individual audio files (`1 file = 1 track`), bae needs to handle CUE/FLAC albums (`1 file = entire album`).

**CUE/FLAC Format:**
- Single FLAC file contains entire album
- CUE sheet defines track boundaries with time positions
- Common in audiophile releases for gapless playback

## Architecture Overview

bae uses album-level chunking where all files (audio + artwork + notes) are concatenated and split into uniform encrypted chunks.

### Individual Tracks vs. CUE/FLAC Model

**Individual Tracks (bae chunking):**
```
Import: cover.jpg + notes.txt + track01.flac + track02.flac + track03.flac
Re-chunk → bae chunks 001-145 (uniform size, encrypted, bae UUIDs)

Track 1: track01.flac 00:00-03:45 → chunks 003-048
Track 2: track02.flac 00:00-03:37 → chunks 048-095  
Track 3: track03.flac 00:00-03:23 → chunks 095-145
```

**CUE/FLAC (bae chunking):**
```
Import: cover.jpg + notes.txt + album.flac + album.cue
Re-chunk → bae chunks 001-145 (uniform size, encrypted, bae UUIDs)

Track 1: album.flac 00:00-03:45 → chunks 003-048
Track 2: album.flac 03:45-07:22 → chunks 048-095
Track 3: album.flac 07:22-11:05 → chunks 095-145
```

## Data Storage

### CUE/FLAC File Records

**During Import:**
- The FLAC file is chunked and uploaded as-is (no modification to audio data)
- After chunks upload, we generate corrected FLAC headers for each track
- Headers are stored in the database as metadata (not in the chunks themselves)

**Database-Stored FLAC Headers (per track):**
- Only STREAMINFO block (all other metadata blocks removed)
- `total_samples` corrected to reflect track duration (not album duration)
- MD5 and min/max frame sizes zeroed (unknown for extracted track)

**Chunks (uploaded audio data):**
- Original FLAC file bytes, including original headers and all frames
- No modification during import

**Why This Matters:**
- Enables playback without downloading initial chunks (headers come from database)
- Ensures decoder shows correct track duration

### Track Position Records  
- Track timing boundaries in milliseconds
- Chunk index ranges for efficient retrieval

## FLAC Processing for CUE Tracks

### Header Generation (Metadata Persistence)
After chunks are uploaded, we generate corrected FLAC headers and store them in the database:

- **Extract STREAMINFO** from album FLAC
- **Update `total_samples`** to track duration (samples in this track, not entire album)
- **Zero MD5 signature** - signals "no signature" (unknown for extracted track)
- **Zero min/max frame sizes** - signals "unknown" for extracted track
- **Remove all other metadata blocks**:
  - SEEKTABLE (type 3) - offsets are incorrect for extracted track
  - VORBIS_COMMENT (type 4) - album-level tags don't apply to track
  - PADDING (type 1) - unnecessary space filler
  - APPLICATION (type 2) - encoder-specific data not needed
- **Keep only STREAMINFO** (type 0) - required for playback

Note: The FLAC file is chunked and uploaded as-is. Headers are generated and stored as metadata
in the database, not written to the chunks. Track metadata lives in the database. SEEKTABLE
could be rebuilt with correct offsets if needed in the future. Tags can be added later if needed.

### Frame Rewriting (Playback Time)
During track reassembly, we rewrite FLAC frame headers to create valid standalone FLAC files:

1. **Calculate track's starting position**: `start_sample = (start_time_ms * sample_rate) / 1000`
2. **Scan reassembled audio** for FLAC frame boundaries (sync code 0xFFF8)
3. **For each frame header**:
   - Parse frame/sample number (UTF-8 coded variable-length integer)
   - Subtract track start to get relative number (starts from 0)
   - Re-encode number in UTF-8 (size may change)
   - Rebuild header with new number
   - Recalculate CRC-8 checksum
4. **Result**: Byte-correct FLAC file with frames starting from 0

### Why Frame Rewriting?
- Makes extracted tracks valid standalone FLAC files
- Enables future streaming without full reassembly
- Ensures correct seeking behavior in decoders
- Frame/sample numbers must start from 0 for proper playback
- Handles both fixed and variable block size strategies

## Requirements

### File Detection
- Detect .flac files with matching .cue files in import folders

### Data Extraction
- Extract FLAC metadata and headers
- Parse CUE sheet track boundaries and timing
- Convert CUE time format (MM:SS:FF) to milliseconds

### Import Process
1. Extract FLAC headers and store in database
2. Parse CUE sheet for track boundaries  
3. Album-level chunking (entire folder concatenated and split)
4. Calculate track positions within chunks
5. Store track position mappings

See `BAE_IMPORT_WORKFLOW.md` for complete import process details.

### Streaming Process
1. Retrieve track position from database
2. Get FLAC headers from database
3. Download only required chunks for track
4. Prepend headers to chunk data
5. Use audio library for precise track extraction

See `BAE_STREAMING_ARCHITECTURE.md` for complete streaming pipeline details.

## Chunk Download Behavior

### Track-Specific Chunk Retrieval
```
Album: 150MB FLAC → 150 chunks
Track 3: chunks 45-55 → Download 10 chunks (10MB)
```

### Header Storage
- FLAC headers stored in database
- No need to download initial chunks for headers


## Dependencies
```toml
# CUE sheet parsing
nom = "7.1"              # Parser combinator for CUE format

# Audio processing and FLAC handling
symphonia = "0.5"        # Audio framework with FLAC support

# Alternative options considered:
# rodio = "0.17"         # Alternative: Audio playback and processing
# ffmpeg-next = "6.0"    # Alternative: FFmpeg bindings for transcoding
# flac = "0.3"           # Alternative: Dedicated FLAC library (not needed with symphonia)
```

## Migration Strategy

### Backward Compatibility
- Existing single-file tracks continue to work unchanged
- New CUE/FLAC detection only applies to new imports
- Database schema supports both individual files and CUE/FLAC


## Testing Strategy

### Test Cases
1. **CUE Parsing**: Various CUE sheet formats and edge cases
2. **FLAC Headers**: Different FLAC encoding parameters
3. **Time Conversion**: Accuracy of time→byte estimation
4. **Chunk Ranges**: Correct chunk selection for track boundaries
5. **Streaming**: End-to-end CUE track playback
6. **Chunk Retrieval**: Verify correct chunk selection for track boundaries

### Test Data
- Sample CUE/FLAC albums with known track boundaries
- Various FLAC encoding settings (different sample rates, bit depths)
- Edge cases: single track, many short tracks, long tracks

This specification covers CUE/FLAC support implementation in bae.
