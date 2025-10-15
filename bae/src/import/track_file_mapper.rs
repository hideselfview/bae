use crate::cue_flac::CueFlacProcessor;
use crate::database::DbTrack;
use crate::import::service::DiscoveredFile;
use crate::import::types::TrackSourceFile;
use std::path::PathBuf;

/// Service responsible for mapping database tracks to their source audio files.
/// This is a validation step that runs BEFORE database insertion.
pub struct TrackFileMapper;

impl TrackFileMapper {
    /// Map tracks using already-discovered files (no filesystem traversal)
    pub async fn map_tracks_to_files(
        tracks: &[DbTrack],
        discovered_files: &[DiscoveredFile],
    ) -> Result<Vec<TrackSourceFile>, String> {
        println!(
            "TrackFileMapper: Mapping {} tracks using {} pre-discovered files",
            tracks.len(),
            discovered_files.len()
        );

        // Extract paths from discovered files
        let file_paths: Vec<PathBuf> = discovered_files.iter().map(|f| f.path.clone()).collect();

        // Check for CUE/FLAC pairs from discovered files
        let cue_flac_pairs = CueFlacProcessor::detect_cue_flac_from_paths(&file_paths)
            .map_err(|e| format!("CUE/FLAC detection failed: {}", e))?;

        if !cue_flac_pairs.is_empty() {
            println!(
                "TrackFileMapper: Found {} CUE/FLAC pairs",
                cue_flac_pairs.len()
            );
            return Self::map_tracks_to_cue_flac(cue_flac_pairs, tracks);
        }

        // Fallback to individual audio files
        let audio_files = Self::filter_audio_files(&file_paths);

        if audio_files.is_empty() {
            return Err("No audio files found in discovered files".to_string());
        }

        // Simple mapping strategy: sort files by name and match to track order
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

    /// Filter audio files from a list of paths
    fn filter_audio_files(paths: &[PathBuf]) -> Vec<PathBuf> {
        let audio_extensions = ["mp3", "flac", "wav", "m4a", "aac", "ogg"];
        let mut audio_files: Vec<PathBuf> = paths
            .iter()
            .filter(|path| {
                if let Some(extension) = path.extension() {
                    if let Some(ext_str) = extension.to_str() {
                        return audio_extensions.contains(&ext_str.to_lowercase().as_str());
                    }
                }
                false
            })
            .cloned()
            .collect();

        // Already sorted by parent function
        audio_files.sort();
        println!(
            "TrackFileMapper: Filtered {} audio files",
            audio_files.len()
        );
        audio_files
    }
}
