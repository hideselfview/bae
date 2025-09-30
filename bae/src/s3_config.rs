use keyring::Entry;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub enum S3ConfigError {
    KeyringError(String),
    SerializationError(String),
    ValidationError(String),
}

impl fmt::Display for S3ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            S3ConfigError::KeyringError(msg) => write!(f, "Keyring error: {}", msg),
            S3ConfigError::SerializationError(msg) => write!(f, "Serialization error: {}", msg),
            S3ConfigError::ValidationError(msg) => write!(f, "Validation error: {}", msg),
        }
    }
}

impl Error for S3ConfigError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3ConfigData {
    pub bucket_name: String,
    pub region: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub endpoint_url: Option<String>, // For MinIO/S3-compatible services
}

impl S3ConfigData {
    pub fn validate(&self) -> Result<(), S3ConfigError> {
        if self.bucket_name.trim().is_empty() {
            return Err(S3ConfigError::ValidationError("Bucket name cannot be empty".to_string()));
        }
        if self.region.trim().is_empty() {
            return Err(S3ConfigError::ValidationError("Region cannot be empty".to_string()));
        }
        if self.access_key_id.trim().is_empty() {
            return Err(S3ConfigError::ValidationError("Access key ID cannot be empty".to_string()));
        }
        if self.secret_access_key.trim().is_empty() {
            return Err(S3ConfigError::ValidationError("Secret access key cannot be empty".to_string()));
        }
        Ok(())
    }
}

const SERVICE: &str = "bae";
const S3_CONFIG_KEY: &str = "s3_config";

/// Store S3 configuration in the system keyring
pub fn store_s3_config(config: &S3ConfigData) -> Result<(), S3ConfigError> {
    config.validate()?;
    
    let entry = Entry::new(SERVICE, S3_CONFIG_KEY)
        .map_err(|e| S3ConfigError::KeyringError(e.to_string()))?;
    
    let config_json = serde_json::to_string(config)
        .map_err(|e| S3ConfigError::SerializationError(e.to_string()))?;
    
    entry.set_password(&config_json)
        .map_err(|e| S3ConfigError::KeyringError(e.to_string()))?;
    
    Ok(())
}

/// Retrieve S3 configuration from the system keyring
pub fn retrieve_s3_config() -> Result<S3ConfigData, S3ConfigError> {
    let entry = Entry::new(SERVICE, S3_CONFIG_KEY)
        .map_err(|e| S3ConfigError::KeyringError(e.to_string()))?;
    
    let config_json = entry.get_password()
        .map_err(|e| S3ConfigError::KeyringError(e.to_string()))?;
    
    let config: S3ConfigData = serde_json::from_str(&config_json)
        .map_err(|e| S3ConfigError::SerializationError(e.to_string()))?;
    
    Ok(config)
}

/// Check if S3 configuration exists in the keyring
pub fn check_s3_config_exists() -> bool {
    if let Ok(entry) = Entry::new(SERVICE, S3_CONFIG_KEY) {
        entry.get_password().is_ok()
    } else {
        false
    }
}

/// Remove S3 configuration from the system keyring
pub fn remove_s3_config() -> Result<(), S3ConfigError> {
    let entry = Entry::new(SERVICE, S3_CONFIG_KEY)
        .map_err(|e| S3ConfigError::KeyringError(e.to_string()))?;
    
    entry.delete_credential()
        .map_err(|e| S3ConfigError::KeyringError(e.to_string()))?;
    
    Ok(())
}

/// Validate S3 configuration by attempting to connect
pub async fn validate_and_store_s3_config(config: &S3ConfigData) -> Result<(), S3ConfigError> {
    config.validate()?;
    
    // Convert to cloud_storage::S3Config for validation
    let s3_config = crate::cloud_storage::S3Config {
        bucket_name: config.bucket_name.clone(),
        region: config.region.clone(),
        access_key_id: config.access_key_id.clone(),
        secret_access_key: config.secret_access_key.clone(),
        endpoint_url: config.endpoint_url.clone(),
    };
    
    // Try to create client (this validates credentials format)
    crate::cloud_storage::S3CloudStorage::new(s3_config)
        .await
        .map_err(|e| S3ConfigError::ValidationError(format!("Failed to initialize S3 client: {}", e)))?;
    
    // If validation succeeded, store the config
    store_s3_config(config)?;
    
    Ok(())
}
