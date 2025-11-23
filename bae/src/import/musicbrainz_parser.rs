use crate::db::{DbAlbum, DbAlbumArtist, DbArtist, DbRelease, DbTrack};
use crate::import::cover_art::fetch_cover_art_for_mb_release;
use crate::musicbrainz::lookup_release_by_id;
use uuid::Uuid;

/// Result of parsing a MusicBrainz release into database entities
pub type ParsedMbAlbum = (
    DbAlbum,
    DbRelease,
    Vec<DbTrack>,
    Vec<DbArtist>,
    Vec<DbAlbumArtist>,
);

/// Fetch full MusicBrainz release with tracklist and parse into database models
///
/// If the MB release has Discogs URLs in relationships, optionally fetches Discogs data
/// to populate both discogs_release and musicbrainz_release fields in DbAlbum.
///
/// cover_art_url: Optional cover art URL that was already fetched during detection phase
pub async fn fetch_and_parse_mb_release(
    release_id: &str,
    master_year: u32,
    cover_art_url: Option<String>,
) -> Result<ParsedMbAlbum, String> {
    // Fetch full release with recordings and URL relationships
    // The JSON is already included in the response, so we don't need a second HTTP request
    let (mb_release, external_urls, json) = lookup_release_by_id(release_id)
        .await
        .map_err(|e| format!("Failed to fetch MusicBrainz release: {}", e))?;

    // Optionally fetch Discogs data if URLs are available
    let discogs_release = if let Some(ref discogs_url) = external_urls.discogs_release_url {
        // Extract release ID from URL (format: https://www.discogs.com/release/123456)
        if let Some(release_id_str) = discogs_url.split("/release/").nth(1) {
            if let Some(id) = release_id_str.split('-').next() {
                // Try to fetch Discogs release
                // Note: We'd need access to DiscogsClient here, but for now we'll just extract the ID
                // The actual fetching can be done at a higher level if needed
                use tracing::info;
                info!("Found Discogs release URL: {}, ID: {}", discogs_url, id);
                // For now, we'll leave discogs_release as None and let the caller handle it
                // This is a design decision - we could pass DiscogsClient here, but that would
                // require changing the function signature. For now, we'll document this.
                None
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    // Use provided cover_art_url (already fetched during detection) or fetch if not provided
    let cover_art = if cover_art_url.is_none() {
        fetch_cover_art_for_mb_release(&mb_release, &external_urls, None).await
    } else {
        cover_art_url
    };

    parse_mb_release_from_json(&json, &mb_release, master_year, discogs_release, cover_art)
}

/// Parse MusicBrainz release JSON into database models
///
/// discogs_release: Optional Discogs release data to populate both fields in DbAlbum
/// cover_art_url: Optional cover art URL fetched from Cover Art Archive
fn parse_mb_release_from_json(
    json: &serde_json::Value,
    mb_release: &crate::musicbrainz::MbRelease,
    master_year: u32,
    discogs_release: Option<crate::discogs::DiscogsRelease>,
    cover_art_url: Option<String>,
) -> Result<ParsedMbAlbum, String> {
    // Create album record
    // If we have Discogs data, populate both fields
    let mut album = if let Some(ref discogs_rel) = discogs_release {
        // Create album with both MB and Discogs data
        let mut album = DbAlbum::from_mb_release(mb_release, master_year);
        // Add Discogs data
        album.discogs_release = Some(crate::db::DiscogsMasterRelease {
            master_id: discogs_rel.master_id.clone(),
            release_id: discogs_rel.id.clone(),
        });
        album
    } else {
        DbAlbum::from_mb_release(mb_release, master_year)
    };

    // Set cover art URL if we fetched one
    if let Some(url) = cover_art_url {
        album.cover_art_url = Some(url);
    }

    // Create release record
    let db_release = DbRelease::from_mb_release(&album.id, mb_release);

    // Create artist records from artist-credit
    let mut artists = Vec::new();
    let mut album_artists = Vec::new();

    if let Some(artist_credits) = json.get("artist-credit").and_then(|ac| ac.as_array()) {
        for (position, credit) in artist_credits.iter().enumerate() {
            if let Some(artist_obj) = credit.get("artist") {
                let artist_name = artist_obj
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown Artist")
                    .to_string();

                let _mb_artist_id = artist_obj
                    .get("id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

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
                    position: position as i32,
                };

                artists.push(artist);
                album_artists.push(album_artist);
            }
        }
    }

    // Fallback if no artists found
    if artists.is_empty() {
        let artist_name = mb_release.artist.clone();
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
    }

    // Extract tracks from media -> tracks -> recording
    let mut tracks = Vec::new();
    let mut track_index = 0;

    if let Some(media_array) = json.get("media").and_then(|m| m.as_array()) {
        for medium in media_array {
            if let Some(tracks_array) = medium.get("tracks").and_then(|t| t.as_array()) {
                for track_json in tracks_array {
                    if let Some(recording) = track_json.get("recording") {
                        let title = recording
                            .get("title")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Unknown Track")
                            .to_string();

                        let position = track_json
                            .get("position")
                            .and_then(|v| v.as_i64())
                            .map(|p| p as i32);

                        let track_number = position.or_else(|| Some(track_index + 1));

                        let track = DbTrack {
                            id: Uuid::new_v4().to_string(),
                            release_id: db_release.id.clone(),
                            title,
                            track_number,
                            duration_ms: None, // Will be filled in during track mapping
                            discogs_position: position.map(|p| p.to_string()),
                            import_status: crate::db::ImportStatus::Queued,
                            created_at: chrono::Utc::now(),
                        };

                        tracks.push(track);
                        track_index += 1;
                    }
                }
            }
        }
    }

    Ok((album, db_release, tracks, artists, album_artists))
}
