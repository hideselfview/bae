use crate::chunking::{ChunkingError, ChunkingService, FileChunkMapping};
use crate::cloud_storage::{CloudStorageError, CloudStorageManager};
use crate::database::{Database, DbAlbum, DbChunk, DbFile, DbTrack};
use crate::models::ImportItem;
use std::fs;
use std::path::{Path, PathBuf};
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
    #[error("Chunking error: {0}")]
    Chunking(#[from] ChunkingError),
    #[error("Cloud storage error: {0}")]
    CloudStorage(#[from] CloudStorageError),
}

/// Progress callback for import operations
pub type ProgressCallback = Box<dyn Fn(usize, usize, String) + Send + Sync>;

/// The main library manager that coordinates all import operations
///
/// This implements the import workflow described in BAE_IMPORT_WORKFLOW.md:
/// 1. Take Discogs metadata and local folder path
/// 2. Map audio files to tracks (using AI eventually)
/// 3. Process files into encrypted chunks
/// 4. Store metadata and file mappings in database
/// 5. Upload chunks to cloud storage
#[derive(Debug)]
pub struct LibraryManager {
    database: Database,
    chunking_service: ChunkingService,
    cloud_storage: CloudStorageManager,
}

impl LibraryManager {
    /// Create a new library manager with all dependencies injected
    pub fn new(
        database: Database,
        chunking_service: ChunkingService,
        cloud_storage: CloudStorageManager,
    ) -> Self {
        LibraryManager {
            database,
            chunking_service,
            cloud_storage,
        }
    }

    /// Import an album from Discogs metadata and local folder
    /// This is the main import function called from the UI
    pub async fn import_album(
        &self,
        import_item: &ImportItem,
        source_folder: &Path,
    ) -> Result<String, LibraryError> {
        println!(
            "LibraryManager: Starting import for {} from {}",
            import_item.title(),
            source_folder.display()
        );

        // Step 1: Extract artist name from Discogs data
        let artist_name = self.extract_artist_name(import_item)?;

        // Step 2: Create album record
        let source_folder_path = Some(source_folder.to_string_lossy().to_string());
        let album = self.create_album_record(import_item, &artist_name, source_folder_path)?;
        let album_id = album.id.clone();

        // Step 3: Create track records from Discogs tracklist
        let tracks = self.create_track_records(import_item, &album_id)?;

        // Step 4: Find and map audio files to tracks
        let file_mappings = self.map_files_to_tracks(source_folder, &tracks).await?;

        // Step 5: Process files (chunking, encryption, upload to cloud)
        // Do this BEFORE database inserts so we can rollback if it fails
        println!("LibraryManager: Processing and uploading files (this may take several minutes for large albums)...");
        self.process_audio_files(&file_mappings, &album_id).await?;

        // Step 6: Save to database AFTER successful upload
        // This ensures we don't have database records for albums that failed to upload
        println!("LibraryManager: Upload complete, saving to database...");
        self.database.insert_album(&album).await?;

        for track in &tracks {
            self.database.insert_track(track).await?;
        }

        println!(
            "LibraryManager: Successfully imported album {} with {} tracks",
            album.title,
            tracks.len()
        );

        Ok(album_id)
    }

    /// Extract artist name from Discogs data
    /// TODO: Handle multiple artists, featured artists, etc.
    fn extract_artist_name(&self, import_item: &ImportItem) -> Result<String, LibraryError> {
        // For now, extract from the title field which usually contains "Artist - Album"
        // In the future, we'll use the proper artists field from Discogs
        let title = import_item.title();

        if let Some(dash_pos) = title.find(" - ") {
            Ok(title[..dash_pos].to_string())
        } else {
            // Fallback: use "Various Artists" or extract from first track
            Ok("Unknown Artist".to_string())
        }
    }

    /// Create album database record from Discogs data
    pub fn create_album_record(
        &self,
        import_item: &ImportItem,
        artist_name: &str,
        source_folder_path: Option<String>,
    ) -> Result<DbAlbum, LibraryError> {
        let album = match import_item {
            ImportItem::Master(master) => {
                DbAlbum::from_discogs_master(master, artist_name, source_folder_path)
            }
            ImportItem::Release(release) => {
                DbAlbum::from_discogs_release(release, artist_name, source_folder_path)
            }
        };

        Ok(album)
    }

    /// Create track database records from Discogs tracklist
    pub fn create_track_records(
        &self,
        import_item: &ImportItem,
        album_id: &str,
    ) -> Result<Vec<DbTrack>, LibraryError> {
        let discogs_tracks = import_item.tracklist();
        let mut tracks = Vec::new();

        for (index, discogs_track) in discogs_tracks.iter().enumerate() {
            let track_number = self.parse_track_number(&discogs_track.position, index);
            let track = DbTrack::from_discogs_track(discogs_track, album_id, track_number);
            tracks.push(track);
        }

        Ok(tracks)
    }

    /// Parse track number from Discogs position string
    /// Discogs positions can be like "1", "A1", "1-1", etc.
    fn parse_track_number(&self, position: &str, fallback_index: usize) -> Option<i32> {
        // Try to extract number from position string
        let numbers: String = position.chars().filter(|c| c.is_numeric()).collect();

        if let Ok(num) = numbers.parse::<i32>() {
            Some(num)
        } else {
            // Fallback to index + 1
            Some((fallback_index + 1) as i32)
        }
    }

    /// Map audio files in source folder to tracks
    /// This is where AI will eventually be used for smart matching
    pub async fn map_files_to_tracks(
        &self,
        source_folder: &Path,
        tracks: &[DbTrack],
    ) -> Result<Vec<FileMapping>, LibraryError> {
        use crate::cue_flac::CueFlacProcessor;

        println!(
            "LibraryManager: Mapping files in {} to {} tracks",
            source_folder.display(),
            tracks.len()
        );

        // First, check for CUE/FLAC pairs
        let cue_flac_pairs = CueFlacProcessor::detect_cue_flac(source_folder)
            .map_err(|e| LibraryError::TrackMapping(format!("CUE/FLAC detection failed: {}", e)))?;

        if !cue_flac_pairs.is_empty() {
            println!(
                "LibraryManager: Found {} CUE/FLAC pairs",
                cue_flac_pairs.len()
            );
            return self.map_cue_flac_to_tracks(cue_flac_pairs, tracks).await;
        }

        // Fallback to individual audio files
        let audio_files = self.find_audio_files(source_folder)?;

        if audio_files.is_empty() {
            return Err(LibraryError::TrackMapping(
                "No audio files found in source folder".to_string(),
            ));
        }

        // Simple mapping strategy for now: sort files by name and match to track order
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
                    "LibraryManager: Warning - no file found for track: {}",
                    track.title
                );
            }
        }

        println!("LibraryManager: Mapped {} files to tracks", mappings.len());
        Ok(mappings)
    }

    /// Map CUE/FLAC pairs to tracks using CUE sheet parsing
    async fn map_cue_flac_to_tracks(
        &self,
        cue_flac_pairs: Vec<crate::cue_flac::CueFlacPair>,
        tracks: &[DbTrack],
    ) -> Result<Vec<FileMapping>, LibraryError> {
        use crate::cue_flac::CueFlacProcessor;

        let mut mappings = Vec::new();

        for pair in cue_flac_pairs {
            println!(
                "LibraryManager: Processing CUE/FLAC pair: {} + {}",
                pair.flac_path.display(),
                pair.cue_path.display()
            );

            // Parse the CUE sheet
            let cue_sheet = CueFlacProcessor::parse_cue_sheet(&pair.cue_path).map_err(|e| {
                LibraryError::TrackMapping(format!("Failed to parse CUE sheet: {}", e))
            })?;

            println!(
                "LibraryManager: CUE sheet contains {} tracks",
                cue_sheet.tracks.len()
            );

            // For CUE/FLAC, all tracks map to the same FLAC file
            // We'll create one mapping per track, all pointing to the same FLAC file
            for (index, cue_track) in cue_sheet.tracks.iter().enumerate() {
                if let Some(db_track) = tracks.get(index) {
                    mappings.push(FileMapping {
                        track_id: db_track.id.clone(),
                        source_path: pair.flac_path.clone(),
                    });

                    println!(
                        "LibraryManager: Mapped CUE track '{}' to DB track '{}'",
                        cue_track.title, db_track.title
                    );
                } else {
                    println!(
                        "LibraryManager: Warning - CUE track '{}' has no corresponding DB track",
                        cue_track.title
                    );
                }
            }
        }

        println!(
            "LibraryManager: Created {} CUE/FLAC mappings",
            mappings.len()
        );
        Ok(mappings)
    }

    /// Find all audio files in a directory
    fn find_audio_files(&self, dir: &Path) -> Result<Vec<PathBuf>, LibraryError> {
        let mut audio_files = Vec::new();
        let audio_extensions = ["mp3", "flac", "wav", "m4a", "aac", "ogg"];

        for entry in fs::read_dir(dir)? {
            let entry = entry?;
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

        println!("LibraryManager: Found {} audio files", audio_files.len());
        Ok(audio_files)
    }

    /// Find ALL files in a folder (for album-level chunking)
    fn find_all_files_in_folder(&self, dir: &Path) -> Result<Vec<PathBuf>, LibraryError> {
        let mut all_files = Vec::new();

        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() {
                all_files.push(path);
            }
        }

        // Sort files by name for consistent ordering (important for BitTorrent compatibility)
        all_files.sort();

        println!(
            "LibraryManager: Found {} total files in folder",
            all_files.len()
        );
        Ok(all_files)
    }

    /// Process audio files using streaming chunk pipeline - chunk, encrypt and upload in parallel
    pub async fn process_audio_files_with_progress(
        &self,
        mappings: &[FileMapping],
        album_id: &str,
        progress_callback: Option<ProgressCallback>,
    ) -> Result<(), LibraryError> {
        if mappings.is_empty() {
            return Ok(());
        }

        // Get the album folder from the first mapping
        let album_folder = mappings[0]
            .source_path
            .parent()
            .ok_or_else(|| LibraryError::TrackMapping("Invalid source path".to_string()))?;

        println!(
            "LibraryManager: Processing album folder {} with streaming pipeline",
            album_folder.display()
        );

        // Find ALL files in the album folder (audio, artwork, notes, etc.)
        let all_files = self.find_all_files_in_folder(album_folder)?;
        println!(
            "LibraryManager: Found {} total files in album folder",
            all_files.len()
        );

        // Shared state for parallel uploads
        let cloud_storage = self.cloud_storage.clone();
        let database = self.database.clone();
        let album_id = album_id.to_string();
        let chunks_completed = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let total_chunks_ref = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let progress_callback = std::sync::Arc::new(progress_callback);
        let upload_handles = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));

        // Create chunk callback for streaming pipeline with parallel uploads
        let chunk_callback: crate::chunking::ChunkCallback = {
            let chunks_completed = chunks_completed.clone();
            let total_chunks_ref = total_chunks_ref.clone();
            let progress_callback = progress_callback.clone();
            let upload_handles = upload_handles.clone();

            Box::new(move |chunk: crate::chunking::AlbumChunk| {
                let cloud_storage = cloud_storage.clone();
                let database = database.clone();
                let album_id = album_id.clone();
                let chunks_completed = chunks_completed.clone();
                let total_chunks_ref = total_chunks_ref.clone();
                let progress_callback = progress_callback.clone();
                let upload_handles = upload_handles.clone();

                Box::pin(async move {
                    // Spawn parallel upload task
                    let handle = tokio::spawn(async move {
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
                        database
                            .insert_chunk(&db_chunk)
                            .await
                            .map_err(|e| format!("Database insert failed: {}", e))?;

                        // Update progress
                        let completed =
                            chunks_completed.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                        let total = total_chunks_ref.load(std::sync::atomic::Ordering::SeqCst);

                        if total > 0 {
                            let progress = ((completed as f64 / total as f64) * 100.0) as u8;
                            println!(
                                "  Chunk progress: {}/{} ({:.0}%)",
                                completed, total, progress
                            );

                            if let Some(ref callback) = progress_callback.as_ref() {
                                callback(completed, total, "processing".to_string());
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

        // Stream chunks through the pipeline (spawns uploads in parallel)
        let album_result = self
            .chunking_service
            .chunk_album_streaming(album_folder, &all_files, chunk_callback)
            .await?;

        // Update total chunks for progress tracking
        total_chunks_ref.store(
            album_result.total_chunks,
            std::sync::atomic::Ordering::SeqCst,
        );

        // Wait for all parallel uploads to complete
        let mut handles_vec = upload_handles.lock().await;
        println!(
            "LibraryManager: Waiting for {} parallel uploads to complete...",
            handles_vec.len()
        );
        while let Some(handle) = handles_vec.pop() {
            handle
                .await
                .map_err(|e| LibraryError::Import(format!("Task join failed: {}", e)))?
                .map_err(LibraryError::Import)?;
        }

        let final_completed = chunks_completed.load(std::sync::atomic::Ordering::SeqCst);
        println!(
            "LibraryManager: Completed {} chunks from {} files",
            final_completed,
            album_result.file_mappings.len()
        );

        // Process each audio file mapping and store file records + chunk mappings
        self.process_file_mappings(mappings, &album_result.file_mappings)
            .await?;

        println!(
            "LibraryManager: Successfully processed album with {} chunks",
            album_result.total_chunks
        );

        Ok(())
    }

    /// Process audio files using album-level chunking (without progress callback)
    pub async fn process_audio_files(
        &self,
        mappings: &[FileMapping],
        album_id: &str,
    ) -> Result<(), LibraryError> {
        self.process_audio_files_with_progress(mappings, album_id, None)
            .await
    }

    /// Process file mappings - create file records and chunk mappings
    async fn process_file_mappings(
        &self,
        file_mappings: &[FileMapping],
        chunk_mappings: &[FileChunkMapping],
    ) -> Result<(), LibraryError> {
        use std::collections::HashMap;

        // Create a lookup map for chunk mappings by file path
        let chunk_lookup: HashMap<&Path, &FileChunkMapping> = chunk_mappings
            .iter()
            .map(|mapping| (mapping.file_path.as_path(), mapping))
            .collect();

        // Group track mappings by source file to handle CUE/FLAC
        let mut file_groups: HashMap<&Path, Vec<&FileMapping>> = HashMap::new();
        for mapping in file_mappings {
            file_groups
                .entry(mapping.source_path.as_path())
                .or_default()
                .push(mapping);
        }

        for (source_path, file_mappings) in file_groups {
            let chunk_mapping = chunk_lookup.get(source_path).ok_or_else(|| {
                LibraryError::TrackMapping(format!(
                    "No chunk mapping found for file: {}",
                    source_path.display()
                ))
            })?;

            // Get file metadata
            let file_metadata = fs::metadata(source_path)?;
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
                self.process_cue_flac_mapping(source_path, file_mappings, chunk_mapping, file_size)
                    .await?;
            } else {
                // Process as individual file
                for mapping in file_mappings {
                    self.process_individual_mapping(mapping, chunk_mapping, file_size, &format)
                        .await?;
                }
            }
        }

        Ok(())
    }

    /// Process CUE/FLAC file mapping - create file record, CUE sheet, and track positions
    async fn process_cue_flac_mapping(
        &self,
        source_path: &Path,
        file_mappings: Vec<&FileMapping>,
        chunk_mapping: &FileChunkMapping,
        file_size: i64,
    ) -> Result<(), LibraryError> {
        use crate::cue_flac::CueFlacProcessor;
        use crate::database::{DbCueSheet, DbFileChunk, DbTrackPosition};

        // Extract FLAC headers
        let flac_headers = CueFlacProcessor::extract_flac_headers(source_path).map_err(|e| {
            LibraryError::TrackMapping(format!("Failed to extract FLAC headers: {}", e))
        })?;

        // Create file record with FLAC headers (use first track's ID as primary)
        let primary_track_id = &file_mappings[0].track_id;
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
        self.database.insert_file(&db_file).await?;

        // Store file-to-chunk mapping in database
        let db_file_chunk = DbFileChunk::new(
            &file_id,
            chunk_mapping.start_chunk_index,
            chunk_mapping.end_chunk_index,
            chunk_mapping.start_byte_offset,
            chunk_mapping.end_byte_offset,
        );
        self.database.insert_file_chunk(&db_file_chunk).await?;

        // Store CUE sheet in database
        let cue_path = source_path.with_extension("cue");
        if cue_path.exists() {
            let cue_content = std::fs::read_to_string(&cue_path)?;
            let db_cue_sheet = DbCueSheet::new(&file_id, &cue_content);
            self.database.insert_cue_sheet(&db_cue_sheet).await?;

            // Parse CUE sheet and create track positions
            let cue_sheet = CueFlacProcessor::parse_cue_sheet(&cue_path).map_err(|e| {
                LibraryError::TrackMapping(format!("Failed to parse CUE sheet: {}", e))
            })?;

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
                    CueFlacProcessor::estimate_byte_position(
                        end_time,
                        &flac_headers,
                        file_size as u64,
                    ) as i64
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
                    &mapping.track_id,
                    &file_id,
                    cue_track.start_time_ms as i64,
                    cue_track.end_time_ms.unwrap_or(0) as i64,
                    start_chunk_index,
                    end_chunk_index,
                );
                self.database.insert_track_position(&track_position).await?;
            }
        }

        Ok(())
    }

    /// Process individual file mapping - create file record and chunk mapping
    async fn process_individual_mapping(
        &self,
        mapping: &FileMapping,
        chunk_mapping: &FileChunkMapping,
        file_size: i64,
        format: &str,
    ) -> Result<(), LibraryError> {
        use crate::database::DbFileChunk;

        let filename = mapping.source_path.file_name().unwrap().to_str().unwrap();

        // Create file record
        let db_file = crate::database::DbFile::new(&mapping.track_id, filename, file_size, format);
        let file_id = db_file.id.clone();

        // Save file record to database
        self.database.insert_file(&db_file).await?;

        // Store file-to-chunk mapping in database
        let db_file_chunk = DbFileChunk::new(
            &file_id,
            chunk_mapping.start_chunk_index,
            chunk_mapping.end_chunk_index,
            chunk_mapping.start_byte_offset,
            chunk_mapping.end_byte_offset,
        );
        self.database.insert_file_chunk(&db_file_chunk).await?;

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

    /// Get files for a specific track
    pub async fn get_files_for_track(&self, track_id: &str) -> Result<Vec<DbFile>, LibraryError> {
        Ok(self.database.get_files_for_track(track_id).await?)
    }

    /// Get chunks for a specific file
    pub async fn get_chunks_for_file(&self, file_id: &str) -> Result<Vec<DbChunk>, LibraryError> {
        Ok(self.database.get_chunks_for_file(file_id).await?)
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

/// Represents a mapping between a track and its source audio file
#[derive(Debug, Clone)]
pub struct FileMapping {
    pub track_id: String,
    pub source_path: PathBuf,
}
