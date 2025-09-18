use reqwest::Client;
use std::collections::HashMap;
use std::sync::OnceLock;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ApiKeyError {
    #[error("Keyring error: {0}")]
    Keyring(#[from] keyring::Error),
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("API key is invalid")]
    InvalidKey,
    #[error("API key not found")]
    NotFound,
}

pub struct ApiKeyManager {
    entry: keyring::Entry,
}

// Global instance - created once and reused
static API_KEY_MANAGER: OnceLock<ApiKeyManager> = OnceLock::new();

// Helper function to get the singleton instance
fn get_manager() -> Result<&'static ApiKeyManager, ApiKeyError> {
    match API_KEY_MANAGER.get() {
        Some(manager) => Ok(manager),
        None => {
            // Try to initialize - if it fails, we can't recover anyway
            let manager = ApiKeyManager::new()?;
            match API_KEY_MANAGER.set(manager) {
                Ok(()) => Ok(API_KEY_MANAGER.get().unwrap()),
                Err(_) => {
                    // Someone else initialized it first, use theirs
                    Ok(API_KEY_MANAGER.get().unwrap())
                }
            }
        }
    }
}

impl ApiKeyManager {
    pub fn new() -> Result<Self, ApiKeyError> {
        let entry = keyring::Entry::new("bae", "discogs_api_key")?;
        Ok(Self { entry })
    }

    /// Store the Discogs API key securely in the system keychain
    pub fn store_api_key(&self, api_key: &str) -> Result<(), ApiKeyError> {
        println!("store_api_key service: bae username: discogs_api_key");
        match self.entry.set_password(api_key) {
            Ok(_) => {
                println!("Successfully stored API key");
                // Read-after-write verification using the public getter (same code path as readers)
                match self.get_api_key() {
                    Ok(password) => {
                        println!(
                            "Read-after-write verification via get_api_key succeeded (len: {} chars)",
                            password.len()
                        );
                    }
                    Err(e) => {
                        println!("Read-after-write verification via get_api_key failed: {}", e);
                    }
                }
                Ok(())
            },
            Err(e) => {
                println!("Error storing API key: {}", e);
                Err(ApiKeyError::Keyring(e))
            },
        }
    }

    /// Retrieve the stored Discogs API key from the system keychain
    pub fn get_api_key(&self) -> Result<String, ApiKeyError> {
        println!("get_api_key service: bae username: discogs_api_key");
        match self.entry.get_password() {
            Ok(password) => {
                println!("Successfully retrieved API key");
                Ok(password)
            },
            Err(keyring::Error::NoEntry) => {
                println!("No API key found in keyring");
                Err(ApiKeyError::NotFound)
            },
            Err(e) => {
                println!("Error retrieving API key from keyring: {}", e);
                Err(ApiKeyError::Keyring(e))
            },
        }
    }

    /// Delete the stored API key
    pub fn delete_api_key(&self) -> Result<(), ApiKeyError> {
        self.entry.delete_credential()?;
        Ok(())
    }

    /// Validate that an API key works with the Discogs API
    pub async fn validate_api_key(&self, api_key: &str) -> Result<bool, ApiKeyError> {
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
            .header("User-Agent", "bae/1.0 +https://github.com/yourusername/bae")
            .send()
            .await?;

        match response.status().as_u16() {
            200 => Ok(true),
            401 => Ok(false), // Invalid API key
            _ => Err(ApiKeyError::Network(
                response.error_for_status().unwrap_err(),
            )),
        }
    }

    /// Set and validate an API key in one operation
    pub async fn set_and_validate_api_key(&self, api_key: &str) -> Result<(), ApiKeyError> {
        // First validate the key
        if !self.validate_api_key(api_key).await? {
            return Err(ApiKeyError::InvalidKey);
        }

        // If valid, store it
        self.store_api_key(api_key)?;
        Ok(())
    }

    /// Check if an API key is stored (without validating it)
    pub fn has_api_key(&self) -> bool {
        println!("has_api_key");
        self.get_api_key().is_ok()
    }

    /// Check if an API key is stored and valid
    pub async fn has_valid_api_key(&self) -> bool {
        if let Ok(api_key) = self.get_api_key() {
            self.validate_api_key(&api_key).await.unwrap_or(false)
        } else {
            false
        }
    }
}

pub fn retrieve_api_key() -> Result<String, ApiKeyError> {
    get_manager()?.get_api_key()
}

pub fn remove_api_key() -> Result<(), ApiKeyError> {
    get_manager()?.delete_api_key()
}

pub async fn validate_and_store_api_key(api_key: &str) -> Result<(), ApiKeyError> {
    get_manager()?.set_and_validate_api_key(api_key).await
}

pub fn check_api_key_exists() -> bool {
    get_manager().map(|m| m.has_api_key()).unwrap_or(false)
}

pub async fn check_api_key_valid() -> bool {
    if let Ok(manager) = get_manager() {
        manager.has_valid_api_key().await
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_key_manager_creation() {
        let _manager = ApiKeyManager::new().expect("Failed to create ApiKeyManager");
        // We can't directly test the entry fields since they're private,
        // but successful creation means the keyring entry was created successfully
    }

    #[tokio::test]
    async fn test_invalid_key_validation() {
        let manager = ApiKeyManager::new().expect("Failed to create ApiKeyManager");
        let result = manager.validate_api_key("invalid_key").await;
        
        match result {
            Ok(false) => (), // Expected: invalid key returns false
            Ok(true) => panic!("Invalid key should not validate as true"),
            Err(e) => println!("Network error during validation test: {}", e), // OK for test env
        }
    }
}
