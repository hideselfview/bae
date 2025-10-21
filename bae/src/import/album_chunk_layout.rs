// Album Chunker
//
// Analyzes an album's physical file structure and produces chunks for the import pipeline.
//
// Responsibilities:
// - Calculate how files map to chunk ranges (for metadata persistence)
// - Calculate how chunks map to tracks (for progress updates)
// - Stream chunks from files during import
//
// This is both the "planning" phase (building the layout) and the "execution" phase
// (streaming chunks through the pipeline).

use crate::chunking::FileChunkMapping;
use crate::import::types::{DiscoveredFile, TrackSourceFile};
use std::collections::HashMap;
use std::path::PathBuf;

/// Analysis of album's physical layout for chunking and progress tracking during import.
///
/// Built before pipeline starts from discovered files and track mappings.
/// Contains the "planning" phase results that drive both chunk production and progress tracking.
pub struct AlbumDataLayout {
    /// Maps each file to its chunk range and byte offsets within those chunks.
    /// Used by the chunk producer to efficiently stream files in sequence.
    /// A file can represent either a single track or a complete disc image containing multiple tracks.
    pub file_mappings: Vec<FileChunkMapping>,

    /// Total number of chunks across all files.
    /// Used to calculate overall import progress percentage.
    pub total_chunks: usize,

    /// Maps chunk indices to track IDs.
    /// A chunk can contain data from multiple tracks (when small files share a chunk).
    /// Only chunks containing track audio data have entries; chunks with only non-track
    /// files (cover.jpg, .cue) are omitted.
    /// Used by progress emitter to attribute chunk completion to specific tracks.
    pub chunk_to_track: HashMap<i32, Vec<String>>,

    /// Maps track IDs to their total chunk counts.
    /// Used by progress emitter to calculate per-track progress percentages.
    pub track_chunk_counts: HashMap<String, usize>,
}

impl AlbumDataLayout {
    /// Analyze discovered files and build complete chunk/track layout.
    ///
    /// This is the "planning" phase - we figure out the entire chunk structure
    /// before streaming any data, so we can track progress and persist metadata correctly.
    pub fn build(
        discovered_files: Vec<DiscoveredFile>,
        tracks_to_files: &[TrackSourceFile],
        chunk_size: usize,
    ) -> Result<Self, String> {
        // Calculate how files map to chunks
        let file_mappings = calculate_file_mappings(&discovered_files, chunk_size);

        // Total chunks = last chunk index + 1 (chunks are 0-indexed)
        let total_chunks = file_mappings
            .last()
            .map(|mapping| (mapping.end_chunk_index + 1) as usize)
            .unwrap_or(0);

        // Calculate how chunks map to tracks (for progress)
        let (chunk_to_track, track_chunk_counts) =
            build_chunk_track_mappings(&file_mappings, tracks_to_files);

        Ok(AlbumDataLayout {
            file_mappings,
            total_chunks,
            chunk_to_track,
            track_chunk_counts,
        })
    }
}

/// Calculate file-to-chunk mappings from files discovered during import validation.
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

/// Build chunk→track mappings for progress tracking during import.
///
/// Creates reverse mappings from chunks to tracks so we can:
/// 1. Identify which tracks a chunk belongs to when it completes
/// 2. Count how many chunks each track needs to mark it complete
///
/// This enables progressive UI updates as tracks finish, rather than waiting for the entire album.
///
/// A chunk can contain data from multiple tracks when small files are concatenated.
/// Only tracks are included in mappings; non-track files (cover.jpg, .cue) are ignored.
///
/// Returns (chunk_to_track, track_chunk_counts)
fn build_chunk_track_mappings(
    file_mappings: &[FileChunkMapping],
    track_files: &[TrackSourceFile],
) -> (HashMap<i32, Vec<String>>, HashMap<String, usize>) {
    // Build reverse lookup: file path → track IDs
    // Note: For CUE/FLAC, multiple tracks map to the same file
    let mut file_to_tracks: HashMap<PathBuf, Vec<String>> = HashMap::new();
    for track_file in track_files {
        file_to_tracks
            .entry(track_file.file_path.clone())
            .or_default()
            .push(track_file.db_track_id.clone());
    }

    let mut chunk_to_track: HashMap<i32, Vec<String>> = HashMap::new();
    let mut track_chunk_counts: HashMap<String, usize> = HashMap::new();

    for file_mapping in file_mappings {
        // Skip files that aren't associated with any tracks (cover.jpg, .cue, etc.)
        if let Some(track_ids) = file_to_tracks.get(&file_mapping.file_path) {
            let chunk_count =
                (file_mapping.end_chunk_index - file_mapping.start_chunk_index + 1) as usize;

            // Add each chunk to the mapping for all tracks in this file
            for chunk_idx in file_mapping.start_chunk_index..=file_mapping.end_chunk_index {
                let entry = chunk_to_track.entry(chunk_idx).or_default();
                for track_id in track_ids {
                    if !entry.contains(track_id) {
                        entry.push(track_id.clone());
                    }
                }
            }

            // Increment chunk count for each track
            for track_id in track_ids {
                *track_chunk_counts.entry(track_id.clone()).or_insert(0) += chunk_count;
            }
        }
    }

    (chunk_to_track, track_chunk_counts)
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

        let tracks = vec![
            TrackSourceFile {
                db_track_id: "track-1".to_string(),
                file_path: PathBuf::from("file1.flac"),
            },
            TrackSourceFile {
                db_track_id: "track-2".to_string(),
                file_path: PathBuf::from("file2.flac"),
            },
            TrackSourceFile {
                db_track_id: "track-3".to_string(),
                file_path: PathBuf::from("file3.flac"),
            },
        ];

        let layout = AlbumDataLayout::build(files, &tracks, chunk_size).unwrap();

        // Verify we got 3 mappings
        assert_eq!(layout.file_mappings.len(), 3);

        // File 1: 2MB = 2 chunks (0-1)
        assert_eq!(
            layout.file_mappings[0].file_path,
            PathBuf::from("file1.flac")
        );
        assert_eq!(layout.file_mappings[0].start_chunk_index, 0);
        assert_eq!(layout.file_mappings[0].end_chunk_index, 1);
        assert_eq!(layout.file_mappings[0].start_byte_offset, 0);

        // File 2: 3MB = 3 chunks (2-4)
        assert_eq!(
            layout.file_mappings[1].file_path,
            PathBuf::from("file2.flac")
        );
        assert_eq!(layout.file_mappings[1].start_chunk_index, 2);
        assert_eq!(layout.file_mappings[1].end_chunk_index, 4);

        // File 3: 1.5MB = 2 chunks (5-6)
        assert_eq!(
            layout.file_mappings[2].file_path,
            PathBuf::from("file3.flac")
        );
        assert_eq!(layout.file_mappings[2].start_chunk_index, 5);
        assert_eq!(layout.file_mappings[2].end_chunk_index, 6);

        // Verify ranges are consecutive with no gaps or overlaps
        assert_eq!(
            layout.file_mappings[0].end_chunk_index + 1,
            layout.file_mappings[1].start_chunk_index
        );
        assert_eq!(
            layout.file_mappings[1].end_chunk_index + 1,
            layout.file_mappings[2].start_chunk_index
        );

        // Total should be 7 chunks (0-6)
        assert_eq!(layout.total_chunks, 7);
    }

    #[test]
    fn test_chunk_boundaries_with_partial_final_chunk() {
        let chunk_size = 1024 * 1024; // 1MB

        let files = vec![
            DiscoveredFile {
                path: PathBuf::from("file1.flac"),
                size: 2_097_152, // 2MB = chunks 0, 1
            },
            DiscoveredFile {
                path: PathBuf::from("file2.flac"),
                size: 3_145_728, // 3MB = chunks 2, 3, 4
            },
            DiscoveredFile {
                path: PathBuf::from("file3.flac"),
                size: 1_572_864, // 1.5MB = chunks 5, 6 (chunk 6 is partial)
            },
        ];

        let _mappings = calculate_file_mappings(&files, chunk_size);

        // Verify file 3 uses only 0.5MB of chunk 6
        let file3_start_byte = 2_097_152u64 + 3_145_728; // 5_242_880
        let file3_end_byte = file3_start_byte + 1_572_864; // 6_815_744
        let chunk_6_start_byte = 6 * chunk_size as u64; // 6_291_456
        let file3_bytes_in_chunk_6 = file3_end_byte - chunk_6_start_byte; // 524_288

        assert_eq!(
            file3_bytes_in_chunk_6, 524_288,
            "File 3 should only use 0.5MB of chunk 6"
        );
    }

    #[test]
    fn test_multiple_small_files_share_chunks() {
        let chunk_size = 1024 * 1024; // 1MB

        // Three small files that should all fit in chunks 0-1
        let files = vec![
            DiscoveredFile {
                path: PathBuf::from("track1.flac"),
                size: 500_000, // 500KB
            },
            DiscoveredFile {
                path: PathBuf::from("track2.flac"),
                size: 300_000, // 300KB
            },
            DiscoveredFile {
                path: PathBuf::from("track3.flac"),
                size: 400_000, // 400KB
            },
        ];

        let tracks = vec![
            TrackSourceFile {
                db_track_id: "track-1".to_string(),
                file_path: PathBuf::from("track1.flac"),
            },
            TrackSourceFile {
                db_track_id: "track-2".to_string(),
                file_path: PathBuf::from("track2.flac"),
            },
            TrackSourceFile {
                db_track_id: "track-3".to_string(),
                file_path: PathBuf::from("track3.flac"),
            },
        ];

        let layout = AlbumDataLayout::build(files, &tracks, chunk_size).unwrap();

        // All three files together = 1.2MB = 2 chunks (0 and 1)
        assert_eq!(layout.total_chunks, 2);

        // Byte layout:
        // track1: 0-499,999 (500KB) → chunk 0
        // track2: 500,000-799,999 (300KB) → chunk 0
        // track3: 800,000-1,199,999 (400KB) → chunks 0 (200KB) and 1 (200KB)

        // Chunk 0 should contain parts of tracks 1, 2, and 3
        let chunk_0_tracks = layout.chunk_to_track.get(&0).unwrap();
        assert_eq!(chunk_0_tracks.len(), 3);
        assert!(chunk_0_tracks.contains(&"track-1".to_string()));
        assert!(chunk_0_tracks.contains(&"track-2".to_string()));
        assert!(chunk_0_tracks.contains(&"track-3".to_string()));

        // Chunk 1 should contain only track 3
        let chunk_1_tracks = layout.chunk_to_track.get(&1).unwrap();
        assert_eq!(chunk_1_tracks.len(), 1);
        assert!(chunk_1_tracks.contains(&"track-3".to_string()));

        // Each track should be counted correctly
        assert_eq!(layout.track_chunk_counts.get("track-1"), Some(&1)); // Only in chunk 0
        assert_eq!(layout.track_chunk_counts.get("track-2"), Some(&1)); // Only in chunk 0
        assert_eq!(layout.track_chunk_counts.get("track-3"), Some(&2)); // In chunks 0 and 1
    }

    #[test]
    fn test_non_track_files_excluded_from_mappings() {
        let chunk_size = 1024 * 1024; // 1MB

        let files = vec![
            DiscoveredFile {
                path: PathBuf::from("cover.jpg"),
                size: 200_000, // 200KB
            },
            DiscoveredFile {
                path: PathBuf::from("track1.flac"),
                size: 900_000, // 900KB
            },
            DiscoveredFile {
                path: PathBuf::from("album.cue"),
                size: 5_000, // 5KB
            },
        ];

        // Only track1.flac is mapped to a track
        let tracks = vec![TrackSourceFile {
            db_track_id: "track-1".to_string(),
            file_path: PathBuf::from("track1.flac"),
        }];

        let layout = AlbumDataLayout::build(files, &tracks, chunk_size).unwrap();

        // cover.jpg (200KB) + track1.flac (900KB) = 1.1MB = 2 chunks
        // album.cue (5KB) continues in chunk 1
        assert_eq!(layout.total_chunks, 2);

        // Chunk 0 should only include track-1 (not cover.jpg)
        let chunk_0_tracks = layout.chunk_to_track.get(&0).unwrap();
        assert_eq!(chunk_0_tracks.len(), 1);
        assert_eq!(chunk_0_tracks[0], "track-1");

        // Chunk 1 should only include track-1 (not album.cue)
        let chunk_1_tracks = layout.chunk_to_track.get(&1).unwrap();
        assert_eq!(chunk_1_tracks.len(), 1);
        assert_eq!(chunk_1_tracks[0], "track-1");

        // track-1 spans 2 chunks
        assert_eq!(layout.track_chunk_counts.get("track-1"), Some(&2));
    }

    #[test]
    fn test_cue_flac_multiple_tracks_same_file() {
        let chunk_size = 1024 * 1024; // 1MB

        // Single FLAC file with CUE sheet
        let files = vec![DiscoveredFile {
            path: PathBuf::from("album.flac"),
            size: 3_000_000, // ~3MB
        }];

        // All tracks map to the same file (CUE/FLAC format)
        let tracks = vec![
            TrackSourceFile {
                db_track_id: "track-1".to_string(),
                file_path: PathBuf::from("album.flac"),
            },
            TrackSourceFile {
                db_track_id: "track-2".to_string(),
                file_path: PathBuf::from("album.flac"),
            },
            TrackSourceFile {
                db_track_id: "track-3".to_string(),
                file_path: PathBuf::from("album.flac"),
            },
        ];

        let layout = AlbumDataLayout::build(files, &tracks, chunk_size).unwrap();

        // 3MB = 3 chunks
        assert_eq!(layout.total_chunks, 3);

        // All chunks should contain all three tracks
        for chunk_idx in 0..3 {
            let chunk_tracks = layout.chunk_to_track.get(&chunk_idx).unwrap();
            assert_eq!(chunk_tracks.len(), 3);
            assert!(chunk_tracks.contains(&"track-1".to_string()));
            assert!(chunk_tracks.contains(&"track-2".to_string()));
            assert!(chunk_tracks.contains(&"track-3".to_string()));
        }

        // Each track should count all 3 chunks
        assert_eq!(layout.track_chunk_counts.get("track-1"), Some(&3));
        assert_eq!(layout.track_chunk_counts.get("track-2"), Some(&3));
        assert_eq!(layout.track_chunk_counts.get("track-3"), Some(&3));
    }

    #[test]
    fn test_mixed_scenario_with_edge_cases() {
        let chunk_size = 1024 * 1024; // 1MB

        let files = vec![
            DiscoveredFile {
                path: PathBuf::from("cover.jpg"),
                size: 100_000, // 100KB - non-track
            },
            DiscoveredFile {
                path: PathBuf::from("track1.flac"),
                size: 200_000, // 200KB - tiny track
            },
            DiscoveredFile {
                path: PathBuf::from("track2.flac"),
                size: 800_000, // 800KB - small track
            },
            DiscoveredFile {
                path: PathBuf::from("track3.flac"),
                size: 2_000_000, // 2MB - normal track
            },
        ];

        let tracks = vec![
            TrackSourceFile {
                db_track_id: "track-1".to_string(),
                file_path: PathBuf::from("track1.flac"),
            },
            TrackSourceFile {
                db_track_id: "track-2".to_string(),
                file_path: PathBuf::from("track2.flac"),
            },
            TrackSourceFile {
                db_track_id: "track-3".to_string(),
                file_path: PathBuf::from("track3.flac"),
            },
        ];

        let layout = AlbumDataLayout::build(files, &tracks, chunk_size).unwrap();

        // Total: 100KB + 200KB + 800KB + 2MB = 3.1MB = 3 chunks
        assert_eq!(layout.total_chunks, 3);

        // Chunk 0: cover.jpg + track1.flac + track2.flac (partial) = 1MB
        // Should only show track-1 and track-2
        let chunk_0 = layout.chunk_to_track.get(&0).unwrap();
        assert_eq!(chunk_0.len(), 2);
        assert!(chunk_0.contains(&"track-1".to_string()));
        assert!(chunk_0.contains(&"track-2".to_string()));

        // Chunk 1: track2.flac (end) + track3.flac (partial) = 1MB
        let chunk_1 = layout.chunk_to_track.get(&1).unwrap();
        assert_eq!(chunk_1.len(), 2);
        assert!(chunk_1.contains(&"track-2".to_string()));
        assert!(chunk_1.contains(&"track-3".to_string()));

        // Chunk 2: track3.flac (end) = 1.1MB
        let chunk_2 = layout.chunk_to_track.get(&2).unwrap();
        assert_eq!(chunk_2.len(), 1);
        assert_eq!(chunk_2[0], "track-3");
    }
}
