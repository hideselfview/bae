use crate::cache::CacheManager;
use crate::cloud_storage::{CloudStorageError, CloudStorageManager};
use crate::db::{
    Database, DbAlbum, DbAlbumArtist, DbArtist, DbAudioFormat, DbChunk, DbFile, DbRelease,
    DbTorrent, DbTrack, DbTrackArtist, DbTrackChunkCoords, ImportStatus,
};
use crate::encryption::EncryptionService;
use crate::library::export::ExportService;
use std::path::Path;
use thiserror::Error;
use tracing::warn;

#[derive(Error, Debug)]
pub enum LibraryError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Import error: {0}")]
    Import(String),
    #[error("Track mapping error: {0}")]
    TrackMapping(String),
    #[error("Cloud storage error: {0}")]
    CloudStorage(#[from] CloudStorageError),
}

/// The main library manager for database operations and entity persistence
///
/// Handles:
/// - Album/track/file/chunk persistence
/// - State transitions (importing -> complete/failed)
/// - Query methods for library browsing
/// - Deletion with cloud storage cleanup
#[derive(Debug, Clone)]
pub struct LibraryManager {
    database: Database,
    cloud_storage: CloudStorageManager,
}

impl LibraryManager {
    /// Create a new library manager
    pub fn new(database: Database, cloud_storage: CloudStorageManager) -> Self {
        LibraryManager {
            database,
            cloud_storage,
        }
    }

    /// Insert album, release, and tracks into database in a transaction
    pub async fn insert_album_with_release_and_tracks(
        &self,
        album: &DbAlbum,
        release: &DbRelease,
        tracks: &[DbTrack],
    ) -> Result<(), LibraryError> {
        self.database
            .insert_album_with_release_and_tracks(album, release, tracks)
            .await?;
        Ok(())
    }

    /// Mark release as importing when pipeline starts processing
    pub async fn mark_release_importing(&self, release_id: &str) -> Result<(), LibraryError> {
        self.database
            .update_release_status(release_id, ImportStatus::Importing)
            .await?;
        Ok(())
    }

    /// Mark track as complete after successful import
    pub async fn mark_track_complete(&self, track_id: &str) -> Result<(), LibraryError> {
        self.database
            .update_track_status(track_id, ImportStatus::Complete)
            .await?;
        Ok(())
    }

    /// Mark track as failed if import errors
    pub async fn mark_track_failed(&self, track_id: &str) -> Result<(), LibraryError> {
        self.database
            .update_track_status(track_id, ImportStatus::Failed)
            .await?;
        Ok(())
    }

    /// Update track duration
    pub async fn update_track_duration(
        &self,
        track_id: &str,
        duration_ms: Option<i64>,
    ) -> Result<(), LibraryError> {
        self.database
            .update_track_duration(track_id, duration_ms)
            .await?;
        Ok(())
    }

    /// Mark release as complete after successful import
    pub async fn mark_release_complete(&self, release_id: &str) -> Result<(), LibraryError> {
        self.database
            .update_release_status(release_id, ImportStatus::Complete)
            .await?;
        Ok(())
    }

    /// Mark release as failed if import errors
    pub async fn mark_release_failed(&self, release_id: &str) -> Result<(), LibraryError> {
        self.database
            .update_release_status(release_id, ImportStatus::Failed)
            .await?;
        Ok(())
    }

    /// Add a chunk to the library
    pub async fn add_chunk(&self, chunk: &DbChunk) -> Result<(), LibraryError> {
        self.database.insert_chunk(chunk).await?;
        Ok(())
    }

    /// Add a file to the library
    pub async fn add_file(&self, file: &DbFile) -> Result<(), LibraryError> {
        self.database.insert_file(file).await?;
        Ok(())
    }

    /// Add audio format for a track
    pub async fn add_audio_format(&self, audio_format: &DbAudioFormat) -> Result<(), LibraryError> {
        self.database.insert_audio_format(audio_format).await?;
        Ok(())
    }

    /// Add track chunk coordinates
    pub async fn add_track_chunk_coords(
        &self,
        coords: &DbTrackChunkCoords,
    ) -> Result<(), LibraryError> {
        self.database.insert_track_chunk_coords(coords).await?;
        Ok(())
    }

    /// Insert torrent metadata
    pub async fn insert_torrent(&self, torrent: &DbTorrent) -> Result<(), LibraryError> {
        self.database.insert_torrent(torrent).await?;
        Ok(())
    }

    /// Get torrent by release ID
    pub async fn get_torrent_by_release(
        &self,
        release_id: &str,
    ) -> Result<Option<DbTorrent>, LibraryError> {
        Ok(self.database.get_torrent_by_release(release_id).await?)
    }

    /// Insert torrent piece mapping
    pub async fn insert_torrent_piece_mapping(
        &self,
        mapping: &crate::db::DbTorrentPieceMapping,
    ) -> Result<(), LibraryError> {
        self.database.insert_torrent_piece_mapping(mapping).await?;
        Ok(())
    }

    /// Get all albums in the library
    pub async fn get_albums(&self) -> Result<Vec<DbAlbum>, LibraryError> {
        Ok(self.database.get_albums().await?)
    }

    /// Get album by ID
    pub async fn get_album_by_id(&self, album_id: &str) -> Result<Option<DbAlbum>, LibraryError> {
        Ok(self.database.get_album_by_id(album_id).await?)
    }

    /// Get all releases for a specific album
    pub async fn get_releases_for_album(
        &self,
        album_id: &str,
    ) -> Result<Vec<DbRelease>, LibraryError> {
        Ok(self.database.get_releases_for_album(album_id).await?)
    }

    /// Get tracks for a specific release
    pub async fn get_tracks(&self, release_id: &str) -> Result<Vec<DbTrack>, LibraryError> {
        Ok(self.database.get_tracks_for_release(release_id).await?)
    }

    /// Get a single track by ID
    pub async fn get_track(&self, track_id: &str) -> Result<Option<DbTrack>, LibraryError> {
        Ok(self.database.get_track_by_id(track_id).await?)
    }

    /// Get all files for a specific release
    ///
    /// Files belong to releases (not albums or tracks). This includes both:
    /// - Audio files (linked to tracks via db_track_position)
    /// - Metadata files (cover art, CUE sheets, etc.)
    pub async fn get_files_for_release(
        &self,
        release_id: &str,
    ) -> Result<Vec<DbFile>, LibraryError> {
        Ok(self.database.get_files_for_release(release_id).await?)
    }

    /// Get a specific file by ID
    ///
    /// Used during streaming to retrieve the file record after looking up
    /// the track→file relationship via db_track_position.
    pub async fn get_file_by_id(&self, file_id: &str) -> Result<Option<DbFile>, LibraryError> {
        Ok(self.database.get_file_by_id(file_id).await?)
    }

    /// Get audio format for a track
    pub async fn get_audio_format_by_track_id(
        &self,
        track_id: &str,
    ) -> Result<Option<DbAudioFormat>, LibraryError> {
        Ok(self.database.get_audio_format_by_track_id(track_id).await?)
    }

    /// Get all chunks for a release (for testing/verification)
    pub async fn get_chunks_for_release(
        &self,
        release_id: &str,
    ) -> Result<Vec<DbChunk>, LibraryError> {
        Ok(self.database.get_chunks_for_release(release_id).await?)
    }

    /// Get track chunk coordinates for a track
    pub async fn get_track_chunk_coords(
        &self,
        track_id: &str,
    ) -> Result<Option<DbTrackChunkCoords>, LibraryError> {
        Ok(self.database.get_track_chunk_coords(track_id).await?)
    }

    /// Get chunks in a specific range for CUE/FLAC streaming
    pub async fn get_chunks_in_range(
        &self,
        release_id: &str,
        chunk_range: std::ops::RangeInclusive<i32>,
    ) -> Result<Vec<DbChunk>, LibraryError> {
        Ok(self
            .database
            .get_chunks_in_range(release_id, chunk_range)
            .await?)
    }

    /// Get release ID for a track
    pub async fn get_release_id_for_track(&self, track_id: &str) -> Result<String, LibraryError> {
        let track = self
            .database
            .get_track_by_id(track_id)
            .await?
            .ok_or_else(|| LibraryError::TrackMapping("Track not found".to_string()))?;
        Ok(track.release_id)
    }

    /// Get album ID for a track
    pub async fn get_album_id_for_track(&self, track_id: &str) -> Result<String, LibraryError> {
        let track = self
            .database
            .get_track_by_id(track_id)
            .await?
            .ok_or_else(|| LibraryError::TrackMapping("Track not found".to_string()))?;
        let album_id = self
            .database
            .get_album_id_for_release(&track.release_id)
            .await?
            .ok_or_else(|| LibraryError::TrackMapping("Release not found".to_string()))?;
        Ok(album_id)
    }

    /// Get album ID for a release
    pub async fn get_album_id_for_release(&self, release_id: &str) -> Result<String, LibraryError> {
        let album_id = self
            .database
            .get_album_id_for_release(release_id)
            .await?
            .ok_or_else(|| LibraryError::TrackMapping("Release not found".to_string()))?;
        Ok(album_id)
    }

    /// Insert an artist
    pub async fn insert_artist(&self, artist: &DbArtist) -> Result<(), LibraryError> {
        self.database.insert_artist(artist).await?;
        Ok(())
    }

    /// Get artist by Discogs ID (for deduplication)
    pub async fn get_artist_by_discogs_id(
        &self,
        discogs_artist_id: &str,
    ) -> Result<Option<DbArtist>, LibraryError> {
        Ok(self
            .database
            .get_artist_by_discogs_id(discogs_artist_id)
            .await?)
    }

    /// Insert album-artist relationship
    pub async fn insert_album_artist(
        &self,
        album_artist: &DbAlbumArtist,
    ) -> Result<(), LibraryError> {
        self.database.insert_album_artist(album_artist).await?;
        Ok(())
    }

    /// Insert track-artist relationship
    pub async fn insert_track_artist(
        &self,
        track_artist: &DbTrackArtist,
    ) -> Result<(), LibraryError> {
        self.database.insert_track_artist(track_artist).await?;
        Ok(())
    }

    /// Get artists for an album
    pub async fn get_artists_for_album(
        &self,
        album_id: &str,
    ) -> Result<Vec<DbArtist>, LibraryError> {
        Ok(self.database.get_artists_for_album(album_id).await?)
    }

    /// Get artists for a track
    pub async fn get_artists_for_track(
        &self,
        track_id: &str,
    ) -> Result<Vec<DbArtist>, LibraryError> {
        Ok(self.database.get_artists_for_track(track_id).await?)
    }

    /// Delete a release and its associated data
    ///
    /// This will:
    /// 1. Get all chunks for the release
    /// 2. Delete chunks from cloud storage (errors are logged but don't stop deletion)
    /// 3. Delete the release from database (cascades to tracks, files, chunks, etc.)
    /// 4. If this was the last release for the album, also delete the album
    pub async fn delete_release(&self, release_id: &str) -> Result<(), LibraryError> {
        // Get album_id before deletion to check if we need to delete the album
        let album_id = self.get_album_id_for_release(release_id).await?;

        // Get all chunks for the release to delete from cloud storage
        let chunks = self.get_chunks_for_release(release_id).await?;

        // Delete chunks from cloud storage
        for chunk in &chunks {
            if let Err(e) = self
                .cloud_storage
                .delete_chunk(&chunk.storage_location)
                .await
            {
                warn!(
                    "Failed to delete chunk {} from cloud storage: {}. Continuing with database deletion.",
                    chunk.id, e
                );
            }
        }

        // Delete release from database (cascades to tracks, files, chunks, etc.)
        self.database.delete_release(release_id).await?;

        // Check if this was the last release for the album
        let remaining_releases = self.get_releases_for_album(&album_id).await?;
        if remaining_releases.is_empty() {
            // Delete the album as well
            self.database.delete_album(&album_id).await?;
        }

        Ok(())
    }

    /// Delete an album and all its associated data
    ///
    /// This will:
    /// 1. Get all releases for the album
    /// 2. For each release, get chunks and delete from cloud storage
    /// 3. Delete the album from database (cascades to releases and all related data)
    pub async fn delete_album(&self, album_id: &str) -> Result<(), LibraryError> {
        // Get all releases for the album
        let releases = self.get_releases_for_album(album_id).await?;

        // For each release, get chunks and delete from cloud storage
        for release in &releases {
            let chunks = self.get_chunks_for_release(&release.id).await?;
            for chunk in &chunks {
                if let Err(e) = self
                    .cloud_storage
                    .delete_chunk(&chunk.storage_location)
                    .await
                {
                    warn!(
                        "Failed to delete chunk {} from cloud storage: {}. Continuing with database deletion.",
                        chunk.id, e
                    );
                }
            }
        }

        // Delete album from database (cascades to releases and all related data)
        self.database.delete_album(album_id).await?;

        Ok(())
    }

    /// Export all files for a release to a directory
    ///
    /// Reconstructs files sequentially from chunks in the order they were imported.
    /// Files are written with their original filenames to the target directory.
    pub async fn export_release(
        &self,
        release_id: &str,
        target_dir: &Path,
        cloud_storage: &CloudStorageManager,
        cache: &CacheManager,
        encryption_service: &EncryptionService,
        chunk_size_bytes: usize,
    ) -> Result<(), LibraryError> {
        ExportService::export_release(
            release_id,
            target_dir,
            self,
            cloud_storage,
            cache,
            encryption_service,
            chunk_size_bytes,
        )
        .await
        .map_err(LibraryError::Import)
    }

    /// Export a single track as a FLAC file
    ///
    /// For one-file-per-track: extracts the original file.
    /// For CUE/FLAC: extracts and re-encodes as a standalone FLAC.
    pub async fn export_track(
        &self,
        track_id: &str,
        output_path: &Path,
        cloud_storage: &CloudStorageManager,
        cache: &CacheManager,
        encryption_service: &EncryptionService,
        chunk_size_bytes: usize,
    ) -> Result<(), LibraryError> {
        ExportService::export_track(
            track_id,
            output_path,
            self,
            cloud_storage,
            cache,
            encryption_service,
            chunk_size_bytes,
        )
        .await
        .map_err(LibraryError::Import)
    }

    /// Check if an album already exists by Discogs IDs
    ///
    /// Used for duplicate detection before import.
    /// Returns the existing album if found, None otherwise.
    pub async fn find_duplicate_by_discogs(
        &self,
        master_id: Option<&str>,
        release_id: Option<&str>,
    ) -> Result<Option<DbAlbum>, LibraryError> {
        Ok(self
            .database
            .find_album_by_discogs_ids(master_id, release_id)
            .await?)
    }

    /// Check if an album already exists by MusicBrainz IDs
    ///
    /// Used for duplicate detection before import.
    /// Returns the existing album if found, None otherwise.
    pub async fn find_duplicate_by_musicbrainz(
        &self,
        release_id: Option<&str>,
        release_group_id: Option<&str>,
    ) -> Result<Option<DbAlbum>, LibraryError> {
        Ok(self
            .database
            .find_album_by_mb_ids(release_id, release_group_id)
            .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud_storage::CloudStorageManager;
    use crate::db::{DbAlbum, DbChunk, DbRelease, ImportStatus};
    use crate::test_support::MockCloudStorage;
    use chrono::Utc;
    use std::sync::Arc;
    use tempfile::TempDir;
    use uuid::Uuid;

    async fn setup_test_manager() -> (LibraryManager, TempDir, CloudStorageManager) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let database = Database::new(db_path.to_str().unwrap()).await.unwrap();
        let mock_storage = Arc::new(MockCloudStorage::new());
        let cloud_storage = CloudStorageManager::from_storage(mock_storage);
        let manager = LibraryManager::new(database, cloud_storage.clone());
        (manager, temp_dir, cloud_storage)
    }

    fn create_test_album() -> DbAlbum {
        DbAlbum {
            id: Uuid::new_v4().to_string(),
            title: "Test Album".to_string(),
            year: Some(2024),
            discogs_release: None,
            musicbrainz_release: None,
            bandcamp_album_id: None,
            cover_art_url: None,
            is_compilation: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn create_test_release(album_id: &str) -> DbRelease {
        DbRelease {
            id: Uuid::new_v4().to_string(),
            album_id: album_id.to_string(),
            release_name: None,
            year: Some(2024),
            discogs_release_id: None,
            bandcamp_release_id: None,
            import_status: ImportStatus::Complete,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    async fn create_test_chunk(
        release_id: &str,
        chunk_index: i32,
        cloud_storage: &CloudStorageManager,
    ) -> DbChunk {
        let chunk_id = Uuid::new_v4().to_string();
        let storage_location = cloud_storage
            .upload_chunk_data(&chunk_id, &[0u8; 1024])
            .await
            .unwrap();
        DbChunk {
            id: chunk_id,
            release_id: release_id.to_string(),
            chunk_index,
            encrypted_size: 1024,
            storage_location,
            last_accessed: None,
            created_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn test_delete_release_with_single_release_deletes_album() {
        let (manager, _temp_dir, _cloud_storage) = setup_test_manager().await;

        // Create album and release
        let album = create_test_album();
        let release = create_test_release(&album.id);
        let chunk = create_test_chunk(&release.id, 0, &manager.cloud_storage).await;

        manager.database.insert_album(&album).await.unwrap();
        manager.database.insert_release(&release).await.unwrap();
        manager.database.insert_chunk(&chunk).await.unwrap();

        // Delete release
        manager.delete_release(&release.id).await.unwrap();

        // Verify album is deleted
        let album_result = manager.database.get_album_by_id(&album.id).await.unwrap();
        assert!(album_result.is_none());

        // Verify release is deleted
        let releases = manager
            .database
            .get_releases_for_album(&album.id)
            .await
            .unwrap();
        assert!(releases.is_empty());
    }

    #[tokio::test]
    async fn test_delete_release_with_multiple_releases_preserves_album() {
        let (manager, _temp_dir, _cloud_storage) = setup_test_manager().await;

        // Create album with two releases
        let album = create_test_album();
        let release1 = create_test_release(&album.id);
        let release2 = create_test_release(&album.id);
        let chunk1 = create_test_chunk(&release1.id, 0, &manager.cloud_storage).await;
        let chunk2 = create_test_chunk(&release2.id, 0, &manager.cloud_storage).await;

        manager.database.insert_album(&album).await.unwrap();
        manager.database.insert_release(&release1).await.unwrap();
        manager.database.insert_release(&release2).await.unwrap();
        manager.database.insert_chunk(&chunk1).await.unwrap();
        manager.database.insert_chunk(&chunk2).await.unwrap();

        // Delete first release
        manager.delete_release(&release1.id).await.unwrap();

        // Verify album still exists
        let album_result = manager.database.get_album_by_id(&album.id).await.unwrap();
        assert!(album_result.is_some());

        // Verify only release2 remains
        let releases = manager
            .database
            .get_releases_for_album(&album.id)
            .await
            .unwrap();
        assert_eq!(releases.len(), 1);
        assert_eq!(releases[0].id, release2.id);
    }

    #[tokio::test]
    async fn test_delete_album_deletes_all_releases_and_chunks() {
        let (manager, _temp_dir, cloud_storage) = setup_test_manager().await;

        // Create album with two releases
        let album = create_test_album();
        let release1 = create_test_release(&album.id);
        let release2 = create_test_release(&album.id);
        let chunk1 = create_test_chunk(&release1.id, 0, &manager.cloud_storage).await;
        let chunk2 = create_test_chunk(&release2.id, 0, &manager.cloud_storage).await;
        let location1 = chunk1.storage_location.clone();
        let location2 = chunk2.storage_location.clone();

        manager.database.insert_album(&album).await.unwrap();
        manager.database.insert_release(&release1).await.unwrap();
        manager.database.insert_release(&release2).await.unwrap();
        manager.database.insert_chunk(&chunk1).await.unwrap();
        manager.database.insert_chunk(&chunk2).await.unwrap();

        // Verify chunks exist in storage
        assert!(cloud_storage.download_chunk(&location1).await.is_ok());
        assert!(cloud_storage.download_chunk(&location2).await.is_ok());

        // Delete album
        manager.delete_album(&album.id).await.unwrap();

        // Verify album is deleted
        let album_result = manager.database.get_album_by_id(&album.id).await.unwrap();
        assert!(album_result.is_none());

        // Verify releases are deleted
        let releases = manager
            .database
            .get_releases_for_album(&album.id)
            .await
            .unwrap();
        assert!(releases.is_empty());

        // Verify chunks are deleted from cloud storage
        assert!(cloud_storage.download_chunk(&location1).await.is_err());
        assert!(cloud_storage.download_chunk(&location2).await.is_err());
    }

    #[tokio::test]
    async fn test_delete_release_cloud_storage_cleanup() {
        let (manager, _temp_dir, cloud_storage) = setup_test_manager().await;

        // Create album and release with chunk
        let album = create_test_album();
        let release = create_test_release(&album.id);
        let chunk = create_test_chunk(&release.id, 0, &manager.cloud_storage).await;
        let location = chunk.storage_location.clone();

        manager.database.insert_album(&album).await.unwrap();
        manager.database.insert_release(&release).await.unwrap();
        manager.database.insert_chunk(&chunk).await.unwrap();

        // Verify chunk exists
        assert!(cloud_storage.download_chunk(&location).await.is_ok());

        // Delete release
        manager.delete_release(&release.id).await.unwrap();

        // Verify chunk is deleted from cloud storage
        assert!(cloud_storage.download_chunk(&location).await.is_err());
    }
}
