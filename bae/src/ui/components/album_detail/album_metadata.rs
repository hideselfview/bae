use crate::db::{DbAlbum, DbArtist, DbRelease};
use dioxus::prelude::*;

#[component]
pub fn AlbumMetadata(
    album: DbAlbum,
    artists: Vec<DbArtist>,
    track_count: usize,
    selected_release: Option<DbRelease>,
) -> Element {
    let artist_name = if artists.is_empty() {
        "Unknown Artist".to_string()
    } else if artists.len() == 1 {
        artists[0].name.clone()
    } else {
        artists
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };

    rsx! {
        div {
            h1 { class: "text-2xl font-bold text-white mb-2", "{album.title}" }
            p { class: "text-lg text-gray-300 mb-4", "{artist_name}" }

            div { class: "space-y-2 text-sm text-gray-400",
                // Show both album year (original) and release year if different
                if let Some(album_year) = album.year {
                    if let Some(ref release) = selected_release {
                        if let Some(release_year) = release.year {
                            if album_year != release_year {
                                // Different years - show both
                                div {
                                    span { class: "font-medium", "Original Release: " }
                                    span { "{album_year}" }
                                }
                                div {
                                    span { class: "font-medium", "This Release: " }
                                    span { "{release_year}" }
                                }
                            } else {
                                // Same year - show once
                                div {
                                    span { class: "font-medium", "Year: " }
                                    span { "{album_year}" }
                                }
                            }
                        } else {
                            // Only album year available
                            div {
                                span { class: "font-medium", "Year: " }
                                span { "{album_year}" }
                            }
                        }
                    } else {
                        // No release selected, show album year
                        div {
                            span { class: "font-medium", "Year: " }
                            span { "{album_year}" }
                        }
                    }
                }

                // Show MusicBrainz release details if available
                if let Some(ref mb_release) = album.musicbrainz_release {
                    if let Some(ref release) = selected_release {
                        // Get the MbRelease from the musicbrainz module to show format, label, etc.
                        // For now, just show the IDs
                        div {
                            span { class: "font-medium", "MusicBrainz Release: " }
                            span { class: "text-xs font-mono", "{mb_release.release_id}" }
                        }
                    }
                }

                // Show Discogs info if available
                if let Some(discogs_release) = &album.discogs_release {
                    div {
                        span { class: "font-medium", "Discogs Master: " }
                        span { "{discogs_release.master_id}" }
                    }
                }

                div {
                    span { class: "font-medium", "Tracks: " }
                    span { "{track_count}" }
                }
            }
        }
    }
}
