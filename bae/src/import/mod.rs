// # Import Module
//
// Stream-based import service with focused, testable components:
//
// - **TrackFileMapper**: Validates track-to-file mapping before DB insertion
// - **AlbumLayout**: Analyzes album's physical structure (files → chunks → tracks)
// - **Pipeline**: Streaming read → encrypt → upload → persist stages
// - **MetadataPersister**: Persists file/chunk metadata to database
// - **ImportService**: Orchestrates the import workflow
//
// Public API:
// - `ImportService`: Create and start the service
// - `ImportServiceHandle`: Send requests and subscribe to progress
// - `ImportRequest`: Album import requests
// - `ImportProgress`: Real-time progress updates

mod album_chunk_layout;
mod discogs_parser;
mod handle;
mod metadata_persister;
mod pipeline;
mod progress;
mod service;
mod track_to_file_mapper;
mod types;

// Public API exports
pub use handle::ImportHandle;
pub use service::{ImportConfig, ImportService};
pub use types::{ImportProgress, ImportRequestParams};
