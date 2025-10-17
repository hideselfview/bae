use crate::cloud_storage::CloudStorageError;
use crate::database::{Database, DbAlbum, DbChunk, DbCueSheet, DbFile, DbTrack, DbTrackPosition};
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

    /// Insert album and tracks into database in a transaction
    pub async fn insert_album_with_tracks(
        &self,
        album: &DbAlbum,
        tracks: &[DbTrack],
    ) -> Result<(), LibraryError> {
        self.database
            .insert_album_with_tracks(album, tracks)
            .await?;
        Ok(())
    }

    /// Mark album as importing when pipeline starts processing
    pub async fn mark_album_importing(&self, album_id: &str) -> Result<(), LibraryError> {
        self.database
            .update_album_status(album_id, crate::database::ImportStatus::Importing)
            .await?;
        Ok(())
    }

    /// Mark track as complete after successful import
    pub async fn mark_track_complete(&self, track_id: &str) -> Result<(), LibraryError> {
        self.database
            .update_track_status(track_id, crate::database::ImportStatus::Complete)
            .await?;
        Ok(())
    }

    /// Mark track as failed if import errors
    pub async fn mark_track_failed(&self, track_id: &str) -> Result<(), LibraryError> {
        self.database
            .update_track_status(track_id, crate::database::ImportStatus::Failed)
            .await?;
        Ok(())
    }

    /// Mark album as complete after successful import
    pub async fn mark_album_complete(&self, album_id: &str) -> Result<(), LibraryError> {
        self.database
            .update_album_status(album_id, crate::database::ImportStatus::Complete)
            .await?;
        Ok(())
    }

    /// Mark album as failed if import errors
    pub async fn mark_album_failed(&self, album_id: &str) -> Result<(), LibraryError> {
        self.database
            .update_album_status(album_id, crate::database::ImportStatus::Failed)
            .await?;
        Ok(())
    }

    /// Add a chunk to the library
    pub async fn add_chunk(&self, chunk: &crate::database::DbChunk) -> Result<(), LibraryError> {
        self.database.insert_chunk(chunk).await?;
        Ok(())
    }

    /// Add a file to the library
    pub async fn add_file(&self, file: &crate::database::DbFile) -> Result<(), LibraryError> {
        self.database.insert_file(file).await?;
        Ok(())
    }

    /// Add a file-chunk mapping to the library
    pub async fn add_file_chunk_mapping(
        &self,
        file_chunk: &crate::database::DbFileChunk,
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

    /// Get tracks for a specific album
    pub async fn get_tracks(&self, album_id: &str) -> Result<Vec<DbTrack>, LibraryError> {
        Ok(self.database.get_tracks_for_album(album_id).await?)
    }

    /// Get a single track by ID
    pub async fn get_track(&self, track_id: &str) -> Result<Option<DbTrack>, LibraryError> {
        Ok(self.database.get_track_by_id(track_id).await?)
    }

    /// Get files for a specific track
    pub async fn get_files_for_track(&self, track_id: &str) -> Result<Vec<DbFile>, LibraryError> {
        Ok(self.database.get_files_for_track(track_id).await?)
    }

    /// Get chunks for a specific file
    pub async fn get_chunks_for_file(&self, file_id: &str) -> Result<Vec<DbChunk>, LibraryError> {
        Ok(self.database.get_chunks_for_file(file_id).await?)
    }

    /// Get all chunks for an album (for testing/verification)
    pub async fn get_chunks_for_album(&self, album_id: &str) -> Result<Vec<DbChunk>, LibraryError> {
        Ok(self.database.get_chunks_for_album(album_id).await?)
    }

    /// Get track position for CUE/FLAC tracks
    pub async fn get_track_position(
        &self,
        track_id: &str,
    ) -> Result<Option<crate::database::DbTrackPosition>, LibraryError> {
        Ok(self.database.get_track_position(track_id).await?)
    }

    /// Get chunks in a specific range for CUE/FLAC streaming
    pub async fn get_chunks_in_range(
        &self,
        album_id: &str,
        chunk_range: std::ops::RangeInclusive<i32>,
    ) -> Result<Vec<DbChunk>, LibraryError> {
        Ok(self
            .database
            .get_chunks_in_range(album_id, chunk_range)
            .await?)
    }

    /// Get album ID for a track
    pub async fn get_album_id_for_track(&self, track_id: &str) -> Result<String, LibraryError> {
        // TODO: Add a proper database method to lookup album_id by track_id directly
        // For now, iterate through all albums to find the track
        let albums = self.database.get_albums().await?;
        for album in albums {
            let tracks = self.database.get_tracks_for_album(&album.id).await?;
            if tracks.iter().any(|t| t.id == track_id) {
                return Ok(album.id);
            }
        }
        Err(LibraryError::TrackMapping(
            "Track not found in any album".to_string(),
        ))
    }
}
