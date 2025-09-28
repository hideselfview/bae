use crate::database::{Database, DbAlbum, DbTrack, DbFile, DbChunk};
use crate::models::ImportItem;
use crate::chunking::{ChunkingService, ChunkingError};
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
}

impl LibraryManager {
    /// Create a new library manager
    pub async fn new(library_path: PathBuf) -> Result<Self, LibraryError> {
        // Ensure library directory exists
        fs::create_dir_all(&library_path)?;
        
        // Initialize database
        let db_path = library_path.join("library.db");
        let database = Database::new(db_path.to_str().unwrap()).await?;
        
        // Initialize chunking service
        let chunking_service = ChunkingService::new()?;
        
        Ok(LibraryManager {
            database,
            library_path,
            chunking_service,
        })
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
        let album = self.create_album_record(import_item, &artist_name)?;
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
        self.process_audio_files(&file_mappings).await?;
        
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
    fn create_album_record(&self, import_item: &ImportItem, artist_name: &str) -> Result<DbAlbum, LibraryError> {
        let album = match import_item {
            ImportItem::Master(master) => DbAlbum::from_discogs_master(master, artist_name),
            ImportItem::Release(release) => DbAlbum::from_discogs_release(release, artist_name),
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
        println!("LibraryManager: Mapping files in {} to {} tracks", 
                source_folder.display(), tracks.len());

        // Get all audio files in the folder
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

    /// Process audio files - chunk, encrypt, and store metadata
    async fn process_audio_files(&self, mappings: &[FileMapping]) -> Result<(), LibraryError> {
        for mapping in mappings {
            println!("LibraryManager: Processing file {} for track {}", 
                    mapping.source_path.display(), mapping.track_title);
            
            // Get file metadata
            let file_metadata = fs::metadata(&mapping.source_path)?;
            let file_size = file_metadata.len() as i64;
            
            // Extract file format from extension
            let format = mapping.source_path
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("unknown")
                .to_lowercase();
            
            // Create file record
            let filename = mapping.source_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("unknown");
                
            let db_file = DbFile::new(&mapping.track_id, filename, file_size, &format);
            let file_id = db_file.id.clone();
            
            // Save file record to database
            self.database.insert_file(&db_file).await?;
            
            // Chunk the file
            println!("  Chunking file: {} bytes", file_size);
            let chunks = self.chunking_service
                .chunk_file(&mapping.source_path, &file_id)
                .await?;
            
            println!("  Created {} chunks", chunks.len());
            
            // Save chunk records to database
            for chunk in &chunks {
                // For now, chunks are stored locally in temp directory
                // TODO: Upload to S3 and update storage_location
                let storage_location = format!("local:{}", chunk.temp_path.display());
                let db_chunk = DbChunk::from_file_chunk(chunk, &storage_location, true);
                
                self.database.insert_chunk(&db_chunk).await?;
            }
            
            println!("  Successfully processed file with {} chunks", chunks.len());
        }
        
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
}

/// Represents a mapping between a track and its source audio file
#[derive(Debug, Clone)]
struct FileMapping {
    track_id: String,
    source_path: PathBuf,
    track_title: String,
}
