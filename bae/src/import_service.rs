use crate::chunking::ChunkingService;
use crate::cloud_storage::CloudStorageManager;
use crate::database::Database;
use crate::library::LibraryManager;
use crate::models::ImportItem;
use std::path::PathBuf;
use std::sync::{
    mpsc::{self, Receiver, Sender},
    Arc,
};
use std::thread;

/// Request to import an album
#[derive(Debug)]
pub enum ImportRequest {
    ImportAlbum { item: ImportItem, folder: PathBuf },
    Shutdown,
}

/// Progress updates during import
#[derive(Debug, Clone)]
pub enum ImportProgress {
    Started {
        album_id: String,
        album_title: String,
    },
    ChunkingProgress {
        album_id: String,
        current: usize,
        total: usize,
        percent: u8,
    },
    UploadProgress {
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

/// Handle for sending import requests and receiving progress updates
#[derive(Clone)]
pub struct ImportServiceHandle {
    request_tx: Sender<ImportRequest>,
    progress_rx: std::sync::Arc<std::sync::Mutex<Receiver<ImportProgress>>>,
}

impl ImportServiceHandle {
    pub fn send_request(&self, request: ImportRequest) -> Result<(), String> {
        self.request_tx
            .send(request)
            .map_err(|e| format!("Failed to send import request: {}", e))
    }

    pub fn try_recv_progress(&self) -> Option<ImportProgress> {
        self.progress_rx.lock().unwrap().try_recv().ok()
    }
}

/// Import service that runs on a dedicated thread
pub struct ImportService {
    handle: Option<thread::JoinHandle<()>>,
    request_tx: Sender<ImportRequest>,
    progress_rx: std::sync::Arc<std::sync::Mutex<Receiver<ImportProgress>>>,
}

impl ImportService {
    /// Start the import service on a dedicated thread
    pub fn start(
        database: Database,
        chunking_service: ChunkingService,
        cloud_storage: Option<CloudStorageManager>,
    ) -> Self {
        let (request_tx, request_rx) = mpsc::channel();
        let (progress_tx, progress_rx) = mpsc::channel();
        let progress_rx = Arc::new(std::sync::Mutex::new(progress_rx));

        let handle = thread::spawn(move || {
            println!("ImportService: Thread started");

            // Create a tokio runtime for async operations (S3 uploads)
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create tokio runtime");

            let library_manager =
                LibraryManager::new(database.clone(), chunking_service, cloud_storage);

            // Process import requests
            loop {
                match request_rx.recv() {
                    Ok(ImportRequest::ImportAlbum { item, folder }) => {
                        println!(
                            "ImportService: Received import request for {}",
                            item.title()
                        );

                        // Run the import on this thread
                        let result = runtime.block_on(Self::handle_import(
                            &library_manager,
                            &database,
                            &item,
                            &folder,
                            &progress_tx,
                        ));

                        if let Err(e) = result {
                            println!("ImportService: Import failed: {}", e);
                        }
                    }
                    Ok(ImportRequest::Shutdown) => {
                        println!("ImportService: Shutdown requested");
                        break;
                    }
                    Err(e) => {
                        println!("ImportService: Channel error: {}", e);
                        break;
                    }
                }
            }

            println!("ImportService: Thread exiting");
        });

        ImportService {
            handle: Some(handle),
            request_tx,
            progress_rx,
        }
    }

    /// Get a handle for sending requests and receiving progress
    pub fn handle(&self) -> ImportServiceHandle {
        ImportServiceHandle {
            request_tx: self.request_tx.clone(),
            progress_rx: self.progress_rx.clone(),
        }
    }

    /// Handle a single import request
    async fn handle_import(
        library_manager: &LibraryManager,
        database: &Database,
        item: &ImportItem,
        folder: &PathBuf,
        progress_tx: &Sender<ImportProgress>,
    ) -> Result<(), String> {
        println!(
            "ImportService: Starting import for {} from {}",
            item.title(),
            folder.display()
        );

        // Extract artist and create records
        let artist_name = Self::extract_artist_name(item);
        let album = library_manager
            .create_album_record(
                item,
                &artist_name,
                Some(folder.to_string_lossy().to_string()),
            )
            .map_err(|e| format!("Failed to create album record: {}", e))?;
        let album_id = album.id.clone();
        let album_title = album.title.clone();

        let tracks = library_manager
            .create_track_records(item, &album_id)
            .map_err(|e| format!("Failed to create track records: {}", e))?;

        // Send started progress
        let _ = progress_tx.send(ImportProgress::Started {
            album_id: album_id.clone(),
            album_title: album_title.clone(),
        });

        // Insert album + tracks in transaction (with status = 'importing')
        database
            .insert_album_with_tracks(&album, &tracks)
            .await
            .map_err(|e| format!("Database error: {}", e))?;

        println!(
            "ImportService: Inserted album and {} tracks into database",
            tracks.len()
        );

        // Map files to tracks
        let file_mappings = library_manager
            .map_files_to_tracks(folder, &tracks)
            .await
            .map_err(|e| format!("File mapping error: {}", e))?;

        // Process and upload files with progress reporting
        let progress_tx_clone = progress_tx.clone();
        let album_id_clone = album_id.clone();
        let progress_callback = Box::new(move |current, total, phase: String| {
            let percent = ((current as f64 / total as f64) * 100.0) as u8;
            let progress_update = match phase.as_str() {
                "chunking" => ImportProgress::ChunkingProgress {
                    album_id: album_id_clone.clone(),
                    current,
                    total,
                    percent,
                },
                "uploading" => ImportProgress::UploadProgress {
                    album_id: album_id_clone.clone(),
                    current,
                    total,
                    percent,
                },
                _ => return,
            };
            let _ = progress_tx_clone.send(progress_update);
        });

        library_manager
            .process_audio_files_with_progress(&file_mappings, &album_id, Some(progress_callback))
            .await
            .map_err(|e| {
                // Mark as failed
                let _ = tokio::runtime::Handle::current()
                    .block_on(database.update_album_status(&album_id, "failed"));
                for track in &tracks {
                    let _ = tokio::runtime::Handle::current()
                        .block_on(database.update_track_status(&track.id, "failed"));
                }
                format!("Import failed: {}", e)
            })?;

        // Mark all tracks as complete
        for track in &tracks {
            database
                .update_track_status(&track.id, "complete")
                .await
                .map_err(|e| format!("Failed to update track status: {}", e))?;

            let _ = progress_tx.send(ImportProgress::TrackComplete {
                album_id: album_id.clone(),
                track_id: track.id.clone(),
            });
        }

        // Mark album as complete
        database
            .update_album_status(&album_id, "complete")
            .await
            .map_err(|e| format!("Failed to update album status: {}", e))?;

        let _ = progress_tx.send(ImportProgress::Complete {
            album_id: album_id.clone(),
        });

        println!(
            "ImportService: Import completed successfully for {}",
            album_title
        );
        Ok(())
    }

    /// Extract artist name from import item
    fn extract_artist_name(import_item: &ImportItem) -> String {
        let title = import_item.title();
        if let Some(dash_pos) = title.find(" - ") {
            title[..dash_pos].to_string()
        } else {
            "Unknown Artist".to_string()
        }
    }

    /// Shutdown the service
    pub fn shutdown(mut self) {
        println!("ImportService: Sending shutdown signal");
        let _ = self.request_tx.send(ImportRequest::Shutdown);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}
