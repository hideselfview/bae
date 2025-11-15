//! Import type definitions
//!
//! # Import Architecture
//!
//! All imports follow the same data flow, regardless of whether tracks are stored as
//! individual files (one-file-per-track) or as a single file with a CUE sheet (one-file-per-album):
//!
//! ## Phase 1: Track-to-File Mapping (Validation)
//! - Map logical tracks (from Discogs metadata) to physical audio files (from user's folder)
//! - Validates that the user's files match the expected album structure
//! - For one-file-per-track: Each logical track maps to its own file (01.flac, 02.flac, etc.)
//! - For CUE/FLAC: All logical tracks map to the same FLAC file, CUE sheet parsed for validation
//! - Output: `TrackToFileMappingResult` (track→file mappings + optional CUE metadata)
//!
//! ## Phase 2: Chunk Layout Calculation
//! - Concatenate all files into a single byte stream
//! - Divide the stream into fixed-size chunks
//! - Calculate chunk ranges for each track:
//!   - One-file-per-track: Track boundaries = file boundaries in the stream
//!   - CUE/FLAC: Track boundaries = time-based byte positions from CUE sheet
//! - Output: `AlbumFileLayout` (file→chunk mappings, chunk→track mappings, track chunk counts)
//!
//! ## Phase 3: Streaming & Upload
//! - Stream each file sequentially, producing chunks in order
//! - Encrypt and upload chunks to cloud storage
//! - Track progress by mapping completed chunks back to affected tracks
//!
//! ## Phase 4: Metadata Persistence
//! - Store file records with chunk ranges
//! - Store track position records with chunk ranges and time ranges
//! - Both import types produce the same database structure
//!
//! The key insight: Both import types use identical data structures. The only difference
//! is how we calculate the byte positions and chunk ranges for each track.

use crate::{
    cd::drive::CdToc,
    cue_flac::{CueSheet, FlacHeaders},
    db::{DbAlbum, DbRelease, DbTrack},
    discogs::DiscogsRelease,
    import::handle::TorrentImportMetadata,
    musicbrainz::MbRelease,
};
use std::{collections::HashMap, path::PathBuf};

/// Request to import an album
#[derive(Debug)]
pub enum ImportRequest {
    Folder {
        discogs_release: Option<DiscogsRelease>,
        mb_release: Option<MbRelease>,
        folder: PathBuf,
        master_year: u32,
    },
    Torrent {
        torrent_source: TorrentSource,
        discogs_release: Option<DiscogsRelease>,
        mb_release: Option<MbRelease>,
        master_year: u32,
        seed_after_download: bool,
        torrent_metadata: TorrentImportMetadata,
    },
    CD {
        discogs_release: Option<DiscogsRelease>,
        mb_release: Option<MbRelease>,
        drive_path: PathBuf,
        master_year: u32,
    },
}

/// Source for torrent import
#[derive(Debug, Clone)]
pub enum TorrentSource {
    File(PathBuf),
    MagnetLink(String),
}

/// Progress updates during import
#[derive(Debug, Clone)]
pub enum ImportProgress {
    Started {
        id: String,
    },
    Progress {
        id: String,
        percent: u8,
        /// Phase of import: Acquire (data fetching) or Chunk (upload/encryption)
        /// - Folder imports: Only Chunk phase (acquire is instant)
        /// - Torrent imports: Acquire phase (download), then Chunk phase (upload)
        /// - CD imports: Acquire phase (rip), then Chunk phase (upload)
        phase: Option<ImportPhase>,
    },
    Complete {
        id: String,
    },
    Failed {
        id: String,
        error: String,
    },
}

/// Phase of import process (applies to all import types)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportPhase {
    /// Acquire phase: Get data ready for import
    /// - Folder: No-op (files already available)
    /// - Torrent: Download torrent to temporary folder
    /// - CD: Rip CD tracks to FLAC files
    Acquire,
    /// Chunk phase: Upload and encrypt data to cloud storage
    /// Same for all import types: stream files → encrypt → upload chunks
    Chunk,
}

/// Maps a logical track to its physical audio file.
///
/// Links a track from album metadata (e.g., Discogs) to an audio file provided by the user.
/// Created during Phase 1 (validation).
///
/// Mapping types:
/// - **One-file-per-track**: Each logical track maps to its own file (e.g., "01.flac", "02.flac")
/// - **CUE/FLAC**: Multiple logical tracks map to the same FLAC file (e.g., all tracks → "album.flac")
///
/// After validation, tracks are inserted into the database with status='queued'.
#[derive(Debug, Clone)]
pub struct TrackFile {
    /// Database track ID (UUID) - represents the logical track from metadata
    pub db_track_id: String,
    /// Path to the physical audio file containing this track's audio data
    pub file_path: PathBuf,
}

/// Output of Phase 1: Validated mapping of logical tracks to physical files.
///
/// Links logical tracks (from album metadata) to physical files (from user's folder).
/// Both import types produce these mappings. CUE/FLAC imports additionally include
/// parsed CUE sheet data to avoid re-parsing in later phases.
#[derive(Debug, Clone)]
pub struct TrackToFileMappingResult {
    /// Logical track → physical file mappings (always populated)
    pub track_files: Vec<TrackFile>,
    /// Parsed CUE/FLAC metadata (only for CUE/FLAC imports)
    /// Key: FLAC file path
    /// None for one-file-per-track imports
    pub cue_flac_metadata: Option<HashMap<PathBuf, CueFlacMetadata>>,
}

/// Pre-parsed CUE/FLAC metadata from the track mapping phase.
/// Parsed once during validation, then passed through to avoid re-parsing.
#[derive(Debug, Clone)]
pub struct CueFlacMetadata {
    /// Parsed CUE sheet with track timing and metadata
    pub cue_sheet: CueSheet,
    /// Path to the CUE file
    pub cue_path: PathBuf,
    /// Path to the FLAC file
    pub flac_path: PathBuf,
}

/// A file discovered during folder scan (Phase 1).
///
/// All files in the album folder are discovered and their sizes recorded.
/// This includes audio files, CUE sheets, cover art, and other metadata files.
///
/// Used in Phase 2 to calculate the chunk layout by treating all files
/// as a single concatenated byte stream.
#[derive(Clone, Debug)]
pub struct DiscoveredFile {
    pub path: PathBuf,
    pub size: u64,
}

/// Maps a file to its position in the chunked album stream (Phase 2 output).
///
/// When all album files are concatenated into a single byte stream and divided into
/// fixed-size chunks, this records which chunks each file spans and the byte offsets
/// within the first and last chunks.
///
/// Used during Phase 3 to stream files in the correct sequence and produce chunks.
#[derive(Debug, Clone)]
pub struct FileToChunks {
    pub file_path: PathBuf,
    /// First chunk that contains bytes from this file
    pub start_chunk_index: i32,
    /// Last chunk that contains bytes from this file
    pub end_chunk_index: i32,
    /// Byte offset within start_chunk where this file begins
    pub start_byte_offset: i64,
    /// Byte offset within end_chunk where this file ends
    pub end_byte_offset: i64,
}

/// CUE/FLAC-specific layout data calculated during Phase 2.
///
/// For CUE/FLAC imports, Phase 2 calculates per-track chunk ranges by converting
/// CUE sheet timestamps to byte positions, then to chunk indices. This data is
/// passed to Phase 4 (metadata persistence) alongside the regular file layout.
///
/// Note: Regular imports don't need this because track boundaries = file boundaries.
#[derive(Debug, Clone)]
pub struct CueFlacLayoutData {
    /// Parsed CUE sheet with track timing information
    pub cue_sheet: CueSheet,
    /// Extracted FLAC headers (needed for byte position estimation)
    pub flac_headers: FlacHeaders,
    /// Per-track chunk ranges: track_id → (start_chunk_index, end_chunk_index)
    pub track_chunk_ranges: HashMap<String, (i32, i32)>,
    /// Per-track byte ranges: track_id → (start_byte, end_byte) in file
    pub track_byte_ranges: HashMap<String, (i64, i64)>,
    /// Seektable mapping sample positions to byte positions in the original FLAC file
    /// Used for accurate seeking during playback
    pub seektable: Option<HashMap<u64, u64>>,
}

/// Validated import command ready for pipeline execution.
///
/// All imports follow a two-phase model:
/// - **Acquire phase**: Get data ready (folder: no-op, torrent: download, CD: rip)
/// - **Chunk phase**: Upload and encrypt (same for all types)
///
/// Handle only validates and sends commands. Service executes both phases.
#[derive(Debug)]
pub enum ImportCommand {
    /// Folder-based import: all files available upfront
    Folder {
        /// Database album record
        db_album: DbAlbum,
        /// Database release record
        db_release: DbRelease,
        /// Logical track → physical file mappings
        tracks_to_files: Vec<TrackFile>,
        /// Files discovered during folder scan
        discovered_files: Vec<DiscoveredFile>,
        /// Pre-parsed CUE/FLAC metadata (for CUE/FLAC imports only)
        cue_flac_metadata: Option<HashMap<PathBuf, CueFlacMetadata>>,
    },
    /// Torrent-based import: files arrive incrementally
    Torrent {
        /// Database album record
        db_album: DbAlbum,
        /// Database release record
        db_release: DbRelease,
        /// Logical track → physical file mappings
        tracks_to_files: Vec<TrackFile>,
        /// Torrent source (stored to recreate handle in import service)
        /// We can't send TorrentClient/TorrentHandle through channels as they contain UniquePtr
        torrent_source: TorrentSource,
        /// Torrent-specific metadata
        torrent_metadata: TorrentImportMetadata,
        /// Whether to start seeding after download completes
        seed_after_download: bool,
    },
    /// CD-based import: service will rip CD first (acquire phase), then process like folder import
    CD {
        /// Database album record
        db_album: DbAlbum,
        /// Database release record
        db_release: DbRelease,
        /// Database tracks (for mapping after ripping)
        db_tracks: Vec<DbTrack>,
        /// CD drive path
        drive_path: PathBuf,
        /// CD TOC (Table of Contents) - read during validation
        toc: CdToc,
    },
}
