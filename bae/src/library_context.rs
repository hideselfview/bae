use dioxus::prelude::*;
use crate::library::LibraryManager;
use std::sync::Arc;

/// Shared library manager that can be accessed from both UI and Subsonic server
#[derive(Clone, Debug)]
pub struct SharedLibraryManager {
    inner: Arc<LibraryManager>,
}

impl PartialEq for SharedLibraryManager {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl SharedLibraryManager {
    /// Create a new shared library manager
    pub fn new(library_manager: LibraryManager) -> Self {
        SharedLibraryManager {
            inner: Arc::new(library_manager),
        }
    }

    /// Get a reference to the library manager
    pub fn get(&self) -> &LibraryManager {
        &self.inner
    }
}

/// Hook to access the shared library manager from components
/// The library manager is provided via AppContext in main.rs
pub fn use_library_manager() -> SharedLibraryManager {
    let app_context = use_context::<crate::AppContext>();
    app_context.library_manager
}
