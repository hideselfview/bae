use crate::models::DiscogsAlbum;
use std::path::PathBuf;

/// Request to import an album
#[derive(Debug)]
pub enum ImportRequest {
    FromFolder {
        album: DiscogsAlbum,
        folder: PathBuf,
    },
}

/// Progress updates during import
#[derive(Debug, Clone)]
pub enum ImportProgress {
    Started {
        album_id: String,
    },
    ProcessingProgress {
        album_id: String,
        current: usize,
        total: usize,
        percent: u8,
    },
    TrackComplete {
        album_id: String,
        track_id: String,
    },
    Complete {
        album_id: String,
    },
    Failed {
        album_id: String,
        error: String,
    },
}

/// Links a database track (already inserted with status='importing') to its source audio file.
/// Used during import to know which file contains the audio data for each track.
/// Tracks can share files (CUE/FLAC) or have dedicated files (one file per track).
#[derive(Debug, Clone)]
pub struct TrackSourceFile {
    /// Database track ID (UUID) - track already exists in DB with status='importing'
    pub db_track_id: String,
    /// Path to the source audio file on disk (FLAC, MP3, etc.)
    pub file_path: PathBuf,
}
