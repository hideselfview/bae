use crate::db::{DbAudioFormat, DbFile, DbTrackChunkCoords};
use crate::import::types::{CueFlacLayoutData, FileToChunks, TrackFile};
use crate::library::LibraryManager;
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::debug;

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

            // For CUE/FLAC, we store the original album FLAC headers and seektable
            // Playback will download track's chunks, prepend headers,
            // and use Symphonia to seek to the track's time position and decode
            // The seektable enables accurate seeking by mapping sample positions to byte positions
            let flac_seektable = if let Some(ref seektable) = cue_flac_layout.seektable {
                Some(
                    bincode::serialize(seektable)
                        .map_err(|e| format!("Failed to serialize seektable: {}", e))?,
                )
            } else {
                None
            };

            let audio_format = DbAudioFormat::new_with_seektable(
                track_id,
                "flac",
                Some(cue_flac_layout.flac_headers.headers.clone()), // Original album headers
                flac_seektable, // Serialized seektable for accurate seeking
                true,           // needs_headers = true for CUE/FLAC
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
