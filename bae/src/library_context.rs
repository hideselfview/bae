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
        
        // Try to configure cloud storage
        if let Err(e) = library_manager.try_configure_cloud_storage().await {
            println!("Warning: Cloud storage not configured: {}", e);
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
