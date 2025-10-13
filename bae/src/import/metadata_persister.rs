use crate::chunking::FileChunkMapping;
use crate::cue_flac::CueFlacProcessor;
use crate::database::{DbCueSheet, DbFile, DbFileChunk, DbTrackPosition};
use crate::import::types::TrackSourceFile;
use crate::library::LibraryManager;
use std::collections::HashMap;
use std::path::Path;

/// Service responsible for persisting file metadata to the database.
/// Creates DbFile, DbFileChunk, DbCueSheet, and DbTrackPosition records.
pub struct MetadataPersister<'a> {
    library: &'a LibraryManager,
}

impl<'a> MetadataPersister<'a> {
    /// Create a new metadata persister
    pub fn new(library: &'a LibraryManager) -> Self {
        Self { library }
    }

    /// Persist all album metadata to database.
    ///
    /// For each source file:
    /// - Creates DbFile record (links to track)
    /// - Creates DbFileChunk record (maps byte ranges to chunks)
    /// - For CUE/FLAC: Creates DbCueSheet and DbTrackPosition records
    pub async fn persist_album_metadata(
        &self,
        track_files: &[TrackSourceFile],
        chunk_mappings: &[FileChunkMapping],
    ) -> Result<(), String> {
        // Create a lookup map for chunk mappings by file path
        let chunk_lookup: HashMap<&Path, &FileChunkMapping> = chunk_mappings
            .iter()
            .map(|mapping| (mapping.file_path.as_path(), mapping))
            .collect();

        // Group track mappings by source file to handle CUE/FLAC
        let mut file_groups: HashMap<&Path, Vec<&TrackSourceFile>> = HashMap::new();
        for mapping in track_files {
            file_groups
                .entry(mapping.file_path.as_path())
                .or_default()
                .push(mapping);
        }

        for (source_path, file_mappings) in file_groups {
            let chunk_mapping = chunk_lookup.get(source_path).ok_or_else(|| {
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
                println!(
                    "  Processing CUE/FLAC file with {} tracks",
                    file_mappings.len()
                );
                self.persist_cue_flac_metadata(
                    source_path,
                    file_mappings,
                    chunk_mapping,
                    file_size,
                )
                .await?;
            } else {
                // Process as individual file
                for mapping in file_mappings {
                    self.persist_individual_file(mapping, chunk_mapping, file_size, &format)
                        .await?;
                }
            }
        }

        Ok(())
    }

    /// Persist CUE/FLAC file metadata - file record, CUE sheet, and track positions
    async fn persist_cue_flac_metadata(
        &self,
        source_path: &Path,
        file_mappings: Vec<&TrackSourceFile>,
        chunk_mapping: &FileChunkMapping,
        file_size: i64,
    ) -> Result<(), String> {
        // Extract FLAC headers
        let flac_headers = CueFlacProcessor::extract_flac_headers(source_path)
            .map_err(|e| format!("Failed to extract FLAC headers: {}", e))?;

        // Create file record with FLAC headers (use first track's ID as primary)
        let primary_track_id = &file_mappings[0].db_track_id;
        let filename = source_path.file_name().unwrap().to_str().unwrap();

        let db_file = DbFile::new_cue_flac(
            primary_track_id,
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
            chunk_mapping.start_chunk_index,
            chunk_mapping.end_chunk_index,
            chunk_mapping.start_byte_offset,
            chunk_mapping.end_byte_offset,
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
            const CHUNK_SIZE: i64 = 1024 * 1024; // 1MB chunks

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
                let file_start_byte = chunk_mapping.start_byte_offset
                    + (chunk_mapping.start_chunk_index as i64 * CHUNK_SIZE);
                let absolute_start_byte = file_start_byte + start_byte;
                let absolute_end_byte = file_start_byte + end_byte;

                let start_chunk_index = (absolute_start_byte / CHUNK_SIZE) as i32;
                let end_chunk_index = ((absolute_end_byte - 1) / CHUNK_SIZE) as i32;

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

    /// Persist individual file metadata - file record and chunk mapping
    async fn persist_individual_file(
        &self,
        mapping: &TrackSourceFile,
        chunk_mapping: &FileChunkMapping,
        file_size: i64,
        format: &str,
    ) -> Result<(), String> {
        let filename = mapping.file_path.file_name().unwrap().to_str().unwrap();

        // Create file record
        let db_file = DbFile::new(&mapping.db_track_id, filename, file_size, format);
        let file_id = db_file.id.clone();

        // Save file record to database
        self.library
            .add_file(&db_file)
            .await
            .map_err(|e| format!("Failed to insert file: {}", e))?;

        // Store file-to-chunk mapping in database
        let db_file_chunk = DbFileChunk::new(
            &file_id,
            chunk_mapping.start_chunk_index,
            chunk_mapping.end_chunk_index,
            chunk_mapping.start_byte_offset,
            chunk_mapping.end_byte_offset,
        );
        self.library
            .add_file_chunk_mapping(&db_file_chunk)
            .await
            .map_err(|e| format!("Failed to insert file chunk: {}", e))?;

        Ok(())
    }
}
