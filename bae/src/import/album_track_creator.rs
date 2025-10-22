use crate::database::{DbAlbum, DbAlbumArtist, DbArtist, DbRelease, DbTrack};
use crate::models::DiscogsAlbum;
use uuid::Uuid;

/// Result of parsing a Discogs album into database entities
pub type ParsedAlbum = (
    DbAlbum,
    DbRelease,
    Vec<DbTrack>,
    Vec<DbArtist>,
    Vec<DbAlbumArtist>,
);

/// Parse Discogs album metadata into database models including artist information.
///
/// Converts a DiscogsAlbum (from the API) into DbAlbum, DbRelease, DbTrack, and artist records
/// ready for database insertion. Extracts artist data from Discogs API response,
/// generates IDs, and links all entities together.
///
/// Returns: (album, release, tracks, artists, album_artists)
pub fn parse_discogs_album(import_item: &DiscogsAlbum) -> Result<ParsedAlbum, String> {
    // Create album record (logical album entity)
    let album = match import_item {
        DiscogsAlbum::Master(master) => DbAlbum::from_discogs_master(master),
        DiscogsAlbum::Release(release) => DbAlbum::from_discogs_release(release),
    };

    // Create release record (specific version/pressing)
    let db_release = match import_item {
        DiscogsAlbum::Master(_) => DbRelease::default_for_master(&album.id, album.year),
        DiscogsAlbum::Release(release) => DbRelease::from_discogs_release(&album.id, release),
    };

    // Create artist records from Discogs API data
    let discogs_artists = import_item.artists();
    let mut artists = Vec::new();
    let mut album_artists = Vec::new();

    if discogs_artists.is_empty() {
        // Fallback: parse artist from title if Discogs API didn't return artists
        let artist_name = import_item.extract_artist_name();
        let artist = DbArtist {
            id: Uuid::new_v4().to_string(),
            name: artist_name.clone(),
            sort_name: Some(artist_name.clone()),
            discogs_artist_id: None,
            bandcamp_artist_id: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let album_artist = DbAlbumArtist {
            id: Uuid::new_v4().to_string(),
            album_id: album.id.clone(),
            artist_id: artist.id.clone(),
            position: 0,
        };

        artists.push(artist);
        album_artists.push(album_artist);
    } else {
        // Use artist data from Discogs API
        for (position, discogs_artist) in discogs_artists.iter().enumerate() {
            let artist = DbArtist {
                id: Uuid::new_v4().to_string(),
                name: discogs_artist.name.clone(),
                sort_name: Some(discogs_artist.name.clone()),
                discogs_artist_id: Some(discogs_artist.id.clone()),
                bandcamp_artist_id: None,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            };

            let album_artist = DbAlbumArtist {
                id: Uuid::new_v4().to_string(),
                album_id: album.id.clone(),
                artist_id: artist.id.clone(),
                position: position as i32,
            };

            artists.push(artist);
            album_artists.push(album_artist);
        }
    }

    // Create track records linked to this release
    let discogs_tracks = import_item.tracklist();
    let mut tracks = Vec::new();

    for (index, discogs_track) in discogs_tracks.iter().enumerate() {
        let track = DbTrack::from_discogs_track(discogs_track, &db_release.id, index)?;
        tracks.push(track);
    }

    Ok((album, db_release, tracks, artists, album_artists))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{DiscogsMaster, DiscogsTrack};

    #[test]
    fn test_parse_discogs_album_basic() {
        let album = DiscogsAlbum::Master(DiscogsMaster {
            id: "12345".to_string(),
            title: "Test Artist - Test Album".to_string(),
            year: Some(2023),
            thumb: None,
            label: vec!["Test Label".to_string()],
            country: Some("US".to_string()),
            artists: vec![crate::models::DiscogsArtist {
                id: "artist-123".to_string(),
                name: "Test Artist".to_string(),
            }],
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

        let (db_album, db_release, db_tracks, artists, album_artists) = result.unwrap();

        // Verify album
        assert_eq!(db_album.title, "Test Artist - Test Album");
        assert_eq!(db_album.year, Some(2023));
        assert_eq!(db_album.discogs_master_id, Some("12345".to_string()));

        // Verify release
        assert_eq!(db_release.album_id, db_album.id);

        // Verify tracks
        assert_eq!(db_tracks.len(), 2);
        assert_eq!(db_tracks[0].title, "Track 1");
        assert_eq!(db_tracks[0].track_number, Some(1));
        assert_eq!(db_tracks[0].release_id, db_release.id);
        assert_eq!(db_tracks[1].title, "Track 2");
        assert_eq!(db_tracks[1].track_number, Some(2));
        assert_eq!(db_tracks[1].release_id, db_release.id);

        // Verify artist (from Discogs API)
        assert_eq!(artists.len(), 1);
        assert_eq!(artists[0].name, "Test Artist");
        assert_eq!(artists[0].discogs_artist_id, Some("artist-123".to_string()));
        assert_eq!(album_artists.len(), 1);
        assert_eq!(album_artists[0].album_id, db_album.id);
        assert_eq!(album_artists[0].artist_id, artists[0].id);
    }

    #[test]
    fn test_parse_discogs_album_no_year() {
        let album = DiscogsAlbum::Master(DiscogsMaster {
            id: "67890".to_string(),
            title: "Artist Name - Another Album".to_string(),
            year: None,
            thumb: None,
            label: vec![],
            country: None,
            artists: vec![crate::models::DiscogsArtist {
                id: "artist-456".to_string(),
                name: "Artist Name".to_string(),
            }],
            tracklist: vec![DiscogsTrack {
                position: "1".to_string(),
                title: "Only Track".to_string(),
                duration: None,
            }],
        });

        let result = parse_discogs_album(&album);
        assert!(result.is_ok());

        let (db_album, db_release, db_tracks, artists, album_artists) = result.unwrap();

        assert_eq!(db_album.title, "Artist Name - Another Album");
        assert_eq!(db_album.year, None);
        assert_eq!(db_release.album_id, db_album.id);
        assert_eq!(db_tracks.len(), 1);
        assert_eq!(db_tracks[0].title, "Only Track");
        assert_eq!(artists.len(), 1); // Should have one artist
        assert_eq!(album_artists.len(), 1); // Should have one album-artist relationship
    }

    #[test]
    fn test_parse_discogs_album_empty_tracklist() {
        let album = DiscogsAlbum::Master(DiscogsMaster {
            id: "empty".to_string(),
            title: "Some Artist - Empty Album".to_string(),
            year: Some(2024),
            thumb: None,
            label: vec![],
            country: None,
            artists: vec![crate::models::DiscogsArtist {
                id: "artist-789".to_string(),
                name: "Some Artist".to_string(),
            }],
            tracklist: vec![],
        });

        let result = parse_discogs_album(&album);
        assert!(result.is_ok());

        let (db_album, db_release, db_tracks, artists, album_artists) = result.unwrap();

        assert_eq!(db_album.title, "Some Artist - Empty Album");
        assert_eq!(db_release.album_id, db_album.id);
        assert_eq!(db_tracks.len(), 0);
        assert_eq!(artists.len(), 1); // Should have artist
        assert_eq!(album_artists.len(), 1);
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

        let (db_album, db_release, db_tracks, artists, album_artists) = result.unwrap();

        // Verify album metadata
        assert_eq!(db_album.title, "Test Vinyl Album");
        assert_eq!(db_album.year, Some(1992));
        assert_eq!(
            db_album.discogs_master_id,
            Some("test-vinyl-master".to_string())
        );

        // Verify release
        assert_eq!(db_release.album_id, db_album.id);

        // Verify artist
        assert_eq!(artists.len(), 1);
        assert_eq!(album_artists.len(), 1);

        // Verify tracks
        assert_eq!(
            db_tracks.len(),
            2,
            "Should have 2 tracks (A1-A2) matching fixture"
        );

        // Verify all tracks are linked to the release
        for track in &db_tracks {
            assert_eq!(track.release_id, db_release.id);
        }

        // Verify track numbers are sequential
        let track_numbers: Vec<Option<i32>> = db_tracks.iter().map(|t| t.track_number).collect();
        let expected_numbers: Vec<Option<i32>> = (1..=2).map(Some).collect();
        assert_eq!(
            track_numbers, expected_numbers,
            "Track numbers should be sequential despite vinyl side notation"
        );
    }
}
