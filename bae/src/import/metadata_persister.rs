use crate::chunking::FileToChunks;
use crate::cue_flac::CueFlacProcessor;
use crate::db::{DbCueSheet, DbFile, DbFileChunk, DbTrackPosition};
use crate::import::types::TrackFile;
use crate::library::LibraryManager;
use std::collections::HashMap;
use std::path::Path;
use tracing::debug;

/// Service responsible for persisting file metadata to the database.
///
/// After the streaming pipeline uploads all chunks, this service creates:
/// - DbFile records (linked to album, not individual tracks)
/// - DbFileChunk records (which chunks contain which files)
/// - DbTrackPosition records (which tracks use which files, with time ranges)
/// - DbCueSheet records (for CUE/FLAC albums)
///
/// Key insight: Files belong to albums. The track→file relationship is
/// established through db_track_position, which allows:
/// - Regular albums: 1 file = 1 track
/// - CUE/FLAC albums: 1 file = N tracks (each with different time ranges)
pub struct MetadataPersister<'a> {
    library: &'a LibraryManager,
}

impl<'a> MetadataPersister<'a> {
    /// Create a new metadata persister
    pub fn new(library: &'a LibraryManager) -> Self {
        Self { library }
    }

    /// Persist all release metadata to database.
    ///
    /// For each source file:
    /// - Creates DbFile record (links to release)
    /// - Creates DbFileChunk record (maps byte ranges to chunks)
    /// - For CUE/FLAC: Creates DbCueSheet and DbTrackPosition records
    pub async fn persist_album_metadata(
        &self,
        release_id: &str,
        track_files: &[TrackFile],
        files_to_chunks: Vec<FileToChunks>,
        chunk_size_bytes: usize,
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
                self.persist_cue_flac_metadata(
                    release_id,
                    source_path,
                    file_mappings,
                    file_to_chunks,
                    file_size,
                    chunk_size_bytes,
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

    /// Persist CUE/FLAC file metadata - file record, CUE sheet, and track positions
    ///
    /// For CUE/FLAC albums:
    /// - One FLAC file contains the entire album
    /// - The file is linked to the release (not to any specific track)
    /// - Multiple tracks will reference this same file via db_track_position
    /// - Each track has its own time range within the file (from CUE sheet)
    async fn persist_cue_flac_metadata(
        &self,
        release_id: &str,
        source_path: &Path,
        file_mappings: Vec<&TrackFile>,
        files_to_chunks: &FileToChunks,
        file_size: i64,
        chunk_size_bytes: usize,
    ) -> Result<(), String> {
        // Extract FLAC headers for instant streaming
        let flac_headers = CueFlacProcessor::extract_flac_headers(source_path)
            .map_err(|e| format!("Failed to extract FLAC headers: {}", e))?;

        // Create file record linked to release (not to any specific track)
        let filename = source_path.file_name().unwrap().to_str().unwrap();

        let db_file = DbFile::new_cue_flac(
            release_id,
            filename,
            file_size,
            flac_headers.headers.clone(),
            flac_headers.audio_start_byte as i64,
        );
        let file_id = db_file.id.clone();

        // Save file record to database
        self.library
            .add_file(&db_file)
            .await
            .map_err(|e| format!("Failed to insert file: {}", e))?;

        // Store file-to-chunk mapping in database
        let db_file_chunk = DbFileChunk::new(
            &file_id,
            files_to_chunks.start_chunk_index,
            files_to_chunks.end_chunk_index,
            files_to_chunks.start_byte_offset,
            files_to_chunks.end_byte_offset,
        );
        self.library
            .add_file_chunk_mapping(&db_file_chunk)
            .await
            .map_err(|e| format!("Failed to insert file chunk: {}", e))?;

        // Store CUE sheet in database
        let cue_path = source_path.with_extension("cue");
        if cue_path.exists() {
            let cue_content = std::fs::read_to_string(&cue_path)
                .map_err(|e| format!("Failed to read CUE file: {}", e))?;
            let db_cue_sheet = DbCueSheet::new(&file_id, &cue_content);
            self.library
                .add_cue_sheet(&db_cue_sheet)
                .await
                .map_err(|e| format!("Failed to insert CUE sheet: {}", e))?;

            // Parse CUE sheet and create track positions
            let cue_sheet = CueFlacProcessor::parse_cue_sheet(&cue_path)
                .map_err(|e| format!("Failed to parse CUE sheet: {}", e))?;

            // Create track position records for each track
            let chunk_size = chunk_size_bytes as i64;

            for (mapping, cue_track) in file_mappings.iter().zip(cue_sheet.tracks.iter()) {
                // Calculate track boundaries within the file
                let start_byte = CueFlacProcessor::estimate_byte_position(
                    cue_track.start_time_ms,
                    &flac_headers,
                    file_size as u64,
                ) as i64;

                let end_byte = if let Some(end_time) = cue_track.end_time_ms {
                    CueFlacProcessor::estimate_byte_position(
                        end_time,
                        &flac_headers,
                        file_size as u64,
                    ) as i64
                } else {
                    file_size
                };

                // Calculate chunk indices relative to the file's position in the album
                let file_start_byte = files_to_chunks.start_byte_offset
                    + (files_to_chunks.start_chunk_index as i64 * chunk_size);
                let absolute_start_byte = file_start_byte + start_byte;
                let absolute_end_byte = file_start_byte + end_byte;

                let start_chunk_index = (absolute_start_byte / chunk_size) as i32;
                let end_chunk_index = ((absolute_end_byte - 1) / chunk_size) as i32;

                let track_position = DbTrackPosition::new(
                    &mapping.db_track_id,
                    &file_id,
                    cue_track.start_time_ms as i64,
                    cue_track.end_time_ms.unwrap_or(0) as i64,
                    start_chunk_index,
                    end_chunk_index,
                );
                self.library
                    .add_track_position(&track_position)
                    .await
                    .map_err(|e| format!("Failed to insert track position: {}", e))?;
            }
        }

        Ok(())
    }

    /// Persist individual file metadata - file record, chunk mapping, and track position
    ///
    /// For regular albums (1 file = 1 track):
    /// - File is linked to release (not directly to track)
    /// - Track position is created with start_time=0, end_time=0 (indicates "use full file")
    /// - This establishes the track→file relationship via db_track_position
    async fn persist_individual_file(
        &self,
        release_id: &str,
        mapping: &TrackFile,
        files_to_chunks: &FileToChunks,
        file_size: i64,
        format: &str,
    ) -> Result<(), String> {
        let filename = mapping.file_path.file_name().unwrap().to_str().unwrap();

        // Create file record linked to release (not to specific track)
        let db_file = DbFile::new(release_id, filename, file_size, format);
        let file_id = db_file.id.clone();

        // Save file record to database
        self.library
            .add_file(&db_file)
            .await
            .map_err(|e| format!("Failed to insert file: {}", e))?;

        // Store file-to-chunk mapping in database
        let db_file_chunk = DbFileChunk::new(
            &file_id,
            files_to_chunks.start_chunk_index,
            files_to_chunks.end_chunk_index,
            files_to_chunks.start_byte_offset,
            files_to_chunks.end_byte_offset,
        );
        self.library
            .add_file_chunk_mapping(&db_file_chunk)
            .await
            .map_err(|e| format!("Failed to insert file chunk: {}", e))?;

        // Create track position record to link this track to its file
        // For individual files: start_time=0, end_time=0 indicates "use full file"
        // This is how we establish the track→file relationship for streaming
        let track_position = DbTrackPosition::new(
            &mapping.db_track_id,
            &file_id,
            0, // start_time_ms: 0 = beginning of file
            0, // end_time_ms: 0 = end of file (convention for full file)
            files_to_chunks.start_chunk_index,
            files_to_chunks.end_chunk_index,
        );
        self.library
            .add_track_position(&track_position)
            .await
            .map_err(|e| format!("Failed to insert track position: {}", e))?;

        Ok(())
    }
}
