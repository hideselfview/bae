// # Import Module
//
// Stream-based import service with focused, testable components:
//
// - **TrackFileMapper**: Validates track-to-file mapping before DB insertion
// - **AlbumLayout**: Analyzes album's physical structure (files → chunks → tracks)
// - **MetadataPersister**: Persists file/chunk metadata to database
// - **ImportService**: Orchestrates streaming pipeline (read → encrypt → upload → persist)
//
// Public API:
// - `ImportService`: Create and start the service
// - `ImportServiceHandle`: Send requests and subscribe to progress
// - `ImportRequest`: Album import requests
// - `ImportProgress`: Real-time progress updates

mod album_layout;
mod metadata_persister;
mod progress_service;
mod service;
mod track_file_mapper;
mod types;

// Public API exports
pub use service::{ImportConfig, ImportHandle, ImportService};
pub use types::{ImportProgress, ImportRequest};
