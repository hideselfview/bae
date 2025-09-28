use sqlx::{SqlitePool, Row};
use uuid::Uuid;
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};

/// Database models for bae storage system
/// 
/// This implements the storage strategy described in the README:
/// - Albums and tracks stored as metadata
/// - Files split into encrypted chunks
/// - Chunks uploaded to cloud storage
/// - Local cache management for recently used chunks

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DbAlbum {
    pub id: String,
    pub title: String,
    pub artist_name: String,
    pub year: Option<i32>,
    pub discogs_master_id: Option<String>,
    pub discogs_release_id: Option<String>,
    pub cover_art_url: Option<String>,
    pub source_folder_path: Option<String>, // Path to original folder for checkout
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
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbFile {
    pub id: String,
    pub track_id: String,
    pub original_filename: String,
    pub file_size: i64,
    pub format: String, // "flac", "mp3", etc.
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbChunk {
    pub id: String,
    pub file_id: String,
    pub chunk_index: i32,
    pub chunk_size: i64,
    pub encrypted_size: i64,
    pub checksum: String,
    pub storage_location: String, // S3 key or local path
    pub is_local: bool,
    pub last_accessed: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

pub struct Database {
    pool: SqlitePool,
}

impl Database {
    /// Initialize database connection and create tables
    pub async fn new(database_path: &str) -> Result<Self, sqlx::Error> {
        let database_url = format!("sqlite:{}", database_path);
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
                source_folder_path TEXT,
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
                created_at TEXT NOT NULL,
                FOREIGN KEY (track_id) REFERENCES tracks (id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Chunks table (encrypted file chunks for cloud storage)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS chunks (
                id TEXT PRIMARY KEY,
                file_id TEXT NOT NULL,
                chunk_index INTEGER NOT NULL,
                chunk_size INTEGER NOT NULL,
                encrypted_size INTEGER NOT NULL,
                checksum TEXT NOT NULL,
                storage_location TEXT NOT NULL,
                is_local BOOLEAN NOT NULL DEFAULT FALSE,
                last_accessed TEXT,
                created_at TEXT NOT NULL,
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
            
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_chunks_file_id ON chunks (file_id)")
            .execute(&self.pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_chunks_last_accessed ON chunks (last_accessed)")
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
                discogs_release_id, cover_art_url, source_folder_path, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&album.id)
        .bind(&album.title)
        .bind(&album.artist_name)
        .bind(album.year)
        .bind(&album.discogs_master_id)
        .bind(&album.discogs_release_id)
        .bind(&album.cover_art_url)
        .bind(&album.source_folder_path)
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
                artist_name, discogs_position, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&track.id)
        .bind(&track.album_id)
        .bind(&track.title)
        .bind(track.track_number)
        .bind(track.duration_ms)
        .bind(&track.artist_name)
        .bind(&track.discogs_position)
        .bind(track.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }

    /// Get all albums
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
                source_folder_path: row.get("source_folder_path"),
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
                id, track_id, original_filename, file_size, format, created_at
            ) VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&file.id)
        .bind(&file.track_id)
        .bind(&file.original_filename)
        .bind(file.file_size)
        .bind(&file.format)
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
                id, file_id, chunk_index, chunk_size, encrypted_size, 
                checksum, storage_location, is_local, last_accessed, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&chunk.id)
        .bind(&chunk.file_id)
        .bind(chunk.chunk_index)
        .bind(chunk.chunk_size)
        .bind(chunk.encrypted_size)
        .bind(&chunk.checksum)
        .bind(&chunk.storage_location)
        .bind(chunk.is_local)
        .bind(chunk.last_accessed.as_ref().map(|dt| dt.to_rfc3339()))
        .bind(chunk.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }

    /// Get chunks for a file
    pub async fn get_chunks_for_file(&self, file_id: &str) -> Result<Vec<DbChunk>, sqlx::Error> {
        let rows = sqlx::query("SELECT * FROM chunks WHERE file_id = ? ORDER BY chunk_index")
            .bind(file_id)
            .fetch_all(&self.pool)
            .await?;

        let mut chunks = Vec::new();
        for row in rows {
            chunks.push(DbChunk {
                id: row.get("id"),
                file_id: row.get("file_id"),
                chunk_index: row.get("chunk_index"),
                chunk_size: row.get("chunk_size"),
                encrypted_size: row.get("encrypted_size"),
                checksum: row.get("checksum"),
                storage_location: row.get("storage_location"),
                is_local: row.get("is_local"),
                last_accessed: row.get::<Option<String>, _>("last_accessed")
                    .map(|s| DateTime::parse_from_rfc3339(&s).unwrap().with_timezone(&Utc)),
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
                created_at: DateTime::parse_from_rfc3339(&row.get::<String, _>("created_at"))
                    .unwrap()
                    .with_timezone(&Utc),
            });
        }
        
        Ok(files)
    }
}

/// Helper functions for creating database records from Discogs data
impl DbAlbum {
    pub fn from_discogs_master(
        master: &crate::models::DiscogsMaster,
        artist_name: &str,
        source_folder_path: Option<String>,
    ) -> Self {
        let now = Utc::now();
        DbAlbum {
            id: Uuid::new_v4().to_string(),
            title: master.title.clone(),
            artist_name: artist_name.to_string(),
            year: master.year.map(|y| y as i32),
            discogs_master_id: Some(master.id.clone()),
            discogs_release_id: None,
            cover_art_url: master.thumb.clone(),
            source_folder_path,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn from_discogs_release(
        release: &crate::models::DiscogsRelease,
        artist_name: &str,
        source_folder_path: Option<String>,
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
            source_folder_path,
            created_at: now,
            updated_at: now,
        }
    }
}

impl DbTrack {
    pub fn from_discogs_track(
        discogs_track: &crate::models::DiscogsTrack,
        album_id: &str,
        track_number: Option<i32>,
    ) -> Self {
        DbTrack {
            id: Uuid::new_v4().to_string(),
            album_id: album_id.to_string(),
            title: discogs_track.title.clone(),
            track_number,
            duration_ms: None, // Will be filled in during track mapping
            artist_name: None, // Will be filled in during track mapping
            discogs_position: Some(discogs_track.position.clone()),
            created_at: Utc::now(),
        }
    }
}

impl DbFile {
    pub fn new(
        track_id: &str,
        original_filename: &str,
        file_size: i64,
        format: &str,
    ) -> Self {
        DbFile {
            id: Uuid::new_v4().to_string(),
            track_id: track_id.to_string(),
            original_filename: original_filename.to_string(),
            file_size,
            format: format.to_string(),
            created_at: Utc::now(),
        }
    }
}

impl DbChunk {
    pub fn from_file_chunk(
        file_chunk: &crate::chunking::FileChunk,
        storage_location: &str,
        is_local: bool,
    ) -> Self {
        DbChunk {
            id: file_chunk.id.clone(),
            file_id: file_chunk.file_id.clone(),
            chunk_index: file_chunk.chunk_index,
            chunk_size: file_chunk.original_size as i64,
            encrypted_size: file_chunk.encrypted_size as i64,
            checksum: file_chunk.checksum.clone(),
            storage_location: storage_location.to_string(),
            is_local,
            last_accessed: if is_local { Some(Utc::now()) } else { None },
            created_at: Utc::now(),
        }
    }
}
