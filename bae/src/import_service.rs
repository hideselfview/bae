use crate::models::ImportItem;
use std::path::{Path, PathBuf};
use std::sync::{
    mpsc::{self, Receiver, Sender},
    Arc,
};
use std::thread;

/// Request to import an album
#[derive(Debug)]
#[allow(clippy::large_enum_variant)] // ImportAlbum is the common case, Shutdown is rare
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

/// File mapping for import workflow
#[derive(Debug, Clone)]
pub struct FileMapping {
    pub track_id: String,
    pub source_path: PathBuf,
}

/// Map audio files in source folder to tracks
async fn map_files_to_tracks(
    source_folder: &Path,
    tracks: &[crate::database::DbTrack],
) -> Result<Vec<FileMapping>, String> {
    use crate::cue_flac::CueFlacProcessor;

    println!(
        "ImportService: Mapping files in {} to {} tracks",
        source_folder.display(),
        tracks.len()
    );

    // First, check for CUE/FLAC pairs
    let cue_flac_pairs = CueFlacProcessor::detect_cue_flac(source_folder)
        .map_err(|e| format!("CUE/FLAC detection failed: {}", e))?;

    if !cue_flac_pairs.is_empty() {
        println!(
            "ImportService: Found {} CUE/FLAC pairs",
            cue_flac_pairs.len()
        );
        return map_cue_flac_to_tracks(cue_flac_pairs, tracks);
    }

    // Fallback to individual audio files
    let audio_files = find_audio_files(source_folder)?;

    if audio_files.is_empty() {
        return Err("No audio files found in source folder".to_string());
    }

    // Simple mapping strategy: sort files by name and match to track order
    // TODO: Replace with AI-powered matching
    let mut mappings = Vec::new();

    for (index, track) in tracks.iter().enumerate() {
        if let Some(audio_file) = audio_files.get(index) {
            mappings.push(FileMapping {
                track_id: track.id.clone(),
                source_path: audio_file.clone(),
            });
        } else {
            println!(
                "ImportService: Warning - no file found for track: {}",
                track.title
            );
        }
    }

    println!("ImportService: Mapped {} files to tracks", mappings.len());
    Ok(mappings)
}

/// Map CUE/FLAC pairs to tracks using CUE sheet parsing
fn map_cue_flac_to_tracks(
    cue_flac_pairs: Vec<crate::cue_flac::CueFlacPair>,
    tracks: &[crate::database::DbTrack],
) -> Result<Vec<FileMapping>, String> {
    use crate::cue_flac::CueFlacProcessor;

    let mut mappings = Vec::new();

    for pair in cue_flac_pairs {
        println!(
            "ImportService: Processing CUE/FLAC pair: {} + {}",
            pair.flac_path.display(),
            pair.cue_path.display()
        );

        // Parse the CUE sheet
        let cue_sheet = CueFlacProcessor::parse_cue_sheet(&pair.cue_path)
            .map_err(|e| format!("Failed to parse CUE sheet: {}", e))?;

        println!(
            "ImportService: CUE sheet contains {} tracks",
            cue_sheet.tracks.len()
        );

        // For CUE/FLAC, all tracks map to the same FLAC file
        for (index, cue_track) in cue_sheet.tracks.iter().enumerate() {
            if let Some(db_track) = tracks.get(index) {
                mappings.push(FileMapping {
                    track_id: db_track.id.clone(),
                    source_path: pair.flac_path.clone(),
                });

                println!(
                    "ImportService: Mapped CUE track '{}' to DB track '{}'",
                    cue_track.title, db_track.title
                );
            } else {
                println!(
                    "ImportService: Warning - CUE track '{}' has no corresponding DB track",
                    cue_track.title
                );
            }
        }
    }

    println!(
        "ImportService: Created {} CUE/FLAC mappings",
        mappings.len()
    );
    Ok(mappings)
}

/// Find all audio files in a directory
fn find_audio_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut audio_files = Vec::new();
    let audio_extensions = ["mp3", "flac", "wav", "m4a", "aac", "ogg"];

    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();

        if path.is_file() {
            if let Some(extension) = path.extension() {
                if let Some(ext_str) = extension.to_str() {
                    if audio_extensions.contains(&ext_str.to_lowercase().as_str()) {
                        audio_files.push(path);
                    }
                }
            }
        }
    }

    // Sort files by name for consistent ordering
    audio_files.sort();

    println!("ImportService: Found {} audio files", audio_files.len());
    Ok(audio_files)
}

/// Import service that runs on a dedicated thread
pub struct ImportService {
    request_tx: Sender<ImportRequest>,
    progress_rx: std::sync::Arc<std::sync::Mutex<Receiver<ImportProgress>>>,
}

impl ImportService {
    /// Start the import service on a dedicated thread
    pub fn start(library_manager: crate::library_context::SharedLibraryManager) -> Self {
        let (request_tx, request_rx) = mpsc::channel();
        let (progress_tx, progress_rx) = mpsc::channel();
        let progress_rx = Arc::new(std::sync::Mutex::new(progress_rx));

        // Spawn thread and let it run detached (no graceful shutdown yet - see TASKS.md)
        let _ = thread::spawn(move || {
            println!("ImportService: Thread started");

            // Create a tokio runtime for async operations (S3 uploads)
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create tokio runtime");

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
                            library_manager.get(),
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
        library_manager: &crate::library::LibraryManager,
        item: &ImportItem,
        folder: &Path,
        progress_tx: &Sender<ImportProgress>,
    ) -> Result<(), String> {
        println!(
            "ImportService: Starting import for {} from {}",
            item.title(),
            folder.display()
        );

        // Extract artist and create records
        let artist_name = Self::extract_artist_name(item);
        let album = crate::library::create_album_record(
            item,
            &artist_name,
            Some(folder.to_string_lossy().to_string()),
        )
        .map_err(|e| format!("Failed to create album record: {}", e))?;

        let album_id = album.id.clone();
        let album_title = album.title.clone();

        let tracks = crate::library::create_track_records(item, &album_id)
            .map_err(|e| format!("Failed to create track records: {}", e))?;

        // Send started progress
        let _ = progress_tx.send(ImportProgress::Started {
            album_id: album_id.clone(),
            album_title: album_title.clone(),
        });

        // Insert album + tracks in transaction (with status = 'importing')
        library_manager
            .insert_album_with_tracks(&album, &tracks)
            .await
            .map_err(|e| format!("Database error: {}", e))?;

        println!(
            "ImportService: Inserted album and {} tracks into database",
            tracks.len()
        );

        // Map files to tracks
        let file_mappings = map_files_to_tracks(folder, &tracks).await?;

        // Process and upload files with progress reporting
        let progress_tx_clone = progress_tx.clone();
        let album_id_clone = album_id.clone();
        let progress_callback = Box::new(
            move |current, total, phase: crate::library::ProcessingPhase| {
                let percent = ((current as f64 / total as f64) * 100.0) as u8;
                let progress_update = match phase {
                    crate::library::ProcessingPhase::Processing => {
                        ImportProgress::ProcessingProgress {
                            album_id: album_id_clone.clone(),
                            current,
                            total,
                            percent,
                        }
                    }
                };
                let _ = progress_tx_clone.send(progress_update);
            },
        );

        library_manager
            .process_audio_files_with_progress(&file_mappings, &album_id, Some(progress_callback))
            .await
            .map_err(|e| {
                // Mark as failed
                let _ = tokio::runtime::Handle::current()
                    .block_on(library_manager.mark_album_failed(&album_id));
                for track in &tracks {
                    let _ = tokio::runtime::Handle::current()
                        .block_on(library_manager.mark_track_failed(&track.id));
                }
                format!("Import failed: {}", e)
            })?;

        // Mark all tracks as complete
        for track in &tracks {
            library_manager
                .mark_track_complete(&track.id)
                .await
                .map_err(|e| format!("Failed to mark track complete: {}", e))?;

            let _ = progress_tx.send(ImportProgress::TrackComplete {
                album_id: album_id.clone(),
                track_id: track.id.clone(),
            });
        }

        // Mark album as complete
        library_manager
            .mark_album_complete(&album_id)
            .await
            .map_err(|e| format!("Failed to mark album complete: {}", e))?;

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
}
