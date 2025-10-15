// # Import Module
//
// Stream-based import service with focused, testable components:
//
// - **TrackFileMapper**: Validates track-to-file mapping before DB insertion
// - **MetadataPersister**: Persists file/chunk metadata to database
// - **ImportService**: Orchestrates streaming pipeline (read → encrypt → upload → persist)
//
// Public API:
// - `ImportService`: Create and start the service
// - `ImportServiceHandle`: Send requests and subscribe to progress
// - `ImportRequest`: Album import requests
// - `ImportProgress`: Real-time progress updates

mod metadata_persister;
mod progress_service;
mod service;
mod track_file_mapper;
mod types;

// Public API exports
pub use service::{ImportConfig, ImportHandle, ImportService};
pub use types::{ImportProgress, ImportRequest};
