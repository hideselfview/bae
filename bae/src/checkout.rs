use std::path::{Path, PathBuf};
use tokio::fs;
use thiserror::Error;
use crate::database::{Database, DbAlbum};
use crate::library::LibraryManager;
use crate::cloud_storage::CloudStorageManager;
use crate::encryption::EncryptionService;

/// Errors that can occur during checkout operations
#[derive(Error, Debug)]
pub enum CheckoutError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("Cloud storage error: {0}")]
    CloudStorage(#[from] crate::cloud_storage::CloudStorageError),
    #[error("Encryption error: {0}")]
    Encryption(#[from] crate::encryption::EncryptionError),
    #[error("Album not found: {0}")]
    AlbumNotFound(String),
    #[error("Checkout error: {0}")]
    Checkout(String),
}

/// Configuration for checkout operations
#[derive(Debug, Clone)]
pub struct CheckoutConfig {
    /// Base directory for checkouts (default: ~/Downloads/bae_checkouts)
    pub checkout_base_dir: PathBuf,
    /// Whether to overwrite existing checkouts
    pub overwrite_existing: bool,
}

impl Default for CheckoutConfig {
    fn default() -> Self {
        let home_dir = dirs::home_dir().expect("Failed to get home directory");
        CheckoutConfig {
            checkout_base_dir: home_dir.join("Downloads").join("bae_checkouts"),
            overwrite_existing: false,
        }
    }
}

/// Manages source folder checkouts - recreating original files from cloud chunks
pub struct CheckoutManager {
    config: CheckoutConfig,
    library_manager: LibraryManager,
}

impl CheckoutManager {
    /// Create a new checkout manager
    pub async fn new(library_path: PathBuf) -> Result<Self, CheckoutError> {
        let config = CheckoutConfig::default();
        Self::new_with_config(library_path, config).await
    }

    /// Create a new checkout manager with custom configuration
    pub async fn new_with_config(library_path: PathBuf, config: CheckoutConfig) -> Result<Self, CheckoutError> {
        // Ensure checkout base directory exists
        fs::create_dir_all(&config.checkout_base_dir).await?;

        let library_manager = LibraryManager::new(library_path).await
            .map_err(|e| CheckoutError::Checkout(format!("Failed to initialize library: {}", e)))?;

        Ok(CheckoutManager {
            config,
            library_manager,
        })
    }

    /// Checkout an album - recreate original files from cloud chunks
    pub async fn checkout_album(&self, album_id: &str) -> Result<PathBuf, CheckoutError> {
        println!("Starting checkout for album: {}", album_id);

        // Get album metadata
        let albums = self.library_manager.get_albums().await
            .map_err(|e| CheckoutError::Checkout(format!("Failed to get albums: {}", e)))?;
        
        let album = albums.iter()
            .find(|a| a.id == album_id)
            .ok_or_else(|| CheckoutError::AlbumNotFound(album_id.to_string()))?;

        // Create checkout directory
        let checkout_dir = self.config.checkout_base_dir.join(format!("{}_{}", 
            sanitize_filename(&album.artist_name), 
            sanitize_filename(&album.title)
        ));

        if checkout_dir.exists() && !self.config.overwrite_existing {
            return Err(CheckoutError::Checkout(format!(
                "Checkout directory already exists: {}", checkout_dir.display()
            )));
        }

        fs::create_dir_all(&checkout_dir).await?;
        println!("Created checkout directory: {}", checkout_dir.display());

        // Get tracks for this album
        let tracks = self.library_manager.get_tracks(album_id).await
            .map_err(|e| CheckoutError::Checkout(format!("Failed to get tracks: {}", e)))?;

        println!("Found {} tracks to checkout", tracks.len());

        // Process each track
        for track in tracks {
            println!("Checking out track: {}", track.title);
            
            // Get files for this track
            let files = self.library_manager.get_files_for_track(&track.id).await
                .map_err(|e| CheckoutError::Checkout(format!("Failed to get files for track: {}", e)))?;

            for file in files {
                let output_path = checkout_dir.join(&file.original_filename);
                self.checkout_file(&file.id, &output_path).await?;
                println!("  Recreated: {}", file.original_filename);
            }
        }

        println!("Successfully checked out album to: {}", checkout_dir.display());
        Ok(checkout_dir)
    }

    /// Checkout a single file - recreate from cloud chunks
    async fn checkout_file(&self, file_id: &str, output_path: &Path) -> Result<(), CheckoutError> {
        // Get chunks for this file
        let chunks = self.library_manager.get_chunks_for_file(file_id).await
            .map_err(|e| CheckoutError::Checkout(format!("Failed to get chunks: {}", e)))?;

        if chunks.is_empty() {
            return Err(CheckoutError::Checkout("No chunks found for file".to_string()));
        }

        // Sort chunks by index
        let mut sorted_chunks = chunks;
        sorted_chunks.sort_by_key(|c| c.chunk_index);

        // Initialize cloud storage and encryption
        let config = crate::cloud_storage::S3Config::from_env()
            .map_err(|e| CheckoutError::Checkout(format!("Failed to load S3 config: {}", e)))?;
        let cloud_storage = CloudStorageManager::new_s3(config).await?;
        let encryption_service = EncryptionService::new()?;

        // Create output file
        let mut output_file = fs::File::create(output_path).await?;

        // Download, decrypt, and write each chunk
        use tokio::io::AsyncWriteExt;
        for chunk in sorted_chunks {
            // Download encrypted chunk from cloud
            let encrypted_data = cloud_storage.download_chunk(&chunk.storage_location).await?;
            
            // Decrypt chunk
            let decrypted_data = encryption_service.decrypt_chunk(&encrypted_data)?;
            
            // Write to output file
            output_file.write_all(&decrypted_data).await?;
        }

        output_file.flush().await?;
        Ok(())
    }

    /// List all albums that can be checked out
    pub async fn list_albums(&self) -> Result<Vec<DbAlbum>, CheckoutError> {
        let albums = self.library_manager.get_albums().await
            .map_err(|e| CheckoutError::Checkout(format!("Failed to get albums: {}", e)))?;
        Ok(albums)
    }

    /// Check if an album has a source folder path (can be checked out)
    pub fn can_checkout_album(&self, album: &DbAlbum) -> bool {
        album.source_folder_path.is_some()
    }

    /// Get the original source folder path for an album (if available)
    pub fn get_source_folder_path<'a>(&self, album: &'a DbAlbum) -> Option<&'a str> {
        album.source_folder_path.as_deref()
    }

    /// Delete a checkout directory
    pub async fn delete_checkout(&self, checkout_path: &Path) -> Result<(), CheckoutError> {
        if checkout_path.exists() {
            fs::remove_dir_all(checkout_path).await?;
            println!("Deleted checkout: {}", checkout_path.display());
        }
        Ok(())
    }

    /// List existing checkouts
    pub async fn list_checkouts(&self) -> Result<Vec<PathBuf>, CheckoutError> {
        let mut checkouts = Vec::new();
        
        if !self.config.checkout_base_dir.exists() {
            return Ok(checkouts);
        }

        let mut dir_entries = fs::read_dir(&self.config.checkout_base_dir).await?;
        while let Some(entry) = dir_entries.next_entry().await? {
            let path = entry.path();
            if path.is_dir() {
                checkouts.push(path);
            }
        }

        Ok(checkouts)
    }
}

/// Sanitize a filename by removing/replacing invalid characters
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("Normal Name"), "Normal Name");
        assert_eq!(sanitize_filename("Name/With\\Bad:Chars"), "Name_With_Bad_Chars");
        assert_eq!(sanitize_filename("Artist: Album (2023)"), "Artist_ Album (2023)");
    }
}
