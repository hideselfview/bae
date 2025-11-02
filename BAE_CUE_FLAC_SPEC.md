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
- FLAC header blocks stored per-track in database with corrected `total_samples` for each track
- Headers are modified during import to reflect track duration (not album duration)
- Enables streaming without downloading initial chunks and ensures correct decoder duration

### Track Position Records  
- Track timing boundaries in milliseconds
- Chunk index ranges for efficient retrieval

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
