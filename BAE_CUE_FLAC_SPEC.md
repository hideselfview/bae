# bae CUE/FLAC Support Specification

This document specifies how bae handles CUE sheet + FLAC albums for efficient streaming and storage.

## Problem Statement

**Current Limitation:** bae assumes `1 file = 1 track`, which breaks with CUE/FLAC albums where `1 file = entire album`.

**CUE/FLAC Format:**
- Single FLAC file contains entire album
- CUE sheet defines track boundaries with time positions
- Common in audiophile releases for gapless playback

## Architecture Overview

### Current vs. CUE/FLAC Model

**Current (Individual Tracks):**
```
track01.flac → chunks 1-5   → stream chunks 1-5
track02.flac → chunks 6-10  → stream chunks 6-10
track03.flac → chunks 11-15 → stream chunks 11-15
```

**CUE/FLAC (Single File):**
```
album.flac → chunks 1-150
album.cue  → track boundaries

Track 1: 00:00-03:45 → chunks 1-4   (only download these)
Track 2: 03:45-07:22 → chunks 4-8   (only download these)
Track 3: 07:22-11:05 → chunks 8-12  (only download these)
```

## Database Schema Changes

### Files Table Updates
```sql
ALTER TABLE files ADD COLUMN flac_headers BLOB;           -- Store FLAC header blocks
ALTER TABLE files ADD COLUMN audio_start_byte INTEGER;    -- Where audio frames begin
ALTER TABLE files ADD COLUMN has_cue_sheet BOOLEAN;       -- Is this a CUE/FLAC file?
```

### New CUE Sheets Table
```sql
CREATE TABLE cue_sheets (
    id TEXT PRIMARY KEY,
    file_id TEXT NOT NULL,
    cue_content TEXT NOT NULL,              -- Raw CUE file content
    created_at TEXT NOT NULL,
    FOREIGN KEY (file_id) REFERENCES files(id)
);
```

### New Track Positions Table
```sql
CREATE TABLE track_positions (
    id TEXT PRIMARY KEY,
    track_id TEXT NOT NULL,
    file_id TEXT NOT NULL,
    start_time_ms INTEGER NOT NULL,         -- Track start in milliseconds
    end_time_ms INTEGER NOT NULL,           -- Track end in milliseconds
    start_byte_estimate INTEGER,            -- Estimated byte position (optional)
    end_byte_estimate INTEGER,              -- Estimated byte position (optional)
    start_chunk_index INTEGER,              -- First chunk containing this track
    end_chunk_index INTEGER,                -- Last chunk containing this track
    created_at TEXT NOT NULL,
    FOREIGN KEY (track_id) REFERENCES tracks(id),
    FOREIGN KEY (file_id) REFERENCES files(id)
);
```

## Import Process Changes

### CUE/FLAC Detection
```rust
fn detect_cue_flac(folder_path: &Path) -> Vec<CueFlacPair> {
    // Look for .flac files with matching .cue files
    // album.flac + album.cue
    // disc1.flac + disc1.cue
}
```

### FLAC Header Extraction
```rust
struct FlacHeaders {
    headers: Vec<u8>,           // Raw header blocks
    audio_start_byte: u64,      // Where audio frames begin
    sample_rate: u32,
    total_samples: u64,
    channels: u16,
    bits_per_sample: u16,
}

fn extract_flac_headers(flac_path: &Path) -> Result<FlacHeaders, Error> {
    // Parse FLAC file to extract:
    // 1. All metadata blocks (STREAMINFO, VORBIS_COMMENT, etc.)
    // 2. Find where audio frames start
    // 3. Extract audio properties for time→byte conversion
}
```

### CUE Sheet Parsing
```rust
struct CueTrack {
    number: u32,
    title: String,
    performer: Option<String>,
    start_time_ms: u64,         // Converted from MM:SS:FF format
    end_time_ms: Option<u64>,   // Calculated from next track or file end
}

struct CueSheet {
    title: String,
    performer: String,
    tracks: Vec<CueTrack>,
}

fn parse_cue_sheet(cue_path: &Path) -> Result<CueSheet, Error> {
    // Parse CUE file format:
    // TITLE "Album Title"
    // PERFORMER "Artist Name"
    // TRACK 01 AUDIO
    //   TITLE "Track Title"
    //   INDEX 01 00:00:00
    // TRACK 02 AUDIO
    //   TITLE "Track 2"
    //   INDEX 01 03:45:12
}
```

### Time to Byte Conversion
```rust
fn estimate_byte_position(
    time_ms: u64,
    flac_headers: &FlacHeaders,
    file_size: u64,
) -> u64 {
    // Rough estimation based on:
    // - Sample rate and bit depth from FLAC headers
    // - Total file size and duration
    // - Linear interpolation (good enough for 1MB chunks)
    
    let total_duration_ms = (flac_headers.total_samples * 1000) / flac_headers.sample_rate as u64;
    let audio_size = file_size - flac_headers.audio_start_byte;
    let estimated_audio_byte = (time_ms * audio_size) / total_duration_ms;
    
    flac_headers.audio_start_byte + estimated_audio_byte
}
```

### Modified Import Flow
```rust
async fn import_cue_flac(
    cue_flac_pair: &CueFlacPair,
    discogs_item: &ImportItem,
) -> Result<String, Error> {
    // 1. Extract FLAC headers
    let flac_headers = extract_flac_headers(&cue_flac_pair.flac_path)?;
    
    // 2. Parse CUE sheet
    let cue_sheet = parse_cue_sheet(&cue_flac_pair.cue_path)?;
    
    // 3. Create album record
    let album = create_album_record(discogs_item)?;
    
    // 4. Create single file record with headers
    let file = DbFile {
        // ... standard fields
        flac_headers: Some(flac_headers.headers),
        audio_start_byte: Some(flac_headers.audio_start_byte),
        has_cue_sheet: true,
    };
    
    // 5. Chunk entire FLAC file (audio portion only)
    let audio_start = flac_headers.audio_start_byte;
    let chunks = chunk_file_from_offset(&cue_flac_pair.flac_path, audio_start)?;
    
    // 6. Create track records from CUE + Discogs
    let tracks = create_tracks_from_cue(&cue_sheet, &discogs_item.tracklist(), &album.id)?;
    
    // 7. Create track position records
    for (track, cue_track) in tracks.iter().zip(cue_sheet.tracks.iter()) {
        let start_byte = estimate_byte_position(cue_track.start_time_ms, &flac_headers, file_size);
        let end_byte = estimate_byte_position(cue_track.end_time_ms.unwrap_or(total_duration), &flac_headers, file_size);
        
        let position = TrackPosition {
            track_id: track.id.clone(),
            file_id: file.id.clone(),
            start_time_ms: cue_track.start_time_ms,
            end_time_ms: cue_track.end_time_ms.unwrap_or(total_duration),
            start_byte_estimate: Some(start_byte),
            end_byte_estimate: Some(end_byte),
            start_chunk_index: (start_byte - audio_start) / CHUNK_SIZE,
            end_chunk_index: (end_byte - audio_start) / CHUNK_SIZE,
        };
        
        database.insert_track_position(&position).await?;
    }
    
    // 8. Upload chunks to S3 (same as current process)
    upload_chunks_to_s3(&chunks).await?;
    
    Ok(album.id)
}
```

## Streaming Changes

### CUE Track Streaming
```rust
async fn stream_cue_track(track_id: &str) -> Result<Vec<u8>, Error> {
    // 1. Get track position info
    let position = database.get_track_position(track_id).await?;
    let file = database.get_file(&position.file_id).await?;
    
    // 2. Get FLAC headers from database (instant!)
    let flac_headers = file.flac_headers.ok_or("No FLAC headers stored")?;
    
    // 3. Calculate required chunks (much more efficient!)
    let chunk_range = position.start_chunk_index..=position.end_chunk_index;
    let chunks = database.get_chunks_in_range(&file.id, chunk_range).await?;
    
    // 4. Download only required chunks
    let mut audio_data = Vec::new();
    for chunk in chunks {
        let decrypted = download_and_decrypt_chunk(&chunk, &cache_manager).await?;
        audio_data.extend_from_slice(&decrypted);
    }
    
    // 5. Prepend FLAC headers
    let mut complete_audio = flac_headers;
    complete_audio.extend_from_slice(&audio_data);
    
    // 6. Use audio library to seek to exact track position
    let track_audio = seek_to_track_position(
        &complete_audio,
        position.start_time_ms,
        position.end_time_ms,
    )?;
    
    Ok(track_audio)
}
```

### Audio Seeking Implementation
```rust
fn seek_to_track_position(
    flac_data: &[u8],
    start_time_ms: u64,
    end_time_ms: u64,
) -> Result<Vec<u8>, Error> {
    // Use audio library (symphonia, rodio, etc.) to:
    // 1. Decode FLAC stream
    // 2. Seek to start_time_ms
    // 3. Read until end_time_ms
    // 4. Re-encode as FLAC or stream raw PCM
    
    // This is the complex part - need audio processing library
    // For MVP, could return entire chunk range and let client seek
}
```

## Performance Benefits

### Chunk Download Efficiency
**Before (download entire album):**
```
Album: 150MB FLAC → 150 chunks
Track 3 request → Download all 150 chunks (150MB)
```

**After (download only track chunks):**
```
Album: 150MB FLAC → 150 chunks  
Track 3: chunks 45-55 → Download 10 chunks (10MB)
85% reduction in download size!
```

### Header Storage Benefits
**Before:**
- Always need chunks 0-2 for FLAC headers
- Headers downloaded from S3 every time

**After:**
- Headers stored in database (instant access)
- No need to download initial chunks
- Streaming starts immediately

## Implementation Plan

### Phase 1: Database Schema
1. Add new columns to `files` table
2. Create `cue_sheets` and `track_positions` tables
3. Update database models and queries

### Phase 2: Import Changes
1. Implement CUE sheet parser
2. Implement FLAC header extractor
3. Add CUE/FLAC detection to import workflow
4. Update chunking to skip headers

### Phase 3: Streaming Changes
1. Update streaming to use track positions
2. Implement chunk range queries
3. Add header prepending logic
4. Integrate audio seeking library

### Phase 4: Audio Processing
1. Research audio libraries (symphonia, rodio, ffmpeg)
2. Implement precise track seeking
3. Add format conversion if needed
4. Optimize for streaming performance

## Dependencies

### New Crates Needed
```toml
# CUE sheet parsing
nom = "7.1"              # Parser combinator for CUE format

# FLAC header parsing  
flac = "0.3"             # FLAC metadata parsing
# OR
symphonia = "0.5"        # Full audio framework

# Audio processing for seeking
rodio = "0.17"           # Audio playback and processing
# OR  
ffmpeg-next = "6.0"      # FFmpeg bindings for transcoding
```

## Migration Strategy

### Backward Compatibility
- Existing single-file tracks continue to work unchanged
- New CUE/FLAC detection only applies to new imports
- Database migrations handle schema changes gracefully

### Gradual Rollout
1. **Phase 1**: Database changes (no functional impact)
2. **Phase 2**: Import support (new feature, doesn't break existing)
3. **Phase 3**: Streaming optimization (performance improvement)
4. **Phase 4**: Audio processing (quality improvement)

## Testing Strategy

### Test Cases
1. **CUE Parsing**: Various CUE sheet formats and edge cases
2. **FLAC Headers**: Different FLAC encoding parameters
3. **Time Conversion**: Accuracy of time→byte estimation
4. **Chunk Ranges**: Correct chunk selection for track boundaries
5. **Streaming**: End-to-end CUE track playback
6. **Performance**: Download size reduction verification

### Test Data
- Sample CUE/FLAC albums with known track boundaries
- Various FLAC encoding settings (different sample rates, bit depths)
- Edge cases: single track, many short tracks, long tracks

This specification provides a complete roadmap for implementing efficient CUE/FLAC support in bae while maintaining backward compatibility and optimizing for performance.
