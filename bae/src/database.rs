use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use tracing::info;
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
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

#[derive(Debug, Clone)]
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    /// Initialize database connection and create tables
    pub async fn new(database_path: &str) -> Result<Self, sqlx::Error> {
        // Use sqlite:// with ?mode=rwc to create if it doesn't exist
        let database_url = format!("sqlite://{}?mode=rwc", database_path);
        info!("Connecting to {}", database_url);
        let pool = SqlitePool::connect(&database_url).await?;

        let db = Database { pool };
        db.create_tables().await?;
        Ok(db)
    }

    /// Create all necessary tables
    async fn create_tables(&self) -> Result<(), sqlx::Error> {
        // Artists table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS artists (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                sort_name TEXT,
                discogs_artist_id TEXT,
                bandcamp_artist_id TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Albums table (logical albums)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS albums (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                year INTEGER,
                discogs_master_id TEXT,
                bandcamp_album_id TEXT,
                cover_art_url TEXT,
                is_compilation BOOLEAN NOT NULL DEFAULT FALSE,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Album-Artist junction table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS album_artists (
                id TEXT PRIMARY KEY,
                album_id TEXT NOT NULL,
                artist_id TEXT NOT NULL,
                position INTEGER NOT NULL,
                FOREIGN KEY (album_id) REFERENCES albums (id) ON DELETE CASCADE,
                FOREIGN KEY (artist_id) REFERENCES artists (id) ON DELETE CASCADE,
                UNIQUE(album_id, artist_id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Releases table (specific versions/pressings of albums)
        sqlx::query(&format!(
            r#"
            CREATE TABLE IF NOT EXISTS releases (
                id TEXT PRIMARY KEY,
                album_id TEXT NOT NULL,
                release_name TEXT,
                year INTEGER,
                discogs_release_id TEXT,
                bandcamp_release_id TEXT,
                import_status TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (album_id) REFERENCES albums (id) ON DELETE CASCADE
            )
            "#,
            IMPORT_STATUS_QUEUED
        ))
        .execute(&self.pool)
        .await?;

        // Tracks table
        sqlx::query(&format!(
            r#"
            CREATE TABLE IF NOT EXISTS tracks (
                id TEXT PRIMARY KEY,
                release_id TEXT NOT NULL,
                title TEXT NOT NULL,
                track_number INTEGER,
                duration_ms INTEGER,
                discogs_position TEXT,
                import_status TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL,
                FOREIGN KEY (release_id) REFERENCES releases (id) ON DELETE CASCADE
            )
            "#,
            IMPORT_STATUS_QUEUED
        ))
        .execute(&self.pool)
        .await?;

        // Track-Artist junction table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS track_artists (
                id TEXT PRIMARY KEY,
                track_id TEXT NOT NULL,
                artist_id TEXT NOT NULL,
                position INTEGER NOT NULL,
                role TEXT,
                FOREIGN KEY (track_id) REFERENCES tracks (id) ON DELETE CASCADE,
                FOREIGN KEY (artist_id) REFERENCES artists (id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Files table (maps releases to actual files - audio or metadata)
        // Files belong to releases, not tracks, because:
        // - Metadata files (cover.jpg, .cue) aren't tied to specific tracks
        // - CUE/FLAC files contain multiple tracks in one file
        // Track→file relationship is tracked via track_positions table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS files (
                id TEXT PRIMARY KEY,
                release_id TEXT NOT NULL,
                original_filename TEXT NOT NULL,
                file_size INTEGER NOT NULL,
                format TEXT NOT NULL,
                flac_headers BLOB,
                audio_start_byte INTEGER,
                has_cue_sheet BOOLEAN NOT NULL DEFAULT FALSE,
                created_at TEXT NOT NULL,
                FOREIGN KEY (release_id) REFERENCES releases (id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Chunks table (encrypted release chunks for cloud storage)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS chunks (
                id TEXT PRIMARY KEY,
                release_id TEXT NOT NULL,
                chunk_index INTEGER NOT NULL,
                encrypted_size INTEGER NOT NULL,
                storage_location TEXT NOT NULL,
                last_accessed TEXT,
                created_at TEXT NOT NULL,
                FOREIGN KEY (release_id) REFERENCES releases (id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // File chunks mapping (which chunks contain which files)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS file_chunks (
                id TEXT PRIMARY KEY,
                file_id TEXT NOT NULL,
                start_chunk_index INTEGER NOT NULL,
                end_chunk_index INTEGER NOT NULL,
                start_byte_offset INTEGER NOT NULL,
                end_byte_offset INTEGER NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY (file_id) REFERENCES files (id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // CUE sheets table (for CUE/FLAC albums)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS cue_sheets (
                id TEXT PRIMARY KEY,
                file_id TEXT NOT NULL,
                cue_content TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY (file_id) REFERENCES files (id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Track positions table (links tracks to files with timing information)
        // Created for ALL tracks during import:
        // - Regular tracks: start_time_ms=0, end_time_ms=0 (full file)
        // - CUE tracks: actual timestamps from CUE sheet
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS track_positions (
                id TEXT PRIMARY KEY,
                track_id TEXT NOT NULL,
                file_id TEXT NOT NULL,
                start_time_ms INTEGER NOT NULL,
                end_time_ms INTEGER NOT NULL,
                start_chunk_index INTEGER NOT NULL,
                end_chunk_index INTEGER NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY (track_id) REFERENCES tracks (id) ON DELETE CASCADE,
                FOREIGN KEY (file_id) REFERENCES files (id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create indexes for performance
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_artists_discogs_id ON artists (discogs_artist_id)",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_album_artists_album_id ON album_artists (album_id)",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_album_artists_artist_id ON album_artists (artist_id)",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_track_artists_track_id ON track_artists (track_id)",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_track_artists_artist_id ON track_artists (artist_id)",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_releases_album_id ON releases (album_id)")
            .execute(&self.pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_tracks_release_id ON tracks (release_id)")
            .execute(&self.pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_files_release_id ON files (release_id)")
            .execute(&self.pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_chunks_release_id ON chunks (release_id)")
            .execute(&self.pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_file_chunks_file_id ON file_chunks (file_id)")
            .execute(&self.pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_cue_sheets_file_id ON cue_sheets (file_id)")
            .execute(&self.pool)
            .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_track_positions_track_id ON track_positions (track_id)",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_track_positions_file_id ON track_positions (file_id)",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_chunks_last_accessed ON chunks (last_accessed)",
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Insert a new artist
    pub async fn insert_artist(&self, artist: &DbArtist) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO artists (
                id, name, sort_name, discogs_artist_id, 
                bandcamp_artist_id, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&artist.id)
        .bind(&artist.name)
        .bind(&artist.sort_name)
        .bind(&artist.discogs_artist_id)
        .bind(&artist.bandcamp_artist_id)
        .bind(artist.created_at.to_rfc3339())
        .bind(artist.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get artist by Discogs artist ID (for deduplication)
    pub async fn get_artist_by_discogs_id(
        &self,
        discogs_artist_id: &str,
    ) -> Result<Option<DbArtist>, sqlx::Error> {
        let row = sqlx::query("SELECT * FROM artists WHERE discogs_artist_id = ?")
            .bind(discogs_artist_id)
            .fetch_optional(&self.pool)
            .await?;

        if let Some(row) = row {
            Ok(Some(DbArtist {
                id: row.get("id"),
                name: row.get("name"),
                sort_name: row.get("sort_name"),
                discogs_artist_id: row.get("discogs_artist_id"),
                bandcamp_artist_id: row.get("bandcamp_artist_id"),
                created_at: DateTime::parse_from_rfc3339(&row.get::<String, _>("created_at"))
                    .unwrap()
                    .with_timezone(&Utc),
                updated_at: DateTime::parse_from_rfc3339(&row.get::<String, _>("updated_at"))
                    .unwrap()
                    .with_timezone(&Utc),
            }))
        } else {
            Ok(None)
        }
    }

    /// Insert album-artist relationship
    pub async fn insert_album_artist(
        &self,
        album_artist: &DbAlbumArtist,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO album_artists (id, album_id, artist_id, position)
            VALUES (?, ?, ?, ?)
            "#,
        )
        .bind(&album_artist.id)
        .bind(&album_artist.album_id)
        .bind(&album_artist.artist_id)
        .bind(album_artist.position)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Insert track-artist relationship
    pub async fn insert_track_artist(
        &self,
        track_artist: &DbTrackArtist,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO track_artists (id, track_id, artist_id, position, role)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(&track_artist.id)
        .bind(&track_artist.track_id)
        .bind(&track_artist.artist_id)
        .bind(track_artist.position)
        .bind(&track_artist.role)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get artists for an album (ordered by position)
    pub async fn get_artists_for_album(
        &self,
        album_id: &str,
    ) -> Result<Vec<DbArtist>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT a.* FROM artists a
            JOIN album_artists aa ON a.id = aa.artist_id
            WHERE aa.album_id = ?
            ORDER BY aa.position
            "#,
        )
        .bind(album_id)
        .fetch_all(&self.pool)
        .await?;

        let mut artists = Vec::new();
        for row in rows {
            artists.push(DbArtist {
                id: row.get("id"),
                name: row.get("name"),
                sort_name: row.get("sort_name"),
                discogs_artist_id: row.get("discogs_artist_id"),
                bandcamp_artist_id: row.get("bandcamp_artist_id"),
                created_at: DateTime::parse_from_rfc3339(&row.get::<String, _>("created_at"))
                    .unwrap()
                    .with_timezone(&Utc),
                updated_at: DateTime::parse_from_rfc3339(&row.get::<String, _>("updated_at"))
                    .unwrap()
                    .with_timezone(&Utc),
            });
        }

        Ok(artists)
    }

    /// Get artists for a track (ordered by position)
    pub async fn get_artists_for_track(
        &self,
        track_id: &str,
    ) -> Result<Vec<DbArtist>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT a.* FROM artists a
            JOIN track_artists ta ON a.id = ta.artist_id
            WHERE ta.track_id = ?
            ORDER BY ta.position
            "#,
        )
        .bind(track_id)
        .fetch_all(&self.pool)
        .await?;

        let mut artists = Vec::new();
        for row in rows {
            artists.push(DbArtist {
                id: row.get("id"),
                name: row.get("name"),
                sort_name: row.get("sort_name"),
                discogs_artist_id: row.get("discogs_artist_id"),
                bandcamp_artist_id: row.get("bandcamp_artist_id"),
                created_at: DateTime::parse_from_rfc3339(&row.get::<String, _>("created_at"))
                    .unwrap()
                    .with_timezone(&Utc),
                updated_at: DateTime::parse_from_rfc3339(&row.get::<String, _>("updated_at"))
                    .unwrap()
                    .with_timezone(&Utc),
            });
        }

        Ok(artists)
    }

    /// Insert a new album
    pub async fn insert_album(&self, album: &DbAlbum) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO albums (
                id, title, year, discogs_master_id, 
                bandcamp_album_id, cover_art_url, is_compilation, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&album.id)
        .bind(&album.title)
        .bind(album.year)
        .bind(&album.discogs_master_id)
        .bind(&album.bandcamp_album_id)
        .bind(&album.cover_art_url)
        .bind(album.is_compilation)
        .bind(album.created_at.to_rfc3339())
        .bind(album.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Insert a new release
    pub async fn insert_release(&self, release: &DbRelease) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO releases (
                id, album_id, release_name, year, discogs_release_id,
                bandcamp_release_id, import_status, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&release.id)
        .bind(&release.album_id)
        .bind(&release.release_name)
        .bind(release.year)
        .bind(&release.discogs_release_id)
        .bind(&release.bandcamp_release_id)
        .bind(release.import_status)
        .bind(release.created_at.to_rfc3339())
        .bind(release.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Insert a new track
    pub async fn insert_track(&self, track: &DbTrack) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO tracks (
                id, release_id, title, track_number, duration_ms, 
                discogs_position, import_status, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&track.id)
        .bind(&track.release_id)
        .bind(&track.title)
        .bind(track.track_number)
        .bind(track.duration_ms)
        .bind(&track.discogs_position)
        .bind(track.import_status)
        .bind(track.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Insert album, release, and tracks in a single transaction
    /// Note: Artists and artist relationships should be inserted separately before calling this
    pub async fn insert_album_with_release_and_tracks(
        &self,
        album: &DbAlbum,
        release: &DbRelease,
        tracks: &[DbTrack],
    ) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        // Insert album
        sqlx::query(
            r#"
            INSERT INTO albums (
                id, title, year, discogs_master_id, 
                bandcamp_album_id, cover_art_url, is_compilation, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&album.id)
        .bind(&album.title)
        .bind(album.year)
        .bind(&album.discogs_master_id)
        .bind(&album.bandcamp_album_id)
        .bind(&album.cover_art_url)
        .bind(album.is_compilation)
        .bind(album.created_at.to_rfc3339())
        .bind(album.updated_at.to_rfc3339())
        .execute(&mut *tx)
        .await?;

        // Insert release
        sqlx::query(
            r#"
            INSERT INTO releases (
                id, album_id, release_name, year, discogs_release_id,
                bandcamp_release_id, import_status, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&release.id)
        .bind(&release.album_id)
        .bind(&release.release_name)
        .bind(release.year)
        .bind(&release.discogs_release_id)
        .bind(&release.bandcamp_release_id)
        .bind(release.import_status)
        .bind(release.created_at.to_rfc3339())
        .bind(release.updated_at.to_rfc3339())
        .execute(&mut *tx)
        .await?;

        // Insert all tracks
        for track in tracks {
            sqlx::query(
                r#"
                INSERT INTO tracks (
                    id, release_id, title, track_number, duration_ms, 
                    discogs_position, import_status, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(&track.id)
            .bind(&track.release_id)
            .bind(&track.title)
            .bind(track.track_number)
            .bind(track.duration_ms)
            .bind(&track.discogs_position)
            .bind(track.import_status)
            .bind(track.created_at.to_rfc3339())
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// Update track import status
    pub async fn update_track_status(
        &self,
        track_id: &str,
        status: ImportStatus,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE tracks SET import_status = ? WHERE id = ?")
            .bind(status)
            .bind(track_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Update release import status
    pub async fn update_release_status(
        &self,
        release_id: &str,
        status: ImportStatus,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE releases SET import_status = ?, updated_at = ? WHERE id = ?")
            .bind(status)
            .bind(Utc::now().to_rfc3339())
            .bind(release_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Get all albums
    pub async fn get_albums(&self) -> Result<Vec<DbAlbum>, sqlx::Error> {
        let rows = sqlx::query("SELECT * FROM albums ORDER BY title")
            .fetch_all(&self.pool)
            .await?;

        let mut albums = Vec::new();
        for row in rows {
            albums.push(DbAlbum {
                id: row.get("id"),
                title: row.get("title"),
                year: row.get("year"),
                discogs_master_id: row.get("discogs_master_id"),
                bandcamp_album_id: row.get("bandcamp_album_id"),
                cover_art_url: row.get("cover_art_url"),
                is_compilation: row.get("is_compilation"),
                created_at: DateTime::parse_from_rfc3339(&row.get::<String, _>("created_at"))
                    .unwrap()
                    .with_timezone(&Utc),
                updated_at: DateTime::parse_from_rfc3339(&row.get::<String, _>("updated_at"))
                    .unwrap()
                    .with_timezone(&Utc),
            });
        }

        Ok(albums)
    }

    /// Get all releases for an album
    pub async fn get_releases_for_album(
        &self,
        album_id: &str,
    ) -> Result<Vec<DbRelease>, sqlx::Error> {
        let rows = sqlx::query("SELECT * FROM releases WHERE album_id = ? ORDER BY created_at")
            .bind(album_id)
            .fetch_all(&self.pool)
            .await?;

        let mut releases = Vec::new();
        for row in rows {
            releases.push(DbRelease {
                id: row.get("id"),
                album_id: row.get("album_id"),
                release_name: row.get("release_name"),
                year: row.get("year"),
                discogs_release_id: row.get("discogs_release_id"),
                bandcamp_release_id: row.get("bandcamp_release_id"),
                import_status: row.get("import_status"),
                created_at: DateTime::parse_from_rfc3339(&row.get::<String, _>("created_at"))
                    .unwrap()
                    .with_timezone(&Utc),
                updated_at: DateTime::parse_from_rfc3339(&row.get::<String, _>("updated_at"))
                    .unwrap()
                    .with_timezone(&Utc),
            });
        }

        Ok(releases)
    }

    /// Get a track by ID
    pub async fn get_track_by_id(&self, track_id: &str) -> Result<Option<DbTrack>, sqlx::Error> {
        let row = sqlx::query("SELECT * FROM tracks WHERE id = ?")
            .bind(track_id)
            .fetch_optional(&self.pool)
            .await?;

        if let Some(row) = row {
            Ok(Some(DbTrack {
                id: row.get("id"),
                release_id: row.get("release_id"),
                title: row.get("title"),
                track_number: row.get("track_number"),
                duration_ms: row.get("duration_ms"),
                discogs_position: row.get("discogs_position"),
                import_status: row.get("import_status"),
                created_at: row.get("created_at"),
            }))
        } else {
            Ok(None)
        }
    }

    /// Get tracks for a release
    pub async fn get_tracks_for_release(
        &self,
        release_id: &str,
    ) -> Result<Vec<DbTrack>, sqlx::Error> {
        let rows = sqlx::query("SELECT * FROM tracks WHERE release_id = ? ORDER BY track_number")
            .bind(release_id)
            .fetch_all(&self.pool)
            .await?;

        let mut tracks = Vec::new();
        for row in rows {
            tracks.push(DbTrack {
                id: row.get("id"),
                release_id: row.get("release_id"),
                title: row.get("title"),
                track_number: row.get("track_number"),
                duration_ms: row.get("duration_ms"),
                discogs_position: row.get("discogs_position"),
                import_status: row.get("import_status"),
                created_at: DateTime::parse_from_rfc3339(&row.get::<String, _>("created_at"))
                    .unwrap()
                    .with_timezone(&Utc),
            });
        }

        Ok(tracks)
    }

    /// Insert a new file record
    pub async fn insert_file(&self, file: &DbFile) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO files (
                id, release_id, original_filename, file_size, format, 
                flac_headers, audio_start_byte, has_cue_sheet, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&file.id)
        .bind(&file.release_id)
        .bind(&file.original_filename)
        .bind(file.file_size)
        .bind(&file.format)
        .bind(&file.flac_headers)
        .bind(file.audio_start_byte)
        .bind(file.has_cue_sheet)
        .bind(file.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Insert a new chunk record
    pub async fn insert_chunk(&self, chunk: &DbChunk) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO chunks (
                id, release_id, chunk_index, encrypted_size, 
                storage_location, last_accessed, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&chunk.id)
        .bind(&chunk.release_id)
        .bind(chunk.chunk_index)
        .bind(chunk.encrypted_size)
        .bind(&chunk.storage_location)
        .bind(chunk.last_accessed.as_ref().map(|dt| dt.to_rfc3339()))
        .bind(chunk.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Insert a new file chunk mapping record
    pub async fn insert_file_chunk(&self, file_chunk: &DbFileChunk) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO file_chunks (
                id, file_id, start_chunk_index, end_chunk_index,
                start_byte_offset, end_byte_offset, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&file_chunk.id)
        .bind(&file_chunk.file_id)
        .bind(file_chunk.start_chunk_index)
        .bind(file_chunk.end_chunk_index)
        .bind(file_chunk.start_byte_offset)
        .bind(file_chunk.end_byte_offset)
        .bind(file_chunk.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get all chunks for a release (for testing/verification)
    pub async fn get_chunks_for_release(
        &self,
        release_id: &str,
    ) -> Result<Vec<DbChunk>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT * FROM chunks
            WHERE release_id = ?
            ORDER BY chunk_index
            "#,
        )
        .bind(release_id)
        .fetch_all(&self.pool)
        .await?;

        let mut chunks = Vec::new();
        for row in rows {
            chunks.push(DbChunk {
                id: row.get("id"),
                release_id: row.get("release_id"),
                chunk_index: row.get("chunk_index"),
                encrypted_size: row.get("encrypted_size"),
                storage_location: row.get("storage_location"),
                last_accessed: row.get("last_accessed"),
                created_at: row.get("created_at"),
            });
        }
        Ok(chunks)
    }

    /// Get chunks for a file (via file_chunks mapping)
    ///
    /// Since files belong to releases, we join through the release_id.
    /// The file_chunks table tells us which chunk range this file spans.
    pub async fn get_chunks_for_file(&self, file_id: &str) -> Result<Vec<DbChunk>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT c.* FROM chunks c
            JOIN file_chunks fc ON c.chunk_index >= fc.start_chunk_index 
                AND c.chunk_index <= fc.end_chunk_index
                AND c.release_id = (SELECT release_id FROM files WHERE id = ?)
            WHERE fc.file_id = ?
            ORDER BY c.chunk_index
            "#,
        )
        .bind(file_id)
        .bind(file_id)
        .fetch_all(&self.pool)
        .await?;

        let mut chunks = Vec::new();
        for row in rows {
            chunks.push(DbChunk {
                id: row.get("id"),
                release_id: row.get("release_id"),
                chunk_index: row.get("chunk_index"),
                encrypted_size: row.get("encrypted_size"),
                storage_location: row.get("storage_location"),
                last_accessed: row.get::<Option<String>, _>("last_accessed").map(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .unwrap()
                        .with_timezone(&Utc)
                }),
                created_at: DateTime::parse_from_rfc3339(&row.get::<String, _>("created_at"))
                    .unwrap()
                    .with_timezone(&Utc),
            });
        }

        Ok(chunks)
    }

    /// Get files for a release
    pub async fn get_files_for_release(
        &self,
        release_id: &str,
    ) -> Result<Vec<DbFile>, sqlx::Error> {
        let rows = sqlx::query("SELECT * FROM files WHERE release_id = ?")
            .bind(release_id)
            .fetch_all(&self.pool)
            .await?;

        let mut files = Vec::new();
        for row in rows {
            files.push(DbFile {
                id: row.get("id"),
                release_id: row.get("release_id"),
                original_filename: row.get("original_filename"),
                file_size: row.get("file_size"),
                format: row.get("format"),
                flac_headers: row.get("flac_headers"),
                audio_start_byte: row.get("audio_start_byte"),
                has_cue_sheet: row.get("has_cue_sheet"),
                created_at: DateTime::parse_from_rfc3339(&row.get::<String, _>("created_at"))
                    .unwrap()
                    .with_timezone(&Utc),
            });
        }

        Ok(files)
    }

    /// Get a specific file by ID
    pub async fn get_file_by_id(&self, file_id: &str) -> Result<Option<DbFile>, sqlx::Error> {
        let row = sqlx::query("SELECT * FROM files WHERE id = ?")
            .bind(file_id)
            .fetch_optional(&self.pool)
            .await?;

        if let Some(row) = row {
            Ok(Some(DbFile {
                id: row.get("id"),
                release_id: row.get("release_id"),
                original_filename: row.get("original_filename"),
                file_size: row.get("file_size"),
                format: row.get("format"),
                flac_headers: row.get("flac_headers"),
                audio_start_byte: row.get("audio_start_byte"),
                has_cue_sheet: row.get("has_cue_sheet"),
                created_at: DateTime::parse_from_rfc3339(&row.get::<String, _>("created_at"))
                    .unwrap()
                    .with_timezone(&Utc),
            }))
        } else {
            Ok(None)
        }
    }

    /// Insert a new CUE sheet record
    pub async fn insert_cue_sheet(&self, cue_sheet: &DbCueSheet) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO cue_sheets (
                id, file_id, cue_content, created_at
            ) VALUES (?, ?, ?, ?)
            "#,
        )
        .bind(&cue_sheet.id)
        .bind(&cue_sheet.file_id)
        .bind(&cue_sheet.cue_content)
        .bind(cue_sheet.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Insert a new track position record
    pub async fn insert_track_position(
        &self,
        position: &DbTrackPosition,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO track_positions (
                id, track_id, file_id, start_time_ms, end_time_ms,
                start_chunk_index, end_chunk_index, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&position.id)
        .bind(&position.track_id)
        .bind(&position.file_id)
        .bind(position.start_time_ms)
        .bind(position.end_time_ms)
        .bind(position.start_chunk_index)
        .bind(position.end_chunk_index)
        .bind(position.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get track position for a track
    pub async fn get_track_position(
        &self,
        track_id: &str,
    ) -> Result<Option<DbTrackPosition>, sqlx::Error> {
        let row = sqlx::query("SELECT * FROM track_positions WHERE track_id = ?")
            .bind(track_id)
            .fetch_optional(&self.pool)
            .await?;

        if let Some(row) = row {
            Ok(Some(DbTrackPosition {
                id: row.get("id"),
                track_id: row.get("track_id"),
                file_id: row.get("file_id"),
                start_time_ms: row.get("start_time_ms"),
                end_time_ms: row.get("end_time_ms"),
                start_chunk_index: row.get("start_chunk_index"),
                end_chunk_index: row.get("end_chunk_index"),
                created_at: DateTime::parse_from_rfc3339(&row.get::<String, _>("created_at"))
                    .unwrap()
                    .with_timezone(&Utc),
            }))
        } else {
            Ok(None)
        }
    }

    /// Get chunks in a specific range for a release (for CUE track streaming)
    pub async fn get_chunks_in_range(
        &self,
        release_id: &str,
        chunk_range: std::ops::RangeInclusive<i32>,
    ) -> Result<Vec<DbChunk>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT * FROM chunks WHERE release_id = ? AND chunk_index >= ? AND chunk_index <= ? ORDER BY chunk_index"
        )
        .bind(release_id)
        .bind(*chunk_range.start())
        .bind(*chunk_range.end())
        .fetch_all(&self.pool)
        .await?;

        let mut chunks = Vec::new();
        for row in rows {
            chunks.push(DbChunk {
                id: row.get("id"),
                release_id: row.get("release_id"),
                chunk_index: row.get("chunk_index"),
                encrypted_size: row.get("encrypted_size"),
                storage_location: row.get("storage_location"),
                last_accessed: row.get::<Option<String>, _>("last_accessed").map(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .unwrap()
                        .with_timezone(&Utc)
                }),
                created_at: DateTime::parse_from_rfc3339(&row.get::<String, _>("created_at"))
                    .unwrap()
                    .with_timezone(&Utc),
            });
        }

        Ok(chunks)
    }
}

/// Helper functions for creating database records from Discogs data
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
    /// Create a logical album from a Discogs master
    /// Note: Artists should be created separately and linked via DbAlbumArtist
    pub fn from_discogs_master(master: &crate::models::DiscogsMaster) -> Self {
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
    pub fn from_discogs_release(release: &crate::models::DiscogsRelease) -> Self {
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
    /// Create a release from a Discogs release
    pub fn from_discogs_release(album_id: &str, release: &crate::models::DiscogsRelease) -> Self {
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
    pub fn from_discogs_track(
        discogs_track: &crate::models::DiscogsTrack,
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
