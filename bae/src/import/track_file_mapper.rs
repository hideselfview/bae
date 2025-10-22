use std::path::PathBuf;
use tracing::{debug, info, warn};

use crate::cue_flac::CueFlacProcessor;
use crate::db::DbTrack;
use crate::import::types::{DiscoveredFile, TrackSourceFile};

/// Map tracks to their source audio files using already-discovered files.
///
/// This is a validation step that runs BEFORE database insertion.
/// No filesystem traversal is performed.
pub async fn map_tracks_to_files(
    tracks: &[DbTrack],
    discovered_files: &[DiscoveredFile],
) -> Result<Vec<TrackSourceFile>, String> {
    info!(
        "Mapping {} tracks using {} pre-discovered files",
        tracks.len(),
        discovered_files.len()
    );

    // Extract paths from discovered files
    let file_paths: Vec<PathBuf> = discovered_files.iter().map(|f| f.path.clone()).collect();

    // Check for CUE/FLAC pairs from discovered files
    let cue_flac_pairs = CueFlacProcessor::detect_cue_flac_from_paths(&file_paths)
        .map_err(|e| format!("CUE/FLAC detection failed: {}", e))?;

    // If CUE/FLAC pairs were found, use them to map tracks to files
    if !cue_flac_pairs.is_empty() {
        info!("Found {} CUE/FLAC pairs", cue_flac_pairs.len());
        return map_tracks_to_cue_flac(cue_flac_pairs, tracks);
    }

    // Otherwise, map tracks to individual audio files
    map_tracks_to_individual_files(&file_paths, tracks)
}

/// Map tracks to CUE/FLAC source files using CUE sheet parsing
fn map_tracks_to_cue_flac(
    cue_flac_pairs: Vec<crate::cue_flac::CueFlacPair>,
    tracks: &[DbTrack],
) -> Result<Vec<TrackSourceFile>, String> {
    let mut mappings = Vec::new();

    for pair in cue_flac_pairs {
        debug!(
            "Processing CUE/FLAC pair: {} + {}",
            pair.flac_path.display(),
            pair.cue_path.display()
        );

        // Parse the CUE sheet
        let cue_sheet = CueFlacProcessor::parse_cue_sheet(&pair.cue_path)
            .map_err(|e| format!("Failed to parse CUE sheet: {}", e))?;

        debug!("CUE sheet contains {} tracks", cue_sheet.tracks.len());

        // For CUE/FLAC, all tracks map to the same FLAC file
        for (index, cue_track) in cue_sheet.tracks.iter().enumerate() {
            if let Some(db_track) = tracks.get(index) {
                mappings.push(TrackSourceFile {
                    db_track_id: db_track.id.clone(),
                    file_path: pair.flac_path.clone(),
                });

                debug!(
                    "Mapped CUE track '{}' to DB track '{}'",
                    cue_track.title, db_track.title
                );
            } else {
                warn!(
                    "CUE track '{}' has no corresponding DB track",
                    cue_track.title
                );
            }
        }
    }

    info!("Created {} CUE/FLAC mappings", mappings.len());
    Ok(mappings)
}

/// Map tracks to individual audio files using simple name-based matching
fn map_tracks_to_individual_files(
    file_paths: &[PathBuf],
    tracks: &[DbTrack],
) -> Result<Vec<TrackSourceFile>, String> {
    let audio_files = filter_audio_files(file_paths);

    if audio_files.is_empty() {
        return Err("No audio files found in discovered files".to_string());
    }

    // Require exact 1:1 match between tracks and files
    if audio_files.len() != tracks.len() {
        return Err(format!(
            "Track count mismatch: found {} audio files but have {} tracks",
            audio_files.len(),
            tracks.len()
        ));
    }

    // Verify all files have the same format
    let formats: std::collections::HashSet<_> = audio_files
        .iter()
        .filter_map(|p| p.extension())
        .filter_map(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .collect();

    if formats.len() > 1 {
        return Err(format!(
            "Mixed audio formats detected: {:?}. All tracks should be in the same format",
            formats
        ));
    }

    // Simple mapping strategy: sort files by name and match to track order
    let mut mappings = Vec::new();

    for (index, track) in tracks.iter().enumerate() {
        if let Some(audio_file) = audio_files.get(index) {
            mappings.push(TrackSourceFile {
                db_track_id: track.id.clone(),
                file_path: audio_file.clone(),
            });
        }
    }

    info!("Mapped {} tracks to source files", mappings.len());
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
    debug!("Filtered {} audio files", audio_files.len());
    audio_files
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::ImportStatus;
    use chrono::Utc;

    fn create_test_tracks(count: usize) -> Vec<DbTrack> {
        (0..count)
            .map(|i| DbTrack {
                id: format!("track-{}", i),
                release_id: "release-1".to_string(),
                title: format!("Track {}", i + 1),
                track_number: Some((i + 1) as i32),
                duration_ms: None,
                discogs_position: Some((i + 1).to_string()),
                import_status: ImportStatus::Queued,
                created_at: Utc::now(),
            })
            .collect()
    }

    fn create_discovered_files(paths: Vec<&str>) -> Vec<DiscoveredFile> {
        paths
            .into_iter()
            .map(|p| DiscoveredFile {
                path: PathBuf::from(p),
                size: 1024 * 1024, // 1 MB
            })
            .collect()
    }

    #[tokio::test]
    async fn test_map_tracks_to_files_individual_files() {
        let tracks = create_test_tracks(3);
        let discovered_files = create_discovered_files(vec![
            "/album/01-track1.flac",
            "/album/02-track2.flac",
            "/album/03-track3.flac",
        ]);

        let result = map_tracks_to_files(&tracks, &discovered_files).await;
        assert!(result.is_ok());

        let mappings = result.unwrap();
        assert_eq!(mappings.len(), 3);

        // Verify each track maps to corresponding file
        assert_eq!(mappings[0].db_track_id, "track-0");
        assert_eq!(
            mappings[0].file_path,
            PathBuf::from("/album/01-track1.flac")
        );
        assert_eq!(mappings[1].db_track_id, "track-1");
        assert_eq!(
            mappings[1].file_path,
            PathBuf::from("/album/02-track2.flac")
        );
        assert_eq!(mappings[2].db_track_id, "track-2");
        assert_eq!(
            mappings[2].file_path,
            PathBuf::from("/album/03-track3.flac")
        );
    }

    #[tokio::test]
    async fn test_map_tracks_to_files_no_audio_files() {
        let tracks = create_test_tracks(2);
        let discovered_files =
            create_discovered_files(vec!["/album/cover.jpg", "/album/readme.txt"]);

        let result = map_tracks_to_files(&tracks, &discovered_files).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No audio files found"));
    }

    #[tokio::test]
    async fn test_map_tracks_to_files_more_tracks_than_files() {
        let tracks = create_test_tracks(5);
        let discovered_files =
            create_discovered_files(vec!["/album/01.flac", "/album/02.flac", "/album/03.flac"]);

        let result = map_tracks_to_files(&tracks, &discovered_files).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Track count mismatch"));
    }

    #[tokio::test]
    async fn test_map_tracks_to_files_more_files_than_tracks() {
        let tracks = create_test_tracks(2);
        let discovered_files = create_discovered_files(vec![
            "/album/01.flac",
            "/album/02.flac",
            "/album/03.flac",
            "/album/04.flac",
        ]);

        let result = map_tracks_to_files(&tracks, &discovered_files).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Track count mismatch"));
    }

    #[tokio::test]
    async fn test_map_tracks_to_files_mixed_formats() {
        let tracks = create_test_tracks(4);
        let discovered_files = create_discovered_files(vec![
            "/album/cover.jpg",
            "/album/track1.mp3",
            "/album/track2.flac",
            "/album/track3.wav",
            "/album/track4.m4a",
            "/album/readme.txt",
        ]);

        let result = map_tracks_to_files(&tracks, &discovered_files).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Mixed audio formats detected"));
    }

    #[tokio::test]
    async fn test_map_tracks_to_files_cue_flac() {
        let tracks = create_test_tracks(10);

        // Simulate a CUE+FLAC pair (detection works based on naming convention)
        let discovered_files = create_discovered_files(vec![
            "/album/album.flac",
            "/album/album.cue",
            "/album/cover.jpg",
        ]);

        let result = map_tracks_to_files(&tracks, &discovered_files).await;

        // Without a real CUE file, parsing will fail
        // This test verifies the CUE/FLAC detection path is triggered and errors appropriately
        assert!(result.is_err());
        let err = result.unwrap_err();

        // Should fail when trying to parse the non-existent CUE file
        assert!(
            err.contains("Failed to parse CUE sheet") || err.contains("CUE"),
            "Expected CUE parsing error, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_map_tracks_to_files_vinyl_with_numbered_files() {
        // Load the vinyl fixture which has position notation A1-A7, B1-B9
        let fixture_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/vinyl_master_test.json");
        let json_data =
            std::fs::read_to_string(&fixture_path).expect("Failed to read vinyl_master_test.json");
        let master: crate::discogs::DiscogsMaster =
            serde_json::from_str(&json_data).expect("Failed to parse JSON");
        let album = crate::discogs::DiscogsAlbum::Master(master);

        // Parse through album_track_creator to get real DbTracks with vinyl positions
        let (_, _, tracks, _, _) =
            crate::import::album_track_creator::parse_discogs_album(&album).unwrap();

        // Verify tracks have vinyl positions but sequential track_numbers
        assert_eq!(tracks.len(), 2); // Fixture only has 2 tracks (A1-A2)
        assert_eq!(tracks[0].discogs_position, Some("A1".to_string()));
        assert_eq!(tracks[0].track_number, Some(1));
        assert_eq!(tracks[6].discogs_position, Some("A7".to_string()));
        assert_eq!(tracks[6].track_number, Some(7));
        assert_eq!(tracks[7].discogs_position, Some("B1".to_string()));
        assert_eq!(tracks[7].track_number, Some(8));
        assert_eq!(tracks[15].discogs_position, Some("B9".to_string()));
        assert_eq!(tracks[15].track_number, Some(16));

        // Simulate individual FLAC files numbered 01-16 matching track titles
        let discovered_files = create_discovered_files(vec![
            "/vinyl/01 Track A1.flac",
            "/vinyl/02 Track A2.flac",
            "/vinyl/03 Track A3.flac",
            "/vinyl/04 Track A4.flac",
            "/vinyl/05 Track A5.flac",
            "/vinyl/06 Track A6.flac",
            "/vinyl/07 Track A7.flac",
            "/vinyl/08 Track B1.flac",
            "/vinyl/09 Track B2.flac",
            "/vinyl/10 Track B3.flac",
            "/vinyl/11 Track B4.flac",
            "/vinyl/12 Track B5.flac",
            "/vinyl/13 Track B6.flac",
            "/vinyl/14 Track B7.flac",
            "/vinyl/15 Track B8.flac",
            "/vinyl/16 Track B9.flac",
            "/vinyl/album.cue",
            "/vinyl/album.log",
        ]);

        let result = map_tracks_to_files(&tracks, &discovered_files).await;
        assert!(result.is_ok());

        let mappings = result.unwrap();
        assert_eq!(mappings.len(), 16, "All 16 tracks should be mapped");

        // Verify sequential mapping works despite vinyl notation:
        // Track with position A1 (track_number=1) → file "01 Track A1.flac"
        // Track with position B1 (track_number=8) → file "08 Track B1.flac"
        assert_eq!(
            mappings[0].file_path,
            PathBuf::from("/vinyl/01 Track A1.flac")
        );
        assert_eq!(tracks[0].discogs_position, Some("A1".to_string()));

        assert_eq!(
            mappings[7].file_path,
            PathBuf::from("/vinyl/08 Track B1.flac")
        );
        assert_eq!(tracks[7].discogs_position, Some("B1".to_string()));

        assert_eq!(
            mappings[15].file_path,
            PathBuf::from("/vinyl/16 Track B9.flac")
        );
        assert_eq!(tracks[15].discogs_position, Some("B9".to_string()));
    }

    #[tokio::test]
    async fn test_map_tracks_to_files_vinyl_cue_flac() {
        // Load the vinyl fixture which has position notation A1-A7, B1-B9
        let fixture_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/vinyl_master_test.json");
        let json_data =
            std::fs::read_to_string(&fixture_path).expect("Failed to read vinyl_master_test.json");
        let master: crate::discogs::DiscogsMaster =
            serde_json::from_str(&json_data).expect("Failed to parse JSON");
        let album = crate::discogs::DiscogsAlbum::Master(master);

        // Parse through album_track_creator to get real DbTracks with vinyl positions
        let (_, _, tracks, _, _) =
            crate::import::album_track_creator::parse_discogs_album(&album).unwrap();

        assert_eq!(tracks.len(), 2); // Fixture only has 2 tracks (A1-A2)

        // Simulate CUE+FLAC pair (using real CUE fixture)
        let cue_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/vinyl_album.cue");
        let flac_path = cue_path.with_extension("flac");

        let discovered_files = vec![
            DiscoveredFile {
                path: flac_path.clone(),
                size: 300 * 1024 * 1024, // 300 MB
            },
            DiscoveredFile {
                path: cue_path,
                size: 2048,
            },
        ];

        let result = map_tracks_to_files(&tracks, &discovered_files).await;
        assert!(result.is_ok(), "CUE/FLAC mapping should succeed");

        let mappings = result.unwrap();
        assert_eq!(
            mappings.len(),
            16,
            "All 16 tracks should be mapped from CUE sheet"
        );

        // Verify ALL tracks map to the SAME FLAC file (CUE/FLAC characteristic)
        for (i, mapping) in mappings.iter().enumerate() {
            assert_eq!(
                mapping.file_path, flac_path,
                "Track {} should map to single FLAC file",
                i
            );
        }

        // Verify vinyl positions are preserved
        assert_eq!(tracks[0].discogs_position, Some("A1".to_string()));
        assert_eq!(tracks[7].discogs_position, Some("B1".to_string()));
        assert_eq!(tracks[15].discogs_position, Some("B9".to_string()));
    }
}
