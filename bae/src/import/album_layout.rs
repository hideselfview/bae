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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_file_mappings_integration_test_sizes() {
        let chunk_size = 1024 * 1024; // 1MB

        // Three files with exact sizes from integration test
        let files = vec![
            DiscoveredFile {
                path: PathBuf::from("file1.flac"),
                size: 2_097_152, // 2MB
            },
            DiscoveredFile {
                path: PathBuf::from("file2.flac"),
                size: 3_145_728, // 3MB
            },
            DiscoveredFile {
                path: PathBuf::from("file3.flac"),
                size: 1_572_864, // 1.5MB
            },
        ];

        let mappings = calculate_file_mappings(&files, chunk_size);

        // Verify we got 3 mappings
        assert_eq!(mappings.len(), 3);

        // File 1: 2MB = 2 chunks (0-1)
        assert_eq!(mappings[0].file_path, PathBuf::from("file1.flac"));
        assert_eq!(mappings[0].start_chunk_index, 0);
        assert_eq!(mappings[0].end_chunk_index, 1);
        assert_eq!(mappings[0].start_byte_offset, 0);

        // File 2: 3MB = 3 chunks (2-4)
        assert_eq!(mappings[1].file_path, PathBuf::from("file2.flac"));
        assert_eq!(mappings[1].start_chunk_index, 2);
        assert_eq!(mappings[1].end_chunk_index, 4);

        // File 3: 1.5MB = 2 chunks (5-6)
        assert_eq!(mappings[2].file_path, PathBuf::from("file3.flac"));
        assert_eq!(mappings[2].start_chunk_index, 5);
        assert_eq!(mappings[2].end_chunk_index, 6);

        // Verify ranges are consecutive with no gaps or overlaps
        assert_eq!(
            mappings[0].end_chunk_index + 1,
            mappings[1].start_chunk_index
        );
        assert_eq!(
            mappings[1].end_chunk_index + 1,
            mappings[2].start_chunk_index
        );

        // Total should be 7 chunks (0-6)
        let total_chunks = (mappings[2].end_chunk_index + 1) as usize;
        assert_eq!(total_chunks, 7);
    }

    #[test]
    fn test_chunk_boundaries_reveal_the_bug() {
        // This test demonstrates THE BUG: when we reassemble a file from chunks,
        // we get the ENTIRE chunk, including padding bytes that belong to the next file.

        let chunk_size = 1024 * 1024; // 1MB

        let files = vec![
            DiscoveredFile {
                path: PathBuf::from("file1.flac"),
                size: 2_097_152, // Exactly 2MB = chunks 0, 1
            },
            DiscoveredFile {
                path: PathBuf::from("file2.flac"),
                size: 3_145_728, // Exactly 3MB = chunks 2, 3, 4
            },
            DiscoveredFile {
                path: PathBuf::from("file3.flac"),
                size: 1_572_864, // 1.5MB = chunks 5, 6 (but only 0.5MB of chunk 6!)
            },
        ];

        let mappings = calculate_file_mappings(&files, chunk_size);

        // File 3 calculation:
        let file3_start_byte = 2_097_152u64 + 3_145_728;
        let file3_end_byte = file3_start_byte + 1_572_864;

        println!("File 3 start: {}", file3_start_byte);
        println!("File 3 end: {}", file3_end_byte);

        // Chunk 5 spans bytes: 5_242_880 to 6_291_455 (1MB)
        // Chunk 6 spans bytes: 6_291_456 to 7_340_031 (1MB)

        // File 3 data occupies:
        // - ALL of chunk 5: bytes 5_242_880 to 6_291_455 (1MB)
        // - PART of chunk 6: bytes 6_291_456 to 6_815_743 (524,288 bytes = 0.5MB)

        let chunk_6_start_byte = 6 * chunk_size as u64;
        let file3_bytes_in_chunk_6 = file3_end_byte - chunk_6_start_byte;

        println!("Chunk 6 starts at byte: {}", chunk_6_start_byte);
        println!("File 3 ends at byte: {}", file3_end_byte);
        println!("File 3 bytes in chunk 6: {}", file3_bytes_in_chunk_6);

        assert_eq!(file3_bytes_in_chunk_6, 524_288);

        // THE BUG: When we retrieve "all chunks for file 3" from the database,
        // we get chunks 5 and 6 in their ENTIRETY (2MB total).
        // But file 3 is only 1.5MB, so we get 0.5MB of EXTRA DATA!

        let reassembled_size_from_full_chunks = 2 * chunk_size; // Chunks 5 and 6
        let actual_file_size = files[2].size;
        let extra_bytes = reassembled_size_from_full_chunks - actual_file_size as usize;

        println!("Expected file size: {} bytes", actual_file_size);
        println!(
            "Reassembled from full chunks: {} bytes",
            reassembled_size_from_full_chunks
        );
        println!("Extra bytes: {} bytes", extra_bytes);

        // This matches the integration test failure!
        // Expected: 1,572,864
        // Got: 2,097,152 (1,572,864 + 524,288)
        // But wait, the integration test said we got 1,675,264 bytes, not 2,097,152...

        // Let me recalculate: 1,675,264 - 1,572,864 = 102,400 bytes extra
        // That's not 524,288...

        // Maybe there's a different issue? Let me think about this differently.
    }
}
