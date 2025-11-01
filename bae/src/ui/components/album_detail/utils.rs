use crate::db::{DbAlbum, DbRelease};
use crate::library::{LibraryError, SharedLibraryManager};
use dioxus::prelude::*;

/// Format duration from milliseconds to MM:SS
pub fn format_duration(duration_ms: i64) -> String {
    let total_seconds = duration_ms / 1000;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{}:{:02}", minutes, seconds)
}

/// Load album and its releases from the database
pub async fn load_album_and_releases(
    library_manager: &SharedLibraryManager,
    album_id: &str,
) -> Result<(DbAlbum, Vec<DbRelease>), LibraryError> {
    let albums = library_manager.get().get_albums().await?;
    let album = albums
        .into_iter()
        .find(|a| a.id == album_id)
        .ok_or_else(|| LibraryError::Import("Album not found".to_string()))?;

    let releases = library_manager
        .get()
        .get_releases_for_album(album_id)
        .await?;

    Ok((album, releases))
}

/// Converts an empty string to None, otherwise wraps the string in Some
pub fn maybe_not_empty(s: String) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Extracts a release ID from the album resource and path parameters.
/// Returns None if the resource is still loading, or an error string if the data is invalid.
/// Falls back to the first release if no specific release ID is provided.
pub fn get_selected_release_id_from_params(
    album_resource: &Resource<Result<(DbAlbum, Vec<DbRelease>), LibraryError>>,
    maybe_release_id_param: Option<String>,
) -> Option<Result<String, String>> {
    album_resource
        .value()
        .read()
        .as_ref()
        .map(|result| match result {
            Err(e) => Err(e.to_string()),
            Ok((_, releases)) => {
                if releases.is_empty() {
                    return Err("Album has no releases (data integrity violation)".to_string());
                }
                match &maybe_release_id_param {
                    Some(id) => releases
                        .iter()
                        .find(|r| &r.id == id)
                        .map(|r| r.id.clone())
                        .ok_or_else(|| format!("Release {} not found in album", id)),
                    None => Ok(releases[0].id.clone()),
                }
            }
        })
}

/// Get track IDs for an album's first release, sorted by track number.
/// Returns track IDs ready to be passed to playback.play_album().
pub async fn get_album_track_ids(
    library_manager: &SharedLibraryManager,
    album_id: &str,
) -> Result<Vec<String>, LibraryError> {
    let releases = library_manager
        .get()
        .get_releases_for_album(album_id)
        .await?;

    if releases.is_empty() {
        return Ok(Vec::new());
    }

    let first_release = &releases[0];
    let mut tracks = library_manager.get().get_tracks(&first_release.id).await?;

    // Sort tracks by track_number (same logic as playback service)
    tracks.sort_by(|a, b| match (a.track_number, b.track_number) {
        (Some(a_num), Some(b_num)) => a_num.cmp(&b_num),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });

    Ok(tracks.iter().map(|t| t.id.clone()).collect())
}
