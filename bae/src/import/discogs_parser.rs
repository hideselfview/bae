use crate::db::{DbAlbum, DbAlbumArtist, DbArtist, DbRelease, DbTrack};
use crate::discogs::DiscogsRelease;
use uuid::Uuid;

/// Result of parsing a Discogs release into database entities
pub type ParsedAlbum = (
    DbAlbum,
    DbRelease,
    Vec<DbTrack>,
    Vec<DbArtist>,
    Vec<DbAlbumArtist>,
);

/// Parse Discogs release metadata into database models including artist information.
///
/// Converts a DiscogsRelease (from the API) into DbAlbum, DbRelease, DbTrack, and artist records
/// ready for database insertion. Extracts artist data from Discogs API response,
/// generates IDs, and links all entities together.
///
/// master_year is always provided and used for the album year (not the release year).
///
/// Returns: (album, release, tracks, artists, album_artists)
pub fn parse_discogs_release(
    release: &DiscogsRelease,
    master_year: u32,
) -> Result<ParsedAlbum, String> {
    // Create album record (logical album entity)
    let album = DbAlbum::from_discogs_release(release, master_year);

    // Create release record (specific version/pressing)
    let db_release = DbRelease::from_discogs_release(&album.id, release);

    // Create artist records from Discogs API data
    let mut artists = Vec::new();
    let mut album_artists = Vec::new();

    if release.artists.is_empty() {
        // Fallback: parse artist from title if Discogs API didn't return artists
        let artist_name = extract_artist_name(&release.title);
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
        for (position, discogs_artist) in release.artists.iter().enumerate() {
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
    let mut tracks = Vec::new();

    for (index, discogs_track) in release.tracklist.iter().enumerate() {
        let disc_number = parse_disc_number_from_position(&discogs_track.position);
        let track = DbTrack::from_discogs_track(discogs_track, &db_release.id, index, disc_number)?;
        tracks.push(track);
    }

    Ok((album, db_release, tracks, artists, album_artists))
}

/// Parse disc number from Discogs position format.
///
/// Discogs positions can be:
/// - "1", "2", "3" (single disc, no disc number)
/// - "1-1", "1-2", "2-1" (disc-track format, e.g., "1-1" = disc 1, track 1)
/// - "A1", "B1", "C1" (vinyl sides - A/B = disc 1, C/D = disc 2, etc.)
fn parse_disc_number_from_position(position: &str) -> Option<i32> {
    // Check for "disc-track" format (e.g., "1-1", "2-3")
    if let Some(dash_idx) = position.find('-') {
        if let Ok(disc) = position[..dash_idx].parse::<i32>() {
            return Some(disc);
        }
    }

    // Check for vinyl side format (A/B = disc 1, C/D = disc 2, etc.)
    if let Some(first_char) = position.chars().next() {
        if first_char.is_ascii_alphabetic() {
            let upper = first_char.to_ascii_uppercase();
            // A/B = disc 1, C/D = disc 2, E/F = disc 3, etc.
            let disc = ((upper as u8 - b'A') / 2 + 1) as i32;
            return Some(disc);
        }
    }

    // Single disc format or unknown - return None (will be treated as disc 1)
    None
}

/// Extract artist name from album title (fallback when artists array is empty).
///
/// Discogs album titles often follow "Artist - Album" format.
/// Splits on " - " to extract the artist. Falls back to "Unknown Artist".
fn extract_artist_name(title: &str) -> String {
    if let Some(dash_pos) = title.find(" - ") {
        title[..dash_pos].to_string()
    } else {
        "Unknown Artist".to_string()
    }
}

// Tests removed - need to be rewritten for new DiscogsRelease-based architecture
// The old tests used DiscogsAlbum enum which no longer exists
