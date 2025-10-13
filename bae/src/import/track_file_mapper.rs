use crate::cue_flac::CueFlacProcessor;
use crate::database::DbTrack;
use crate::import::types::TrackSourceFile;
use std::path::{Path, PathBuf};

/// Service responsible for mapping database tracks to their source audio files.
/// This is a validation step that runs BEFORE database insertion.
pub struct TrackFileMapper;

impl TrackFileMapper {
    /// Map database tracks to their source audio files in the folder.
    /// Returns a TrackSourceFile for each track, linking track ID to file path.
    ///
    /// Validation step: If we can't find files for all tracks, import is rejected.
    /// Handles both:
    /// - Individual files (one file per track)
    /// - CUE/FLAC (multiple tracks in single FLAC file)
    pub async fn map_tracks_to_files(
        source_folder: &Path,
        tracks: &[DbTrack],
    ) -> Result<Vec<TrackSourceFile>, String> {
        println!(
            "TrackFileMapper: Mapping {} tracks to source files in {}",
            tracks.len(),
            source_folder.display()
        );

        // First, check for CUE/FLAC pairs
        let cue_flac_pairs = CueFlacProcessor::detect_cue_flac(source_folder)
            .map_err(|e| format!("CUE/FLAC detection failed: {}", e))?;

        if !cue_flac_pairs.is_empty() {
            println!(
                "TrackFileMapper: Found {} CUE/FLAC pairs",
                cue_flac_pairs.len()
            );
            return Self::map_tracks_to_cue_flac(cue_flac_pairs, tracks);
        }

        // Fallback to individual audio files
        let audio_files = Self::find_audio_files(source_folder)?;

        if audio_files.is_empty() {
            return Err("No audio files found in source folder".to_string());
        }

        // Simple mapping strategy: sort files by name and match to track order
        // TODO: Replace with AI-powered matching
        let mut mappings = Vec::new();

        for (index, track) in tracks.iter().enumerate() {
            if let Some(audio_file) = audio_files.get(index) {
                mappings.push(TrackSourceFile {
                    db_track_id: track.id.clone(),
                    file_path: audio_file.clone(),
                });
            } else {
                println!(
                    "TrackFileMapper: Warning - no file found for track: {}",
                    track.title
                );
            }
        }

        println!(
            "TrackFileMapper: Mapped {} tracks to source files",
            mappings.len()
        );
        Ok(mappings)
    }

    /// Map tracks to CUE/FLAC source files using CUE sheet parsing
    fn map_tracks_to_cue_flac(
        cue_flac_pairs: Vec<crate::cue_flac::CueFlacPair>,
        tracks: &[DbTrack],
    ) -> Result<Vec<TrackSourceFile>, String> {
        let mut mappings = Vec::new();

        for pair in cue_flac_pairs {
            println!(
                "TrackFileMapper: Processing CUE/FLAC pair: {} + {}",
                pair.flac_path.display(),
                pair.cue_path.display()
            );

            // Parse the CUE sheet
            let cue_sheet = CueFlacProcessor::parse_cue_sheet(&pair.cue_path)
                .map_err(|e| format!("Failed to parse CUE sheet: {}", e))?;

            println!(
                "TrackFileMapper: CUE sheet contains {} tracks",
                cue_sheet.tracks.len()
            );

            // For CUE/FLAC, all tracks map to the same FLAC file
            for (index, cue_track) in cue_sheet.tracks.iter().enumerate() {
                if let Some(db_track) = tracks.get(index) {
                    mappings.push(TrackSourceFile {
                        db_track_id: db_track.id.clone(),
                        file_path: pair.flac_path.clone(),
                    });

                    println!(
                        "TrackFileMapper: Mapped CUE track '{}' to DB track '{}'",
                        cue_track.title, db_track.title
                    );
                } else {
                    println!(
                        "TrackFileMapper: Warning - CUE track '{}' has no corresponding DB track",
                        cue_track.title
                    );
                }
            }
        }

        println!(
            "TrackFileMapper: Created {} CUE/FLAC mappings",
            mappings.len()
        );
        Ok(mappings)
    }

    /// Find audio files in a directory
    fn find_audio_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
        let mut audio_files = Vec::new();
        let audio_extensions = ["mp3", "flac", "wav", "m4a", "aac", "ogg"];

        for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();

            if path.is_file() {
                if let Some(extension) = path.extension() {
                    if let Some(ext_str) = extension.to_str() {
                        if audio_extensions.contains(&ext_str.to_lowercase().as_str()) {
                            audio_files.push(path);
                        }
                    }
                }
            }
        }

        // Sort files by name for consistent ordering
        audio_files.sort();

        println!("TrackFileMapper: Found {} audio files", audio_files.len());
        Ok(audio_files)
    }
}
