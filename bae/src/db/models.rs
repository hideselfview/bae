use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Type;
use uuid::Uuid;

// String constants for SQL DEFAULT clauses (keep in sync with as_str())
const IMPORT_STATUS_QUEUED: &str = "queued";
const IMPORT_STATUS_IMPORTING: &str = "importing";
const IMPORT_STATUS_COMPLETE: &str = "complete";
const IMPORT_STATUS_FAILED: &str = "failed";

/// Database models for bae storage system
///
/// This implements the storage strategy described in the README:
/// - Albums and tracks stored as metadata
/// - Files split into encrypted chunks
/// - Chunks uploaded to cloud storage
/// - Local cache management for recently used chunks
///
/// Import status for albums and tracks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[sqlx(type_name = "TEXT", rename_all = "lowercase")]
pub enum ImportStatus {
    Queued,    // Validated and in import queue, waiting to start
    Importing, // Actively being processed (chunks being read/encrypted/uploaded)
    Complete,  // Successfully imported
    Failed,    // Import failed
}

impl ImportStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ImportStatus::Queued => IMPORT_STATUS_QUEUED,
            ImportStatus::Importing => IMPORT_STATUS_IMPORTING,
            ImportStatus::Complete => IMPORT_STATUS_COMPLETE,
            ImportStatus::Failed => IMPORT_STATUS_FAILED,
        }
    }
}

/// Artist metadata
///
/// Represents an individual artist or band. Artists are linked to albums and tracks
/// via junction tables (album_artists, track_artists) to support:
/// - Multiple artists per album (collaborations)
/// - Different artists per track (compilations, features)
/// - Artist deduplication across imports
///
/// Supports multiple metadata sources:
/// - Discogs: discogs_artist_id for deduplication
/// - Bandcamp: bandcamp_artist_id for future integration
/// - Other sources can be added as needed
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DbArtist {
    pub id: String,
    pub name: String,
    /// Sort name for alphabetical ordering (e.g., "Beatles, The")
    pub sort_name: Option<String>,
    /// Artist ID from Discogs (for deduplication across imports)
    pub discogs_artist_id: Option<String>,
    /// Artist ID from Bandcamp (for future multi-source support)
    pub bandcamp_artist_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Links artists to albums (many-to-many)
///
/// Supports albums with multiple artists (e.g., collaborations).
/// Position field maintains the order of artists for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbAlbumArtist {
    pub id: String,
    pub album_id: String,
    pub artist_id: String,
    /// Order of this artist in multi-artist albums (0-indexed)
    pub position: i32,
}

/// Links artists to tracks (many-to-many)
///
/// Supports tracks with multiple artists (features, remixes, etc.).
/// Role field distinguishes between main artist, featured artist, remixer, etc.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbTrackArtist {
    pub id: String,
    pub track_id: String,
    pub artist_id: String,
    /// Order of this artist in multi-artist tracks (0-indexed)
    pub position: i32,
    /// Role: "main", "featuring", "remixer", etc.
    pub role: Option<String>,
}

/// Album metadata - represents a logical album (the "master")
///
/// A logical album can have multiple physical releases (e.g., "1973 Original", "2016 Remaster").
/// This table stores the high-level album information that's common across all releases.
/// Specific release details and import status are tracked in the `releases` table.
///
/// Artists are linked via the `album_artists` junction table to support multiple artists.
///
/// Supports multiple metadata sources:
/// - Discogs: discogs_master_id links to the Discogs master release
/// - Bandcamp: bandcamp_album_id would link to the Bandcamp album
/// - Other sources can be added as needed
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DbAlbum {
    pub id: String,
    pub title: String,
    pub year: Option<i32>,
    /// Master ID from Discogs (optional to support other metadata sources)
    pub discogs_master_id: Option<String>,
    /// Album ID from Bandcamp (optional, for future multi-source support)
    pub bandcamp_album_id: Option<String>,
    pub cover_art_url: Option<String>,
    /// True for "Various Artists" compilation albums
    pub is_compilation: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Release metadata - represents a specific version/pressing of an album
///
/// A release is a physical or digital version of a logical album.
/// Examples: "1973 Original Pressing", "2016 Remaster", "180g Vinyl", "Digital Release"
///
/// Files, tracks, and chunks belong to releases (not albums), because:
/// - Users import specific releases, not abstract albums
/// - Each release has its own audio files and metadata
/// - Multiple releases of the same album can coexist in the library
///
/// The release_name field distinguishes between versions (e.g., "2016 Remaster").
/// If the user doesn't specify a release, we create one with release_name=None.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DbRelease {
    pub id: String,
    /// Links to the logical album (DbAlbum)
    pub album_id: String,
    /// Human-readable release name (e.g., "2016 Remaster", "180g Vinyl")
    pub release_name: Option<String>,
    /// Release-specific year (may differ from album year)
    pub year: Option<i32>,
    /// Discogs release ID (optional)
    pub discogs_release_id: Option<String>,
    /// Bandcamp release ID (optional, for future multi-source support)
    pub bandcamp_release_id: Option<String>,
    pub import_status: ImportStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Track metadata within a release
///
/// Represents a single track on a specific release. Tracks are linked to releases
/// (not logical albums) because track listings can vary between releases.
///
/// Track artists are linked via the `track_artists` junction table to support:
/// - Multiple artists per track (features, collaborations)
/// - Different artists than the album artist (compilations)
///
/// The discogs_position field stores the track position from metadata
/// (e.g., "A1", "1", "1-1" for vinyl sides).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DbTrack {
    pub id: String,
    /// Links to the specific release (DbRelease), not the logical album
    pub release_id: String,
    pub title: String,
    pub track_number: Option<i32>,
    pub duration_ms: Option<i64>,
    /// Position from metadata source (e.g., "A1", "1", "1-1")
    pub discogs_position: Option<String>,
    pub import_status: ImportStatus,
    pub created_at: DateTime<Utc>,
}

/// Physical file belonging to a release
///
/// Files are linked to releases (not logical albums or tracks), because:
/// - Files are part of a specific release (e.g., "2016 Remaster" has different files than "1973 Original")
/// - Some files are metadata (cover.jpg, .cue sheets) not associated with any track
/// - Some files contain multiple tracks (CUE/FLAC: one FLAC file = entire album)
/// - The file→track relationship is tracked via `db_track_position` table
///
/// For regular albums: 1 file = 1 track (linked via db_track_position)
/// For CUE/FLAC albums: 1 file = N tracks (all link to same file via db_track_position)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbFile {
    pub id: String,
    /// Release this file belongs to
    pub release_id: String,
    pub original_filename: String,
    pub file_size: i64,
    pub format: String,                // "flac", "mp3", etc.
    pub flac_headers: Option<Vec<u8>>, // FLAC header blocks for instant streaming
    pub audio_start_byte: Option<i64>, // Where audio frames begin (after headers)
    pub has_cue_sheet: bool,           // Is this a CUE/FLAC file?
    pub created_at: DateTime<Utc>,
}

/// Encrypted chunk of a release's data
///
/// Releases are split into encrypted chunks for cloud storage.
/// Each chunk is stored separately and can be cached/streamed independently.
/// The storage_location points to the encrypted chunk in cloud storage.
///
/// Chunks belong to releases (not logical albums) because each release has
/// its own set of files and therefore its own chunks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbChunk {
    pub id: String,
    /// Release this chunk belongs to
    pub release_id: String,
    pub chunk_index: i32,
    pub encrypted_size: i64,
    /// Cloud storage URI (e.g., s3://bucket/chunks/{shard}/{chunk_id}.enc)
    pub storage_location: String,
    /// Last time this chunk was accessed (for cache management)
    pub last_accessed: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Maps files to their chunk ranges
///
/// Since files are split into chunks for storage, this table tracks
/// which chunks contain which files and the byte offsets within those chunks.
/// This allows efficient streaming of individual files from the chunked storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbFileChunk {
    pub id: String,
    pub file_id: String,
    pub start_chunk_index: i32,
    pub end_chunk_index: i32,
    pub start_byte_offset: i64,
    pub end_byte_offset: i64,
    pub created_at: DateTime<Utc>,
}

/// CUE sheet metadata for CUE/FLAC albums
///
/// Stores the raw CUE file content for albums that use CUE/FLAC format.
/// The CUE sheet contains track timing information that allows splitting
/// a single FLAC file into multiple tracks during playback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbCueSheet {
    pub id: String,
    pub file_id: String,
    /// Raw CUE file content as text
    pub cue_content: String,
    pub created_at: DateTime<Utc>,
}

/// Links tracks to their source files with position information
///
/// This table connects the logical track entity to its physical file.
/// Created for ALL tracks during import (not just CUE/FLAC).
///
/// For regular tracks (1 file = 1 track):
/// - start_time_ms = 0, end_time_ms = 0 (indicates "use full file")
/// - start/end_chunk_index = the chunk range containing this file
///
/// For CUE/FLAC tracks (1 file = N tracks):
/// - start_time_ms/end_time_ms = actual timestamps from CUE sheet
/// - start/end_chunk_index = subset of chunks for this track's time range
/// - Multiple tracks point to the same file_id
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbTrackPosition {
    pub id: String,
    pub track_id: String,
    pub file_id: String,
    pub start_time_ms: i64, // Track start in milliseconds (0 = beginning of file)
    pub end_time_ms: i64,   // Track end in milliseconds (0 = end of file)
    pub start_chunk_index: i32, // First chunk containing this track
    pub end_chunk_index: i32, // Last chunk containing this track
    pub created_at: DateTime<Utc>,
}

// Helper functions for creating database records from Discogs data
impl DbArtist {
    /// Create an artist from Discogs artist data
    pub fn from_discogs_artist(discogs_artist_id: &str, name: &str) -> Self {
        let now = Utc::now();
        DbArtist {
            id: Uuid::new_v4().to_string(),
            name: name.to_string(),
            sort_name: None, // Could be computed from name (e.g., "Beatles, The")
            discogs_artist_id: Some(discogs_artist_id.to_string()),
            bandcamp_artist_id: None,
            created_at: now,
            updated_at: now,
        }
    }
}

impl DbAlbumArtist {
    pub fn new(album_id: &str, artist_id: &str, position: i32) -> Self {
        DbAlbumArtist {
            id: Uuid::new_v4().to_string(),
            album_id: album_id.to_string(),
            artist_id: artist_id.to_string(),
            position,
        }
    }
}

impl DbTrackArtist {
    pub fn new(track_id: &str, artist_id: &str, position: i32, role: Option<String>) -> Self {
        DbTrackArtist {
            id: Uuid::new_v4().to_string(),
            track_id: track_id.to_string(),
            artist_id: artist_id.to_string(),
            position,
            role,
        }
    }
}

impl DbAlbum {
    #[cfg(test)]
    pub fn new_test(title: &str) -> Self {
        let now = chrono::Utc::now();
        DbAlbum {
            id: uuid::Uuid::new_v4().to_string(),
            title: title.to_string(),
            year: None,
            discogs_master_id: None,
            bandcamp_album_id: None,
            cover_art_url: None,
            is_compilation: false,
            created_at: now,
            updated_at: now,
        }
    }

    /// Create a logical album from a Discogs master
    /// Note: Artists should be created separately and linked via DbAlbumArtist
    pub fn from_discogs_master(master: &crate::discogs::DiscogsMaster) -> Self {
        let now = Utc::now();
        DbAlbum {
            id: Uuid::new_v4().to_string(),
            title: master.title.clone(),
            year: master.year.map(|y| y as i32),
            discogs_master_id: Some(master.id.clone()),
            bandcamp_album_id: None,
            cover_art_url: master.thumb.clone(),
            is_compilation: false, // Will be set based on artist analysis
            created_at: now,
            updated_at: now,
        }
    }

    /// Create a logical album from a Discogs release
    /// Note: Artists should be created separately and linked via DbAlbumArtist
    pub fn from_discogs_release(release: &crate::discogs::DiscogsRelease) -> Self {
        let now = Utc::now();
        DbAlbum {
            id: Uuid::new_v4().to_string(),
            title: release.title.clone(),
            year: release.year.map(|y| y as i32),
            discogs_master_id: release.master_id.clone(),
            bandcamp_album_id: None,
            cover_art_url: release.thumb.clone(),
            is_compilation: false, // Will be set based on artist analysis
            created_at: now,
            updated_at: now,
        }
    }
}

impl DbRelease {
    #[cfg(test)]
    pub fn new_test(album_id: &str, release_id: &str) -> Self {
        let now = chrono::Utc::now();
        DbRelease {
            id: release_id.to_string(),
            album_id: album_id.to_string(),
            release_name: None,
            year: None,
            discogs_release_id: None,
            bandcamp_release_id: None,
            import_status: ImportStatus::Queued,
            created_at: now,
            updated_at: now,
        }
    }

    /// Create a release from a Discogs release
    pub fn from_discogs_release(album_id: &str, release: &crate::discogs::DiscogsRelease) -> Self {
        let now = Utc::now();
        DbRelease {
            id: Uuid::new_v4().to_string(),
            album_id: album_id.to_string(),
            release_name: None, // Could parse from release title if needed
            year: release.year.map(|y| y as i32),
            discogs_release_id: Some(release.id.clone()),
            bandcamp_release_id: None,
            import_status: ImportStatus::Queued,
            created_at: now,
            updated_at: now,
        }
    }

    /// Create a default release when user only selects a master (no specific release)
    pub fn default_for_master(album_id: &str, year: Option<i32>) -> Self {
        let now = Utc::now();
        DbRelease {
            id: Uuid::new_v4().to_string(),
            album_id: album_id.to_string(),
            release_name: None,
            year,
            discogs_release_id: None,
            bandcamp_release_id: None,
            import_status: ImportStatus::Queued,
            created_at: now,
            updated_at: now,
        }
    }
}

impl DbTrack {
    #[cfg(test)]
    pub fn new_test(
        release_id: &str,
        track_id: &str,
        title: &str,
        track_number: Option<i32>,
    ) -> Self {
        DbTrack {
            id: track_id.to_string(),
            release_id: release_id.to_string(),
            title: title.to_string(),
            track_number,
            duration_ms: None,
            discogs_position: None,
            import_status: ImportStatus::Queued,
            created_at: chrono::Utc::now(),
        }
    }

    pub fn from_discogs_track(
        discogs_track: &crate::discogs::DiscogsTrack,
        release_id: &str,
        track_index: usize,
    ) -> Result<Self, String> {
        Ok(DbTrack {
            id: Uuid::new_v4().to_string(),
            release_id: release_id.to_string(),
            title: discogs_track.title.clone(),
            track_number: Some((track_index + 1) as i32),
            duration_ms: None, // Will be filled in during track mapping
            discogs_position: Some(discogs_track.position.clone()),
            import_status: ImportStatus::Queued,
            created_at: Utc::now(),
        })
    }
}

impl DbFile {
    /// Create a regular file record (one file = one track)
    ///
    /// The file is linked to the release, not a specific track.
    /// Track→file relationship is established via db_track_position.
    pub fn new(release_id: &str, original_filename: &str, file_size: i64, format: &str) -> Self {
        DbFile {
            id: Uuid::new_v4().to_string(),
            release_id: release_id.to_string(),
            original_filename: original_filename.to_string(),
            file_size,
            format: format.to_string(),
            flac_headers: None,
            audio_start_byte: None,
            has_cue_sheet: false,
            created_at: Utc::now(),
        }
    }

    /// Create a CUE/FLAC file record (one file = multiple tracks)
    ///
    /// The file is linked to the release. Multiple tracks will reference this
    /// same file via db_track_position, each with different time ranges.
    pub fn new_cue_flac(
        release_id: &str,
        original_filename: &str,
        file_size: i64,
        flac_headers: Vec<u8>,
        audio_start_byte: i64,
    ) -> Self {
        DbFile {
            id: Uuid::new_v4().to_string(),
            release_id: release_id.to_string(),
            original_filename: original_filename.to_string(),
            file_size,
            format: "flac".to_string(),
            flac_headers: Some(flac_headers),
            audio_start_byte: Some(audio_start_byte),
            has_cue_sheet: true,
            created_at: Utc::now(),
        }
    }
}

impl DbChunk {
    pub fn from_release_chunk(
        release_id: &str,
        chunk_id: &str,
        chunk_index: i32,
        encrypted_size: usize,
        storage_location: &str,
    ) -> Self {
        DbChunk {
            id: chunk_id.to_string(),
            release_id: release_id.to_string(),
            chunk_index,
            encrypted_size: encrypted_size as i64,
            storage_location: storage_location.to_string(),
            last_accessed: None,
            created_at: Utc::now(),
        }
    }
}

impl DbFileChunk {
    pub fn new(
        file_id: &str,
        start_chunk_index: i32,
        end_chunk_index: i32,
        start_byte_offset: i64,
        end_byte_offset: i64,
    ) -> Self {
        DbFileChunk {
            id: Uuid::new_v4().to_string(),
            file_id: file_id.to_string(),
            start_chunk_index,
            end_chunk_index,
            start_byte_offset,
            end_byte_offset,
            created_at: Utc::now(),
        }
    }
}

impl DbCueSheet {
    pub fn new(file_id: &str, cue_content: &str) -> Self {
        DbCueSheet {
            id: Uuid::new_v4().to_string(),
            file_id: file_id.to_string(),
            cue_content: cue_content.to_string(),
            created_at: Utc::now(),
        }
    }
}

impl DbTrackPosition {
    pub fn new(
        track_id: &str,
        file_id: &str,
        start_time_ms: i64,
        end_time_ms: i64,
        start_chunk_index: i32,
        end_chunk_index: i32,
    ) -> Self {
        DbTrackPosition {
            id: Uuid::new_v4().to_string(),
            track_id: track_id.to_string(),
            file_id: file_id.to_string(),
            start_time_ms,
            end_time_ms,
            start_chunk_index,
            end_chunk_index,
            created_at: Utc::now(),
        }
    }
}
