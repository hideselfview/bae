// Album Layout Analysis
//
// Analyzes an album's physical file structure to determine:
// - How files map to chunk ranges (for metadata persistence)
// - How chunks map to tracks (for progress updates)
// - Total chunk count (for progress calculation)
//
// This is the "planning" phase before the streaming pipeline starts.

use crate::chunking::FileChunkMapping;
use crate::import::service::DiscoveredFile;
use crate::import::types::TrackSourceFile;
use std::collections::HashMap;
use std::path::PathBuf;

/// Complete analysis of album's physical layout for chunking and progress tracking.
///
/// Built once before pipeline starts by analyzing discovered files and track mappings.
/// Contains everything needed to stream chunks and track progress.
pub struct AlbumLayout {
    pub file_mappings: Vec<FileChunkMapping>,
    pub total_chunks: usize,
    pub progress_tracker: TrackProgressTracker,
}

impl AlbumLayout {
    /// Analyze discovered files and build complete chunk/track layout.
    ///
    /// This is the "planning" phase - we figure out the entire chunk structure
    /// before streaming any data, so we can track progress and persist metadata correctly.
    pub fn analyze(
        discovered_files: &[DiscoveredFile],
        tracks_to_files: &[TrackSourceFile],
        chunk_size: usize,
    ) -> Result<Self, String> {
        // Calculate how files map to chunks
        let file_mappings = calculate_file_mappings(discovered_files, chunk_size);

        // Total chunks = last chunk index + 1 (chunks are 0-indexed)
        let total_chunks = file_mappings
            .last()
            .map(|mapping| (mapping.end_chunk_index + 1) as usize)
            .unwrap_or(0);

        // Calculate how chunks map to tracks (for progress)
        let progress_tracker = build_progress_tracker(&file_mappings, tracks_to_files);

        Ok(AlbumLayout {
            file_mappings,
            total_chunks,
            progress_tracker,
        })
    }
}

/// Tracks which chunks belong to which tracks for progress updates.
///
/// Built before pipeline starts by mapping file ranges to chunk indices.
/// Used during pipeline to determine when a track is complete (all its chunks uploaded).
///
/// Example:
/// ```
/// chunk_to_track: { 0 -> "track-id-1", 1 -> "track-id-1", 2 -> "track-id-2", ... }
/// track_chunk_counts: { "track-id-1" -> 2, "track-id-2" -> 3, ... }
/// ```
pub struct TrackProgressTracker {
    pub chunk_to_track: HashMap<i32, String>,
    pub track_chunk_counts: HashMap<String, usize>,
}

/// Calculate file mappings from already-discovered files.
///
/// Treats all files as a single concatenated byte stream, divided into fixed-size chunks.
/// Each file mapping records which chunks it spans and byte offsets within those chunks.
/// This enables efficient streaming: open each file once, read its chunks sequentially.
fn calculate_file_mappings(files: &[DiscoveredFile], chunk_size: usize) -> Vec<FileChunkMapping> {
    let mut file_mappings = Vec::new();
    let mut total_bytes_processed = 0u64;

    for file in files {
        let start_byte = total_bytes_processed;
        let end_byte = total_bytes_processed + file.size;

        let start_chunk_index = (start_byte / chunk_size as u64) as i32;
        let end_chunk_index = ((end_byte - 1) / chunk_size as u64) as i32;

        file_mappings.push(FileChunkMapping {
            file_path: file.path.clone(),
            start_chunk_index,
            end_chunk_index,
            start_byte_offset: (start_byte % chunk_size as u64) as i64,
            end_byte_offset: ((end_byte - 1) % chunk_size as u64) as i64,
        });

        total_bytes_processed = end_byte;
    }

    file_mappings
}

/// Build progress tracker for tracks during import.
///
/// Creates reverse mappings from chunks to tracks so we can:
/// 1. Identify which track a chunk belongs to when it completes
/// 2. Count how many chunks each track needs to mark it complete
///
/// This enables progressive UI updates as tracks finish, rather than waiting for the entire album.
fn build_progress_tracker(
    file_mappings: &[FileChunkMapping],
    track_files: &[TrackSourceFile],
) -> TrackProgressTracker {
    let mut file_to_track: HashMap<PathBuf, String> = HashMap::new();
    for track_file in track_files {
        file_to_track.insert(track_file.file_path.clone(), track_file.db_track_id.clone());
    }

    let mut chunk_to_track: HashMap<i32, String> = HashMap::new();
    let mut track_chunk_counts: HashMap<String, usize> = HashMap::new();

    for file_mapping in file_mappings {
        if let Some(track_id) = file_to_track.get(&file_mapping.file_path) {
            let chunk_count =
                (file_mapping.end_chunk_index - file_mapping.start_chunk_index + 1) as usize;

            for chunk_idx in file_mapping.start_chunk_index..=file_mapping.end_chunk_index {
                chunk_to_track.insert(chunk_idx, track_id.clone());
            }

            *track_chunk_counts.entry(track_id.clone()).or_insert(0) += chunk_count;
        }
    }

    TrackProgressTracker {
        chunk_to_track,
        track_chunk_counts,
    }
}
