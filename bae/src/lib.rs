// Library exports for integration tests and reusable components

// Internal modules needed for compilation (hidden from docs)
#[doc(hidden)]
pub mod config;
#[doc(hidden)]
pub mod ui;

// Re-export UIContext at crate root for easier access
pub use ui::AppContext;

pub mod cache;
pub mod cloud_storage;
pub mod db;
pub mod discogs;
pub mod encryption;
pub mod import;
pub mod library;

// Optional modules
pub mod cue_flac;
pub mod playback;
pub mod subsonic;

// Test support (only available with test-utils feature)
#[cfg(feature = "test-utils")]
pub mod test_support;
