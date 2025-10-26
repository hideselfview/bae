use crate::cloud_storage::CloudStorageError;
use crate::db::{
    Database, DbAlbum, DbAlbumArtist, DbArtist, DbChunk, DbCueSheet, DbFile, DbFileChunk,
    DbRelease, DbTrack, DbTrackArtist, DbTrackPosition, ImportStatus,
};
use thiserror::Error;

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
#[derive(Debug, Clone)]
pub struct LibraryManager {
    database: Database,
}

impl LibraryManager {
    /// Create a new library manager
    pub fn new(database: Database) -> Self {
        LibraryManager { database }
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

    /// Add a file-chunk mapping to the library
    pub async fn add_file_chunk_mapping(
        &self,
        file_chunk: &DbFileChunk,
    ) -> Result<(), LibraryError> {
        self.database.insert_file_chunk(file_chunk).await?;
        Ok(())
    }

    /// Add a CUE sheet to the library
    pub async fn add_cue_sheet(&self, cue_sheet: &DbCueSheet) -> Result<(), LibraryError> {
        self.database.insert_cue_sheet(cue_sheet).await?;
        Ok(())
    }

    /// Add a track position to the library
    pub async fn add_track_position(
        &self,
        track_position: &DbTrackPosition,
    ) -> Result<(), LibraryError> {
        self.database.insert_track_position(track_position).await?;
        Ok(())
    }

    /// Get all albums in the library
    pub async fn get_albums(&self) -> Result<Vec<DbAlbum>, LibraryError> {
        Ok(self.database.get_albums().await?)
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

    /// Get file_chunk mapping for a file
    ///
    /// Returns the chunk range and byte offsets for a file.
    /// Used during reassembly to extract the correct bytes from chunks.
    pub async fn get_file_chunk_mapping(
        &self,
        file_id: &str,
    ) -> Result<Option<DbFileChunk>, LibraryError> {
        Ok(self.database.get_file_chunk_mapping(file_id).await?)
    }

    /// Get chunks for a specific file
    pub async fn get_chunks_for_file(&self, file_id: &str) -> Result<Vec<DbChunk>, LibraryError> {
        Ok(self.database.get_chunks_for_file(file_id).await?)
    }

    /// Get all chunks for a release (for testing/verification)
    pub async fn get_chunks_for_release(
        &self,
        release_id: &str,
    ) -> Result<Vec<DbChunk>, LibraryError> {
        Ok(self.database.get_chunks_for_release(release_id).await?)
    }

    /// Get track position for CUE/FLAC tracks
    pub async fn get_track_position(
        &self,
        track_id: &str,
    ) -> Result<Option<DbTrackPosition>, LibraryError> {
        Ok(self.database.get_track_position(track_id).await?)
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
}
