use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

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
            ImportStatus::Queued => "queued",
            ImportStatus::Importing => "importing",
            ImportStatus::Complete => "complete",
            ImportStatus::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DbAlbum {
    pub id: String,
    pub title: String,
    pub artist_name: String,
    pub year: Option<i32>,
    pub discogs_master_id: Option<String>,
    pub discogs_release_id: Option<String>,
    pub cover_art_url: Option<String>,
    pub import_status: ImportStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DbTrack {
    pub id: String,
    pub album_id: String,
    pub title: String,
    pub track_number: Option<i32>,
    pub duration_ms: Option<i64>,

    pub artist_name: Option<String>, // Can differ from album artist
    pub discogs_position: Option<String>, // e.g., "A1", "1", "1-1"
    pub import_status: ImportStatus,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbFile {
    pub id: String,
    pub track_id: String,
    pub original_filename: String,
    pub file_size: i64,
    pub format: String,                // "flac", "mp3", etc.
    pub flac_headers: Option<Vec<u8>>, // FLAC header blocks for instant streaming
    pub audio_start_byte: Option<i64>, // Where audio frames begin (after headers)
    pub has_cue_sheet: bool,           // Is this a CUE/FLAC file?
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbChunk {
    pub id: String,
    pub album_id: String,
    pub chunk_index: i32,
    pub encrypted_size: i64,
    pub storage_location: String, // S3 URI: s3://bucket/chunks/{shard}/{chunk_id}.enc
    pub is_local: bool,           // Legacy field, always false
    pub last_accessed: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbCueSheet {
    pub id: String,
    pub file_id: String,
    pub cue_content: String, // Raw CUE file content
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbTrackPosition {
    pub id: String,
    pub track_id: String,
    pub file_id: String,
    pub start_time_ms: i64,     // Track start in milliseconds
    pub end_time_ms: i64,       // Track end in milliseconds
    pub start_chunk_index: i32, // First chunk containing this track
    pub end_chunk_index: i32,   // Last chunk containing this track
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
        println!("Database: Connecting to {}", database_url);
        let pool = SqlitePool::connect(&database_url).await?;

        let db = Database { pool };
        db.create_tables().await?;
        Ok(db)
    }

    /// Create all necessary tables
    async fn create_tables(&self) -> Result<(), sqlx::Error> {
        // Albums table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS albums (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                artist_name TEXT NOT NULL,
                year INTEGER,
                discogs_master_id TEXT,
                discogs_release_id TEXT,
                cover_art_url TEXT,
                import_status TEXT NOT NULL DEFAULT 'importing',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Tracks table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS tracks (
                id TEXT PRIMARY KEY,
                album_id TEXT NOT NULL,
                title TEXT NOT NULL,
                track_number INTEGER,
                duration_ms INTEGER,
                artist_name TEXT,
                discogs_position TEXT,
                import_status TEXT NOT NULL DEFAULT 'importing',
                created_at TEXT NOT NULL,
                FOREIGN KEY (album_id) REFERENCES albums (id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Files table (maps tracks to actual audio files)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS files (
                id TEXT PRIMARY KEY,
                track_id TEXT NOT NULL,
                original_filename TEXT NOT NULL,
                file_size INTEGER NOT NULL,
                format TEXT NOT NULL,
                flac_headers BLOB,
                audio_start_byte INTEGER,
                has_cue_sheet BOOLEAN NOT NULL DEFAULT FALSE,
                created_at TEXT NOT NULL,
                FOREIGN KEY (track_id) REFERENCES tracks (id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Chunks table (encrypted album chunks for cloud storage)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS chunks (
                id TEXT PRIMARY KEY,
                album_id TEXT NOT NULL,
                chunk_index INTEGER NOT NULL,
                encrypted_size INTEGER NOT NULL,
                storage_location TEXT NOT NULL,
                is_local BOOLEAN NOT NULL DEFAULT FALSE,
                last_accessed TEXT,
                created_at TEXT NOT NULL,
                FOREIGN KEY (album_id) REFERENCES albums (id) ON DELETE CASCADE
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

        // Track positions table (for CUE track boundaries)
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
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_tracks_album_id ON tracks (album_id)")
            .execute(&self.pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_files_track_id ON files (track_id)")
            .execute(&self.pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_chunks_album_id ON chunks (album_id)")
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

    /// Insert a new album
    pub async fn insert_album(&self, album: &DbAlbum) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO albums (
                id, title, artist_name, year, discogs_master_id, 
                discogs_release_id, cover_art_url, import_status, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&album.id)
        .bind(&album.title)
        .bind(&album.artist_name)
        .bind(album.year)
        .bind(&album.discogs_master_id)
        .bind(&album.discogs_release_id)
        .bind(&album.cover_art_url)
        .bind(album.import_status)
        .bind(album.created_at.to_rfc3339())
        .bind(album.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Insert a new track
    pub async fn insert_track(&self, track: &DbTrack) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO tracks (
                id, album_id, title, track_number, duration_ms, 
                artist_name, discogs_position, import_status, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&track.id)
        .bind(&track.album_id)
        .bind(&track.title)
        .bind(track.track_number)
        .bind(track.duration_ms)
        .bind(&track.artist_name)
        .bind(&track.discogs_position)
        .bind(track.import_status)
        .bind(track.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Insert album and tracks in a single transaction
    pub async fn insert_album_with_tracks(
        &self,
        album: &DbAlbum,
        tracks: &[DbTrack],
    ) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        // Insert album
        sqlx::query(
            r#"
            INSERT INTO albums (
                id, title, artist_name, year, discogs_master_id, 
                discogs_release_id, cover_art_url, import_status, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&album.id)
        .bind(&album.title)
        .bind(&album.artist_name)
        .bind(album.year)
        .bind(&album.discogs_master_id)
        .bind(&album.discogs_release_id)
        .bind(&album.cover_art_url)
        .bind(album.import_status)
        .bind(album.created_at.to_rfc3339())
        .bind(album.updated_at.to_rfc3339())
        .execute(&mut *tx)
        .await?;

        // Insert all tracks
        for track in tracks {
            sqlx::query(
                r#"
                INSERT INTO tracks (
                    id, album_id, title, track_number, duration_ms, 
                    artist_name, discogs_position, import_status, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(&track.id)
            .bind(&track.album_id)
            .bind(&track.title)
            .bind(track.track_number)
            .bind(track.duration_ms)
            .bind(&track.artist_name)
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

    /// Update album import status
    pub async fn update_album_status(
        &self,
        album_id: &str,
        status: ImportStatus,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE albums SET import_status = ?, updated_at = ? WHERE id = ?")
            .bind(status)
            .bind(Utc::now().to_rfc3339())
            .bind(album_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Get all albums regardless of import status
    pub async fn get_albums(&self) -> Result<Vec<DbAlbum>, sqlx::Error> {
        let rows = sqlx::query("SELECT * FROM albums ORDER BY artist_name, title")
            .fetch_all(&self.pool)
            .await?;

        let mut albums = Vec::new();
        for row in rows {
            albums.push(DbAlbum {
                id: row.get("id"),
                title: row.get("title"),
                artist_name: row.get("artist_name"),
                year: row.get("year"),
                discogs_master_id: row.get("discogs_master_id"),
                discogs_release_id: row.get("discogs_release_id"),
                cover_art_url: row.get("cover_art_url"),
                import_status: row.get("import_status"),
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

    /// Get tracks for an album
    pub async fn get_tracks_for_album(&self, album_id: &str) -> Result<Vec<DbTrack>, sqlx::Error> {
        let rows = sqlx::query("SELECT * FROM tracks WHERE album_id = ? ORDER BY track_number")
            .bind(album_id)
            .fetch_all(&self.pool)
            .await?;

        let mut tracks = Vec::new();
        for row in rows {
            tracks.push(DbTrack {
                id: row.get("id"),
                album_id: row.get("album_id"),
                title: row.get("title"),
                track_number: row.get("track_number"),
                duration_ms: row.get("duration_ms"),
                artist_name: row.get("artist_name"),
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
                id, track_id, original_filename, file_size, format, 
                flac_headers, audio_start_byte, has_cue_sheet, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&file.id)
        .bind(&file.track_id)
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
                id, album_id, chunk_index, encrypted_size, 
                storage_location, is_local, last_accessed, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&chunk.id)
        .bind(&chunk.album_id)
        .bind(chunk.chunk_index)
        .bind(chunk.encrypted_size)
        .bind(&chunk.storage_location)
        .bind(chunk.is_local)
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

    /// Get chunks for a file (via file_chunks mapping)
    pub async fn get_chunks_for_file(&self, file_id: &str) -> Result<Vec<DbChunk>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT c.* FROM chunks c
            JOIN file_chunks fc ON c.chunk_index >= fc.start_chunk_index 
                AND c.chunk_index <= fc.end_chunk_index
                AND c.album_id = (SELECT album_id FROM tracks WHERE id = (SELECT track_id FROM files WHERE id = ?))
            WHERE fc.file_id = ?
            ORDER BY c.chunk_index
            "#
        )
        .bind(file_id)
        .bind(file_id)
        .fetch_all(&self.pool)
        .await?;

        let mut chunks = Vec::new();
        for row in rows {
            chunks.push(DbChunk {
                id: row.get("id"),
                album_id: row.get("album_id"),
                chunk_index: row.get("chunk_index"),
                encrypted_size: row.get("encrypted_size"),
                storage_location: row.get("storage_location"),
                is_local: row.get("is_local"),
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

    /// Get files for a track
    pub async fn get_files_for_track(&self, track_id: &str) -> Result<Vec<DbFile>, sqlx::Error> {
        let rows = sqlx::query("SELECT * FROM files WHERE track_id = ?")
            .bind(track_id)
            .fetch_all(&self.pool)
            .await?;

        let mut files = Vec::new();
        for row in rows {
            files.push(DbFile {
                id: row.get("id"),
                track_id: row.get("track_id"),
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

    /// Get chunks in a specific range for an album (for CUE track streaming)
    pub async fn get_chunks_in_range(
        &self,
        album_id: &str,
        chunk_range: std::ops::RangeInclusive<i32>,
    ) -> Result<Vec<DbChunk>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT * FROM chunks WHERE album_id = ? AND chunk_index >= ? AND chunk_index <= ? ORDER BY chunk_index"
        )
        .bind(album_id)
        .bind(*chunk_range.start())
        .bind(*chunk_range.end())
        .fetch_all(&self.pool)
        .await?;

        let mut chunks = Vec::new();
        for row in rows {
            chunks.push(DbChunk {
                id: row.get("id"),
                album_id: row.get("album_id"),
                chunk_index: row.get("chunk_index"),
                encrypted_size: row.get("encrypted_size"),
                storage_location: row.get("storage_location"),
                is_local: row.get("is_local"),
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
impl DbAlbum {
    pub fn from_discogs_master(master: &crate::models::DiscogsMaster, artist_name: &str) -> Self {
        let now = Utc::now();
        DbAlbum {
            id: Uuid::new_v4().to_string(),
            title: master.title.clone(),
            artist_name: artist_name.to_string(),
            year: master.year.map(|y| y as i32),
            discogs_master_id: Some(master.id.clone()),
            discogs_release_id: None,
            cover_art_url: master.thumb.clone(),
            import_status: ImportStatus::Queued,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn from_discogs_release(
        release: &crate::models::DiscogsRelease,
        artist_name: &str,
    ) -> Self {
        let now = Utc::now();
        DbAlbum {
            id: Uuid::new_v4().to_string(),
            title: release.title.clone(),
            artist_name: artist_name.to_string(),
            year: release.year.map(|y| y as i32),
            discogs_master_id: release.master_id.clone(),
            discogs_release_id: Some(release.id.clone()),
            cover_art_url: release.thumb.clone(),
            import_status: ImportStatus::Queued,
            created_at: now,
            updated_at: now,
        }
    }
}

impl DbTrack {
    pub fn from_discogs_track(
        discogs_track: &crate::models::DiscogsTrack,
        album_id: &str,
    ) -> Result<Self, String> {
        let track_number = Some(discogs_track.parse_track_number()?);

        Ok(DbTrack {
            id: Uuid::new_v4().to_string(),
            album_id: album_id.to_string(),
            title: discogs_track.title.clone(),
            track_number,
            duration_ms: None, // Will be filled in during track mapping
            artist_name: None, // Will be filled in during track mapping
            discogs_position: Some(discogs_track.position.clone()),
            import_status: ImportStatus::Queued,
            created_at: Utc::now(),
        })
    }
}

impl DbFile {
    pub fn new(track_id: &str, original_filename: &str, file_size: i64, format: &str) -> Self {
        DbFile {
            id: Uuid::new_v4().to_string(),
            track_id: track_id.to_string(),
            original_filename: original_filename.to_string(),
            file_size,
            format: format.to_string(),
            flac_headers: None,
            audio_start_byte: None,
            has_cue_sheet: false,
            created_at: Utc::now(),
        }
    }

    pub fn new_cue_flac(
        track_id: &str,
        original_filename: &str,
        file_size: i64,
        flac_headers: Vec<u8>,
        audio_start_byte: i64,
    ) -> Self {
        DbFile {
            id: Uuid::new_v4().to_string(),
            track_id: track_id.to_string(),
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
    pub fn from_album_chunk(
        chunk_id: &str,
        album_id: &str,
        chunk_index: i32,
        encrypted_size: usize,
        storage_location: &str,
        is_local: bool,
    ) -> Self {
        DbChunk {
            id: chunk_id.to_string(),
            album_id: album_id.to_string(),
            chunk_index,
            encrypted_size: encrypted_size as i64,
            storage_location: storage_location.to_string(),
            is_local,
            last_accessed: if is_local { Some(Utc::now()) } else { None },
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
