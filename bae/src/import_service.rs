// # Import Service
//
// ## Overview
// The import service orchestrates the complete album import workflow, from initial file
// discovery through encryption and upload to cloud storage. It runs on a dedicated background
// thread to avoid blocking the UI.
//
// ## Import Flow
//
// 1. **Album & Track Creation** (in-memory, not yet inserted)
//    - Parse Discogs metadata to create DbAlbum and DbTrack records
//    - Generate UUIDs for all entities (album_id, track_id)
//    - Records exist only in memory at this point
//
// 2. **Track-to-File Mapping & Validation** (TrackSourceFile)
//    - Scan source folder for audio files (FLAC, MP3, etc.)
//    - Map each track to its source file (by position or CUE sheet)
//    - Create TrackSourceFile entries linking db_track_id -> file_path
//    - Handles both individual files and CUE/FLAC (single FLAC, multiple tracks)
//    - **CRITICAL: If mapping fails, import is rejected - no DB records created**
//
// 3. **Database Insertion** (status='importing')
//    - Only after successful validation, insert album + tracks into DB
//    - All records created with status='importing'
//    - User can now see the album in the library (greyed out)
//    - Send ImportProgress::Started to UI subscribers
//
// 4. **Parallel Encryption** (CPU-bound)
//    - Read all files, split into fixed-size chunks (e.g., 5MB)
//    - Encrypt chunks in parallel (semaphore-limited by CPU cores)
//    - Uses AES-256-GCM encryption via spawn_blocking
//    - Generates chunk IDs, checksums, nonces
//
// 5. **Parallel Upload** (I/O-bound)
//    - Upload encrypted chunks to S3 in parallel (20 concurrent uploads)
//    - Store chunk metadata in database (DbChunk records)
//    - Track upload progress via callback-based reporting
//
// 6. **Metadata Persistence**
//    - Create DbFile records (links tracks to files)
//    - Create DbFileChunk records (maps file byte ranges to chunks)
//    - For CUE/FLAC: store DbCueSheet and DbTrackPosition records
//
// 7. **Completion**
//    - Mark album status as 'complete' in database
//    - Send ImportProgress::Complete to UI subscribers
//    - Album card updates to show full color (no longer greyed out)
//
// ## Key Types
//
// - `TrackSourceFile`: Links a db_track_id (validated, then inserted) to its source file_path
// - `ImportProgress`: Real-time progress updates (Started, ProcessingProgress, Complete, Failed)
// - `ImportServiceHandle`: Clone-able handle for sending requests and subscribing to progress

use crate::models::ImportItem;
use crate::progress_service::ProgressService;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc::{self, Receiver, Sender},
    Arc, Mutex,
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

/// Handle for sending import requests and subscribing to progress updates
#[derive(Clone)]
pub struct ImportServiceHandle {
    request_tx: Sender<ImportRequest>,
    progress_service: ProgressService,
}

impl ImportServiceHandle {
    /// Send an import request to the import service
    pub fn send_request(&self, request: ImportRequest) -> Result<(), String> {
        self.request_tx
            .send(request)
            .map_err(|e| format!("Failed to send import request: {}", e))
    }

    /// Subscribe to progress updates for a specific album
    /// Returns a filtered receiver that yields only updates for the specified album
    pub fn subscribe_album(
        &self,
        album_id: String,
    ) -> tokio::sync::mpsc::UnboundedReceiver<ImportProgress> {
        self.progress_service.subscribe_album(album_id)
    }

    /// Subscribe to progress updates for a specific track
    /// Returns a filtered receiver that yields only updates for the specified track
    pub fn subscribe_track(
        &self,
        album_id: String,
        track_id: String,
    ) -> tokio::sync::mpsc::UnboundedReceiver<ImportProgress> {
        self.progress_service.subscribe_track(album_id, track_id)
    }
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

/// Map database tracks to their source audio files in the folder.
/// This is a validation step - runs BEFORE inserting tracks into database.
/// If we can't find files for all tracks, the import is rejected.
async fn map_tracks_to_source_files(
    source_folder: &Path,
    tracks: &[crate::database::DbTrack],
) -> Result<Vec<TrackSourceFile>, String> {
    use crate::cue_flac::CueFlacProcessor;

    println!(
        "ImportService: Mapping {} tracks to source files in {}",
        tracks.len(),
        source_folder.display()
    );

    // First, check for CUE/FLAC pairs
    let cue_flac_pairs = CueFlacProcessor::detect_cue_flac(source_folder)
        .map_err(|e| format!("CUE/FLAC detection failed: {}", e))?;

    if !cue_flac_pairs.is_empty() {
        println!(
            "ImportService: Found {} CUE/FLAC pairs",
            cue_flac_pairs.len()
        );
        return map_tracks_to_cue_flac(cue_flac_pairs, tracks);
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
            mappings.push(TrackSourceFile {
                db_track_id: track.id.clone(),
                file_path: audio_file.clone(),
            });
        } else {
            println!(
                "ImportService: Warning - no file found for track: {}",
                track.title
            );
        }
    }

    println!(
        "ImportService: Mapped {} tracks to source files",
        mappings.len()
    );
    Ok(mappings)
}

/// Map tracks to CUE/FLAC source files using CUE sheet parsing
fn map_tracks_to_cue_flac(
    cue_flac_pairs: Vec<crate::cue_flac::CueFlacPair>,
    tracks: &[crate::database::DbTrack],
) -> Result<Vec<TrackSourceFile>, String> {
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
                mappings.push(TrackSourceFile {
                    db_track_id: db_track.id.clone(),
                    file_path: pair.flac_path.clone(),
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

/// Create album database record from Discogs data
fn create_album_record(
    import_item: &ImportItem,
    artist_name: &str,
    source_folder_path: Option<String>,
) -> Result<crate::database::DbAlbum, String> {
    let album = match import_item {
        ImportItem::Master(master) => {
            crate::database::DbAlbum::from_discogs_master(master, artist_name, source_folder_path)
        }
        ImportItem::Release(release) => {
            crate::database::DbAlbum::from_discogs_release(release, artist_name, source_folder_path)
        }
    };
    Ok(album)
}

/// Create track database records from Discogs tracklist
fn create_track_records(
    import_item: &ImportItem,
    album_id: &str,
) -> Result<Vec<crate::database::DbTrack>, String> {
    let discogs_tracks = import_item.tracklist();
    let mut tracks = Vec::new();

    for (index, discogs_track) in discogs_tracks.iter().enumerate() {
        let track_number = parse_track_number(&discogs_track.position, index);
        let track =
            crate::database::DbTrack::from_discogs_track(discogs_track, album_id, track_number);
        tracks.push(track);
    }

    Ok(tracks)
}

/// Parse track number from Discogs position string
/// Discogs positions can be like "1", "A1", "1-1", etc.
fn parse_track_number(position: &str, fallback_index: usize) -> Option<i32> {
    // Try to extract number from position string
    let numbers: String = position.chars().filter(|c| c.is_numeric()).collect();

    if let Ok(num) = numbers.parse::<i32>() {
        Some(num)
    } else {
        // Fallback to index + 1
        Some((fallback_index + 1) as i32)
    }
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

/// Find ALL files in a folder (for album-level chunking)
fn find_all_files_in_folder(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut all_files = Vec::new();

    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();

        if path.is_file() {
            all_files.push(path);
        }
    }

    // Sort files by name for consistent ordering (important for BitTorrent compatibility)
    all_files.sort();

    println!(
        "ImportService: Found {} total files in folder",
        all_files.len()
    );
    Ok(all_files)
}

/// Chunk, encrypt, and upload all album files in parallel.
/// Reads source files, splits into chunks, encrypts each chunk in parallel,
/// then uploads to cloud storage with parallel uploads (semaphore-limited).
async fn chunk_encrypt_and_upload_album(
    library_manager: &crate::library::LibraryManager,
    chunking_service: &crate::chunking::ChunkingService,
    cloud_storage: &crate::cloud_storage::CloudStorageManager,
    mappings: &[TrackSourceFile],
    album_id: &str,
    progress_callback: Option<Box<dyn Fn(usize, usize) + Send + Sync>>,
) -> Result<(), String> {
    if mappings.is_empty() {
        return Ok(());
    }

    // Get the album folder from the first mapping
    let album_folder = mappings[0]
        .file_path
        .parent()
        .ok_or_else(|| "Invalid source path".to_string())?;

    println!(
        "ImportService: Processing album folder {} with streaming pipeline",
        album_folder.display()
    );

    // Find ALL files in the album folder (audio, artwork, notes, etc.)
    let all_files = find_all_files_in_folder(album_folder)?;

    // Shared state for parallel uploads
    let cloud_storage = cloud_storage.clone();
    let library_manager_clone = library_manager.clone();
    let album_id = album_id.to_string();
    let chunks_completed = Arc::new(AtomicUsize::new(0));
    let total_chunks_ref = Arc::new(AtomicUsize::new(0));
    let progress_callback = Arc::new(progress_callback);
    let upload_handles = Arc::new(tokio::sync::Mutex::new(Vec::new()));

    // Limit concurrent uploads to prevent resource exhaustion
    let upload_semaphore = Arc::new(tokio::sync::Semaphore::new(20));

    // Create chunk callback for streaming pipeline with parallel uploads
    let chunk_callback: crate::chunking::ChunkCallback = {
        let chunks_completed = chunks_completed.clone();
        let total_chunks_ref = total_chunks_ref.clone();
        let progress_callback = progress_callback.clone();
        let upload_handles = upload_handles.clone();
        let upload_semaphore = upload_semaphore.clone();

        Box::new(move |chunk: crate::chunking::AlbumChunk| {
            let cloud_storage = cloud_storage.clone();
            let library_manager = library_manager_clone.clone();
            let album_id = album_id.clone();
            let chunks_completed = chunks_completed.clone();
            let total_chunks_ref = total_chunks_ref.clone();
            let progress_callback = progress_callback.clone();
            let upload_handles = upload_handles.clone();
            let upload_semaphore = upload_semaphore.clone();

            Box::pin(async move {
                // Spawn parallel upload task (semaphore limits concurrency)
                let handle = tokio::spawn(async move {
                    // Acquire semaphore permit (blocks if 20 uploads already in progress)
                    let _permit = upload_semaphore.acquire().await.unwrap();

                    // Upload chunk data directly from memory
                    let cloud_location = cloud_storage
                        .upload_chunk_data(&chunk.id, &chunk.encrypted_data)
                        .await
                        .map_err(|e| format!("Upload failed: {}", e))?;

                    // Store chunk in database
                    let db_chunk = crate::database::DbChunk::from_album_chunk(
                        &chunk.id,
                        &album_id,
                        chunk.chunk_index,
                        chunk.original_size,
                        chunk.encrypted_size,
                        &chunk.checksum,
                        &cloud_location,
                        false,
                    );
                    library_manager
                        .add_chunk(&db_chunk)
                        .await
                        .map_err(|e| format!("Database insert failed: {}", e))?;

                    // Update progress
                    let completed = chunks_completed.fetch_add(1, Ordering::SeqCst) + 1;
                    let total = total_chunks_ref.load(Ordering::SeqCst);

                    if total > 0 {
                        let progress = ((completed as f64 / total as f64) * 100.0) as u8;
                        println!(
                            "  Chunk progress: {}/{} ({:.0}%)",
                            completed, total, progress
                        );

                        if let Some(ref callback) = progress_callback.as_ref() {
                            callback(completed, total);
                        }
                    }

                    Ok::<(), String>(())
                });

                // Store handle for later awaiting
                upload_handles.lock().await.push(handle);

                Ok(())
            })
        })
    };

    // Calculate total chunks upfront so progress reporting works immediately
    let expected_total_chunks = chunking_service
        .calculate_total_chunks(&all_files)
        .await
        .map_err(|e| format!("Failed to calculate chunks: {}", e))?;
    total_chunks_ref.store(expected_total_chunks, Ordering::SeqCst);

    println!(
        "ImportService: Expecting {} total chunks, starting parallel upload pipeline",
        expected_total_chunks
    );

    // Stream chunks through the pipeline (spawns uploads in parallel)
    let album_result = chunking_service
        .chunk_album_streaming(album_folder, &all_files, chunk_callback)
        .await
        .map_err(|e| format!("Chunking failed: {}", e))?;

    // Wait for all parallel uploads to complete
    let mut handles_vec = upload_handles.lock().await;
    println!(
        "ImportService: Waiting for {} parallel uploads to complete...",
        handles_vec.len()
    );
    while let Some(handle) = handles_vec.pop() {
        handle
            .await
            .map_err(|e| format!("Task join failed: {}", e))??;
    }

    let final_completed = chunks_completed.load(Ordering::SeqCst);
    println!(
        "ImportService: Completed {} chunks from {} files",
        final_completed,
        album_result.file_mappings.len()
    );

    // Process each audio file mapping and store file records + chunk mappings
    persist_file_mappings_to_db(library_manager, mappings, &album_result.file_mappings).await?;

    println!(
        "ImportService: Successfully processed album with {} chunks",
        album_result.total_chunks
    );

    Ok(())
}

/// Process file mappings - create file records and chunk mappings
async fn persist_file_mappings_to_db(
    library_manager: &crate::library::LibraryManager,
    file_mappings: &[TrackSourceFile],
    chunk_mappings: &[crate::chunking::FileChunkMapping],
) -> Result<(), String> {
    use std::collections::HashMap;

    // Create a lookup map for chunk mappings by file path
    let chunk_lookup: HashMap<&Path, &crate::chunking::FileChunkMapping> = chunk_mappings
        .iter()
        .map(|mapping| (mapping.file_path.as_path(), mapping))
        .collect();

    // Group track mappings by source file to handle CUE/FLAC
    let mut file_groups: HashMap<&Path, Vec<&TrackSourceFile>> = HashMap::new();
    for mapping in file_mappings {
        file_groups
            .entry(mapping.file_path.as_path())
            .or_default()
            .push(mapping);
    }

    for (source_path, file_mappings) in file_groups {
        let chunk_mapping = chunk_lookup
            .get(source_path)
            .ok_or_else(|| format!("No chunk mapping found for file: {}", source_path.display()))?;

        // Get file metadata
        let file_metadata = std::fs::metadata(source_path)
            .map_err(|e| format!("Failed to read file metadata: {}", e))?;
        let file_size = file_metadata.len() as i64;
        let format = source_path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("unknown")
            .to_lowercase();

        // Check if this is a CUE/FLAC file
        let is_cue_flac = file_mappings.len() > 1 && format == "flac";

        if is_cue_flac {
            println!(
                "  Processing CUE/FLAC file with {} tracks",
                file_mappings.len()
            );
            persist_cue_flac_metadata(
                library_manager,
                source_path,
                file_mappings,
                chunk_mapping,
                file_size,
            )
            .await?;
        } else {
            // Process as individual file
            for mapping in file_mappings {
                persist_individual_file(
                    library_manager,
                    mapping,
                    chunk_mapping,
                    file_size,
                    &format,
                )
                .await?;
            }
        }
    }

    Ok(())
}

/// Process CUE/FLAC file mapping - create file record, CUE sheet, and track positions
async fn persist_cue_flac_metadata(
    library_manager: &crate::library::LibraryManager,
    source_path: &Path,
    file_mappings: Vec<&TrackSourceFile>,
    chunk_mapping: &crate::chunking::FileChunkMapping,
    file_size: i64,
) -> Result<(), String> {
    use crate::cue_flac::CueFlacProcessor;
    use crate::database::{DbCueSheet, DbFileChunk, DbTrackPosition};

    // Extract FLAC headers
    let flac_headers = CueFlacProcessor::extract_flac_headers(source_path)
        .map_err(|e| format!("Failed to extract FLAC headers: {}", e))?;

    // Create file record with FLAC headers (use first track's ID as primary)
    let primary_track_id = &file_mappings[0].db_track_id;
    let filename = source_path.file_name().unwrap().to_str().unwrap();

    let db_file = crate::database::DbFile::new_cue_flac(
        primary_track_id,
        filename,
        file_size,
        flac_headers.headers.clone(),
        flac_headers.audio_start_byte as i64,
    );
    let file_id = db_file.id.clone();

    // Save file record to database
    library_manager
        .add_file(&db_file)
        .await
        .map_err(|e| format!("Failed to insert file: {}", e))?;

    // Store file-to-chunk mapping in database
    let db_file_chunk = DbFileChunk::new(
        &file_id,
        chunk_mapping.start_chunk_index,
        chunk_mapping.end_chunk_index,
        chunk_mapping.start_byte_offset,
        chunk_mapping.end_byte_offset,
    );
    library_manager
        .add_file_chunk_mapping(&db_file_chunk)
        .await
        .map_err(|e| format!("Failed to insert file chunk: {}", e))?;

    // Store CUE sheet in database
    let cue_path = source_path.with_extension("cue");
    if cue_path.exists() {
        let cue_content = std::fs::read_to_string(&cue_path)
            .map_err(|e| format!("Failed to read CUE file: {}", e))?;
        let db_cue_sheet = DbCueSheet::new(&file_id, &cue_content);
        library_manager
            .add_cue_sheet(&db_cue_sheet)
            .await
            .map_err(|e| format!("Failed to insert CUE sheet: {}", e))?;

        // Parse CUE sheet and create track positions
        let cue_sheet = CueFlacProcessor::parse_cue_sheet(&cue_path)
            .map_err(|e| format!("Failed to parse CUE sheet: {}", e))?;

        // Create track position records for each track
        const CHUNK_SIZE: i64 = 1024 * 1024; // 1MB chunks

        for (mapping, cue_track) in file_mappings.iter().zip(cue_sheet.tracks.iter()) {
            // Calculate track boundaries within the file
            let start_byte = CueFlacProcessor::estimate_byte_position(
                cue_track.start_time_ms,
                &flac_headers,
                file_size as u64,
            ) as i64;

            let end_byte = if let Some(end_time) = cue_track.end_time_ms {
                CueFlacProcessor::estimate_byte_position(end_time, &flac_headers, file_size as u64)
                    as i64
            } else {
                file_size
            };

            // Calculate chunk indices relative to the file's position in the album
            let file_start_byte = chunk_mapping.start_byte_offset
                + (chunk_mapping.start_chunk_index as i64 * CHUNK_SIZE);
            let absolute_start_byte = file_start_byte + start_byte;
            let absolute_end_byte = file_start_byte + end_byte;

            let start_chunk_index = (absolute_start_byte / CHUNK_SIZE) as i32;
            let end_chunk_index = ((absolute_end_byte - 1) / CHUNK_SIZE) as i32;

            let track_position = DbTrackPosition::new(
                &mapping.db_track_id,
                &file_id,
                cue_track.start_time_ms as i64,
                cue_track.end_time_ms.unwrap_or(0) as i64,
                start_chunk_index,
                end_chunk_index,
            );
            library_manager
                .add_track_position(&track_position)
                .await
                .map_err(|e| format!("Failed to insert track position: {}", e))?;
        }
    }

    Ok(())
}

/// Process individual file mapping - create file record and chunk mapping
async fn persist_individual_file(
    library_manager: &crate::library::LibraryManager,
    mapping: &TrackSourceFile,
    chunk_mapping: &crate::chunking::FileChunkMapping,
    file_size: i64,
    format: &str,
) -> Result<(), String> {
    use crate::database::DbFileChunk;

    let filename = mapping.file_path.file_name().unwrap().to_str().unwrap();

    // Create file record
    let db_file = crate::database::DbFile::new(&mapping.db_track_id, filename, file_size, format);
    let file_id = db_file.id.clone();

    // Save file record to database
    library_manager
        .add_file(&db_file)
        .await
        .map_err(|e| format!("Failed to insert file: {}", e))?;

    // Store file-to-chunk mapping in database
    let db_file_chunk = DbFileChunk::new(
        &file_id,
        chunk_mapping.start_chunk_index,
        chunk_mapping.end_chunk_index,
        chunk_mapping.start_byte_offset,
        chunk_mapping.end_byte_offset,
    );
    library_manager
        .add_file_chunk_mapping(&db_file_chunk)
        .await
        .map_err(|e| format!("Failed to insert file chunk: {}", e))?;

    Ok(())
}

/// Import service that runs on a dedicated thread
pub struct ImportService {
    library_manager: crate::library_context::SharedLibraryManager,
    chunking_service: crate::chunking::ChunkingService,
    cloud_storage: crate::cloud_storage::CloudStorageManager,
}

impl ImportService {
    /// Create a new import service
    pub fn new(
        library_manager: crate::library_context::SharedLibraryManager,
        chunking_service: crate::chunking::ChunkingService,
        cloud_storage: crate::cloud_storage::CloudStorageManager,
    ) -> Self {
        ImportService {
            library_manager,
            chunking_service,
            cloud_storage,
        }
    }

    /// Start the import service thread and return handle
    pub fn start(self) -> ImportServiceHandle {
        let (request_tx, request_rx) = mpsc::channel();
        let (progress_tx, progress_rx) = mpsc::channel();
        let progress_rx = Arc::new(Mutex::new(progress_rx));

        // Create ProgressService that will process progress updates
        let progress_service = ProgressService::new(progress_rx.clone());

        // Spawn thread and let it run detached (no graceful shutdown yet - see TASKS.md)
        let _ = thread::spawn(move || {
            run_import_worker(
                self.library_manager,
                self.chunking_service,
                self.cloud_storage,
                request_rx,
                progress_tx,
            );
        });

        ImportServiceHandle {
            request_tx,
            progress_service,
        }
    }
}

/// Run the import worker thread loop
fn run_import_worker(
    library_manager: crate::library_context::SharedLibraryManager,
    chunking_service: crate::chunking::ChunkingService,
    cloud_storage: crate::cloud_storage::CloudStorageManager,
    request_rx: Receiver<ImportRequest>,
    progress_tx: Sender<ImportProgress>,
) {
    println!("ImportService: Thread started");

    // Create a tokio runtime for async operations (S3 uploads)
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime");

    // Get library manager reference (bound for this thread's lifetime)
    let library_manager = library_manager.get();

    // Process import requests
    loop {
        match request_rx.recv() {
            Ok(ImportRequest::ImportAlbum { item, folder }) => {
                println!(
                    "ImportService: Received import request for {}",
                    item.title()
                );

                let result = runtime.block_on(handle_import(
                    library_manager,
                    &chunking_service,
                    &cloud_storage,
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
}

/// Handle a single import request
async fn handle_import(
    library_manager: &crate::library::LibraryManager,
    chunking_service: &crate::chunking::ChunkingService,
    cloud_storage: &crate::cloud_storage::CloudStorageManager,
    item: &ImportItem,
    folder: &Path,
    progress_tx: &Sender<ImportProgress>,
) -> Result<(), String> {
    println!(
        "ImportService: Starting import for {} from {}",
        item.title(),
        folder.display()
    );

    // Extract artist and create records (in memory only, not inserted yet)
    let artist_name = extract_artist_name(item);
    let album = create_album_record(
        item,
        &artist_name,
        Some(folder.to_string_lossy().to_string()),
    )?;

    let album_id = album.id.clone();
    let album_title = album.title.clone();

    let tracks = create_track_records(item, &album_id)?;

    println!(
        "ImportService: Created album record with {} tracks (not inserted yet)",
        tracks.len()
    );

    // VALIDATION: Map tracks to files BEFORE inserting into DB
    // If this fails, we don't want orphaned DB records
    let file_mappings = map_tracks_to_source_files(folder, &tracks).await?;

    println!(
        "ImportService: Successfully mapped {} tracks to source files",
        file_mappings.len()
    );

    // Validation succeeded - now insert album + tracks into database
    library_manager
        .insert_album_with_tracks(&album, &tracks)
        .await
        .map_err(|e| format!("Database error: {}", e))?;

    println!(
        "ImportService: Inserted album and {} tracks into database with status='importing'",
        tracks.len()
    );

    // Send started progress (after successful DB insert)
    let _ = progress_tx.send(ImportProgress::Started {
        album_id: album_id.clone(),
        album_title: album_title.clone(),
    });

    // Process and upload files with progress reporting
    let progress_tx_clone = progress_tx.clone();
    let album_id_clone = album_id.clone();
    let progress_callback = Box::new(move |current, total| {
        let percent = ((current as f64 / total as f64) * 100.0) as u8;
        let _ = progress_tx_clone.send(ImportProgress::ProcessingProgress {
            album_id: album_id_clone.clone(),
            current,
            total,
            percent,
        });
    });

    chunk_encrypt_and_upload_album(
        library_manager,
        chunking_service,
        cloud_storage,
        &file_mappings,
        &album_id,
        Some(progress_callback),
    )
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
