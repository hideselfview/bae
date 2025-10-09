# bae Album Import Workflow

This document specifies how bae imports albums using the [Discogs API](https://www.discogs.com/developers).

## How bae Uses Discogs Data

bae uses Discogs as the source of truth for album metadata. Discogs organizes music data hierarchically:

- **Masters** represent abstract albums (e.g., "Abbey Road")
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
1. **User selects source folder** containing album files
2. **File scanning** identifies audio files and matches them to Discogs tracklist
3. **Format detection** handles both individual tracks and CUE/FLAC albums
4. **FLAC header extraction** stores headers in database (CUE/FLAC only)
5. **Chunking and encryption** concatenates entire album folder (audio + artwork + notes) into single stream and splits into uniform 1MB AES-256-GCM encrypted chunks (see `BAE_STREAMING_ARCHITECTURE.md` for chunk format details)
6. **Cloud upload** stores all chunks in S3 storage with hash-based partitioning
7. **Database sync** immediately uploads SQLite database to S3 after successful import
8. **Source folder** remains untouched on disk

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
- `ChunkingService` splits files into encrypted chunks
- `CloudStorageManager` handles S3 upload/download with hash-based partitioning and database sync
- `CacheManager` manages local chunk cache with LRU eviction
- `LibraryManager` handles library initialization and manifest detection in S3
- `CueFlacProcessor` handles CUE sheet parsing and FLAC header extraction (see `BAE_CUE_FLAC_SPEC.md`)

**UI Components:**
- `SearchList` displays `Vec<DiscogsSearchResult>`
- `SearchItem` displays `DiscogsSearchResult`
- `ReleaseList` displays `Vec<DiscogsMasterReleaseVersion>`
- `ReleaseItem` displays `DiscogsMasterReleaseVersion`