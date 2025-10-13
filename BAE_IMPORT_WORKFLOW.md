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

## Storage Model

bae uses a cloud-first storage approach. For library configuration and initialization details, see [BAE_LIBRARY_CONFIGURATION.md](BAE_LIBRARY_CONFIGURATION.md).

### Import Process

bae uses a dedicated `ImportService` running on a separate thread to prevent UI blocking during imports. The service communicates via channels for requests and progress updates.

**Import Flow:**
1. **User selects source folder** containing album files
2. **Database transaction** inserts album and tracks with `import_status='importing'` before processing files (prevents foreign key constraint errors)
3. **File scanning** identifies audio files and matches them to Discogs tracklist
4. **Format detection** handles both individual tracks and CUE/FLAC albums
5. **FLAC header extraction** stores headers in database (CUE/FLAC only)
6. **Three-stage streaming pipeline** (runs on ImportService thread):
   
   **Stage 1 - Sequential Reader:**
   - Reads files sequentially using `BufReader`
   - Accumulates data into 5MB chunk buffers
   - Sends raw chunks to encryption stage via async channel
   
   **Stage 2 - Bounded Parallel Encryption:**
   - Pool of N workers (CPU cores × 2) consuming raw chunks from channel
   - Each worker encrypts via `tokio::task::spawn_blocking` (CPU-bound work on blocking thread pool)
   - Sends encrypted chunks to upload stage via async channel
   - Bounded by `FuturesUnordered` for controlled parallelism
   
   **Stage 3 - Bounded Parallel Upload:**
   - Pool of M workers (20) consuming encrypted chunks from channel
   - Each worker uploads to S3 and writes metadata to database
   - Bounded by `FuturesUnordered` for controlled parallelism
   
   All three stages run concurrently. Chunks flow through immediately as each stage completes. No temporary files created, all processing happens in memory.

7. **Real-time progress** updates after each chunk uploads, showing actual chunks_done/total_chunks
8. **Status update** marks album and tracks as `import_status='complete'` (or `'failed'` on error)
9. **Database sync** uploads SQLite database to S3 after successful import
10. **Source folder** remains untouched on disk

**Progress Updates:**
- UI subscribes to `ImportService` progress channel for real-time updates
- Single progress metric: chunks completed / total chunks (0-100%)
- Progress updates as each chunk finishes uploading
- Shows per-track completion status

### Storage Locations
- **Primary storage**: S3 cloud storage (encrypted chunks + SQLite database backup)
- **Local cache**: `~/.bae/cache/` (encrypted chunks, LRU eviction)
- **Local database**: `~/.bae/libraries/{library_id}/library.db` (SQLite, synced to S3)
- **Library config**: `~/.bae/config.yaml` (S3 settings, library list)


## Implementation Requirements

**Discogs API Client Methods:**
- `search_masters()` → `Vec<DiscogsSearchResult>`
- `get_master_versions()` → `Vec<DiscogsMasterReleaseVersion>`
- `get_master()` → `DiscogsMaster`
- `get_release()` → `DiscogsRelease`

**Storage Components:**
- `ImportService` orchestrates import workflow on shared tokio runtime, handles file mapping and coordinates pipeline stages without blocking UI
- `ChunkingService` manages Stages 1 & 2 (read + encrypt), returns channel of encrypted chunks, spawns reader task and encryption coordinator with bounded worker pool
- `UploadPipeline` manages Stage 3 (upload), consumes encrypted chunks from channel using bounded worker pool (`FuturesUnordered`)
- `CloudStorageManager` handles S3 upload/download with hash-based partitioning and database sync
- `CacheManager` manages local chunk cache with LRU eviction
- `LibraryManager` manages entity lifecycle and state transitions (mark_album_complete, mark_track_failed, etc), provides storage operations with progress callbacks
- `CueFlacProcessor` handles CUE sheet parsing and FLAC header extraction (see `BAE_CUE_FLAC_SPEC.md`)

**UI Components:**
- `SearchList` displays `Vec<DiscogsSearchResult>`
- `SearchItem` displays `DiscogsSearchResult`
- `ReleaseList` displays `Vec<DiscogsMasterReleaseVersion>`
- `ReleaseItem` displays `DiscogsMasterReleaseVersion`