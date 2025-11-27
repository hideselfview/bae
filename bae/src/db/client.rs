use chrono::{DateTime, Utc};
use sqlx::{Row, SqlitePool};
use tracing::info;
use uuid::Uuid;

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

        // Album-Discogs join table (one-to-one relationship)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS album_discogs (
                id TEXT PRIMARY KEY,
                album_id TEXT NOT NULL UNIQUE,
                discogs_master_id TEXT NOT NULL,
                discogs_release_id TEXT NOT NULL,
                FOREIGN KEY (album_id) REFERENCES albums (id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Album-MusicBrainz join table (one-to-one relationship)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS album_musicbrainz (
                id TEXT PRIMARY KEY,
                album_id TEXT NOT NULL UNIQUE,
                musicbrainz_release_group_id TEXT NOT NULL,
                musicbrainz_release_id TEXT NOT NULL,
                FOREIGN KEY (album_id) REFERENCES albums (id) ON DELETE CASCADE
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
                format TEXT,
                label TEXT,
                catalog_number TEXT,
                country TEXT,
                barcode TEXT,
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

        // Files table (metadata for export/torrent features)
        // Files belong to releases, not tracks. Used for reconstructing original
        // file structure during export or BitTorrent seeding.
        // Playback uses TrackChunkCoords + AudioFormat, not files.
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS files (
                id TEXT PRIMARY KEY,
                release_id TEXT NOT NULL,
                original_filename TEXT NOT NULL,
                file_size INTEGER NOT NULL,
                format TEXT NOT NULL,
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

        // Audio formats table (format metadata per track)
        // Stores format information needed for playback.
        // FLAC headers only needed for CUE/FLAC tracks.
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS audio_formats (
                id TEXT PRIMARY KEY,
                track_id TEXT NOT NULL UNIQUE,
                format TEXT NOT NULL,
                flac_headers BLOB,
                flac_seektable BLOB,
                needs_headers BOOLEAN NOT NULL DEFAULT FALSE,
                created_at TEXT NOT NULL,
                FOREIGN KEY (track_id) REFERENCES tracks (id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Track chunk coordinates table (precise location of track audio in chunked stream)
        // This IS the TrackChunkCoords concept. Stores coordinates that locate a track's
        // audio data within the chunked album stream, regardless of source structure.
        // Both one-file-per-track and CUE/FLAC imports produce identical records here.
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS track_chunk_coords (
                id TEXT PRIMARY KEY,
                track_id TEXT NOT NULL UNIQUE,
                start_chunk_index INTEGER NOT NULL,
                end_chunk_index INTEGER NOT NULL,
                start_byte_offset INTEGER NOT NULL,
                end_byte_offset INTEGER NOT NULL,
                start_time_ms INTEGER NOT NULL,
                end_time_ms INTEGER NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY (track_id) REFERENCES tracks (id) ON DELETE CASCADE
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

        // Torrents table (torrent import metadata)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS torrents (
                id TEXT PRIMARY KEY,
                release_id TEXT NOT NULL,
                info_hash TEXT NOT NULL UNIQUE,
                magnet_link TEXT,
                torrent_name TEXT NOT NULL,
                total_size_bytes INTEGER NOT NULL,
                piece_length INTEGER NOT NULL,
                num_pieces INTEGER NOT NULL,
                is_seeding BOOLEAN NOT NULL DEFAULT FALSE,
                created_at TEXT NOT NULL,
                FOREIGN KEY (release_id) REFERENCES releases (id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Torrent piece mappings table (maps torrent pieces to bae chunks)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS torrent_piece_mappings (
                id TEXT PRIMARY KEY,
                torrent_id TEXT NOT NULL,
                piece_index INTEGER NOT NULL,
                chunk_ids TEXT NOT NULL,
                start_byte_in_first_chunk INTEGER NOT NULL,
                end_byte_in_last_chunk INTEGER NOT NULL,
                FOREIGN KEY (torrent_id) REFERENCES torrents (id) ON DELETE CASCADE,
                UNIQUE(torrent_id, piece_index)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_torrents_release_id ON torrents (release_id)")
            .execute(&self.pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_torrents_info_hash ON torrents (info_hash)")
            .execute(&self.pool)
            .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_torrent_piece_mappings_torrent_id ON torrent_piece_mappings (torrent_id)",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_audio_formats_track_id ON audio_formats (track_id)",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_track_chunk_coords_track_id ON track_chunk_coords (track_id)",
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
        let mut tx = self.pool.begin().await?;

        // Insert album
        sqlx::query(
            r#"
            INSERT INTO albums (
                id, title, year, bandcamp_album_id, cover_art_url, is_compilation, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&album.id)
        .bind(&album.title)
        .bind(album.year)
        .bind(&album.bandcamp_album_id)
        .bind(&album.cover_art_url)
        .bind(album.is_compilation)
        .bind(album.created_at.to_rfc3339())
        .bind(album.updated_at.to_rfc3339())
        .execute(&mut *tx)
        .await?;

        // Insert Discogs info if present
        if let Some(discogs_release) = &album.discogs_release {
            sqlx::query(
                r#"
                INSERT INTO album_discogs (
                    id, album_id, discogs_master_id, discogs_release_id
                ) VALUES (?, ?, ?, ?)
                "#,
            )
            .bind(Uuid::new_v4().to_string())
            .bind(&album.id)
            .bind(&discogs_release.master_id)
            .bind(&discogs_release.release_id)
            .execute(&mut *tx)
            .await?;
        }

        // Insert MusicBrainz info if present
        if let Some(mb_release) = &album.musicbrainz_release {
            sqlx::query(
                r#"
                INSERT INTO album_musicbrainz (
                    id, album_id, musicbrainz_release_group_id, musicbrainz_release_id
                ) VALUES (?, ?, ?, ?)
                "#,
            )
            .bind(Uuid::new_v4().to_string())
            .bind(&album.id)
            .bind(&mb_release.release_group_id)
            .bind(&mb_release.release_id)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// Insert a new release
    pub async fn insert_release(&self, release: &DbRelease) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO releases (
                id, album_id, release_name, year, discogs_release_id,
                bandcamp_release_id, format, label, catalog_number, country, barcode,
                import_status, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&release.id)
        .bind(&release.album_id)
        .bind(&release.release_name)
        .bind(release.year)
        .bind(&release.discogs_release_id)
        .bind(&release.bandcamp_release_id)
        .bind(&release.format)
        .bind(&release.label)
        .bind(&release.catalog_number)
        .bind(&release.country)
        .bind(&release.barcode)
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
                id, title, year, bandcamp_album_id, cover_art_url, is_compilation, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&album.id)
        .bind(&album.title)
        .bind(album.year)
        .bind(&album.bandcamp_album_id)
        .bind(&album.cover_art_url)
        .bind(album.is_compilation)
        .bind(album.created_at.to_rfc3339())
        .bind(album.updated_at.to_rfc3339())
        .execute(&mut *tx)
        .await?;

        // Insert Discogs info if present
        if let Some(discogs_release) = &album.discogs_release {
            sqlx::query(
                r#"
                INSERT INTO album_discogs (
                    id, album_id, discogs_master_id, discogs_release_id
                ) VALUES (?, ?, ?, ?)
                "#,
            )
            .bind(Uuid::new_v4().to_string())
            .bind(&album.id)
            .bind(&discogs_release.master_id)
            .bind(&discogs_release.release_id)
            .execute(&mut *tx)
            .await?;
        }

        // Insert MusicBrainz info if present
        if let Some(mb_release) = &album.musicbrainz_release {
            sqlx::query(
                r#"
                INSERT INTO album_musicbrainz (
                    id, album_id, musicbrainz_release_group_id, musicbrainz_release_id
                ) VALUES (?, ?, ?, ?)
                "#,
            )
            .bind(Uuid::new_v4().to_string())
            .bind(&album.id)
            .bind(&mb_release.release_group_id)
            .bind(&mb_release.release_id)
            .execute(&mut *tx)
            .await?;
        }

        // Insert release
        sqlx::query(
            r#"
            INSERT INTO releases (
                id, album_id, release_name, year, discogs_release_id,
                bandcamp_release_id, format, label, catalog_number, country, barcode,
                import_status, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&release.id)
        .bind(&release.album_id)
        .bind(&release.release_name)
        .bind(release.year)
        .bind(&release.discogs_release_id)
        .bind(&release.bandcamp_release_id)
        .bind(&release.format)
        .bind(&release.label)
        .bind(&release.catalog_number)
        .bind(&release.country)
        .bind(&release.barcode)
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
        let rows = sqlx::query(
            r#"
            SELECT 
                a.id, a.title, a.year, a.bandcamp_album_id, a.cover_art_url, 
                a.is_compilation, a.created_at, a.updated_at,
                ad.discogs_master_id, ad.discogs_release_id,
                amb.musicbrainz_release_group_id, amb.musicbrainz_release_id
            FROM albums a
            LEFT JOIN album_discogs ad ON a.id = ad.album_id
            LEFT JOIN album_musicbrainz amb ON a.id = amb.album_id
            ORDER BY a.title
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut albums = Vec::new();
        for row in rows {
            let discogs_master_id: Option<String> = row.get("discogs_master_id");
            let discogs_release_id: Option<String> = row.get("discogs_release_id");
            let discogs_release = match (discogs_master_id, discogs_release_id) {
                (Some(mid), Some(rid)) => Some(crate::db::models::DiscogsMasterRelease {
                    master_id: mid,
                    release_id: rid,
                }),
                _ => None,
            };

            let mb_release_group_id: Option<String> = row.get("musicbrainz_release_group_id");
            let mb_release_id: Option<String> = row.get("musicbrainz_release_id");
            let musicbrainz_release = match (mb_release_group_id, mb_release_id) {
                (Some(rgid), Some(rid)) => Some(crate::db::models::MusicBrainzRelease {
                    release_group_id: rgid,
                    release_id: rid,
                }),
                _ => None,
            };

            albums.push(DbAlbum {
                id: row.get("id"),
                title: row.get("title"),
                year: row.get("year"),
                discogs_release,
                musicbrainz_release,
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
        let row = sqlx::query(
            r#"
            SELECT 
                a.id, a.title, a.year, a.bandcamp_album_id, a.cover_art_url, 
                a.is_compilation, a.created_at, a.updated_at,
                ad.discogs_master_id, ad.discogs_release_id,
                amb.musicbrainz_release_group_id, amb.musicbrainz_release_id
            FROM albums a
            LEFT JOIN album_discogs ad ON a.id = ad.album_id
            LEFT JOIN album_musicbrainz amb ON a.id = amb.album_id
            WHERE a.id = ?
            "#,
        )
        .bind(album_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| {
            let discogs_master_id: Option<String> = row.get("discogs_master_id");
            let discogs_release_id: Option<String> = row.get("discogs_release_id");
            let discogs_release = match (discogs_master_id, discogs_release_id) {
                (Some(mid), Some(rid)) => Some(crate::db::models::DiscogsMasterRelease {
                    master_id: mid,
                    release_id: rid,
                }),
                _ => None,
            };

            let mb_release_group_id: Option<String> = row.get("musicbrainz_release_group_id");
            let mb_release_id: Option<String> = row.get("musicbrainz_release_id");
            let musicbrainz_release = match (mb_release_group_id, mb_release_id) {
                (Some(rgid), Some(rid)) => Some(crate::db::models::MusicBrainzRelease {
                    release_group_id: rgid,
                    release_id: rid,
                }),
                _ => None,
            };

            DbAlbum {
                id: row.get("id"),
                title: row.get("title"),
                year: row.get("year"),
                discogs_release,
                musicbrainz_release,
                bandcamp_album_id: row.get("bandcamp_album_id"),
                cover_art_url: row.get("cover_art_url"),
                is_compilation: row.get("is_compilation"),
                created_at: DateTime::parse_from_rfc3339(&row.get::<String, _>("created_at"))
                    .unwrap()
                    .with_timezone(&Utc),
                updated_at: DateTime::parse_from_rfc3339(&row.get::<String, _>("updated_at"))
                    .unwrap()
                    .with_timezone(&Utc),
            }
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
                format: row.get("format"),
                label: row.get("label"),
                catalog_number: row.get("catalog_number"),
                country: row.get("country"),
                barcode: row.get("barcode"),
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
                id, release_id, original_filename, file_size, format, created_at
            ) VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&file.id)
        .bind(&file.release_id)
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
                created_at: DateTime::parse_from_rfc3339(&row.get::<String, _>("created_at"))
                    .unwrap()
                    .with_timezone(&Utc),
            }))
        } else {
            Ok(None)
        }
    }

    /// Insert audio format for a track
    pub async fn insert_audio_format(
        &self,
        audio_format: &DbAudioFormat,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO audio_formats (
                id, track_id, format, flac_headers, flac_seektable, needs_headers, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&audio_format.id)
        .bind(&audio_format.track_id)
        .bind(&audio_format.format)
        .bind(&audio_format.flac_headers)
        .bind(&audio_format.flac_seektable)
        .bind(audio_format.needs_headers)
        .bind(audio_format.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get audio format for a track
    pub async fn get_audio_format_by_track_id(
        &self,
        track_id: &str,
    ) -> Result<Option<DbAudioFormat>, sqlx::Error> {
        let row = sqlx::query("SELECT * FROM audio_formats WHERE track_id = ?")
            .bind(track_id)
            .fetch_optional(&self.pool)
            .await?;

        if let Some(row) = row {
            Ok(Some(DbAudioFormat {
                id: row.get("id"),
                track_id: row.get("track_id"),
                format: row.get("format"),
                flac_headers: row.get("flac_headers"),
                flac_seektable: row.get("flac_seektable"),
                needs_headers: row.get("needs_headers"),
                created_at: DateTime::parse_from_rfc3339(&row.get::<String, _>("created_at"))
                    .unwrap()
                    .with_timezone(&Utc),
            }))
        } else {
            Ok(None)
        }
    }

    /// Insert track chunk coordinates
    pub async fn insert_track_chunk_coords(
        &self,
        coords: &DbTrackChunkCoords,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO track_chunk_coords (
                id, track_id, start_chunk_index, end_chunk_index,
                start_byte_offset, end_byte_offset,
                start_time_ms, end_time_ms, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&coords.id)
        .bind(&coords.track_id)
        .bind(coords.start_chunk_index)
        .bind(coords.end_chunk_index)
        .bind(coords.start_byte_offset)
        .bind(coords.end_byte_offset)
        .bind(coords.start_time_ms)
        .bind(coords.end_time_ms)
        .bind(coords.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get track chunk coordinates for a track
    pub async fn get_track_chunk_coords(
        &self,
        track_id: &str,
    ) -> Result<Option<DbTrackChunkCoords>, sqlx::Error> {
        let row = sqlx::query("SELECT * FROM track_chunk_coords WHERE track_id = ?")
            .bind(track_id)
            .fetch_optional(&self.pool)
            .await?;

        if let Some(row) = row {
            Ok(Some(DbTrackChunkCoords {
                id: row.get("id"),
                track_id: row.get("track_id"),
                start_chunk_index: row.get("start_chunk_index"),
                end_chunk_index: row.get("end_chunk_index"),
                start_byte_offset: row.get("start_byte_offset"),
                end_byte_offset: row.get("end_byte_offset"),
                start_time_ms: row.get("start_time_ms"),
                end_time_ms: row.get("end_time_ms"),
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

    /// Delete a release by ID
    ///
    /// This will cascade delete all related records:
    /// - Tracks (via FOREIGN KEY ON DELETE CASCADE)
    /// - Files (via FOREIGN KEY ON DELETE CASCADE)
    /// - Chunks (via FOREIGN KEY ON DELETE CASCADE)
    /// - Track artists, audio formats, track chunk coords (via FOREIGN KEY ON DELETE CASCADE)
    pub async fn delete_release(&self, release_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM releases WHERE id = ?")
            .bind(release_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Delete an album by ID
    ///
    /// This will cascade delete all related records:
    /// - Releases (via FOREIGN KEY ON DELETE CASCADE)
    /// - Album artists (via FOREIGN KEY ON DELETE CASCADE)
    /// - Album discogs (via FOREIGN KEY ON DELETE CASCADE)
    /// - All tracks, files, chunks, etc. from releases (via cascading)
    pub async fn delete_album(&self, album_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM albums WHERE id = ?")
            .bind(album_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Find album by Discogs master_id or release_id
    ///
    /// Used for duplicate detection before import.
    /// Returns the album if it exists with matching Discogs IDs.
    pub async fn find_album_by_discogs_ids(
        &self,
        master_id: Option<&str>,
        release_id: Option<&str>,
    ) -> Result<Option<DbAlbum>, sqlx::Error> {
        let query = if master_id.is_some() && release_id.is_some() {
            r#"
            SELECT 
                a.id, a.title, a.year, a.bandcamp_album_id, a.cover_art_url, 
                a.is_compilation, a.created_at, a.updated_at,
                ad.discogs_master_id, ad.discogs_release_id,
                amb.musicbrainz_release_group_id, amb.musicbrainz_release_id
            FROM albums a
            LEFT JOIN album_discogs ad ON a.id = ad.album_id
            LEFT JOIN album_musicbrainz amb ON a.id = amb.album_id
            WHERE ad.discogs_master_id = ? OR ad.discogs_release_id = ?
            LIMIT 1
            "#
        } else if master_id.is_some() {
            r#"
            SELECT 
                a.id, a.title, a.year, a.bandcamp_album_id, a.cover_art_url, 
                a.is_compilation, a.created_at, a.updated_at,
                ad.discogs_master_id, ad.discogs_release_id,
                amb.musicbrainz_release_group_id, amb.musicbrainz_release_id
            FROM albums a
            LEFT JOIN album_discogs ad ON a.id = ad.album_id
            LEFT JOIN album_musicbrainz amb ON a.id = amb.album_id
            WHERE ad.discogs_master_id = ?
            LIMIT 1
            "#
        } else if release_id.is_some() {
            r#"
            SELECT 
                a.id, a.title, a.year, a.bandcamp_album_id, a.cover_art_url, 
                a.is_compilation, a.created_at, a.updated_at,
                ad.discogs_master_id, ad.discogs_release_id,
                amb.musicbrainz_release_group_id, amb.musicbrainz_release_id
            FROM albums a
            LEFT JOIN album_discogs ad ON a.id = ad.album_id
            LEFT JOIN album_musicbrainz amb ON a.id = amb.album_id
            WHERE ad.discogs_release_id = ?
            LIMIT 1
            "#
        } else {
            return Ok(None);
        };

        let row = if master_id.is_some() && release_id.is_some() {
            sqlx::query(query)
                .bind(master_id.unwrap())
                .bind(release_id.unwrap())
                .fetch_optional(&self.pool)
                .await?
        } else if master_id.is_some() {
            sqlx::query(query)
                .bind(master_id.unwrap())
                .fetch_optional(&self.pool)
                .await?
        } else {
            sqlx::query(query)
                .bind(release_id.unwrap())
                .fetch_optional(&self.pool)
                .await?
        };

        Ok(row.map(|row| {
            let discogs_master_id: Option<String> = row.get("discogs_master_id");
            let discogs_release_id: Option<String> = row.get("discogs_release_id");
            let discogs_release = match (discogs_master_id, discogs_release_id) {
                (Some(mid), Some(rid)) => Some(crate::db::models::DiscogsMasterRelease {
                    master_id: mid,
                    release_id: rid,
                }),
                _ => None,
            };

            let mb_release_group_id: Option<String> = row.get("musicbrainz_release_group_id");
            let mb_release_id: Option<String> = row.get("musicbrainz_release_id");
            let musicbrainz_release = match (mb_release_group_id, mb_release_id) {
                (Some(rgid), Some(rid)) => Some(crate::db::models::MusicBrainzRelease {
                    release_group_id: rgid,
                    release_id: rid,
                }),
                _ => None,
            };

            DbAlbum {
                id: row.get("id"),
                title: row.get("title"),
                year: row.get("year"),
                discogs_release,
                musicbrainz_release,
                bandcamp_album_id: row.get("bandcamp_album_id"),
                cover_art_url: row.get("cover_art_url"),
                is_compilation: row.get("is_compilation"),
                created_at: DateTime::parse_from_rfc3339(&row.get::<String, _>("created_at"))
                    .unwrap()
                    .with_timezone(&Utc),
                updated_at: DateTime::parse_from_rfc3339(&row.get::<String, _>("updated_at"))
                    .unwrap()
                    .with_timezone(&Utc),
            }
        }))
    }

    /// Find album by MusicBrainz release_id or release_group_id
    ///
    /// Used for duplicate detection before import.
    /// Returns the album if it exists with matching MusicBrainz IDs.
    pub async fn find_album_by_mb_ids(
        &self,
        release_id: Option<&str>,
        release_group_id: Option<&str>,
    ) -> Result<Option<DbAlbum>, sqlx::Error> {
        let query = if release_id.is_some() && release_group_id.is_some() {
            r#"
            SELECT 
                a.id, a.title, a.year, a.bandcamp_album_id, a.cover_art_url, 
                a.is_compilation, a.created_at, a.updated_at,
                ad.discogs_master_id, ad.discogs_release_id,
                amb.musicbrainz_release_group_id, amb.musicbrainz_release_id
            FROM albums a
            LEFT JOIN album_discogs ad ON a.id = ad.album_id
            LEFT JOIN album_musicbrainz amb ON a.id = amb.album_id
            WHERE amb.musicbrainz_release_id = ? OR amb.musicbrainz_release_group_id = ?
            LIMIT 1
            "#
        } else if release_id.is_some() {
            r#"
            SELECT 
                a.id, a.title, a.year, a.bandcamp_album_id, a.cover_art_url, 
                a.is_compilation, a.created_at, a.updated_at,
                ad.discogs_master_id, ad.discogs_release_id,
                amb.musicbrainz_release_group_id, amb.musicbrainz_release_id
            FROM albums a
            LEFT JOIN album_discogs ad ON a.id = ad.album_id
            LEFT JOIN album_musicbrainz amb ON a.id = amb.album_id
            WHERE amb.musicbrainz_release_id = ?
            LIMIT 1
            "#
        } else if release_group_id.is_some() {
            r#"
            SELECT 
                a.id, a.title, a.year, a.bandcamp_album_id, a.cover_art_url, 
                a.is_compilation, a.created_at, a.updated_at,
                ad.discogs_master_id, ad.discogs_release_id,
                amb.musicbrainz_release_group_id, amb.musicbrainz_release_id
            FROM albums a
            LEFT JOIN album_discogs ad ON a.id = ad.album_id
            LEFT JOIN album_musicbrainz amb ON a.id = amb.album_id
            WHERE amb.musicbrainz_release_group_id = ?
            LIMIT 1
            "#
        } else {
            return Ok(None);
        };

        let row = if release_id.is_some() && release_group_id.is_some() {
            sqlx::query(query)
                .bind(release_id.unwrap())
                .bind(release_group_id.unwrap())
                .fetch_optional(&self.pool)
                .await?
        } else if release_id.is_some() {
            sqlx::query(query)
                .bind(release_id.unwrap())
                .fetch_optional(&self.pool)
                .await?
        } else {
            sqlx::query(query)
                .bind(release_group_id.unwrap())
                .fetch_optional(&self.pool)
                .await?
        };

        Ok(row.map(|row| {
            let discogs_master_id: Option<String> = row.get("discogs_master_id");
            let discogs_release_id: Option<String> = row.get("discogs_release_id");
            let discogs_release = match (discogs_master_id, discogs_release_id) {
                (Some(mid), Some(rid)) => Some(crate::db::models::DiscogsMasterRelease {
                    master_id: mid,
                    release_id: rid,
                }),
                _ => None,
            };

            let mb_release_group_id: Option<String> = row.get("musicbrainz_release_group_id");
            let mb_release_id: Option<String> = row.get("musicbrainz_release_id");
            let musicbrainz_release = match (mb_release_group_id, mb_release_id) {
                (Some(rgid), Some(rid)) => Some(crate::db::models::MusicBrainzRelease {
                    release_group_id: rgid,
                    release_id: rid,
                }),
                _ => None,
            };

            DbAlbum {
                id: row.get("id"),
                title: row.get("title"),
                year: row.get("year"),
                discogs_release,
                musicbrainz_release,
                bandcamp_album_id: row.get("bandcamp_album_id"),
                cover_art_url: row.get("cover_art_url"),
                is_compilation: row.get("is_compilation"),
                created_at: DateTime::parse_from_rfc3339(&row.get::<String, _>("created_at"))
                    .unwrap()
                    .with_timezone(&Utc),
                updated_at: DateTime::parse_from_rfc3339(&row.get::<String, _>("updated_at"))
                    .unwrap()
                    .with_timezone(&Utc),
            }
        }))
    }

    /// Insert a torrent record
    pub async fn insert_torrent(&self, torrent: &DbTorrent) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO torrents (
                id, release_id, info_hash, magnet_link, torrent_name,
                total_size_bytes, piece_length, num_pieces, is_seeding, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&torrent.id)
        .bind(&torrent.release_id)
        .bind(&torrent.info_hash)
        .bind(&torrent.magnet_link)
        .bind(&torrent.torrent_name)
        .bind(torrent.total_size_bytes)
        .bind(torrent.piece_length)
        .bind(torrent.num_pieces)
        .bind(torrent.is_seeding)
        .bind(torrent.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Get torrent by release ID
    pub async fn get_torrent_by_release(
        &self,
        release_id: &str,
    ) -> Result<Option<DbTorrent>, sqlx::Error> {
        let row = sqlx::query(
            r#"
            SELECT id, release_id, info_hash, magnet_link, torrent_name,
                   total_size_bytes, piece_length, num_pieces, is_seeding, created_at
            FROM torrents
            WHERE release_id = ?
            LIMIT 1
            "#,
        )
        .bind(release_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| DbTorrent {
            id: row.get("id"),
            release_id: row.get("release_id"),
            info_hash: row.get("info_hash"),
            magnet_link: row.get("magnet_link"),
            torrent_name: row.get("torrent_name"),
            total_size_bytes: row.get("total_size_bytes"),
            piece_length: row.get("piece_length"),
            num_pieces: row.get("num_pieces"),
            is_seeding: row.get("is_seeding"),
            created_at: DateTime::parse_from_rfc3339(&row.get::<String, _>("created_at"))
                .unwrap()
                .with_timezone(&Utc),
        }))
    }

    /// Insert a torrent piece mapping
    pub async fn insert_torrent_piece_mapping(
        &self,
        mapping: &DbTorrentPieceMapping,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO torrent_piece_mappings (
                id, torrent_id, piece_index, chunk_ids,
                start_byte_in_first_chunk, end_byte_in_last_chunk
            ) VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&mapping.id)
        .bind(&mapping.torrent_id)
        .bind(mapping.piece_index)
        .bind(&mapping.chunk_ids)
        .bind(mapping.start_byte_in_first_chunk)
        .bind(mapping.end_byte_in_last_chunk)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Get piece mappings for a torrent
    pub async fn get_torrent_piece_mappings(
        &self,
        torrent_id: &str,
    ) -> Result<Vec<DbTorrentPieceMapping>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT id, torrent_id, piece_index, chunk_ids,
                   start_byte_in_first_chunk, end_byte_in_last_chunk
            FROM torrent_piece_mappings
            WHERE torrent_id = ?
            ORDER BY piece_index
            "#,
        )
        .bind(torrent_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| DbTorrentPieceMapping {
                id: row.get("id"),
                torrent_id: row.get("torrent_id"),
                piece_index: row.get("piece_index"),
                chunk_ids: row.get("chunk_ids"),
                start_byte_in_first_chunk: row.get("start_byte_in_first_chunk"),
                end_byte_in_last_chunk: row.get("end_byte_in_last_chunk"),
            })
            .collect())
    }

    /// Get a specific piece mapping
    pub async fn get_torrent_piece_mapping(
        &self,
        torrent_id: &str,
        piece_index: i32,
    ) -> Result<Option<DbTorrentPieceMapping>, sqlx::Error> {
        let row = sqlx::query(
            r#"
            SELECT id, torrent_id, piece_index, chunk_ids,
                   start_byte_in_first_chunk, end_byte_in_last_chunk
            FROM torrent_piece_mappings
            WHERE torrent_id = ? AND piece_index = ?
            LIMIT 1
            "#,
        )
        .bind(torrent_id)
        .bind(piece_index)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| DbTorrentPieceMapping {
            id: row.get("id"),
            torrent_id: row.get("torrent_id"),
            piece_index: row.get("piece_index"),
            chunk_ids: row.get("chunk_ids"),
            start_byte_in_first_chunk: row.get("start_byte_in_first_chunk"),
            end_byte_in_last_chunk: row.get("end_byte_in_last_chunk"),
        }))
    }

    /// Update torrent seeding status
    pub async fn update_torrent_seeding(
        &self,
        torrent_id: &str,
        is_seeding: bool,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE torrents SET is_seeding = ? WHERE id = ?")
            .bind(is_seeding)
            .bind(torrent_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Get all torrents that are currently seeding
    pub async fn get_seeding_torrents(&self) -> Result<Vec<DbTorrent>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT id, release_id, info_hash, magnet_link, torrent_name,
                   total_size_bytes, piece_length, num_pieces, is_seeding, created_at
            FROM torrents
            WHERE is_seeding = TRUE
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| DbTorrent {
                id: row.get("id"),
                release_id: row.get("release_id"),
                info_hash: row.get("info_hash"),
                magnet_link: row.get("magnet_link"),
                torrent_name: row.get("torrent_name"),
                total_size_bytes: row.get("total_size_bytes"),
                piece_length: row.get("piece_length"),
                num_pieces: row.get("num_pieces"),
                is_seeding: row.get("is_seeding"),
                created_at: DateTime::parse_from_rfc3339(&row.get::<String, _>("created_at"))
                    .unwrap()
                    .with_timezone(&Utc),
            })
            .collect())
    }
}
