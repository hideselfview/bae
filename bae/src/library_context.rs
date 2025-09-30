use dioxus::prelude::*;
use crate::library::LibraryManager;
use std::sync::{Arc, OnceLock};
use tokio::sync::RwLock;
use std::path::PathBuf;

/// Shared library manager that can be accessed from both UI and Subsonic server
#[derive(Clone)]
pub struct SharedLibraryManager {
    inner: Arc<RwLock<LibraryManager>>,
}

// Global instance - created once and reused
static LIBRARY_MANAGER: OnceLock<SharedLibraryManager> = OnceLock::new();

impl PartialEq for SharedLibraryManager {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl SharedLibraryManager {
    /// Create a new shared library manager (private - use get() instead)
    async fn new(library_path: PathBuf) -> Result<Self, crate::library::LibraryError> {
        let mut library_manager = LibraryManager::new(library_path).await?;
        
        // Try to configure cloud storage from keyring (not environment variables)
        if let Ok(s3_config_data) = crate::s3_config::retrieve_s3_config() {
            let cloud_config = crate::cloud_storage::S3Config {
                bucket_name: s3_config_data.bucket_name,
                region: s3_config_data.region,
                access_key_id: s3_config_data.access_key_id,
                secret_access_key: s3_config_data.secret_access_key,
                endpoint_url: s3_config_data.endpoint_url,
            };
            
            match crate::cloud_storage::CloudStorageManager::new_s3(cloud_config).await {
                Ok(cloud_storage) => {
                    println!("LibraryManager: Cloud storage configured successfully");
                    library_manager.enable_cloud_storage(cloud_storage);
                }
                Err(e) => {
                    println!("Warning: Failed to initialize cloud storage: {}", e);
                }
            }
        } else {
            println!("LibraryManager: No cloud storage configuration found in keyring");
        }
        
        Ok(SharedLibraryManager {
            inner: Arc::new(RwLock::new(library_manager)),
        })
    }

    /// Get a read lock on the library manager
    pub async fn read(&self) -> tokio::sync::RwLockReadGuard<'_, LibraryManager> {
        self.inner.read().await
    }

    /// Get a write lock on the library manager
    pub async fn write(&self) -> tokio::sync::RwLockWriteGuard<'_, LibraryManager> {
        self.inner.write().await
    }
}

/// Initialize the global library manager (must be called at app startup)
pub async fn initialize_library(library_path: PathBuf) -> Result<(), crate::library::LibraryError> {
    let manager = SharedLibraryManager::new(library_path).await?;
    LIBRARY_MANAGER.set(manager).map_err(|_| {
        crate::library::LibraryError::Import("Library manager already initialized".to_string())
    })?;
    Ok(())
}

/// Get the global library manager instance
pub fn get_library() -> SharedLibraryManager {
    LIBRARY_MANAGER.get()
        .expect("Library manager not initialized - call initialize_library first")
        .clone()
}

/// Context provider for shared library manager
#[component]
pub fn LibraryContextProvider(
    library_manager: SharedLibraryManager,
    children: Element,
) -> Element {
    use_context_provider(|| library_manager);
    
    rsx! {
        {children}
    }
}

/// Hook to access the shared library manager from components
pub fn use_library_manager() -> SharedLibraryManager {
    use_context::<SharedLibraryManager>()
}
