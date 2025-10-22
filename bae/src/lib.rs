// Library exports for integration tests and reusable components

// Internal modules needed for compilation (hidden from docs)
#[doc(hidden)]
pub mod album_import_context;
#[doc(hidden)]
pub mod app_context;
#[doc(hidden)]
pub mod config;
#[doc(hidden)]
pub mod library_context;

// Re-export AppContext at crate root for easier access
pub use app_context::AppContext;

pub mod cache;
pub mod cloud_storage;
pub mod db;
pub mod discogs;
pub mod encryption;
pub mod import;
pub mod library;

// Optional modules
pub mod audio_processing;
pub mod chunking;
pub mod cue_flac;
pub mod playback;
pub mod subsonic;
