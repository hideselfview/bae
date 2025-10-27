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
pub fn maybe_not_empty_string(s: String) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Get the selected release ID from an album resource
pub fn get_selected_release_id(
    album_resource: &Resource<Result<(DbAlbum, Vec<DbRelease>), LibraryError>>,
    maybe_release_id: Option<String>,
) -> Option<String> {
    album_resource
        .value()
        .read()
        .as_ref()
        .and_then(|result| result.as_ref().ok())
        .and_then(|(_, releases)| match &maybe_release_id {
            Some(id) => releases.iter().find(|r| &r.id == id).map(|r| r.id.clone()),
            None => releases.first().map(|r| r.id.clone()),
        })
}
