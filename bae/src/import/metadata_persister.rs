use crate::db::{DbAudioFormat, DbFile, DbTrackChunkCoords};
use crate::import::types::{CueFlacLayoutData, FileToChunks, TrackFile};
use crate::library::LibraryManager;
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{debug, warn};

/// Service responsible for persisting track metadata to the database.
///
/// After the streaming pipeline uploads all chunks, this service creates:
/// - DbFile records (for export/torrent metadata only)
/// - DbAudioFormat records (one per track - format + optional FLAC headers)
/// - DbTrackChunkCoords records (one per track - precise chunk coordinates)
///
/// Post-import, playback only needs TrackChunkCoords + AudioFormat.
/// Files are metadata-only for export/torrent reconstruction.
pub struct MetadataPersister<'a> {
    library: &'a LibraryManager,
}

impl<'a> MetadataPersister<'a> {
    /// Create a new metadata persister
    pub fn new(library: &'a LibraryManager) -> Self {
        Self { library }
    }

    /// Persist metadata for a single track.
    ///
    /// Persists the track's chunk coordinates and audio format needed for playback.
    /// This is called immediately when a track's chunks complete, before marking it complete.
    ///
    /// Returns Ok(()) if the track's metadata was successfully persisted.
    pub async fn persist_track_metadata(
        &self,
        _release_id: &str,
        track_id: &str,
        track_files: &[TrackFile],
        files_to_chunks: &[FileToChunks],
        _chunk_size_bytes: usize,
        cue_flac_data: &HashMap<PathBuf, CueFlacLayoutData>,
    ) -> Result<(), String> {
        // Find the TrackFile for this track
        let track_file = track_files
            .iter()
            .find(|tf| tf.db_track_id == track_id)
            .ok_or_else(|| format!("Track {} not found in track_files", track_id))?;

        // Find the FileToChunks for this track's file
        let file_to_chunks = files_to_chunks
            .iter()
            .find(|ftc| ftc.file_path == track_file.file_path)
            .ok_or_else(|| {
                format!(
                    "No chunk mapping found for file: {}",
                    track_file.file_path.display()
                )
            })?;

        let format = track_file
            .file_path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("unknown")
            .to_lowercase();

        // Check if this is part of a CUE/FLAC file
        // A CUE/FLAC file will have multiple tracks mapping to the same file
        let is_cue_flac = track_files
            .iter()
            .filter(|tf| tf.file_path == track_file.file_path)
            .count()
            > 1
            && format == "flac";

        if is_cue_flac {
            // Get CUE/FLAC layout data
            let cue_flac_layout = cue_flac_data.get(&track_file.file_path).ok_or_else(|| {
                format!(
                    "No pre-calculated CUE/FLAC data found for {}",
                    track_file.file_path.display()
                )
            })?;

            // Find the CUE track corresponding to this track
            let cue_track = cue_flac_layout
                .cue_sheet
                .tracks
                .iter()
                .enumerate()
                .find_map(|(idx, ct)| {
                    // Match by position in the file (assumes order matches)
                    track_files
                        .iter()
                        .filter(|tf| tf.file_path == track_file.file_path)
                        .nth(idx)
                        .filter(|tf| tf.db_track_id == track_id)
                        .map(|_| ct)
                })
                .ok_or_else(|| {
                    format!(
                        "Could not find CUE track corresponding to track {}",
                        track_id
                    )
                })?;

            // Get pre-calculated chunk range for this track
            let (start_chunk_index, end_chunk_index) = cue_flac_layout
                .track_chunk_ranges
                .get(track_id)
                .ok_or_else(|| format!("No chunk range found for track {}", track_id))?;

            // Get the actual byte positions from album_chunk_layout
            // These are stored in the track_byte_ranges map
            let (start_byte, end_byte) = cue_flac_layout
                .track_byte_ranges
                .get(track_id)
                .ok_or_else(|| format!("No byte range found for track {}", track_id))?;

            // Convert absolute byte positions to offsets within the chunk range
            let chunk_size_i64 = _chunk_size_bytes as i64;
            let start_byte_offset = start_byte % chunk_size_i64;
            let end_byte_offset = end_byte % chunk_size_i64;

            debug!(
                "Track {}: storing byte offsets {}-{} within chunks {}-{}",
                track_id, start_byte_offset, end_byte_offset, start_chunk_index, end_chunk_index
            );

            // For CUE/FLAC, we create track-specific headers with an injected seektable
            // The seektable is track-relative (sample 0 = track start) so Symphonia can seek efficiently
            let (track_headers, serialized_seektable) = if let Some(ref album_seektable) =
                cue_flac_layout.seektable
            {
                use crate::cue_flac::update_headers_with_seektable;
                use std::collections::HashMap;

                // Calculate track's sample range from time range
                // Assume 44.1kHz sample rate (standard for CD audio)
                let sample_rate = 44100u64;
                let track_start_sample = (cue_track.start_time_ms * sample_rate) / 1000;
                let track_end_sample = if let Some(end_ms) = cue_track.end_time_ms {
                    (end_ms * sample_rate) / 1000
                } else {
                    u64::MAX // Last track goes to end of file
                };

                // Filter album seektable to track range and make track-relative
                // We filter by byte range (authoritative) and adjust sample numbers to be track-relative

                // First, collect seekpoints within the track's byte range
                let filtered_seekpoints: Vec<(u64, u64)> = album_seektable
                    .iter()
                    .filter_map(|(sample, byte)| {
                        if *byte >= *start_byte as u64 && *byte < *end_byte as u64 {
                            Some((*sample, *byte))
                        } else {
                            None
                        }
                    })
                    .collect();

                // Find the minimum sample number in the filtered set
                // This will be our "sample 0" for the track
                let min_sample = filtered_seekpoints
                    .iter()
                    .map(|(sample, _)| *sample)
                    .min()
                    .unwrap_or(0);

                // Now make everything relative to the track's actual start
                warn!(
                    "Track {}: making seekpoints track-relative (start_byte: {}, end_byte: {})",
                    track_id, start_byte, end_byte
                );
                let track_seektable: HashMap<u64, u64> = filtered_seekpoints
                    .into_iter()
                    .map(|(sample, byte)| {
                        let track_relative_sample = sample.saturating_sub(min_sample);
                        let track_relative_byte = byte - (*start_byte as u64);
                        (track_relative_sample, track_relative_byte)
                    })
                    .collect();

                if track_seektable.is_empty() {
                    warn!(
                        "Track {}: No seekpoints found in track range! This will cause ForwardOnly errors. Track range: samples {}-{}, bytes {}-{}",
                        track_id, track_start_sample, track_end_sample, start_byte, end_byte
                    );
                } else {
                    warn!(
                        "Track {}: created track-specific seektable with {} seekpoints (album had {})",
                        track_id,
                        track_seektable.len(),
                        album_seektable.len()
                    );

                    // Log first and last seekpoints for debugging
                    if let (Some(first_sample), Some(last_sample)) =
                        (track_seektable.keys().min(), track_seektable.keys().max())
                    {
                        let actual_track_duration_s = (last_sample - first_sample) / sample_rate;
                        warn!(
                            "Track {}: seektable range: sample {} to {} (track duration: ~{}s)",
                            track_id, first_sample, last_sample, actual_track_duration_s
                        );

                        // Log seekpoint density
                        // Use actual seektable range for duration (handles last track correctly)
                        let actual_duration_samples = last_sample - first_sample;
                        let avg_samples_per_seekpoint = if track_seektable.len() > 1 {
                            actual_duration_samples / (track_seektable.len() as u64 - 1)
                        } else {
                            0
                        };
                        let avg_seconds_per_seekpoint =
                            avg_samples_per_seekpoint as f64 / sample_rate as f64;
                        warn!(
                            "Track {}: seekpoint density: ~{:.1}s between seekpoints (avg {} samples)",
                            track_id, avg_seconds_per_seekpoint, avg_samples_per_seekpoint
                        );
                    }
                }

                // Use metaflac library to properly update STREAMINFO and inject seektable
                // This is much cleaner than manual byte manipulation!
                let (track_headers, serialized_seektable) = if let (
                    Some(first_sample),
                    Some(last_sample),
                ) =
                    (track_seektable.keys().min(), track_seektable.keys().max())
                {
                    let track_total_samples = last_sample - first_sample;

                    // First pass: create headers to determine header size
                    let temp_headers = update_headers_with_seektable(
                        &cue_flac_layout.flac_headers.headers,
                        track_total_samples,
                        &track_seektable,
                    )
                    .map_err(|e| format!("Failed to create temp headers: {}", e))?;

                    let header_size = temp_headers.len() as u64;

                    // Second pass: adjust seektable offsets to account for prepended headers
                    // When StreamingChunkSource prepends headers, Symphonia expects seektable
                    // offsets to be relative to the first audio frame AFTER the headers
                    let track_seektable_with_header_offset: HashMap<u64, u64> = track_seektable
                        .iter()
                        .map(|(sample, byte)| (*sample, byte + header_size))
                        .collect();

                    // Create final headers with adjusted seektable
                    let final_headers = update_headers_with_seektable(
                        &cue_flac_layout.flac_headers.headers,
                        track_total_samples,
                        &track_seektable_with_header_offset,
                    )
                    .map_err(|e| format!("Failed to update headers: {}", e))?;

                    let final_header_size = final_headers.len();

                    // Debug: log some seekpoints to verify they're correct
                    let mut sorted_samples: Vec<u64> =
                        track_seektable_with_header_offset.keys().copied().collect();
                    sorted_samples.sort();
                    if sorted_samples.len() >= 3 {
                        let first = sorted_samples[0];
                        let mid = sorted_samples[sorted_samples.len() / 2];
                        let last = sorted_samples[sorted_samples.len() - 1];
                        warn!(
                                "Track {}: Seekpoint samples: first={} (offset={}), mid={} (offset={}), last={} (offset={})",
                                track_id,
                                first, track_seektable_with_header_offset.get(&first).unwrap(),
                                mid, track_seektable_with_header_offset.get(&mid).unwrap(),
                                last, track_seektable_with_header_offset.get(&last).unwrap(),
                            );
                    }

                    warn!(
                        "Track {}: created headers with {} seekpoints (header size: {} bytes)",
                        track_id,
                        track_seektable_with_header_offset.len(),
                        final_header_size
                    );

                    // Serialize for storage
                    let serialized = Some(
                        bincode::serialize(&track_seektable_with_header_offset)
                            .map_err(|e| format!("Failed to serialize track seektable: {}", e))?,
                    );

                    (final_headers, serialized)
                } else {
                    (cue_flac_layout.flac_headers.headers.clone(), None)
                };

                (track_headers, serialized_seektable)
            } else {
                // No seektable available
                (cue_flac_layout.flac_headers.headers.clone(), None)
            };

            let audio_format = DbAudioFormat::new_with_seektable(
                track_id,
                "flac",
                Some(track_headers), // Headers with track-specific seektable injected
                serialized_seektable, // Serialized track-relative seektable
                true,                // needs_headers = true for CUE/FLAC
            );
            self.library
                .add_audio_format(&audio_format)
                .await
                .map_err(|e| format!("Failed to insert audio format: {}", e))?;

            // Create track chunk coordinates
            // Byte offsets: which chunks and bytes within them contain the track
            // Time offsets: where to seek with Symphonia during decode
            let coords = DbTrackChunkCoords::new(
                track_id,
                *start_chunk_index,
                *end_chunk_index,
                start_byte_offset,
                end_byte_offset,
                cue_track.start_time_ms as i64,
                cue_track.end_time_ms.unwrap_or(0) as i64,
            );
            self.library
                .add_track_chunk_coords(&coords)
                .await
                .map_err(|e| format!("Failed to insert track chunk coords: {}", e))?;
        } else {
            // Regular one-file-per-track: use single file logic
            // Create audio format (no headers for one-file-per-track)
            let audio_format = DbAudioFormat::new(
                track_id, &format, None,  // No headers - they're already in the chunks
                false, // needs_headers = false for regular files
            );
            self.library
                .add_audio_format(&audio_format)
                .await
                .map_err(|e| format!("Failed to insert audio format: {}", e))?;

            // Create track chunk coordinates
            // For one-file-per-track, the track boundaries = file boundaries in the stream
            let coords = DbTrackChunkCoords::new(
                track_id,
                file_to_chunks.start_chunk_index,
                file_to_chunks.end_chunk_index,
                file_to_chunks.start_byte_offset,
                file_to_chunks.end_byte_offset,
                0, // start_time_ms: 0 = beginning (metadata only)
                0, // end_time_ms: 0 = end (metadata only)
            );
            self.library
                .add_track_chunk_coords(&coords)
                .await
                .map_err(|e| format!("Failed to insert track chunk coords: {}", e))?;
        }

        Ok(())
    }

    /// Persist release-level metadata to database.
    ///
    /// Creates DbFile records for all files in the release (for export metadata).
    /// Track-level metadata (DbAudioFormat and DbTrackChunkCoords) is persisted
    /// per-track as tracks complete via `persist_track_metadata()`.
    pub async fn persist_release_metadata(
        &self,
        release_id: &str,
        track_files: &[TrackFile],
        files_to_chunks: &[FileToChunks],
    ) -> Result<(), String> {
        // Collect unique file paths from tracks
        let mut unique_file_paths: std::collections::HashSet<&PathBuf> =
            track_files.iter().map(|tf| &tf.file_path).collect();

        // Also include files that might not be associated with tracks (cover.jpg, etc.)
        for file_to_chunks in files_to_chunks {
            unique_file_paths.insert(&file_to_chunks.file_path);
        }

        // Create DbFile record for each unique file
        for file_path in unique_file_paths {
            let file_metadata = std::fs::metadata(file_path)
                .map_err(|e| format!("Failed to read file metadata: {}", e))?;
            let file_size = file_metadata.len() as i64;
            let format = file_path
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("unknown")
                .to_lowercase();

            let filename = file_path.file_name().unwrap().to_str().unwrap();
            let db_file = DbFile::new(release_id, filename, file_size, &format);
            self.library
                .add_file(&db_file)
                .await
                .map_err(|e| format!("Failed to insert file: {}", e))?;
        }

        Ok(())
    }
}
