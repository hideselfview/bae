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

/// Discogs master release information for an album
///
/// When an album is imported from Discogs, both the master_id and release_id
/// are always known together (the release_id is the main_release for that master).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiscogsMasterRelease {
    pub master_id: String,
    pub release_id: String,
}

/// MusicBrainz release information for an album
///
/// MusicBrainz has Release Groups (abstract albums) and Releases (specific versions).
/// Similar to Discogs master_id/release_id relationship.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MusicBrainzRelease {
    pub release_group_id: String, // Abstract album (like Discogs master)
    pub release_id: String,       // Specific version/pressing
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
/// - Discogs: discogs_release links to the Discogs master release and its main release
/// - Bandcamp: bandcamp_album_id would link to the Bandcamp album
/// - Other sources can be added as needed
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DbAlbum {
    pub id: String,
    pub title: String,
    pub year: Option<i32>,
    /// Discogs release information
    pub discogs_release: Option<DiscogsMasterRelease>,
    /// MusicBrainz release information
    pub musicbrainz_release: Option<MusicBrainzRelease>,
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
    /// Format (e.g., "CD", "Vinyl", "Digital")
    pub format: Option<String>,
    /// Record label
    pub label: Option<String>,
    /// Catalog number
    pub catalog_number: Option<String>,
    /// Country of release
    pub country: Option<String>,
    /// Barcode
    pub barcode: Option<String>,
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
    /// Disc number (1-indexed) for multi-disc releases
    pub disc_number: Option<i32>,
    pub track_number: Option<i32>,
    pub duration_ms: Option<i64>,
    /// Position from metadata source (e.g., "A1", "1", "1-1")
    pub discogs_position: Option<String>,
    pub import_status: ImportStatus,
    pub created_at: DateTime<Utc>,
}

/// Physical file belonging to a release
/// File metadata for export/torrent features
///
/// Stores original file information needed to reconstruct file structure for export
/// or BitTorrent seeding. Not needed for playback - that uses TrackChunkCoords + AudioFormat.
///
/// Files are linked to releases (not logical albums or tracks), because:
/// - Files are part of a specific release (e.g., "2016 Remaster" has different files than "1973 Original")
/// - Some files are metadata (cover.jpg, .cue sheets) not associated with any track
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbFile {
    pub id: String,
    /// Release this file belongs to
    pub release_id: String,
    pub original_filename: String,
    pub file_size: i64,
    pub format: String, // "flac", "mp3", etc.
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

/// Audio format metadata for a track
///
/// Stores format information needed for playback. One record per track (1:1 with track).
///
/// **FLAC headers are only needed for CUE/FLAC:**
/// - One-file-per-track: Headers are already in the track's first chunk, no prepending needed
/// - CUE/FLAC: Headers are at file start, but track audio starts mid-file. Must prepend headers during playback.
///
/// **Seektables are only needed for CUE/FLAC tracks:**
/// - One-file-per-track: Can calculate byte position from time directly
/// - CUE/FLAC: Seektables map sample positions to byte positions in the original album FLAC file for accurate seeking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbAudioFormat {
    pub id: String,
    pub track_id: String,                // 1:1 with track
    pub format: String,                  // "flac", "mp3", etc.
    pub flac_headers: Option<Vec<u8>>,   // ONLY for CUE/FLAC tracks
    pub flac_seektable: Option<Vec<u8>>, // ONLY for CUE/FLAC tracks, serialized HashMap<u64, u64>
    pub needs_headers: bool,             // True for CUE/FLAC tracks
    pub created_at: DateTime<Utc>,
}

/// Track chunk coordinates - precise location of track audio in chunked album stream
///
/// This IS the TrackChunkCoords concept. Stores the coordinates that locate a track's
/// audio data within the chunked album stream, regardless of whether the source was
/// one-file-per-track or CUE/FLAC. Both import types produce identical records here.
///
/// The key insight: post-import, we only need these coordinates + audio format to play any track.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbTrackChunkCoords {
    pub id: String,
    pub track_id: String,
    pub start_chunk_index: i32, // First chunk containing this track
    pub end_chunk_index: i32,   // Last chunk containing this track
    pub start_byte_offset: i64, // Where track starts in start_chunk
    pub end_byte_offset: i64,   // Where track ends in end_chunk
    pub start_time_ms: i64,     // Track start time (metadata/display)
    pub end_time_ms: i64,       // Track end time (metadata/display)
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
            discogs_release: None,
            musicbrainz_release: None,
            bandcamp_album_id: None,
            cover_art_url: None,
            is_compilation: false,
            created_at: now,
            updated_at: now,
        }
    }

    /// Create a logical album from a Discogs release
    /// Note: Artists should be created separately and linked via DbAlbumArtist
    ///
    /// master_id and master_year are always provided for releases imported from Discogs.
    /// The master year is used for the album year (not the release year).
    pub fn from_discogs_release(
        release: &crate::discogs::DiscogsRelease,
        master_year: u32,
    ) -> Self {
        let now = Utc::now();

        let discogs_release = DiscogsMasterRelease {
            master_id: release.master_id.clone(),
            release_id: release.id.clone(),
        };

        DbAlbum {
            id: Uuid::new_v4().to_string(),
            title: release.title.clone(),
            year: Some(master_year as i32),
            discogs_release: Some(discogs_release),
            musicbrainz_release: None,
            bandcamp_album_id: None,
            cover_art_url: release.thumb.clone(),
            is_compilation: false, // Will be set based on artist analysis
            created_at: now,
            updated_at: now,
        }
    }

    pub fn from_mb_release(release: &crate::musicbrainz::MbRelease, master_year: u32) -> Self {
        let now = Utc::now();

        let musicbrainz_release = crate::db::MusicBrainzRelease {
            release_group_id: release.release_group_id.clone(),
            release_id: release.release_id.clone(),
        };

        // Use first_release_date (original album year) for the album, not the specific release date
        let year = release
            .first_release_date
            .as_ref()
            .and_then(|d| d.split('-').next().and_then(|y| y.parse::<i32>().ok()))
            .or(Some(master_year as i32));

        DbAlbum {
            id: Uuid::new_v4().to_string(),
            title: release.title.clone(),
            year,
            discogs_release: None,
            musicbrainz_release: Some(musicbrainz_release),
            bandcamp_album_id: None,
            cover_art_url: None, // MusicBrainz doesn't provide cover art URLs directly
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
            format: None,
            label: None,
            catalog_number: None,
            country: None,
            barcode: None,
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
            format: None,         // TODO: Extract from Discogs release format data
            label: None,          // TODO: Extract from Discogs release labels
            catalog_number: None, // TODO: Extract from Discogs release
            country: None,        // TODO: Extract from Discogs release country
            barcode: None,        // TODO: Extract from Discogs release identifiers
            import_status: ImportStatus::Queued,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn from_mb_release(album_id: &str, release: &crate::musicbrainz::MbRelease) -> Self {
        let now = Utc::now();

        // Extract year from date string if available
        let year = release
            .date
            .as_ref()
            .and_then(|d| d.split('-').next().and_then(|y| y.parse::<i32>().ok()));

        DbRelease {
            id: Uuid::new_v4().to_string(),
            album_id: album_id.to_string(),
            release_name: None,
            year,
            discogs_release_id: None,
            bandcamp_release_id: None,
            format: release.format.clone(),
            label: release.label.clone(),
            catalog_number: release.catalog_number.clone(),
            country: release.country.clone(),
            barcode: release.barcode.clone(),
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
            disc_number: None,
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
        disc_number: Option<i32>,
    ) -> Result<Self, String> {
        Ok(DbTrack {
            id: Uuid::new_v4().to_string(),
            release_id: release_id.to_string(),
            title: discogs_track.title.clone(),
            disc_number,
            track_number: Some((track_index + 1) as i32),
            duration_ms: None, // Will be filled in during track mapping
            discogs_position: Some(discogs_track.position.clone()),
            import_status: ImportStatus::Queued,
            created_at: Utc::now(),
        })
    }
}

impl DbFile {
    /// Create a file record for export/torrent metadata
    ///
    /// Files are linked to releases. Used for reconstructing original file structure
    /// during export or BitTorrent seeding.
    pub fn new(release_id: &str, original_filename: &str, file_size: i64, format: &str) -> Self {
        DbFile {
            id: Uuid::new_v4().to_string(),
            release_id: release_id.to_string(),
            original_filename: original_filename.to_string(),
            file_size,
            format: format.to_string(),
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

impl DbAudioFormat {
    pub fn new(
        track_id: &str,
        format: &str,
        flac_headers: Option<Vec<u8>>,
        needs_headers: bool,
    ) -> Self {
        Self::new_with_seektable(track_id, format, flac_headers, None, needs_headers)
    }

    pub fn new_with_seektable(
        track_id: &str,
        format: &str,
        flac_headers: Option<Vec<u8>>,
        flac_seektable: Option<Vec<u8>>,
        needs_headers: bool,
    ) -> Self {
        DbAudioFormat {
            id: Uuid::new_v4().to_string(),
            track_id: track_id.to_string(),
            format: format.to_string(),
            flac_headers,
            flac_seektable,
            needs_headers,
            created_at: Utc::now(),
        }
    }
}

impl DbTrackChunkCoords {
    pub fn new(
        track_id: &str,
        start_chunk_index: i32,
        end_chunk_index: i32,
        start_byte_offset: i64,
        end_byte_offset: i64,
        start_time_ms: i64,
        end_time_ms: i64,
    ) -> Self {
        DbTrackChunkCoords {
            id: Uuid::new_v4().to_string(),
            track_id: track_id.to_string(),
            start_chunk_index,
            end_chunk_index,
            start_byte_offset,
            end_byte_offset,
            start_time_ms,
            end_time_ms,
            created_at: Utc::now(),
        }
    }
}

/// Torrent import metadata for a release
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbTorrent {
    pub id: String,
    pub release_id: String,
    pub info_hash: String,
    pub magnet_link: Option<String>,
    pub torrent_name: String,
    pub total_size_bytes: i64,
    pub piece_length: i32,
    pub num_pieces: i32,
    pub is_seeding: bool,
    pub created_at: DateTime<Utc>,
}

/// Maps torrent pieces to bae chunks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbTorrentPieceMapping {
    pub id: String,
    pub torrent_id: String,
    pub piece_index: i32,
    pub chunk_ids: String, // JSON array of chunk IDs
    pub start_byte_in_first_chunk: i64,
    pub end_byte_in_last_chunk: i64,
}

impl DbTorrent {
    pub fn new(
        release_id: &str,
        info_hash: &str,
        magnet_link: Option<String>,
        torrent_name: &str,
        total_size_bytes: i64,
        piece_length: i32,
        num_pieces: i32,
    ) -> Self {
        DbTorrent {
            id: Uuid::new_v4().to_string(),
            release_id: release_id.to_string(),
            info_hash: info_hash.to_string(),
            magnet_link,
            torrent_name: torrent_name.to_string(),
            total_size_bytes,
            piece_length,
            num_pieces,
            is_seeding: false,
            created_at: Utc::now(),
        }
    }
}

impl DbTorrentPieceMapping {
    pub fn new(
        torrent_id: &str,
        piece_index: i32,
        chunk_ids: Vec<String>,
        start_byte_in_first_chunk: i64,
        end_byte_in_last_chunk: i64,
    ) -> Result<Self, serde_json::Error> {
        Ok(DbTorrentPieceMapping {
            id: Uuid::new_v4().to_string(),
            torrent_id: torrent_id.to_string(),
            piece_index,
            chunk_ids: serde_json::to_string(&chunk_ids)?,
            start_byte_in_first_chunk,
            end_byte_in_last_chunk,
        })
    }

    pub fn chunk_ids(&self) -> Result<Vec<String>, serde_json::Error> {
        serde_json::from_str(&self.chunk_ids)
    }
}
