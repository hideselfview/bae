use std::path::PathBuf;

/// Application configuration
/// In debug builds: loads from .env file
/// In release builds: loads from ~/.bae/config.yaml (TODO)
#[derive(Clone, Debug)]
pub struct Config {
    /// Whether to use local filesystem storage instead of S3
    pub use_local_storage: bool,
    /// Path for local storage (dev mode)
    pub local_storage_path: Option<PathBuf>,
    /// Library ID (will be loaded from config or generated)
    pub library_id: Option<String>,
}

impl Config {
    /// Load configuration based on build mode
    pub fn load() -> Self {
        #[cfg(debug_assertions)]
        {
            // Try to load .env file
            if dotenvy::dotenv().is_ok() {
                println!("Config: Dev mode activated - loaded .env file");
            } else {
                println!("Config: No .env file found, using production config");
            }

            Self::from_env()
        }

        #[cfg(not(debug_assertions))]
        {
            Self::from_config_file()
        }
    }

    /// Load configuration from environment variables (dev mode)
    #[cfg(debug_assertions)]
    fn from_env() -> Self {
        let use_local_storage = std::env::var("BAE_USE_LOCAL_STORAGE")
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(false);

        let local_storage_path = std::env::var("BAE_LOCAL_STORAGE_PATH")
            .ok()
            .map(PathBuf::from);

        let library_id = std::env::var("BAE_LIBRARY_ID").ok();

        if use_local_storage {
            println!("Config: Dev mode with local storage enabled");
            if let Some(path) = &local_storage_path {
                println!("Config: Local storage path: {}", path.display());
            }
        } else {
            println!("Config: Dev mode with S3 storage");
        }

        Self {
            use_local_storage,
            local_storage_path,
            library_id,
        }
    }

    /// Load configuration from config.yaml (production mode)
    #[cfg(not(debug_assertions))]
    fn from_config_file() -> Self {
        // TODO: Implement config.yaml loading
        println!("Config: Production mode - loading from config.yaml (not implemented yet)");

        Self {
            use_local_storage: false,
            local_storage_path: None,
            library_id: None,
        }
    }

    /// Get the library storage path
    pub fn get_library_path(&self) -> PathBuf {
        if self.use_local_storage {
            if let Some(path) = &self.local_storage_path {
                return path.clone();
            }
        }

        // Production: use ~/.bae/ directory
        // TODO: This should be ~/.bae/libraries/{library_id}/ once we have library initialization
        let home_dir = dirs::home_dir().expect("Failed to get home directory");
        home_dir.join(".bae")
    }
}
