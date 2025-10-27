use crate::db::{DbAlbum, DbArtist, DbRelease, DbTrack};
use dioxus::prelude::*;

use super::super::use_playback_service;
use super::track_row::TrackRow;

/// Album detail view component
#[component]
pub fn AlbumDetailView(
    album: DbAlbum,
    releases: Vec<DbRelease>,
    artists: Vec<DbArtist>,
    selected_release_id: Option<String>,
    on_release_select: EventHandler<String>,
    tracks: Vec<DbTrack>,
    import_progress: Option<(usize, usize, u8)>,
    completed_tracks: Vec<String>,
) -> Element {
    let playback = use_playback_service();

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
            class: "grid grid-cols-1 lg:grid-cols-3 gap-8",

            // Album artwork and info
            div {
                class: "lg:col-span-1",
                div {
                    class: "bg-gray-800 rounded-lg p-6",

                    // Album cover
                    div {
                        class: "aspect-square bg-gray-700 rounded-lg mb-6 flex items-center justify-center overflow-hidden",
                        if let Some(cover_url) = &album.cover_art_url {
                            img {
                                src: "{cover_url}",
                                alt: "Album cover for {album.title}",
                                class: "w-full h-full object-cover"
                            }
                        } else {
                            div {
                                class: "text-gray-500 text-6xl",
                                "🎵"
                            }
                        }
                    }

                    // Album metadata
                    div {
                        h1 {
                            class: "text-2xl font-bold text-white mb-2",
                            "{album.title}"
                        }
                        p {
                            class: "text-lg text-gray-300 mb-4",
                            "{artist_name}"
                        }

                        div {
                            class: "space-y-2 text-sm text-gray-400",
                            if let Some(year) = album.year {
                                div {
                                    span { class: "font-medium", "Year: " }
                                    span { "{year}" }
                                }
                            }
                            if let Some(master_id) = &album.discogs_master_id {
                                div {
                                    span { class: "font-medium", "Discogs Master ID: " }
                                    span { "{master_id}" }
                                }
                            }
                            div {
                                span { class: "font-medium", "Tracks: " }
                                span { "{tracks.len()}" }
                            }
                        }
                    }

                    // Play Album button
                    button {
                        class: "w-full mt-6 px-6 py-3 bg-blue-600 hover:bg-blue-500 text-white font-semibold rounded-lg transition-colors flex items-center justify-center gap-2",
                        onclick: {
                            let tracks = tracks.clone();
                            move |_| {
                                let track_ids: Vec<String> = tracks.iter().map(|t| t.id.clone()).collect();
                                playback.play_album(track_ids);
                            }
                        },
                        "▶ Play Album"
                    }
                }
            }

            // Tracklist
            div {
                class: "lg:col-span-2",
                div {
                    class: "bg-gray-800 rounded-lg p-6",

                    // Release tabs (if multiple releases exist)
                    if releases.len() > 1 {
                        div {
                            class: "mb-6 border-b border-gray-700",
                            div {
                                class: "flex gap-2 overflow-x-auto",
                                for release in releases.iter() {
                                    {
                                        let is_selected = selected_release_id.as_ref() == Some(&release.id);
                                        let release_id = release.id.clone();
                                        rsx! {
                                            button {
                                                key: "{release.id}",
                                                class: if is_selected {
                                                    "px-4 py-2 text-sm font-medium text-blue-400 border-b-2 border-blue-400 whitespace-nowrap"
                                                } else {
                                                    "px-4 py-2 text-sm font-medium text-gray-400 hover:text-gray-300 border-b-2 border-transparent whitespace-nowrap"
                                                },
                                                onclick: move |_| {
                                                    on_release_select.call(release_id.clone());
                                                },
                                                {
                                                    if let Some(ref name) = release.release_name {
                                                        name.clone()
                                                    } else if let Some(year) = release.year {
                                                        format!("Release ({})", year)
                                                    } else {
                                                        "Release".to_string()
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    h2 {
                        class: "text-xl font-bold text-white mb-4",
                        "Tracklist"
                    }

                    // Show import progress if release is importing
                    if let Some((current, total, percent)) = import_progress {
                        div {
                            class: "mb-4 p-4 bg-blue-900 bg-opacity-30 rounded-lg border border-blue-700",
                            div {
                                class: "flex justify-between items-center mb-2",
                                span {
                                    class: "text-sm font-medium text-blue-300",
                                    "Importing..."
                                }
                                span {
                                    class: "text-sm text-blue-400",
                                    "{current} / {total} chunks ({percent}%)"
                                }
                            }
                            div {
                                class: "w-full bg-gray-700 rounded-full h-2",
                                div {
                                    class: "bg-blue-500 h-2 rounded-full transition-all duration-300",
                                    style: "width: {percent}%"
                                }
                            }
                        }
                    }

                    if tracks.is_empty() {
                        div {
                            class: "text-center py-8 text-gray-400",
                            p { "No tracks found for this album." }
                        }
                    } else {
                        div {
                            class: "space-y-2",
                            for track in &tracks {
                                TrackRow {
                                    track: track.clone(),
                                    is_completed: completed_tracks.contains(&track.id)
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
