pub mod handle;

pub use handle::TorrentProgressHandle;

use crate::import::FolderMetadata;

#[derive(Debug, Clone)]
pub enum TorrentProgress {
    // Emitted when torrent is added and waiting for metadata (magnet links)
    WaitingForMetadata { info_hash: String },
    
    // Emitted when basic torrent info is ready
    TorrentInfoReady {
        info_hash: String,
        name: String,
        total_size: u64,
        num_files: usize,
    },
    
    // Emitted periodically with tracker/peer status
    StatusUpdate {
        info_hash: String,
        num_peers: i32,
        num_seeds: i32,
        trackers: Vec<TrackerStatus>,
    },
    
    // Emitted when metadata files are identified
    MetadataFilesDetected {
        info_hash: String,
        files: Vec<String>, // CUE, log, etc.
    },
    
    // Emitted during metadata file download
    MetadataProgress {
        info_hash: String,
        file: String,
        progress: f32, // 0.0 to 1.0
    },
    
    // Emitted when metadata detection completes
    MetadataComplete {
        info_hash: String,
        detected: Option<FolderMetadata>,
    },
    
    // Emitted on error
    Error {
        info_hash: String,
        message: String,
    },
}

#[derive(Debug, Clone)]
pub struct TrackerStatus {
    pub url: String,
    pub status: String, // "connected", "announcing", "error", etc.
    pub message: Option<String>,
}

