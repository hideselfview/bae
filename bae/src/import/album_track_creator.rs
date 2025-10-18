use crate::database::{DbAlbum, DbTrack};
use crate::models::DiscogsAlbum;

/// Parse Discogs album metadata into database models.
///
/// Converts a DiscogsAlbum (from the API) into DbAlbum and DbTrack records
/// ready for database insertion. Extracts artist name, generates album ID,
/// and links all tracks to that album. All records start with status='queued'.
pub fn parse_discogs_album(import_item: &DiscogsAlbum) -> Result<(DbAlbum, Vec<DbTrack>), String> {
    let artist_name = import_item.extract_artist_name();

    // Create album record
    let album = match import_item {
        DiscogsAlbum::Master(master) => DbAlbum::from_discogs_master(master, &artist_name),
        DiscogsAlbum::Release(release) => DbAlbum::from_discogs_release(release, &artist_name),
    };

    // Create track records linked to this album
    let discogs_tracks = import_item.tracklist();
    let mut tracks = Vec::new();

    for discogs_track in discogs_tracks.iter() {
        let track = DbTrack::from_discogs_track(discogs_track, &album.id)?;
        tracks.push(track);
    }

    Ok((album, tracks))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{DiscogsMaster, DiscogsTrack};

    #[test]
    fn test_parse_discogs_album_basic() {
        let album = DiscogsAlbum::Master(DiscogsMaster {
            id: "12345".to_string(),
            title: "Test Album".to_string(),
            year: Some(2023),
            thumb: None,
            label: vec!["Test Label".to_string()],
            country: Some("US".to_string()),
            tracklist: vec![
                DiscogsTrack {
                    position: "1".to_string(),
                    title: "Track 1".to_string(),
                    duration: Some("3:45".to_string()),
                },
                DiscogsTrack {
                    position: "2".to_string(),
                    title: "Track 2".to_string(),
                    duration: Some("4:20".to_string()),
                },
            ],
        });

        let result = parse_discogs_album(&album);
        assert!(result.is_ok());

        let (db_album, db_tracks) = result.unwrap();

        // Verify album
        assert_eq!(db_album.title, "Test Album");
        assert_eq!(db_album.year, Some(2023));
        assert_eq!(db_album.discogs_master_id, Some("12345".to_string()));

        // Verify tracks
        assert_eq!(db_tracks.len(), 2);
        assert_eq!(db_tracks[0].title, "Track 1");
        assert_eq!(db_tracks[0].track_number, Some(1));
        assert_eq!(db_tracks[0].album_id, db_album.id);
        assert_eq!(db_tracks[1].title, "Track 2");
        assert_eq!(db_tracks[1].track_number, Some(2));
        assert_eq!(db_tracks[1].album_id, db_album.id);
    }

    #[test]
    fn test_parse_discogs_album_no_year() {
        let album = DiscogsAlbum::Master(DiscogsMaster {
            id: "67890".to_string(),
            title: "Another Album".to_string(),
            year: None,
            thumb: None,
            label: vec![],
            country: None,
            tracklist: vec![DiscogsTrack {
                position: "1".to_string(),
                title: "Only Track".to_string(),
                duration: None,
            }],
        });

        let result = parse_discogs_album(&album);
        assert!(result.is_ok());

        let (db_album, db_tracks) = result.unwrap();

        assert_eq!(db_album.title, "Another Album");
        assert_eq!(db_album.year, None);
        assert_eq!(db_tracks.len(), 1);
        assert_eq!(db_tracks[0].title, "Only Track");
    }

    #[test]
    fn test_parse_discogs_album_empty_tracklist() {
        let album = DiscogsAlbum::Master(DiscogsMaster {
            id: "empty".to_string(),
            title: "Empty Album".to_string(),
            year: Some(2024),
            thumb: None,
            label: vec![],
            country: None,
            tracklist: vec![],
        });

        let result = parse_discogs_album(&album);
        assert!(result.is_ok());

        let (db_album, db_tracks) = result.unwrap();

        assert_eq!(db_album.title, "Empty Album");
        assert_eq!(db_tracks.len(), 0);
    }

    #[test]
    fn test_parse_discogs_album_vinyl_side_notation() {
        // Load the vinyl master test fixture
        let fixture_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/vinyl_master_test.json");
        let json_data =
            std::fs::read_to_string(&fixture_path).expect("Failed to read vinyl_master_test.json");
        let master: DiscogsMaster = serde_json::from_str(&json_data).expect("Failed to parse JSON");
        let album = DiscogsAlbum::Master(master);

        let result = parse_discogs_album(&album);
        assert!(result.is_ok());

        let (db_album, db_tracks) = result.unwrap();

        // Verify album metadata
        assert_eq!(db_album.title, "Test Vinyl Album");
        assert_eq!(db_album.year, Some(1992));
        assert_eq!(
            db_album.discogs_master_id,
            Some("test-vinyl-master".to_string())
        );

        // Verify tracks
        assert_eq!(db_tracks.len(), 16, "Should have 16 tracks (A1-A7, B1-B9)");

        // Verify all tracks are linked to the album
        for track in &db_tracks {
            assert_eq!(track.album_id, db_album.id);
        }

        // Verify track numbers are sequential 1-16
        let track_numbers: Vec<Option<i32>> = db_tracks.iter().map(|t| t.track_number).collect();
        let expected_numbers: Vec<Option<i32>> = (1..=16).map(Some).collect();
        assert_eq!(
            track_numbers, expected_numbers,
            "Track numbers should be sequential 1-16 despite vinyl side notation"
        );

        // Verify track titles match the positions
        assert_eq!(db_tracks[0].title, "Track A1");
        assert_eq!(db_tracks[6].title, "Track A7");
        assert_eq!(db_tracks[7].title, "Track B1");
        assert_eq!(db_tracks[15].title, "Track B9");
    }
}
