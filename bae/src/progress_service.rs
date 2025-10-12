use crate::import_service::{ImportProgress, ImportStatus};
use std::collections::HashMap;
use std::sync::{mpsc::Receiver, Arc, RwLock};

/// Album-level import progress
#[derive(Debug, Clone, PartialEq)]
pub struct AlbumImportProgress {
    pub album_id: String,
    pub current: usize,
    pub total: usize,
    pub percent: u8,
    pub status: ImportStatus,
}

/// Track-level status
#[derive(Debug, Clone, PartialEq)]
pub struct TrackImportStatus {
    pub album_id: String,
    pub track_id: String,
    pub status: ImportStatus,
}

/// Progress service that maintains import progress state
#[derive(Clone)]
pub struct ProgressService {
    album_progress: Arc<RwLock<HashMap<String, AlbumImportProgress>>>,
    track_status: Arc<RwLock<HashMap<(String, String), TrackImportStatus>>>,
}

impl ProgressService {
    /// Create a new progress service and spawn background task to process progress updates
    pub fn new(progress_rx: Arc<std::sync::Mutex<Receiver<ImportProgress>>>) -> Self {
        let album_progress = Arc::new(RwLock::new(HashMap::new()));
        let track_status = Arc::new(RwLock::new(HashMap::new()));

        let album_prog = album_progress.clone();
        let track_stat = track_status.clone();

        // Spawn std::thread to block on channel recv
        std::thread::spawn(move || loop {
            let progress = {
                let rx = progress_rx.lock().unwrap();
                rx.recv()
            };

            match progress {
                Ok(progress) => {
                    Self::update_state(progress, &album_prog, &track_stat);
                }
                Err(_) => {
                    println!("ProgressService: Channel closed, exiting");
                    break;
                }
            }
        });

        Self {
            album_progress,
            track_status,
        }
    }

    /// Update internal state based on progress message
    fn update_state(
        progress: ImportProgress,
        album_progress: &Arc<RwLock<HashMap<String, AlbumImportProgress>>>,
        track_status: &Arc<RwLock<HashMap<(String, String), TrackImportStatus>>>,
    ) {
        match progress {
            ImportProgress::Started {
                album_id,
                album_title: _,
            } => {
                println!("ProgressService: Import started for {}", album_id);
                album_progress.write().unwrap().insert(
                    album_id.clone(),
                    AlbumImportProgress {
                        album_id,
                        current: 0,
                        total: 0,
                        percent: 0,
                        status: ImportStatus::InProgress,
                    },
                );
            }
            ImportProgress::ProcessingProgress {
                album_id,
                current,
                total,
                percent,
            } => {
                if let Some(progress) = album_progress.write().unwrap().get_mut(&album_id) {
                    progress.current = current;
                    progress.total = total;
                    progress.percent = percent;
                }
            }
            ImportProgress::TrackComplete { album_id, track_id } => {
                track_status.write().unwrap().insert(
                    (album_id.clone(), track_id.clone()),
                    TrackImportStatus {
                        album_id,
                        track_id,
                        status: ImportStatus::Complete,
                    },
                );
            }
            ImportProgress::Complete { album_id } => {
                println!("ProgressService: Import complete for {}", album_id);
                if let Some(progress) = album_progress.write().unwrap().get_mut(&album_id) {
                    progress.status = ImportStatus::Complete;
                    progress.percent = 100;
                }
            }
            ImportProgress::Failed { album_id, error } => {
                println!("ProgressService: Import failed for {}: {}", album_id, error);
                if let Some(progress) = album_progress.write().unwrap().get_mut(&album_id) {
                    progress.status = ImportStatus::Failed {
                        error: error.clone(),
                    };
                }
            }
        }
    }

    /// Get album progress (for reading from components)
    pub fn get_album_progress(&self, album_id: &str) -> Option<AlbumImportProgress> {
        self.album_progress.read().unwrap().get(album_id).cloned()
    }

    /// Get track status (for reading from components)
    pub fn get_track_status(&self, album_id: &str, track_id: &str) -> Option<TrackImportStatus> {
        self.track_status
            .read()
            .unwrap()
            .get(&(album_id.to_string(), track_id.to_string()))
            .cloned()
    }
}
