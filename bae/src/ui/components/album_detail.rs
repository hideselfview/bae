use crate::db::{DbAlbum, DbArtist, DbTrack};
use crate::library::use_library_manager;
use crate::library::LibraryError;
use crate::ui::Route;
use dioxus::prelude::*;

use super::use_playback_service;

/// Album detail page showing album info and tracklist
#[component]
pub fn AlbumDetail(album_id: String) -> Element {
    let library_manager = use_library_manager();
    let mut album = use_signal(|| None::<DbAlbum>);
    let mut tracks = use_signal(Vec::<DbTrack>::new);
    let mut loading = use_signal(|| true);
    let mut error = use_signal(|| None::<String>);

    // Load album and tracks on component mount
    use_effect({
        let album_id = album_id.clone();
        let library_manager = library_manager.clone();

        move || {
            spawn({
                let album_id = album_id.clone();
                let library_manager = library_manager.clone();

                async move {
                    loading.set(true);
                    error.set(None);

                    match load_album_details(&album_id, &library_manager).await {
                        Ok((album_data, tracks_data)) => {
                            album.set(Some(album_data));
                            tracks.set(tracks_data);
                            loading.set(false);
                        }
                        Err(e) => {
                            error.set(Some(format!("Failed to load album: {}", e)));
                            loading.set(false);
                        }
                    }
                }
            });
        }
    });

    rsx! {
        div {
            class: "container mx-auto p-6",

            // Back button
            div {
                class: "mb-6",
                Link {
                    to: Route::Library {},
                    class: "inline-flex items-center text-blue-400 hover:text-blue-300 transition-colors",
                    "← Back to Library"
                }
            }

            if loading() {
                div {
                    class: "flex justify-center items-center py-12",
                    div {
                        class: "animate-spin rounded-full h-12 w-12 border-b-2 border-blue-500"
                    }
                    p {
                        class: "ml-4 text-gray-300",
                        "Loading album details..."
                    }
                }
            } else if let Some(err) = error() {
                div {
                    class: "bg-red-900 border border-red-700 text-red-100 px-4 py-3 rounded mb-4",
                    p { "{err}" }
                }
            } else if let Some(album_data) = album() {
                AlbumDetailView {
                    album: album_data,
                    tracks: tracks()
                }
            }
        }
    }
}

/// Album detail view component
#[component]
fn AlbumDetailView(album: DbAlbum, tracks: Vec<DbTrack>) -> Element {
    let library_manager = use_library_manager();
    let playback = use_playback_service();
    let mut artist_name = use_signal(|| "Loading...".to_string());

    // Load artists for this album
    use_effect({
        let album_id = album.id.clone();
        move || {
            let library_manager = library_manager.clone();
            let album_id = album_id.clone();
            spawn(async move {
                match library_manager.get().get_artists_for_album(&album_id).await {
                    Ok(artists) => {
                        if artists.is_empty() {
                            artist_name.set("Unknown Artist".to_string());
                        } else if artists.len() == 1 {
                            artist_name.set(artists[0].name.clone());
                        } else {
                            // Multiple artists: join with commas
                            let names: Vec<_> = artists.iter().map(|a| a.name.as_str()).collect();
                            artist_name.set(names.join(", "));
                        }
                    }
                    Err(_) => {
                        artist_name.set("Unknown Artist".to_string());
                    }
                }
            });
        }
    });

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
                            "{artist_name()}"
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
                    h2 {
                        class: "text-xl font-bold text-white mb-4",
                        "Tracklist"
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
                                TrackRow { track: track.clone() }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Individual track row component
#[component]
fn TrackRow(track: DbTrack) -> Element {
    let library_manager = use_library_manager();
    let playback = use_playback_service();
    let mut track_artists = use_signal(Vec::<DbArtist>::new);

    // Load artists for this track (for compilations/features)
    use_effect({
        let track_id = track.id.clone();
        move || {
            let library_manager = library_manager.clone();
            let track_id = track_id.clone();
            spawn(async move {
                if let Ok(artists) = library_manager.get().get_artists_for_track(&track_id).await {
                    track_artists.set(artists);
                }
            });
        }
    });

    rsx! {
        div {
            class: "flex items-center py-3 px-4 rounded-lg hover:bg-gray-700 transition-colors group",

            // Play button (hidden by default, shown on hover)
            button {
                class: "opacity-0 group-hover:opacity-100 transition-opacity text-blue-400 hover:text-blue-300",
                onclick: move |_| {
                    playback.play(track.id.clone());
                },
                "▶"
            }

            // Track number
            div {
                class: "w-12 text-right text-gray-400 text-sm font-mono",
                if let Some(track_num) = track.track_number {
                    "{track_num}."
                } else {
                    "—"
                }
            }

            // Track info
            div {
                class: "flex-1 ml-4",
                h3 {
                    class: "text-white font-medium group-hover:text-blue-300 transition-colors",
                    "{track.title}"
                }
                // Show track artists if any (for compilations/features)
                if !track_artists().is_empty() {
                    p {
                        class: "text-gray-400 text-sm",
                        {
                            let artists = track_artists();
                            if artists.len() == 1 {
                                artists[0].name.clone()
                            } else {
                                artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")
                            }
                        }
                    }
                }
            }

            // Duration (if available)
            div {
                class: "text-gray-400 text-sm font-mono",
                if let Some(duration_ms) = track.duration_ms {
                    {format_duration(duration_ms)}
                } else {
                    "—:—"
                }
            }
        }
    }
}

/// Format duration from milliseconds to MM:SS
fn format_duration(duration_ms: i64) -> String {
    let total_seconds = duration_ms / 1000;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{}:{:02}", minutes, seconds)
}

/// Load album and tracks from the database
async fn load_album_details(
    album_id: &str,
    library_manager: &crate::library::SharedLibraryManager,
) -> Result<(DbAlbum, Vec<DbTrack>), LibraryError> {
    // Get all albums to find the one we want
    let albums = library_manager.get().get_albums().await?;
    let album = albums
        .into_iter()
        .find(|a| a.id == album_id)
        .ok_or_else(|| LibraryError::Import("Album not found".to_string()))?;

    // Get tracks for this album
    let tracks = library_manager.get().get_tracks(album_id).await?;

    Ok((album, tracks))
}
