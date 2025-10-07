use keyring::Entry;
use std::sync::{Arc, OnceLock};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SecureConfigError {
    #[error("Keyring error: {0}")]
    Keyring(#[from] keyring::Error),
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("Validation error: {0}")]
    Validation(String),
}

/// All secure configuration data loaded from keyring
#[derive(Debug, Clone)]
pub struct SecureConfigData {
    pub discogs_api_key: Option<String>,
    pub s3_config: Option<crate::cloud_storage::S3Config>,
    pub encryption_master_key: String, // Hex-encoded 256-bit key
}

/// Lazy-loading secure configuration manager
/// Cloning is cheap (clones Arc), and all clones share the same lazy-loaded data
#[derive(Clone, Debug)]
pub struct SecureConfig {
    inner: Arc<OnceLock<SecureConfigData>>,
}

impl PartialEq for SecureConfig {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl SecureConfig {
    /// Create a new lazy secure config (doesn't access keyring yet)
    pub fn new() -> Self {
        SecureConfig {
            inner: Arc::new(OnceLock::new()),
        }
    }

    /// Create a secure config with pre-populated data (for testing only)
    #[cfg(test)]
    pub fn new_with_data(data: SecureConfigData) -> Self {
        let inner = Arc::new(OnceLock::new());
        let _ = inner.set(data); // Pre-populate to avoid keyring access
        SecureConfig { inner }
    }

    /// Get the secure configuration, loading from keyring on first access
    /// This may prompt for system keychain password
    pub fn get(&self) -> Result<&SecureConfigData, SecureConfigError> {
        // Check if already loaded
        if let Some(data) = self.inner.get() {
            return Ok(data);
        }
        
        // Not loaded yet, need to load from keychain
        println!("SecureConfig: Loading from keychain (password may be required)...");
        let data = Self::load_from_keychain()?;
        
        // Try to store it (race condition is fine, first one wins)
        match self.inner.set(data) {
            Ok(()) => Ok(self.inner.get().unwrap()),
            Err(_) => {
                // Someone else already set it, use theirs
                Ok(self.inner.get().unwrap())
            }
        }
    }

    /// Load all secure data from keyring
    fn load_from_keychain() -> Result<SecureConfigData, SecureConfigError> {
        // Load Discogs API key
        let discogs_api_key = match Entry::new("bae", "discogs_api_key") {
            Ok(entry) => match entry.get_password() {
                Ok(key) => {
                    println!("SecureConfig: Loaded Discogs API key");
                    Some(key)
                }
                Err(keyring::Error::NoEntry) => {
                    println!("SecureConfig: No Discogs API key found");
                    None
                }
                Err(e) => return Err(SecureConfigError::Keyring(e)),
            },
            Err(e) => return Err(SecureConfigError::Keyring(e)),
        };

        // Load S3 config
        let s3_config = match Entry::new("bae", "s3_config") {
            Ok(entry) => match entry.get_password() {
                Ok(json) => {
                    let config: crate::cloud_storage::S3Config = serde_json::from_str(&json)
                        .map_err(|e| SecureConfigError::Serialization(e.to_string()))?;
                    println!("SecureConfig: Loaded S3 configuration");
                    Some(config)
                }
                Err(keyring::Error::NoEntry) => {
                    println!("SecureConfig: No S3 configuration found");
                    None
                }
                Err(e) => return Err(SecureConfigError::Keyring(e)),
            },
            Err(e) => return Err(SecureConfigError::Keyring(e)),
        };
        
        // Load or generate encryption master key
        let encryption_master_key = match Entry::new("bae", "encryption_master_key") {
            Ok(entry) => match entry.get_password() {
                Ok(key_hex) => {
                    println!("SecureConfig: Loaded encryption master key");
                    key_hex
                }
                Err(keyring::Error::NoEntry) => {
                    println!("SecureConfig: Generating new encryption master key...");
                    // Generate new 256-bit key
                    use aes_gcm::{aead::OsRng, Aes256Gcm, KeyInit};
                    let key = Aes256Gcm::generate_key(OsRng);
                    let key_hex = hex::encode(key.as_slice());
                    
                    // Store it in keyring
                    entry.set_password(&key_hex)
                        .map_err(|e| SecureConfigError::Keyring(e))?;
                    println!("SecureConfig: Stored new encryption master key in keyring");
                    
                    key_hex
                }
                Err(e) => return Err(SecureConfigError::Keyring(e)),
            },
            Err(e) => return Err(SecureConfigError::Keyring(e)),
        };
        
        Ok(SecureConfigData {
            discogs_api_key,
            s3_config,
            encryption_master_key,
        })
    }

    /// Validate a Discogs API key by making a test API call
    pub async fn validate_discogs_api_key(api_key: &str) -> Result<bool, SecureConfigError> {
        use reqwest::Client;
        use std::collections::HashMap;
        
        let client = Client::new();
        let url = "https://api.discogs.com/database/search";
        
        let mut params = HashMap::new();
        params.insert("q", "test");
        params.insert("type", "release");
        params.insert("per_page", "1");
        params.insert("token", api_key);

        let response = client
            .get(url)
            .query(&params)
            .header("User-Agent", "bae/1.0")
            .send()
            .await
            .map_err(|e| SecureConfigError::Validation(format!("Network error: {}", e)))?;

        match response.status().as_u16() {
            200 => Ok(true),
            401 => Ok(false), // Invalid API key
            _ => Err(SecureConfigError::Validation(format!("Unexpected API response: {}", response.status()))),
        }
    }
    
    /// Validate and store Discogs API key
    pub async fn validate_and_store_discogs_api_key(api_key: &str) -> Result<(), SecureConfigError> {
        // First validate the key
        if !Self::validate_discogs_api_key(api_key).await? {
            return Err(SecureConfigError::Validation("Invalid API key".to_string()));
        }

        // If valid, store it
        Self::store_discogs_api_key(api_key)?;
        Ok(())
    }
    
    /// Store Discogs API key without validation (requires creating new SecureConfig instance to see changes)
    pub fn store_discogs_api_key(api_key: &str) -> Result<(), SecureConfigError> {
        let entry = Entry::new("bae", "discogs_api_key")?;
        entry.set_password(api_key)?;
        println!("SecureConfig: Stored Discogs API key");
        Ok(())
    }

    /// Delete Discogs API key
    pub fn delete_discogs_api_key() -> Result<(), SecureConfigError> {
        let entry = Entry::new("bae", "discogs_api_key")?;
        entry.delete_credential()?;
        println!("SecureConfig: Deleted Discogs API key");
        Ok(())
    }

    /// Validate and store S3 configuration
    pub async fn validate_and_store_s3_config(config: &crate::cloud_storage::S3Config) -> Result<(), SecureConfigError> {
        // Basic validation
        config.validate()
            .map_err(|e| SecureConfigError::Validation(format!("Config validation failed: {}", e)))?;
        
        // Try to create S3 client (validates credentials work)
        crate::cloud_storage::S3CloudStorage::new(config.clone())
            .await
            .map_err(|e| SecureConfigError::Validation(format!("Failed to initialize S3 client: {}", e)))?;
        
        // If validation succeeded, store the config
        Self::store_s3_config(config)?;
        Ok(())
    }
    
    /// Store S3 configuration without validation (requires creating new SecureConfig instance to see changes)
    pub fn store_s3_config(config: &crate::cloud_storage::S3Config) -> Result<(), SecureConfigError> {
        config.validate()
            .map_err(|e| SecureConfigError::Validation(format!("Config validation failed: {}", e)))?;
        let entry = Entry::new("bae", "s3_config")?;
        let json = serde_json::to_string(config)
            .map_err(|e| SecureConfigError::Serialization(e.to_string()))?;
        entry.set_password(&json)?;
        println!("SecureConfig: Stored S3 configuration");
        Ok(())
    }

    /// Delete S3 configuration
    pub fn delete_s3_config() -> Result<(), SecureConfigError> {
        let entry = Entry::new("bae", "s3_config")?;
        entry.delete_credential()?;
        println!("SecureConfig: Deleted S3 configuration");
        Ok(())
    }
}

impl Default for SecureConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Hook to access secure config from components (using Dioxus context)
/// The secure config is provided via AppContext in main.rs
pub fn use_secure_config() -> SecureConfig {
    use dioxus::prelude::use_context;
    let app_context = use_context::<crate::AppContext>();
    app_context.secure_config
}

