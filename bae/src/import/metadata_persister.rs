use crate::db::{DbAudioFormat, DbFile, DbTrackChunkCoords};
use crate::import::types::{CueFlacLayoutData, FileToChunks, TrackFile};
use crate::library::LibraryManager;
use crate::playback::symphonia_decoder::TrackDecoder;
use std::collections::HashMap;
use std::path::Path;
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

    /// Extract duration from an audio file
    fn extract_duration_from_file(file_path: &Path) -> Option<i64> {
        debug!("Extracting duration from file: {}", file_path.display());
        let file_data = match std::fs::read(file_path) {
            Ok(data) => {
                debug!("Read {} bytes from file", data.len());
                data
            }
            Err(e) => {
                warn!("Failed to read file for duration extraction: {}", e);
                return None;
            }
        };

        match TrackDecoder::new(file_data) {
            Ok(decoder) => {
                let duration = decoder.duration().map(|d| d.as_millis() as i64);
                if let Some(dur_ms) = duration {
                    debug!(
                        "Extracted duration: {} ms from {}",
                        dur_ms,
                        file_path.display()
                    );
                } else {
                    warn!("Duration not available for file: {}", file_path.display());
                }
                duration
            }
            Err(e) => {
                warn!("Failed to decode file for duration extraction: {:?}", e);
                None
            }
        }
    }

    /// Persist all release metadata to database.
    ///
    /// For each track:
    /// - Creates DbAudioFormat (format + optional FLAC headers)
    /// - Creates DbTrackChunkCoords (precise chunk coordinates)
    /// - Creates DbFile records (for export metadata only)
    ///
    /// `cue_flac_data` contains pre-calculated CUE/FLAC layout data from album layout analysis.
    /// This avoids duplicate parsing and ensures consistency.
    pub async fn persist_album_metadata(
        &self,
        release_id: &str,
        track_files: &[TrackFile],
        files_to_chunks: Vec<FileToChunks>,
        chunk_size_bytes: usize,
        cue_flac_data: HashMap<std::path::PathBuf, CueFlacLayoutData>,
    ) -> Result<(), String> {
        // Create a lookup map for chunk mappings by file path
        let chunk_lookup: HashMap<&Path, &FileToChunks> = files_to_chunks
            .iter()
            .map(|mapping| (mapping.file_path.as_path(), mapping))
            .collect();

        // Group track mappings by source file to handle CUE/FLAC
        let mut file_groups: HashMap<&Path, Vec<&TrackFile>> = HashMap::new();
        for mapping in track_files {
            file_groups
                .entry(mapping.file_path.as_path())
                .or_default()
                .push(mapping);
        }

        for (source_path, file_mappings) in file_groups {
            let file_to_chunks = chunk_lookup.get(source_path).ok_or_else(|| {
                format!("No chunk mapping found for file: {}", source_path.display())
            })?;

            // Get file metadata
            let file_metadata = std::fs::metadata(source_path)
                .map_err(|e| format!("Failed to read file metadata: {}", e))?;
            let file_size = file_metadata.len() as i64;
            let format = source_path
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("unknown")
                .to_lowercase();

            // Check if this is a CUE/FLAC file
            let is_cue_flac = file_mappings.len() > 1 && format == "flac";

            if is_cue_flac {
                debug!(
                    "  Processing CUE/FLAC file with {} tracks",
                    file_mappings.len()
                );
                // Get pre-calculated CUE/FLAC data (should always be present)
                let cue_flac_layout = cue_flac_data.get(source_path).ok_or_else(|| {
                    format!(
                        "No pre-calculated CUE/FLAC data found for {}",
                        source_path.display()
                    )
                })?;
                self.persist_cue_flac_metadata(
                    release_id,
                    file_mappings,
                    file_to_chunks,
                    file_size,
                    chunk_size_bytes,
                    cue_flac_layout,
                )
                .await?;
            } else {
                // Process as individual file
                for mapping in file_mappings {
                    self.persist_individual_file(
                        release_id,
                        mapping,
                        file_to_chunks,
                        file_size,
                        &format,
                    )
                    .await?;
                }
            }
        }

        Ok(())
    }

    /// Persist CUE/FLAC file metadata - audio format, track coordinates, and file record
    ///
    /// For CUE/FLAC albums:
    /// - Create DbAudioFormat for each track (format="flac", with headers, needs_headers=true)
    /// - Create DbTrackChunkCoords for each track (calculated from CUE timestamps)
    /// - Create DbFile record (for export metadata only)
    ///
    /// `cue_flac_layout` contains pre-calculated data from album layout analysis.
    async fn persist_cue_flac_metadata(
        &self,
        release_id: &str,
        file_mappings: Vec<&TrackFile>,
        files_to_chunks: &FileToChunks,
        file_size: i64,
        chunk_size_bytes: usize,
        cue_flac_layout: &CueFlacLayoutData,
    ) -> Result<(), String> {
        // Use pre-calculated data
        let cue_sheet = &cue_flac_layout.cue_sheet;
        let flac_headers = &cue_flac_layout.flac_headers;

        // Create file record for export metadata (not needed for playback)
        let filename = files_to_chunks
            .file_path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap();
        let db_file = DbFile::new(release_id, filename, file_size, "flac");
        self.library
            .add_file(&db_file)
            .await
            .map_err(|e| format!("Failed to insert file: {}", e))?;

        // Create audio format and coordinates for each track
        for (mapping, cue_track) in file_mappings.iter().zip(cue_sheet.tracks.iter()) {
            // Get pre-calculated chunk range for this track
            let (start_chunk_index, end_chunk_index) = *cue_flac_layout
                .track_chunk_ranges
                .get(&mapping.db_track_id)
                .ok_or_else(|| format!("No chunk range found for track {}", mapping.db_track_id))?;

            // Calculate track byte boundaries within the file (same logic as album_file_layout.rs)
            use crate::cue_flac::CueFlacProcessor;
            let start_byte = CueFlacProcessor::estimate_byte_position(
                cue_track.start_time_ms,
                flac_headers,
                file_size as u64,
            ) as i64;

            let end_byte = cue_track
                .end_time_ms
                .map(|end_time| {
                    CueFlacProcessor::estimate_byte_position(
                        end_time,
                        flac_headers,
                        file_size as u64,
                    ) as i64
                })
                .unwrap_or(file_size);

            // Convert to absolute chunk positions (relative to album, not file)
            let chunk_size_i64 = chunk_size_bytes as i64;
            let file_start_byte = files_to_chunks.start_byte_offset
                + (files_to_chunks.start_chunk_index as i64 * chunk_size_i64);
            let absolute_start_byte = file_start_byte + start_byte;
            let absolute_end_byte = file_start_byte + end_byte;

            // Calculate byte offsets within the start and end chunks
            let start_byte_offset = absolute_start_byte % chunk_size_i64;
            let end_byte_offset = (absolute_end_byte - 1) % chunk_size_i64;

            // Create audio format (with FLAC headers for CUE/FLAC)
            let audio_format = DbAudioFormat::new(
                &mapping.db_track_id,
                "flac",
                Some(flac_headers.headers.clone()),
                true, // needs_headers = true for CUE/FLAC
            );
            self.library
                .add_audio_format(&audio_format)
                .await
                .map_err(|e| format!("Failed to insert audio format: {}", e))?;

            // Create track chunk coordinates
            let coords = DbTrackChunkCoords::new(
                &mapping.db_track_id,
                start_chunk_index,
                end_chunk_index,
                start_byte_offset,
                end_byte_offset,
                cue_track.start_time_ms as i64,
                cue_track.end_time_ms.unwrap_or(0) as i64,
            );
            self.library
                .add_track_chunk_coords(&coords)
                .await
                .map_err(|e| format!("Failed to insert track chunk coords: {}", e))?;

            // Calculate and store duration from CUE sheet times
            let duration_ms = if let Some(end_time) = cue_track.end_time_ms {
                Some((end_time - cue_track.start_time_ms) as i64)
            } else {
                // Last track - calculate from file duration
                let file_duration = Self::extract_duration_from_file(&files_to_chunks.file_path);
                file_duration
                    .map(|file_duration_ms| file_duration_ms - cue_track.start_time_ms as i64)
            };

            debug!(
                "Updating CUE track {} duration to {:?} ms",
                mapping.db_track_id, duration_ms
            );
            self.library
                .update_track_duration(&mapping.db_track_id, duration_ms)
                .await
                .map_err(|e| format!("Failed to update track duration: {}", e))?;
        }

        Ok(())
    }

    /// Persist individual file metadata - audio format, track coordinates, and file record
    ///
    /// For regular albums (1 file = 1 track):
    /// - Create DbAudioFormat (no headers, needs_headers=false)
    /// - Create DbTrackChunkCoords (byte offsets match file chunk range)
    /// - Create DbFile record (for export metadata only)
    async fn persist_individual_file(
        &self,
        release_id: &str,
        mapping: &TrackFile,
        files_to_chunks: &FileToChunks,
        file_size: i64,
        format: &str,
    ) -> Result<(), String> {
        let filename = mapping.file_path.file_name().unwrap().to_str().unwrap();

        // Create file record for export metadata (not needed for playback)
        let db_file = DbFile::new(release_id, filename, file_size, format);
        self.library
            .add_file(&db_file)
            .await
            .map_err(|e| format!("Failed to insert file: {}", e))?;

        // Create audio format (no headers for one-file-per-track)
        let audio_format = DbAudioFormat::new(
            &mapping.db_track_id,
            format,
            None,  // No headers - they're already in the chunks
            false, // needs_headers = false for regular files
        );
        self.library
            .add_audio_format(&audio_format)
            .await
            .map_err(|e| format!("Failed to insert audio format: {}", e))?;

        // Create track chunk coordinates
        // For one-file-per-track, the track boundaries = file boundaries in the stream
        // Byte offsets are the file's offsets within chunks
        let coords = DbTrackChunkCoords::new(
            &mapping.db_track_id,
            files_to_chunks.start_chunk_index,
            files_to_chunks.end_chunk_index,
            files_to_chunks.start_byte_offset,
            files_to_chunks.end_byte_offset,
            0, // start_time_ms: 0 = beginning (metadata only)
            0, // end_time_ms: 0 = end (metadata only)
        );
        self.library
            .add_track_chunk_coords(&coords)
            .await
            .map_err(|e| format!("Failed to insert track chunk coords: {}", e))?;

        // Extract and store duration from audio file
        let duration_ms = Self::extract_duration_from_file(&mapping.file_path);
        debug!(
            "Updating track {} duration to {:?} ms",
            mapping.db_track_id, duration_ms
        );
        self.library
            .update_track_duration(&mapping.db_track_id, duration_ms)
            .await
            .map_err(|e| format!("Failed to update track duration: {}", e))?;

        Ok(())
    }
}
