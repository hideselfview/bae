# bae Album Import Workflow

This document specifies how bae imports albums using the [Discogs API](https://www.discogs.com/developers).

## How bae Uses Discogs Data

bae uses Discogs as the source of truth for album metadata. Discogs organizes music data hierarchically:

- **Masters** represent abstract albums
- **Releases** represent specific physical pressings of those albums (original UK pressing, US reissue, 180g vinyl remaster, etc.)

bae's import workflow adapts to what we know about our music data. If we know which specific release our files represent, we import that release. If we only know the album title, we import the master with its canonical tracklist.

## User Experience

bae provides two import paths based on what the user knows about their music data:

**Path 1: Import master**

- User searches for albums →
- Selects a master →
- Provides music data →
- bae fetches master details →
- bae imports the master with canonical tracklist

**Path 2: Import specific release** 

- User searches for albums →
- Views available releases →
- Selects specific release →
- Provides music data →
- bae fetches release details →
- bae imports that exact release

The choice depends on whether the user knows which specific pressing their music files represent.

## Album Search

When the user searches for albums in bae, we call the Discogs API:

```
GET /database/search?type=master&q={query}
```

This returns basic search results for browsing. bae displays these using the `DiscogsSearchResult` model with fields like `id`, `title`, `year`, `thumb`, and `master_id`.

## Master Import (Path 1)

When the user clicks "Add to Library" on a master from search results, bae calls the Discogs API:

```
GET /masters/{master_id}
```

This fetches complete master data including tracklist for import. bae converts this to `DiscogsMaster` with fields like `id`, `title`, `year`, `tracklist`, and `artists`.

## Release Browser (Path 2)

When the user clicks "View Releases" on a master in bae, we call the Discogs API:

```
GET /masters/{master_id}/versions?page={page}&per_page={per_page}
```

This shows the user available releases for that master. bae displays these using the `DiscogsMasterReleaseVersion` model with fields like `id`, `title`, `format`, `label`, `catno`, and `country`.

## Release Import (Path 2)

When the user clicks "Add to Library" on a release from the versions list, bae calls the Discogs API:

```
GET /releases/{release_id}
```

This fetches complete release data including tracklist for import. bae converts this to `DiscogsRelease` with fields like `id`, `title`, `year`, `tracklist`, `formats`, `labels`, and `master_id`.

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
2. Create album and track records from Discogs metadata
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

**Discogs API Client:**
- `search_masters()` → `Vec<DiscogsSearchResult>`
- `get_master_versions()` → `Vec<DiscogsMasterReleaseVersion>`
- `get_master()` → `DiscogsMaster`
- `get_release()` → `DiscogsRelease`

**Import Module** (`src/import/`):
- `service.rs` - `ImportService` orchestrator and `ImportHandle` public API
- `pipeline/` - Stream-based pipeline with `build_pipeline()` returning `impl Stream`
- `album_layout.rs` - Analyzes file→chunk and chunk→track mappings
- `track_file_mapper.rs` - Validates track-to-file mapping before DB insertion
- `metadata_persister.rs` - Persists file/chunk metadata to database
- `progress_service.rs` - Broadcasts progress updates to UI subscribers
- `types.rs` - Shared types (`ImportRequest`, `ImportProgress`, etc.)

**Storage Components:**
- `CloudStorageManager` - S3 upload/download with hash-based partitioning
- `CacheManager` - Local chunk cache with LRU eviction
- `LibraryManager` - Entity lifecycle and state transitions
- `EncryptionService` - AES-256-GCM encryption/decryption
- `CueFlacProcessor` - CUE sheet parsing and FLAC header extraction

**UI Components:**
- `SearchList` - Displays `Vec<DiscogsSearchResult>`
- `SearchItem` - Displays `DiscogsSearchResult`
- `ReleaseList` - Displays `Vec<DiscogsMasterReleaseVersion>`
- `ReleaseItem` - Displays `DiscogsMasterReleaseVersion`
- `ImportWorkflow` - Multi-step import wizard
- `AlbumCard` - Subscribes to progress for individual album
