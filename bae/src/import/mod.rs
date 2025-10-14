// # Import Module
//
// Decomposed import service with focused, testable components:
//
// - **TrackFileMapper**: Validates track-to-file mapping before DB insertion
// - **UploadPipeline**: Chunks files and uploads to cloud storage
// - **MetadataPersister**: Persists file/chunk metadata to database
// - **ImportService**: Thin orchestrator coordinating the above services
//
// Public API:
// - `ImportService`: Create and start the service
// - `ImportServiceHandle`: Send requests and subscribe to progress
// - `ImportRequest`: Album import requests
// - `ImportProgress`: Real-time progress updates
// - `TrackSourceFile`: Links database tracks to source files

mod metadata_persister;
mod progress_service;
mod service;
mod track_file_mapper;
mod types;
mod upload_pipeline;

// Public API exports
pub use service::{ImportService, ImportServiceHandle};
pub use types::{ImportProgress, ImportRequest};
pub use upload_pipeline::{UploadConfig, UploadEvent};
