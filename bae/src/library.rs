use crate::database::{Database, DbAlbum, DbTrack, DbFile, DbChunk};
use crate::models::ImportItem;
use crate::chunking::{ChunkingService, ChunkingError, FileChunkMapping};
use crate::cloud_storage::{CloudStorageManager, CloudStorageError};
use std::path::{Path, PathBuf};
use std::fs;
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

/// The main library manager that coordinates all import operations
/// 
/// This implements the import workflow described in BAE_IMPORT_WORKFLOW.md:
/// 1. Take Discogs metadata and local folder path
/// 2. Map audio files to tracks (using AI eventually)
/// 3. Process files into encrypted chunks
/// 4. Store metadata and file mappings in database
/// 5. Upload chunks to cloud storage
pub struct LibraryManager {
    database: Database,
    library_path: PathBuf,
    chunking_service: ChunkingService,
    cloud_storage: Option<CloudStorageManager>,
}

impl LibraryManager {
    /// Create a new library manager
    pub async fn new(library_path: PathBuf) -> Result<Self, LibraryError> {
        // Ensure library directory exists
        println!("LibraryManager: Creating library directory: {}", library_path.display());
        tokio::fs::create_dir_all(&library_path).await?;
        
        // Initialize database
        let db_path = library_path.join("library.db");
        println!("LibraryManager: Initializing database at: {}", db_path.display());
        let database = Database::new(db_path.to_str().unwrap()).await?;
        
        // Initialize chunking service
        let chunking_service = ChunkingService::new()?;
        
        Ok(LibraryManager {
            database,
            library_path,
            chunking_service,
            cloud_storage: None, // Cloud storage is optional
        })
    }

    /// Enable cloud storage with the given manager
    pub fn enable_cloud_storage(&mut self, cloud_storage: CloudStorageManager) {
        self.cloud_storage = Some(cloud_storage);
    }

    /// Check if cloud storage is enabled
    pub fn has_cloud_storage(&self) -> bool {
        self.cloud_storage.is_some()
    }

    /// Create a new library manager with in-memory database for testing
    #[cfg(test)]
    pub async fn new_for_testing() -> Result<Self, LibraryError> {
        // Initialize in-memory database
        let database = Database::new_in_memory().await?;
        
        // Initialize chunking service for testing (avoids keychain access)
        let chunking_service = ChunkingService::new_for_testing()?;
        
        Ok(LibraryManager {
            database,
            library_path: std::env::temp_dir().join("bae_test"),
            chunking_service,
            cloud_storage: None,
        })
    }

    /// Try to configure cloud storage from environment variables
    pub async fn try_configure_cloud_storage(&mut self) -> Result<bool, LibraryError> {
        use crate::cloud_storage::S3Config;
        
        match S3Config::from_env() {
            Ok(config) => {
                println!("LibraryManager: Configuring S3 cloud storage (bucket: {})", config.bucket_name);
                let cloud_storage = CloudStorageManager::new_s3(config).await?;
                self.enable_cloud_storage(cloud_storage);
                Ok(true)
            }
            Err(_) => {
                println!("LibraryManager: No cloud storage configuration found, using local storage only");
                Ok(false)
            }
        }
    }

    /// Import an album from Discogs metadata and local folder
    /// This is the main import function called from the UI
    pub async fn import_album(
        &self, 
        import_item: &ImportItem, 
        source_folder: &Path
    ) -> Result<String, LibraryError> {
        println!("LibraryManager: Starting import for {} from {}", 
                import_item.title(), source_folder.display());

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
        
        // Step 5: Save to database
        self.database.insert_album(&album).await?;
        
        for track in &tracks {
            self.database.insert_track(track).await?;
        }
        
        // Step 6: Process files (chunking, encryption, storage will be added later)
        self.process_audio_files(&file_mappings, &album_id).await?;
        
        println!("LibraryManager: Successfully imported album {} with {} tracks", 
                album.title, tracks.len());
        
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
    fn create_album_record(&self, import_item: &ImportItem, artist_name: &str, source_folder_path: Option<String>) -> Result<DbAlbum, LibraryError> {
        let album = match import_item {
            ImportItem::Master(master) => DbAlbum::from_discogs_master(master, artist_name, source_folder_path),
            ImportItem::Release(release) => DbAlbum::from_discogs_release(release, artist_name, source_folder_path),
        };
        
        Ok(album)
    }

    /// Create track database records from Discogs tracklist
    fn create_track_records(&self, import_item: &ImportItem, album_id: &str) -> Result<Vec<DbTrack>, LibraryError> {
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
    async fn map_files_to_tracks(
        &self, 
        source_folder: &Path, 
        tracks: &[DbTrack]
    ) -> Result<Vec<FileMapping>, LibraryError> {
        use crate::cue_flac::CueFlacProcessor;
        
        println!("LibraryManager: Mapping files in {} to {} tracks", 
                source_folder.display(), tracks.len());

        // First, check for CUE/FLAC pairs
        let cue_flac_pairs = CueFlacProcessor::detect_cue_flac(source_folder)
            .map_err(|e| LibraryError::TrackMapping(format!("CUE/FLAC detection failed: {}", e)))?;
        
        if !cue_flac_pairs.is_empty() {
            println!("LibraryManager: Found {} CUE/FLAC pairs", cue_flac_pairs.len());
            return self.map_cue_flac_to_tracks(cue_flac_pairs, tracks).await;
        }

        // Fallback to individual audio files
        let audio_files = self.find_audio_files(source_folder)?;
        
        if audio_files.is_empty() {
            return Err(LibraryError::TrackMapping(
                "No audio files found in source folder".to_string()
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
                    track_title: track.title.clone(),
                });
            } else {
                println!("LibraryManager: Warning - no file found for track: {}", track.title);
            }
        }
        
        println!("LibraryManager: Mapped {} files to tracks", mappings.len());
        Ok(mappings)
    }

    /// Map CUE/FLAC pairs to tracks using CUE sheet parsing
    async fn map_cue_flac_to_tracks(
        &self,
        cue_flac_pairs: Vec<crate::cue_flac::CueFlacPair>,
        tracks: &[DbTrack]
    ) -> Result<Vec<FileMapping>, LibraryError> {
        use crate::cue_flac::CueFlacProcessor;
        
        let mut mappings = Vec::new();
        
        for pair in cue_flac_pairs {
            println!("LibraryManager: Processing CUE/FLAC pair: {} + {}", 
                    pair.flac_path.display(), pair.cue_path.display());
            
            // Parse the CUE sheet
            let cue_sheet = CueFlacProcessor::parse_cue_sheet(&pair.cue_path)
                .map_err(|e| LibraryError::TrackMapping(format!("Failed to parse CUE sheet: {}", e)))?;
            
            println!("LibraryManager: CUE sheet contains {} tracks", cue_sheet.tracks.len());
            
            // For CUE/FLAC, all tracks map to the same FLAC file
            // We'll create one mapping per track, all pointing to the same FLAC file
            for (index, cue_track) in cue_sheet.tracks.iter().enumerate() {
                if let Some(db_track) = tracks.get(index) {
                    mappings.push(FileMapping {
                        track_id: db_track.id.clone(),
                        source_path: pair.flac_path.clone(),
                        track_title: db_track.title.clone(),
                    });
                    
                    println!("LibraryManager: Mapped CUE track '{}' to DB track '{}'", 
                            cue_track.title, db_track.title);
                } else {
                    println!("LibraryManager: Warning - CUE track '{}' has no corresponding DB track", 
                            cue_track.title);
                }
            }
        }
        
        println!("LibraryManager: Created {} CUE/FLAC mappings", mappings.len());
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
        
        println!("LibraryManager: Found {} total files in folder", all_files.len());
        Ok(all_files)
    }

    /// Process audio files using album-level chunking - chunk entire album folder, encrypt, and store metadata
    async fn process_audio_files(&self, mappings: &[FileMapping], album_id: &str) -> Result<(), LibraryError> {
        use crate::cue_flac::CueFlacProcessor;
        use crate::database::{DbCueSheet, DbTrackPosition, DbFileChunk};
        use crate::chunking::{AlbumChunkingResult, FileChunkMapping};
        use std::collections::HashMap;
        
        if mappings.is_empty() {
            return Ok(());
        }
        
        // Get the album folder from the first mapping
        let album_folder = mappings[0].source_path.parent()
            .ok_or_else(|| LibraryError::TrackMapping("Invalid source path".to_string()))?;
            
        println!("LibraryManager: Processing album folder {} with album-level chunking", album_folder.display());
        
        // Find ALL files in the album folder (audio, artwork, notes, etc.)
        let all_files = self.find_all_files_in_folder(album_folder)?;
        println!("LibraryManager: Found {} total files in album folder", all_files.len());
        
        // Create temporary directory for chunking
        let temp_dir = std::env::temp_dir().join("bae_album_chunks");
        tokio::fs::create_dir_all(&temp_dir).await?;
        
        // Chunk the entire album folder
        let album_result: AlbumChunkingResult = self.chunking_service
            .chunk_album(album_folder, &all_files, &temp_dir)
            .await?;
            
        println!("LibraryManager: Created {} album chunks from {} files", 
                album_result.chunks.len(), album_result.file_mappings.len());
        
        // album_id is now passed as parameter
        
        // Upload chunks to cloud storage and store in database
        let cloud_storage = self.cloud_storage.as_ref()
            .ok_or_else(|| LibraryError::CloudStorage(
                crate::cloud_storage::CloudStorageError::Config("Cloud storage not configured - required for import".to_string())
            ))?;
            
        for chunk in &album_result.chunks {
            println!("  Uploading album chunk {} to cloud storage", chunk.id);
            let cloud_location = cloud_storage.upload_chunk_file(&chunk.id, &chunk.final_path).await?;
            
            // Clean up temp file after successful upload
            if let Err(e) = tokio::fs::remove_file(&chunk.final_path).await {
                println!("    Warning: Failed to clean up temp file {}: {}", chunk.final_path.display(), e);
            }
            
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
            self.database.insert_chunk(&db_chunk).await?;
        }
        
        // Process each audio file mapping and store file records + chunk mappings
        self.process_file_mappings(mappings, &album_result.file_mappings).await?;
        
        // Clean up temp directory
        if let Err(e) = tokio::fs::remove_dir_all(&temp_dir).await {
            println!("Warning: Failed to clean up temp directory {}: {}", temp_dir.display(), e);
        }
        
        println!("LibraryManager: Successfully processed album with {} chunks and {} file mappings", 
                album_result.chunks.len(), album_result.file_mappings.len());
        
        Ok(())
    }

    /// Process file mappings - create file records and chunk mappings
    async fn process_file_mappings(
        &self,
        track_mappings: &[FileMapping],
        chunk_mappings: &[FileChunkMapping],
    ) -> Result<(), LibraryError> {
        use crate::cue_flac::CueFlacProcessor;
        use crate::database::{DbCueSheet, DbTrackPosition, DbFileChunk};
        use std::collections::HashMap;
        
        // Create a lookup map for chunk mappings by file path
        let chunk_lookup: HashMap<&Path, &FileChunkMapping> = chunk_mappings
            .iter()
            .map(|mapping| (mapping.file_path.as_path(), mapping))
            .collect();
        
        // Group track mappings by source file to handle CUE/FLAC
        let mut file_groups: HashMap<&Path, Vec<&FileMapping>> = HashMap::new();
        for mapping in track_mappings {
            file_groups.entry(mapping.source_path.as_path()).or_default().push(mapping);
        }
        
        for (source_path, file_mappings) in file_groups {
            let chunk_mapping = chunk_lookup.get(source_path)
                .ok_or_else(|| LibraryError::TrackMapping(
                    format!("No chunk mapping found for file: {}", source_path.display())
                ))?;
            
            // Get file metadata
            let file_metadata = fs::metadata(source_path)?;
            let file_size = file_metadata.len() as i64;
            let format = source_path
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("unknown")
                .to_lowercase();
            let filename = source_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("unknown");
            
            // Check if this is a CUE/FLAC file
            let is_cue_flac = file_mappings.len() > 1 && format == "flac";
            
            if is_cue_flac {
                println!("  Processing CUE/FLAC file with {} tracks", file_mappings.len());
                self.process_cue_flac_mapping(source_path, file_mappings, chunk_mapping, file_size).await?;
            } else {
                // Process as individual file
                for mapping in file_mappings {
                    self.process_individual_mapping(mapping, chunk_mapping, file_size, &format).await?;
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
        use crate::database::{DbCueSheet, DbTrackPosition, DbFileChunk};
        
        // Extract FLAC headers
        let flac_headers = CueFlacProcessor::extract_flac_headers(source_path)
            .map_err(|e| LibraryError::TrackMapping(format!("Failed to extract FLAC headers: {}", e)))?;
        
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
            let cue_sheet = CueFlacProcessor::parse_cue_sheet(&cue_path)
                .map_err(|e| LibraryError::TrackMapping(format!("Failed to parse CUE sheet: {}", e)))?;
                
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
                let file_start_byte = chunk_mapping.start_byte_offset + (chunk_mapping.start_chunk_index as i64 * CHUNK_SIZE);
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
        let db_file = crate::database::DbFile::new(
            &mapping.track_id,
            filename,
            file_size,
            format,
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
    pub async fn get_track_position(&self, track_id: &str) -> Result<Option<crate::database::DbTrackPosition>, LibraryError> {
        Ok(self.database.get_track_position(track_id).await?)
    }

    /// Get chunks in a specific range for CUE/FLAC streaming
    pub async fn get_chunks_in_range(&self, album_id: &str, chunk_range: std::ops::RangeInclusive<i32>) -> Result<Vec<DbChunk>, LibraryError> {
        Ok(self.database.get_chunks_in_range(album_id, chunk_range).await?)
    }

    /// Get album ID for a track
    pub async fn get_album_id_for_track(&self, track_id: &str) -> Result<String, LibraryError> {
        let tracks = self.database.get_tracks_for_album("").await?; // This won't work
        // For now, let's add a proper database method
        // In the meantime, we'll use a workaround
        let albums = self.database.get_albums().await?;
        for album in albums {
            let tracks = self.database.get_tracks_for_album(&album.id).await?;
            if tracks.iter().any(|t| t.id == track_id) {
                return Ok(album.id);
            }
        }
        Err(LibraryError::TrackMapping("Track not found in any album".to_string()))
    }

    /// Process a CUE/FLAC file with multiple tracks
    async fn process_cue_flac_file(
        &self,
        source_path: &Path,
        file_mappings: Vec<&FileMapping>,
        file_size: i64,
    ) -> Result<(), LibraryError> {
        use crate::cue_flac::CueFlacProcessor;
        use crate::database::{DbCueSheet, DbTrackPosition};
        
        println!("  Processing CUE/FLAC file: {} bytes", file_size);
        
        // Extract FLAC headers
        let flac_headers = CueFlacProcessor::extract_flac_headers(source_path)
            .map_err(|e| LibraryError::TrackMapping(format!("Failed to extract FLAC headers: {}", e)))?;
        
        println!("  Extracted FLAC headers: {} bytes, audio starts at byte {}", 
                flac_headers.headers.len(), flac_headers.audio_start_byte);
        
        // Find the corresponding CUE file
        let cue_path = source_path.with_extension("cue");
        if !cue_path.exists() {
            return Err(LibraryError::TrackMapping(
                format!("CUE file not found: {}", cue_path.display())
            ));
        }
        
        // Parse CUE sheet
        let cue_sheet = CueFlacProcessor::parse_cue_sheet(&cue_path)
            .map_err(|e| LibraryError::TrackMapping(format!("Failed to parse CUE sheet: {}", e)))?;
        
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
        
        // Store CUE sheet in database
        let cue_content = std::fs::read_to_string(&cue_path)?;
        let db_cue_sheet = DbCueSheet::new(&file_id, &cue_content);
        self.database.insert_cue_sheet(&db_cue_sheet).await?;
        
        // Chunk the entire FLAC file once
        println!("  Chunking entire FLAC file");
        let temp_dir = std::env::temp_dir().join("bae_import_chunks");
        tokio::fs::create_dir_all(&temp_dir).await?;
        let chunks = self.chunking_service
            .chunk_file(source_path, &file_id, &temp_dir)
            .await?;
        
        println!("  Created {} chunks for entire file", chunks.len());
        
        // Upload chunks to cloud storage
        let cloud_storage = self.cloud_storage.as_ref()
            .ok_or_else(|| LibraryError::CloudStorage(
                crate::cloud_storage::CloudStorageError::Config("Cloud storage not configured - required for import".to_string())
            ))?;
        
        for chunk in &chunks {
            println!("    Uploading chunk {} to cloud storage", chunk.id);
            let cloud_location = cloud_storage.upload_chunk_file(&chunk.id, &chunk.final_path).await?;
            
            // Clean up temp file after successful upload
            if let Err(e) = tokio::fs::remove_file(&chunk.final_path).await {
                println!("    Warning: Failed to clean up temp file {}: {}", chunk.final_path.display(), e);
            }
            
            // Store only cloud location in database
            let db_chunk = crate::database::DbChunk::from_album_chunk(
                &chunk.id,
                &primary_track_id, // TODO: This should be album_id, not track_id
                chunk.chunk_index,
                chunk.original_size,
                chunk.encrypted_size,
                &chunk.checksum,
                &cloud_location,
                false,
            );
            self.database.insert_chunk(&db_chunk).await?;
        }
        
        // Create track position records for each track
        const CHUNK_SIZE: i64 = 1024 * 1024; // 1MB chunks
        
        for (index, (mapping, cue_track)) in file_mappings.iter().zip(cue_sheet.tracks.iter()).enumerate() {
            // Calculate byte positions
            let start_byte = CueFlacProcessor::estimate_byte_position(
                cue_track.start_time_ms,
                &flac_headers,
                file_size as u64,
            ) as i64;
            
            let end_byte = if let Some(end_time_ms) = cue_track.end_time_ms {
                CueFlacProcessor::estimate_byte_position(
                    end_time_ms,
                    &flac_headers,
                    file_size as u64,
                ) as i64
            } else {
                file_size // Last track goes to end of file
            };
            
            // Calculate chunk indices (relative to audio start)
            let audio_start = flac_headers.audio_start_byte as i64;
            let start_chunk_index = ((start_byte - audio_start).max(0) / CHUNK_SIZE) as i32;
            let end_chunk_index = ((end_byte - audio_start).max(0) / CHUNK_SIZE) as i32;
            
            // Create track position record
            let track_position = DbTrackPosition::new(
                &mapping.track_id,
                &file_id,
                cue_track.start_time_ms as i64,
                cue_track.end_time_ms.unwrap_or(0) as i64,
                start_chunk_index,
                end_chunk_index,
            );
            
            self.database.insert_track_position(&track_position).await?;
            
            println!("  Created track position for '{}': chunks {}-{}", 
                    mapping.track_title, start_chunk_index, end_chunk_index);
        }
        
        println!("  Successfully processed CUE/FLAC file with {} tracks", file_mappings.len());
        Ok(())
    }

    /// Process an individual audio file (non-CUE/FLAC)
    async fn process_individual_file(
        &self,
        mapping: &FileMapping,
        file_size: i64,
        format: &str,
    ) -> Result<(), LibraryError> {
        println!("  Processing individual file for track {}", mapping.track_title);
        
        // Create file record
        let filename = mapping.source_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown");
            
        let db_file = crate::database::DbFile::new(&mapping.track_id, filename, file_size, format);
        let file_id = db_file.id.clone();
        
        // Save file record to database
        self.database.insert_file(&db_file).await?;
        
        // Chunk the file to temp directory for cloud upload
        println!("    Chunking file: {} bytes", file_size);
        let temp_dir = std::env::temp_dir().join("bae_import_chunks");
        tokio::fs::create_dir_all(&temp_dir).await?;
        let chunks = self.chunking_service
            .chunk_file(&mapping.source_path, &file_id, &temp_dir)
            .await?;
        
        println!("    Created {} chunks", chunks.len());
        
        // Upload chunks to cloud storage (required for cloud-first model)
        let cloud_storage = self.cloud_storage.as_ref()
            .ok_or_else(|| LibraryError::CloudStorage(
                crate::cloud_storage::CloudStorageError::Config("Cloud storage not configured - required for import".to_string())
            ))?;
        
        for chunk in &chunks {
            // Upload to cloud storage - fail import if upload fails
            println!("      Uploading chunk {} to cloud storage", chunk.id);
            let cloud_location = cloud_storage.upload_chunk_file(&chunk.id, &chunk.final_path).await?;
            
            // Clean up temp file after successful upload
            if let Err(e) = tokio::fs::remove_file(&chunk.final_path).await {
                println!("      Warning: Failed to clean up temp file {}: {}", chunk.final_path.display(), e);
            }
            
            // Store only cloud location in database
            let db_chunk = crate::database::DbChunk::from_album_chunk(
                &chunk.id,
                &mapping.track_id, // TODO: This should be album_id, not track_id
                chunk.chunk_index,
                chunk.original_size,
                chunk.encrypted_size,
                &chunk.checksum,
                &cloud_location,
                false,
            );
            self.database.insert_chunk(&db_chunk).await?;
        }
        
        println!("    Successfully processed individual file with {} chunks", chunks.len());
        Ok(())
    }
}

/// Represents a mapping between a track and its source audio file
#[derive(Debug, Clone)]
struct FileMapping {
    track_id: String,
    source_path: PathBuf,
    track_title: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use crate::models::DiscogsMaster;

    #[tokio::test]
    async fn test_library_manager_with_mock_cloud_storage() {
        // Create library manager with in-memory database for testing
        let mut library_manager = LibraryManager::new_for_testing().await.unwrap();
        
        // Enable mock cloud storage
        let cloud_storage = CloudStorageManager::new_mock();
        library_manager.enable_cloud_storage(cloud_storage);
        
        assert!(library_manager.has_cloud_storage());
        
        // Test creating an album (without actual files for simplicity)
        let master = DiscogsMaster {
            id: "123".to_string(),
            title: "Test Album".to_string(),
            year: Some(2023),
            thumb: Some("http://example.com/thumb.jpg".to_string()),
            tracklist: vec![],
            label: vec!["Test Label".to_string()],
            country: Some("US".to_string()),
        };
        
        let _import_item = ImportItem::Master(master);
        
        // This would normally process files, but we're just testing the setup
        // The actual file processing is tested in the chunking module
        println!("Library manager with cloud storage created successfully");
    }

    #[tokio::test]
    async fn test_library_manager_without_cloud_storage() {
        let library_manager = LibraryManager::new_for_testing().await.unwrap();
        
        assert!(!library_manager.has_cloud_storage());
    }
}
