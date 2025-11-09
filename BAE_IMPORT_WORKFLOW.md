# bae Album Import Workflow

This document specifies how bae imports albums using multiple metadata sources (Discogs and MusicBrainz).

## Import Philosophy

bae treats releases as ontological concepts that exist above any specific metadata source. A release represents a specific version/pressing of an album, regardless of whether it's cataloged in Discogs, MusicBrainz, or both. When importing, bae:

1. **Prioritizes exact lookups** - Uses MusicBrainz DiscID (calculated from CUE files) for exact matches when available
2. **Falls back to manual search** - When exact lookup isn't possible, allows user to choose between MusicBrainz or Discogs search
3. **Collects cross-source data** - When MusicBrainz releases link to Discogs (via URL relationships), bae can populate both `discogs_release` and `musicbrainz_release` fields in `DbAlbum`
4. **Prevents duplicates** - Checks for existing albums by exact ID matches (Discogs master_id/release_id or MB release_id/release_group_id) before importing

## User Experience

bae provides a folder-first import flow:

**Import from Folder:**

- User clicks "Import from Folder" →
- Selects folder containing music files →
- **Phase 1: Folder Selection** - User selects folder
- **Phase 2: Metadata Detection** - bae detects metadata from CUE files, audio tags, and folder name
- **Phase 3: Exact Lookup** (if available):
  - If MusicBrainz DiscID found in CUE: performs exact lookup via `lookup_by_discid()`
  - Single exact match → auto-proceeds to confirmation
  - Multiple exact matches → shows all for user selection
  - No match or no DiscID → proceeds to Phase 4
- **Phase 4: Manual Search** (if no exact match):
  - User selects search source: MusicBrainz or Discogs (radio buttons)
  - Search input pre-filled with detected metadata
  - User triggers search, sees results list
  - User selects match → proceeds to confirmation
- **Phase 5: Confirmation** - User reviews selected release and confirms import
- bae checks for duplicates before importing
- If duplicate found, shows error with link to existing album
- Otherwise, starts import process

## Metadata Detection

bae scans the selected folder and extracts metadata from multiple sources:

1. **CUE files** (highest priority):
   - Calculates MusicBrainz DiscID from CUE track offsets and FLAC duration
   - Extracts `REM DISCID` line for FreeDB DiscID (legacy)
   - Parses `REM DATE` for year information
   - Reads `PERFORMER` and `TITLE` for artist and album
   - Counts tracks from CUE tracklist

2. **Audio file tags**:
   - FLAC files: Reads metadata using `symphonia` crate
   - MP3 files: Reads ID3 tags using `id3` crate
   - Extracts artist, album, and year fields

3. **Folder name** (fallback):
   - Parses "Artist - Album" format from folder name
   - Low confidence score (30%)

Metadata is aggregated with weighted confidence scoring. MusicBrainz DiscID presence enables exact lookup, which is highly reliable.

## Exact Lookup (MusicBrainz DiscID)

When a CUE file is present and a matching FLAC file is found, bae calculates the MusicBrainz DiscID:

1. Extracts track offsets from CUE `INDEX 01` entries
2. Calculates lead-out offset from FLAC duration
3. Uses `discid` crate to compute DiscID
4. Calls MusicBrainz API: `GET /ws/2/discid/{discid}?inc=recordings+artist-credits+release-groups+url-rels+labels`

**Behavior:**
- **Single match**: Auto-proceeds to confirmation (no user interaction needed)
- **Multiple matches**: Shows all matches for user selection (different pressings of same release)
- **No match**: Falls back to manual search

## Manual Search

When exact lookup isn't available or fails, user enters manual search mode:

1. **Source Selection**: User chooses MusicBrainz or Discogs via radio buttons
2. **Search Input**: Pre-filled with detected metadata (artist + album)
3. **Search Execution**: 
   - MusicBrainz: Calls `search_releases()` with artist/album/year
   - Discogs: Calls `search_by_metadata()` with artist/album/year
4. **Results Display**: Shows all results (no confidence filtering in manual mode)
5. **Selection**: User selects desired release

## Cross-Source Data Collection

When importing from MusicBrainz, bae extracts Discogs URLs from MB relationships:

- MusicBrainz API includes `url-rels` in responses
- bae looks for URLs containing `discogs.com/master/` or `discogs.com/release/`
- If found, bae can optionally fetch Discogs data to populate both `discogs_release` and `musicbrainz_release` fields in `DbAlbum`
- This ensures complete metadata regardless of which source the user selected

**Current implementation**: URLs are extracted and logged. Full Discogs fetching can be added later if needed.

## Duplicate Detection

Before importing, bae checks for existing albums:

- **Discogs releases**: Checks by `master_id` or `release_id`
- **MusicBrainz releases**: Checks by `release_id` or `release_group_id`
- **Only exact ID matches** count as duplicates (no fuzzy matching)

If duplicate found:
- Shows error message: "This release already exists in your library: {title}"
- Provides link to view existing album
- Prevents duplicate import

## Import Architecture

bae uses a cloud-first storage approach. For library configuration details, see [BAE_LIBRARY_CONFIGURATION.md](BAE_LIBRARY_CONFIGURATION.md).

### Import Service

`ImportService` runs as an async task on the shared tokio runtime. It processes import requests sequentially from a queue, preventing UI blocking and ensuring imports run sequentially.

**Key characteristics:**
- Single instance for entire app
- Validates and queues imports synchronously in UI thread
- Executes pipeline asynchronously on background task
- Processes one import at a time (sequential, not concurrent)

### Import Flow

**Phase 1: Validation & Queueing** (synchronous, in `ImportHandle::send_request`)
1. User selects source folder containing album files
2. Create album and track records from metadata (Discogs or MusicBrainz)
3. Discover all files in folder (single filesystem traversal)
4. Validate track-to-file mapping using `TrackFileMapper`
5. Insert album and tracks with `ImportStatus::Queued`
6. If validation succeeds, queue for pipeline execution
7. Returns immediately so next import can be validated

**Phase 2: Pipeline Execution** (asynchronous, in `ImportService::import_from_folder`)
1. Mark album/tracks as `ImportStatus::Importing`
2. Analyze album layout (file→chunk and chunk→track mappings)
3. Build streaming pipeline using `import/pipeline::build_pipeline`
4. Drive pipeline to completion with `.collect().await`
5. Persist file/chunk metadata to database
6. Mark album/tracks as `ImportStatus::Complete`

### Streaming Pipeline

The pipeline is built using `impl Stream` composition and returns a stream of results (one per chunk). The caller drives the stream by collecting it.

**Pipeline stages:**

**Stage 1 - Sequential Reader:**
- Reads files sequentially using `tokio::io::BufReader`
- Treats all files as concatenated byte stream
- Accumulates data into chunk buffers (configurable size, default 1MB)
- Sends raw chunks via bounded channel (capacity: 10)
- Blocks when channel is full (backpressure from encryption)

**Stage 2 - Parallel Encryption:**
- Consumes raw chunks from channel
- Encrypts via `tokio::task::spawn_blocking` (CPU-bound work on blocking thread pool)
- Uses `.buffer_unordered(max_encrypt_workers)` for bounded parallelism
- Default workers: `2 × CPU cores` (configurable via `ImportConfig`)

**Stage 3 - Parallel Upload:**
- Consumes encrypted chunks
- Uploads to S3 and persists chunk metadata to database
- Uses `.buffer_unordered(max_upload_workers)` for bounded parallelism
- Default workers: `20` (configurable via `ImportConfig`)

**Stage 4 - Progress Tracking:**
- Persists chunk to database
- Checks if chunk completion triggers track completion
- Emits progress events via `ImportProgressService`
- Marks tracks complete as soon as all their chunks upload

All stages run concurrently. Bounded channels and `.buffer_unordered()` provide backpressure to prevent unbounded memory growth. No temporary files created, all processing happens in memory.

### Progress Updates

- UI subscribes to `ImportProgressService` for real-time updates
- Progress metric: chunks completed / total chunks (0-100%)
- Events emitted after each chunk uploads
- Tracks marked `ImportStatus::Complete` as soon as all their chunks finish
- Album marked `ImportStatus::Complete` when all tracks done

### Format Detection

bae handles two album formats:

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

## Implementation Components

**MusicBrainz API Client:**
- `lookup_by_discid()` → `(Vec<MbRelease>, ExternalUrls)` - Exact lookup by DiscID
- `search_releases()` → `Vec<MbRelease>` - Text search by artist/album/year
- `lookup_release_by_id()` → `(MbRelease, ExternalUrls)` - Fetch full release with relationships

**Discogs API Client:**
- `search_masters()` → `Vec<DiscogsSearchResult>`
- `search_by_discid()` → `Vec<DiscogsSearchResult>` (searches by DISCID)
- `search_by_metadata()` → `Vec<DiscogsSearchResult>` (searches by artist/album/year)
- `get_master_versions()` → `Vec<DiscogsMasterReleaseVersion>`
- `get_master()` → `DiscogsMaster` (includes `main_release` field)
- `get_release()` → `DiscogsRelease`

**Import Module** (`src/import/`):
- `service.rs` - `ImportService` orchestrator and `ImportHandle` public API
- `discogs_parser.rs` - `parse_discogs_release()` converts `DiscogsRelease` into database models
- `musicbrainz_parser.rs` - `fetch_and_parse_mb_release()` fetches and converts MB release into database models
- `folder_metadata_detector.rs` - Detects metadata from folder (audio tags, CUE files, folder name), calculates MB DiscID
- `discogs_matcher.rs` - Ranks search results by confidence score (used in manual search)
- `pipeline/` - Stream-based pipeline with `build_pipeline()` returning `impl Stream`
- `album_layout.rs` - Analyzes file→chunk and chunk→track mappings
- `track_file_mapper.rs` - Validates track-to-file mapping before DB insertion
- `metadata_persister.rs` - Persists file/chunk metadata to database
- `progress_service.rs` - Broadcasts progress updates to UI subscribers
- `types.rs` - Shared types (`ImportRequest`, `ImportProgress`, etc.)

**Storage Components:**
- `CloudStorageManager` - S3 upload/download with hash-based partitioning
- `CacheManager` - Local chunk cache with LRU eviction
- `LibraryManager` - Entity lifecycle and state transitions, duplicate detection
- `EncryptionService` - AES-256-GCM encryption/decryption
- `CueFlacProcessor` - CUE sheet parsing and FLAC header extraction

**UI Components:**
- `FolderDetectionPage` - Main import UI implementing 4-phase flow
- `FolderSelector` - Folder selection component
- `ManualSearchPanel` - Manual search with source selection (MB/Discogs)
- `SearchSourceSelector` - Radio buttons for choosing search source
- `MatchList` - Displays search results
- `MatchItem` - Individual result item
- `AlbumCard` - Subscribes to progress for individual album
