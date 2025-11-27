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

    let mut show_modal = use_signal(|| false);

    rsx! {
        div {
            // Simple top section
            h1 { class: "text-2xl font-bold text-white mb-2", "{album.title}" }
            p { class: "text-lg text-gray-300 mb-2", "{artist_name}" }
            if let Some(year) = album.year {
                p { class: "text-gray-400 text-sm", "{year}" }
            }

            // Subtle link to open release details modal
            if selected_release.is_some() {
                button {
                    class: "mt-2 text-sm text-blue-400 hover:text-blue-300 underline transition-colors",
                    onclick: move |_| {
                        show_modal.set(true);
                    },
                    "Release details"
                }
            }
        }

        // Modal
        if *show_modal.read() {
            ReleaseDetailsModal {
                album: album.clone(),
                release: selected_release.clone().unwrap(),
                on_close: move |_| {
                    show_modal.set(false);
                }
            }
        }
    }
}

#[component]
fn ReleaseDetailsModal(album: DbAlbum, release: DbRelease, on_close: EventHandler<()>) -> Element {
    rsx! {
        // Modal overlay
        div {
            class: "fixed inset-0 bg-black/50 flex items-center justify-center z-50",
            onclick: move |_| on_close.call(()),

            // Modal content
            div {
                class: "bg-gray-800 rounded-lg p-6 max-w-md w-full mx-4 shadow-xl",
                onclick: move |e| e.stop_propagation(),

                // Header
                div { class: "flex items-center justify-between mb-4",
                    h2 { class: "text-xl font-bold text-white", "Release Details" }
                    button {
                        class: "text-gray-400 hover:text-white transition-colors",
                        onclick: move |_| on_close.call(()),
                        "✕"
                    }
                }

                // Content
                div { class: "space-y-4",

                    // Release year and format
                    if release.year.is_some() || release.format.is_some() {
                        div {
                            if let Some(year) = release.year {
                                span { class: "text-gray-300", "{year}" }
                                if release.format.is_some() {
                                    span { class: "text-gray-300", " " }
                                }
                            }
                            if let Some(ref format) = release.format {
                                span { class: "text-gray-300", "{format}" }
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

                    // Barcode
                    if let Some(ref barcode) = release.barcode {
                        div { class: "text-sm text-gray-400",
                            span { class: "font-medium", "Barcode: " }
                            span { class: "font-mono", "{barcode}" }
                        }
                    }

                    // External links
                    div { class: "pt-4 border-t border-gray-700 space-y-2",
                        // MusicBrainz link
                        if let Some(ref mb_release) = album.musicbrainz_release {
                            a {
                                href: "https://musicbrainz.org/release/{mb_release.release_id}",
                                target: "_blank",
                                class: "flex items-center gap-2 text-sm text-blue-400 hover:text-blue-300 transition-colors",
                                span { "🔗" }
                                span { "View on MusicBrainz" }
                            }
                        }

                        // Discogs link
                        if let Some(ref discogs) = album.discogs_release {
                            a {
                                href: "https://www.discogs.com/release/{discogs.release_id}",
                                target: "_blank",
                                class: "flex items-center gap-2 text-sm text-blue-400 hover:text-blue-300 transition-colors",
                                span { "🔗" }
                                span { "View on Discogs" }
                            }
                        }
                    }
                }
            }
        }
    }
}
