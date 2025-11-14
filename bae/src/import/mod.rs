// # Import Module
//
// Two-phase import architecture with focused, testable components:
//
// ## Two-Phase Model
//
// All imports follow the same two-phase pattern:
//
// **Phase 1: Acquire** - Get data ready for import
// - Folder: No-op (files already available)
// - Torrent: Download torrent to temporary folder
// - CD: Rip CD tracks to FLAC files
//
// **Phase 2: Chunk** - Upload and encrypt (same for all types)
// - Stream files → encrypt → upload chunks → persist metadata
//
// ## Components
//
// - **ImportHandle**: Validates requests, inserts DB records, sends commands to service
// - **ImportService**: Runs on dedicated thread, executes acquire + chunk phases
// - **TrackFileMapper**: Validates track-to-file mapping before DB insertion
// - **AlbumLayout**: Analyzes album's physical structure (files → chunks → tracks)
// - **Pipeline**: Streaming read → encrypt → upload → persist stages
// - **MetadataPersister**: Persists file/chunk metadata to database
//
// ## Public API
//
// - `ImportService`: Create and start the service
// - `ImportHandle`: Send requests and subscribe to progress
// - `ImportRequest`: Album import requests
// - `ImportProgress`: Real-time progress updates with phase information

mod album_chunk_layout;
mod discogs_matcher;
mod discogs_parser;
mod folder_metadata_detector;
mod handle;
mod metadata_persister;
mod musicbrainz_parser;
mod pipeline;
mod progress;
mod service;
mod track_to_file_mapper;
mod types;

// Public API exports
pub use discogs_matcher::{rank_discogs_matches, rank_mb_matches, MatchCandidate, MatchSource};
pub use folder_metadata_detector::{
    calculate_mb_discid_from_cue_flac, calculate_mb_discid_from_log, detect_metadata,
    FolderMetadata,
};
pub use handle::ImportHandle;
pub use service::{ImportConfig, ImportService};
pub use types::{ImportPhase, ImportProgress, ImportRequest, TorrentSource};
