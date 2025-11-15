# bae Album Import Workflow

This document specifies how bae imports albums using multiple metadata sources (Discogs and MusicBrainz).

## Import Philosophy

bae treats releases as ontological concepts that exist above any specific metadata source. A release represents a specific version/pressing of an album, regardless of whether it's cataloged in Discogs, MusicBrainz, or both. When importing, bae:

1. **Prioritizes exact lookups** - Uses MusicBrainz DiscID (calculated from CUE files) for exact matches when available
2. **Falls back to manual search** - When exact lookup isn't possible, allows user to choose between MusicBrainz or Discogs search
3. **Collects cross-source data** - When MusicBrainz releases link to Discogs (via URL relationships), bae can populate both `discogs_release` and `musicbrainz_release` fields in `DbAlbum`
4. **Prevents duplicates** - Checks for existing albums by exact ID matches (Discogs master_id/release_id or MB release_id/release_group_id) before importing

## User Workflow

### 1. Source Selection
User selects import source: folder, torrent, or CD drive.

### 2. Metadata Detection
For folder imports, bae scans and extracts metadata from multiple sources:

**CUE files** (highest priority):
- Calculates MusicBrainz DiscID from CUE track offsets and FLAC duration
- Extracts `REM DISCID` line for FreeDB DiscID (legacy)
- Parses `REM DATE`, `PERFORMER`, and `TITLE`

**Audio file tags**:
- FLAC files: Reads metadata using `symphonia` crate
- MP3 files: Reads ID3 tags
- Extracts artist, album, and year fields

**Folder name** (fallback):
- Parses "Artist - Album" format
- Low confidence score (30%)

Metadata is aggregated with weighted confidence scoring.

### 2.1 Torrent Import Flow

For torrent imports, bae uses a single TorrentClient session created in ImportContext:

**UI Phase (ImportContext):**
- User selects torrent file
- Add torrent to shared `torrent_client_default` (single session)
- Extract torrent metadata: info_hash, name, size, piece_length, num_pieces, file_list
- Download CUE/log files via SelectiveDownloader (prioritizes metadata files)
- Detect album metadata from downloaded CUE/log files
- Store `TorrentImportMetadata` in ImportContext
- User confirms release match

**Import Phase (ImportHandle):**
- Receives `TorrentImportMetadata` from UI (no torrent operations)
- Uses metadata directly for track-to-file mapping
- Parses release metadata (Discogs/MusicBrainz)
- Inserts album/tracks to database
- Sends import command to ImportService

**Download Phase (ImportService):**
- Uses existing TorrentClient (created on dedicated thread)
- Registers BaeStorage for torrent
- Downloads all files during Acquire phase
- Streams → encrypts → uploads during Chunk phase

### 3. Release Matching

**Exact Lookup** (if MusicBrainz DiscID available):
1. Calls MusicBrainz API: `GET /ws/2/discid/{discid}?inc=recordings+artist-credits+release-groups+url-rels+labels`
2. Single match → auto-proceeds to confirmation
3. Multiple matches → user selects from list
4. No match → falls back to manual search

**Manual Search** (if needed):
1. User chooses source: MusicBrainz or Discogs
2. Search input pre-filled with detected metadata
3. User searches and selects desired release

**Cross-Source Data**: When importing from MusicBrainz, bae extracts Discogs URLs from relationships to enrich metadata.

### 4. Duplicate Check
Before importing, bae checks for existing albums by exact ID matches. If duplicate found, shows error with link to existing album.

### 5. Import Execution
After confirmation, import begins with real-time progress updates showing both acquire and chunk phases.

## Import Architecture

bae uses a cloud-first storage approach. For library configuration details, see [BAE_LIBRARY_CONFIGURATION.md](BAE_LIBRARY_CONFIGURATION.md).

### Two-Phase Model

All imports follow a consistent two-phase pattern:

**Phase 1: Acquire** - Get data ready for import
- **Folder**: No-op (files already available)
- **Torrent**: Download torrent to temporary folder
- **CD**: Rip CD tracks to FLAC files

**Phase 2: Chunk** - Upload and encrypt (same for all types)
- Stream files → encrypt → upload chunks → persist metadata

Progress events include `ImportPhase` enum (`Acquire` or `Chunk`) so the UI can display different progress bar colors.

### Service Architecture

**ImportService** runs on a dedicated thread with its own tokio runtime, processing import requests sequentially:

**Key characteristics:**
- Single instance for entire app
- Runs on dedicated thread (handles non-Send types like TorrentClient)
- Processes one import at a time (sequential, not concurrent)

**Responsibilities:**
- **ImportHandle**: Validates requests, inserts DB records, sends commands (synchronous in UI thread)
- **ImportService**: Executes acquire + chunk phases (asynchronous on dedicated thread)

### Complete Import Flow

**1. Validation & Queueing** (in `ImportHandle::send_request`)
- Parse release metadata from Discogs or MusicBrainz
- Discover files or validate expected structure
- Validate track-to-file mapping using `TrackFileMapper`
- Insert album and tracks with `ImportStatus::Queued`
- Send `ImportCommand` to service
- Returns immediately so UI can navigate and show progress

**2. Acquire Phase** (in `ImportService`)
- **Folder**: Instant (no work needed)
- **Torrent**: Download torrent, emit progress with `ImportPhase::Acquire`
- **CD**: Rip tracks to FLAC, emit progress with `ImportPhase::Acquire`

**3. Chunk Phase** (in `ImportService::run_chunk_phase`)
- Mark album/tracks as `ImportStatus::Importing`
- Analyze album layout (file→chunk and chunk→track mappings)
- Build streaming pipeline: read → encrypt → upload → persist
  - **Sequential Reader**: Reads files, accumulates chunk buffers (1MB default)
  - **Parallel Encryption**: CPU-bound work on blocking pool (2× CPU cores)
  - **Parallel Upload**: Uploads to S3 (20 workers)
  - **Progress Tracking**: Emits events via `ImportProgressTracker` with `ImportPhase::Chunk`
- Persist file/chunk metadata to database
- Mark album/tracks as `ImportStatus::Complete`

All pipeline stages run concurrently with bounded parallelism. Bounded channels and `.buffer_unordered()` provide backpressure to prevent unbounded memory growth.

### Progress System

**Acquire Phase:**
- Folder: No progress (instant)
- Torrent: Release-level download percentage
- CD: Release-level and track-level ripping percentage

**Chunk Phase:**
- All types: Release-level and track-level upload percentage
- Progress metric: chunks completed / total chunks (0-100%)
- Tracks marked `ImportStatus::Complete` as soon as all their chunks finish

**Subscription:**
- UI subscribes via `ImportProgressHandle` for real-time updates
- Can subscribe to release-level or track-level progress
- Events include `phase: Option<ImportPhase>` for UI to display different colors

### Format Support

**Individual files** (`1 file = 1 track`):
- MP3, FLAC, WAV, M4A, AAC, OGG
- Simple track-to-file mapping by sort order

**CUE/FLAC** (`1 file = entire album`):
- Detected via `CueFlacProcessor::detect_cue_flac_from_paths`
- Parses CUE sheet for track boundaries
- Extracts FLAC headers and stores in database
- All tracks map to same FLAC file
- See `BAE_CUE_FLAC_SPEC.md` for details

### Storage Locations

- **Primary storage**: S3 cloud storage (encrypted chunks only)
- **Local cache**: `~/.bae/cache/` (encrypted chunks, LRU eviction)
- **Local database**: `~/.bae/library.db` (SQLite)
- **Source folder**: Remains untouched on disk after import
- **Temporary directories** (cleaned up after import):
  - Torrent: `$TMP/{torrent_name}/` (downloaded files)
  - CD: `$TMP/bae_cd_rip_{uuid}/` (ripped FLAC, CUE, log)