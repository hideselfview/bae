use chrono::{DateTime, Utc};
use sqlx::{Row, SqlitePool};
use tracing::info;

use crate::db::models::*;

// String constants for SQL DEFAULT clauses (keep in sync with as_str())
const IMPORT_STATUS_QUEUED: &str = "queued";

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
                FOREIGN KEY (album_id) REFERENCES albums (id) ON DELETE CASCADE,
                UNIQUE(album_id, discogs_release_id),
                UNIQUE(album_id, bandcamp_release_id)
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

    /// Update track duration
    pub async fn update_track_duration(
        &self,
        track_id: &str,
        duration_ms: Option<i64>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE tracks SET duration_ms = ? WHERE id = ?")
            .bind(duration_ms)
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

    /// Get album by ID
    pub async fn get_album_by_id(&self, album_id: &str) -> Result<Option<DbAlbum>, sqlx::Error> {
        let row = sqlx::query("SELECT * FROM albums WHERE id = ?")
            .bind(album_id)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(|row| DbAlbum {
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
        }))
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

    /// Get album_id for a release
    pub async fn get_album_id_for_release(
        &self,
        release_id: &str,
    ) -> Result<Option<String>, sqlx::Error> {
        let row = sqlx::query("SELECT album_id FROM releases WHERE id = ?")
            .bind(release_id)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(|r| r.get("album_id")))
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

    /// Get file_chunk mapping for a file
    pub async fn get_file_chunk_mapping(
        &self,
        file_id: &str,
    ) -> Result<Option<DbFileChunk>, sqlx::Error> {
        let row = sqlx::query("SELECT * FROM file_chunks WHERE file_id = ?")
            .bind(file_id)
            .fetch_optional(&self.pool)
            .await?;

        if let Some(row) = row {
            Ok(Some(DbFileChunk {
                id: row.get("id"),
                file_id: row.get("file_id"),
                start_chunk_index: row.get("start_chunk_index"),
                end_chunk_index: row.get("end_chunk_index"),
                start_byte_offset: row.get("start_byte_offset"),
                end_byte_offset: row.get("end_byte_offset"),
                created_at: DateTime::parse_from_rfc3339(&row.get::<String, _>("created_at"))
                    .unwrap()
                    .with_timezone(&Utc),
            }))
        } else {
            Ok(None)
        }
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
