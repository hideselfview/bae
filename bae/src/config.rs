use std::path::PathBuf;
use tracing::{info, warn};

use thiserror::Error;

/// Configuration errors (production mode only)
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Keyring error: {0}")]
    Keyring(#[from] keyring::Error),
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("Configuration error: {0}")]
    Config(String),
}

/// Application configuration
/// In debug builds: loads from .env file
/// In release builds: loads from ~/.bae/config.yaml + keyring
#[derive(Clone, Debug)]
pub struct Config {
    /// Library ID (loaded from config or auto-generated)
    pub library_id: String,
    /// Discogs API key (required)
    pub discogs_api_key: String,
    /// S3 configuration
    pub s3_config: crate::cloud_storage::S3Config,
    /// Encryption key (hex-encoded 256-bit key)
    pub encryption_key: String,
    /// Number of parallel encryption workers for import (CPU-bound)
    pub max_import_encrypt_workers: usize,
    /// Number of parallel upload workers for import (I/O-bound)
    pub max_import_upload_workers: usize,
    /// Number of parallel DB write workers for import (I/O-bound)
    pub max_import_db_write_workers: usize,
    /// Size of each chunk in bytes (default: 1MB)
    pub chunk_size_bytes: usize,
}

/// Credential data loaded from keyring (production mode only)
#[derive(Debug, Clone)]
struct CredentialData {
    discogs_api_key: String,
    s3_config: crate::cloud_storage::S3Config,
    encryption_key: String,
}

impl Config {
    /// Load configuration based on build mode
    /// Dev mode is activated if .env file exists or BAE_DEV_MODE env var is set
    pub fn load() -> Self {
        // Check for dev mode: .env file exists or BAE_DEV_MODE env var is set
        let dev_mode = std::env::var("BAE_DEV_MODE").is_ok() || dotenvy::dotenv().is_ok();

        if dev_mode {
            info!("Dev mode activated - loading from .env");
            Self::from_env()
        } else {
            info!("Production mode - loading from config.yaml");
            Self::from_config_file()
        }
    }

    /// Load configuration from environment variables (dev mode)
    fn from_env() -> Self {
        let library_id = match std::env::var("BAE_LIBRARY_ID").ok() {
            Some(id) => {
                info!("Using library ID from .env: {}", id);
                id
            }
            None => {
                let id = uuid::Uuid::new_v4().to_string();
                warn!("No BAE_LIBRARY_ID in .env, generated new ID: {}", id);
                info!(
                    "Add this to your .env file to persist: BAE_LIBRARY_ID={}",
                    id
                );
                id
            }
        };

        // Load credentials from environment variables
        let discogs_api_key = std::env::var("BAE_DISCOGS_API_KEY")
            .expect("BAE_DISCOGS_API_KEY must be set in .env for dev mode");

        // Build S3 config from environment variables
        let bucket_name =
            std::env::var("BAE_S3_BUCKET").expect("BAE_S3_BUCKET must be set in .env for dev mode");
        let region =
            std::env::var("BAE_S3_REGION").expect("BAE_S3_REGION must be set in .env for dev mode");
        let access_key_id = std::env::var("BAE_S3_ACCESS_KEY")
            .expect("BAE_S3_ACCESS_KEY must be set in .env for dev mode");
        let secret_access_key = std::env::var("BAE_S3_SECRET_KEY")
            .expect("BAE_S3_SECRET_KEY must be set in .env for dev mode");
        let endpoint_url = std::env::var("BAE_S3_ENDPOINT")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let s3_config = crate::cloud_storage::S3Config {
            bucket_name: bucket_name.clone(),
            region,
            access_key_id,
            secret_access_key,
            endpoint_url: endpoint_url.clone(),
        };

        let encryption_key = std::env::var("BAE_ENCRYPTION_KEY").unwrap_or_else(|_| {
            warn!("No BAE_ENCRYPTION_KEY found, generating temporary key");
            // Generate temporary key for dev
            use aes_gcm::{aead::OsRng, Aes256Gcm, KeyInit};
            let key = Aes256Gcm::generate_key(OsRng);
            hex::encode(key.as_slice())
        });

        // Import worker pool configuration
        let max_import_encrypt_workers = std::env::var("BAE_MAX_IMPORT_ENCRYPT_WORKERS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| {
                std::thread::available_parallelism()
                    .map(|n| n.get() * 2)
                    .unwrap_or(4)
            });

        let max_import_upload_workers = std::env::var("BAE_MAX_IMPORT_UPLOAD_WORKERS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(20);

        let max_import_db_write_workers = std::env::var("BAE_MAX_IMPORT_DB_WRITE_WORKERS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(10);

        let chunk_size_bytes = std::env::var("BAE_CHUNK_SIZE_BYTES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1024 * 1024); // 1MB default

        info!("Dev mode with S3 storage");
        info!("S3 bucket: {}", bucket_name);
        if let Some(endpoint) = &endpoint_url {
            info!("S3 endpoint: {}", endpoint);
        }
        info!(
            "Import worker pools - encrypt: {}, upload: {}, db_write: {}",
            max_import_encrypt_workers, max_import_upload_workers, max_import_db_write_workers
        );
        info!("Chunk size: {} bytes", chunk_size_bytes);

        Self {
            library_id,
            discogs_api_key,
            s3_config,
            encryption_key,
            chunk_size_bytes,
            max_import_encrypt_workers,
            max_import_upload_workers,
            max_import_db_write_workers,
        }
    }

    /// Load configuration from config.yaml + keyring (production mode)
    fn from_config_file() -> Self {
        // TODO: Implement config.yaml loading
        info!("Production mode - loading from config.yaml (not implemented yet)");

        // Load from keyring
        let credentials = Self::load_from_keyring()
            .expect("Failed to load credentials from keyring - run setup wizard first");

        // TODO: Load library_id from config.yaml
        let library_id = {
            let id = uuid::Uuid::new_v4().to_string();
            warn!("config.yaml not implemented, generated library ID: {}", id);
            id
        };

        // Import worker pool configuration (TODO: load from config.yaml)
        let max_import_encrypt_workers = std::thread::available_parallelism()
            .map(|n| n.get() * 2)
            .unwrap_or(4);
        let max_import_upload_workers = 20;
        let max_import_db_write_workers = 10;
        let chunk_size_bytes = 1024 * 1024; // 1MB default

        Self {
            library_id,
            discogs_api_key: credentials.discogs_api_key,
            s3_config: credentials.s3_config,
            encryption_key: credentials.encryption_key,
            max_import_encrypt_workers,
            max_import_upload_workers,
            max_import_db_write_workers,
            chunk_size_bytes,
        }
    }

    /// Get the library storage path
    pub fn get_library_path(&self) -> PathBuf {
        // Use ~/.bae/ directory for local database
        // TODO: This should be ~/.bae/libraries/{library_id}/ once we have library initialization
        let home_dir = dirs::home_dir().expect("Failed to get home directory");
        home_dir.join(".bae")
    }

    /// Load credentials from keyring (production mode only)
    fn load_from_keyring() -> Result<CredentialData, ConfigError> {
        use keyring::Entry;

        info!("Loading credentials from keyring (password may be required)...");

        // Load Discogs API key (required)
        let discogs_api_key = match Entry::new("bae", "discogs_api_key") {
            Ok(entry) => match entry.get_password() {
                Ok(key) => {
                    info!("Loaded Discogs API key");
                    key
                }
                Err(keyring::Error::NoEntry) => {
                    return Err(ConfigError::Config(
                        "No Discogs API key found - run setup wizard first".to_string(),
                    ));
                }
                Err(e) => return Err(ConfigError::Keyring(e)),
            },
            Err(e) => return Err(ConfigError::Keyring(e)),
        };

        // Load S3 config (required)
        let s3_config = match Entry::new("bae", "s3_config") {
            Ok(entry) => match entry.get_password() {
                Ok(json) => {
                    let config: crate::cloud_storage::S3Config = serde_json::from_str(&json)
                        .map_err(|e| ConfigError::Serialization(e.to_string()))?;
                    info!("Loaded S3 configuration");
                    config
                }
                Err(keyring::Error::NoEntry) => {
                    return Err(ConfigError::Config(
                        "No S3 configuration found - run setup wizard first".to_string(),
                    ));
                }
                Err(e) => return Err(ConfigError::Keyring(e)),
            },
            Err(e) => return Err(ConfigError::Keyring(e)),
        };

        // Load encryption master key
        let encryption_key = match Entry::new("bae", "encryption_master_key") {
            Ok(entry) => match entry.get_password() {
                Ok(key_hex) => {
                    info!("Loaded encryption master key");
                    key_hex
                }
                Err(keyring::Error::NoEntry) => {
                    return Err(ConfigError::Config(
                        "No encryption key found - run setup wizard first".to_string(),
                    ));
                }
                Err(e) => return Err(ConfigError::Keyring(e)),
            },
            Err(e) => return Err(ConfigError::Keyring(e)),
        };

        Ok(CredentialData {
            discogs_api_key,
            s3_config,
            encryption_key,
        })
    }
}

/// Hook to access config from components (using Dioxus context)
/// The config is provided via UIContext in main.rs
pub fn use_config() -> Config {
    use dioxus::prelude::use_context;
    let context = use_context::<crate::ui::AppContext>();
    context.config
}
