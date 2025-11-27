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

    let mut is_expanded = use_signal(|| false);
    let expanded = *is_expanded.read();

    rsx! {
        div {
            // Simple top section - always visible
            h1 { class: "text-2xl font-bold text-white mb-2", "{album.title}" }
            p { class: "text-lg text-gray-300 mb-2", "{artist_name}" }
            if let Some(year) = album.year {
                p { class: "text-gray-400 text-sm mb-4", "{year}" }
            }

            // Collapsible Release Details box
            if let Some(ref release) = selected_release {
                div {
                    class: "mt-4 border border-gray-700 rounded-lg bg-gray-800/50",

                    // Header - clickable to expand/collapse
                    button {
                        class: "w-full px-4 py-3 flex items-center justify-between hover:bg-gray-700/30 transition-colors rounded-lg",
                        onclick: move |_| {
                            is_expanded.set(!expanded);
                        },

                        div { class: "flex items-center gap-2",
                            span { class: "text-lg", "ℹ️" }
                            span { class: "text-sm font-medium text-gray-300", "Release Details" }
                        }

                        span { class: "text-gray-400 text-sm",
                            if expanded { "▲" } else { "▼" }
                        }
                    }

                    // Expanded content
                    if expanded {
                        div { class: "px-4 pb-4 space-y-3",

                            // Release year and format
                            if release.year.is_some() || release.format.is_some() {
                                div { class: "text-gray-300",
                                    if let Some(year) = release.year {
                                        span { "{year}" }
                                        if release.format.is_some() {
                                            span { " " }
                                        }
                                    }
                                    if let Some(ref format) = release.format {
                                        span { "{format}" }
                                    }
                                }
                            }

                            // Label and catalog number
                            if release.label.is_some() || release.catalog_number.is_some() {
                                div { class: "text-sm text-gray-400",
                                    if let Some(ref label) = release.label {
                                        span { "{label}" }
                                        if release.catalog_number.is_some() {
                                            span { " • " }
                                        }
                                    }
                                    if let Some(ref catalog) = release.catalog_number {
                                        span { "{catalog}" }
                                    }
                                }
                            }

                            // Country
                            if let Some(ref country) = release.country {
                                div { class: "text-sm text-gray-400",
                                    span { "{country}" }
                                }
                            }

                            // External links
                            div { class: "pt-2 space-y-2",
                                // MusicBrainz link
                                if let Some(ref mb_release) = album.musicbrainz_release {
                                    a {
                                        href: "https://musicbrainz.org/release/{mb_release.release_id}",
                                        target: "_blank",
                                        class: "flex items-center gap-2 text-sm text-blue-400 hover:text-blue-300 transition-colors",
                                        span { "🔗" }
                                        span { "MusicBrainz Release" }
                                    }
                                }

                                // Discogs link
                                if let Some(ref discogs) = album.discogs_release {
                                    a {
                                        href: "https://www.discogs.com/release/{discogs.release_id}",
                                        target: "_blank",
                                        class: "flex items-center gap-2 text-sm text-blue-400 hover:text-blue-300 transition-colors",
                                        span { "🔗" }
                                        span { "Discogs Release" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
