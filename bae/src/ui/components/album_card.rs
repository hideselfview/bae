use crate::db::{DbAlbum, DbArtist};
use crate::library::use_library_manager;
use crate::playback::{PlaybackProgress, PlaybackState};
use crate::ui::Route;
use dioxus::prelude::*;

use super::album_detail::utils::get_album_track_ids;
use super::use_playback_service;

/// Individual album card component
///
/// Note: Albums now represent logical albums that can have multiple releases.
/// For now, we show albums without import status (which moved to releases).
/// Future enhancement: Show all releases for an album in the detail view.
#[component]
pub fn AlbumCard(album: DbAlbum, artists: Vec<DbArtist>) -> Element {
    let playback = use_playback_service();
    let library_manager = use_library_manager();
    let mut is_loading = use_signal(|| false);

    // Format artist names
    let artist_name = if artists.is_empty() {
        "Unknown Artist".to_string()
    } else if artists.len() == 1 {
        artists[0].name.clone()
    } else {
        // Multiple artists: join with commas
        artists
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };

    let card_class = "bg-gray-800 rounded-lg overflow-hidden shadow-lg hover:shadow-xl transition-shadow duration-300 cursor-pointer group";

    rsx! {
        div {
            class: "{card_class}",
            onclick: {
                let album_id = album.id.clone();
                let navigator = navigator();
                move |_| {
                    navigator
                        .push(Route::AlbumDetail {
                            album_id: album_id.clone(),
                            release_id: String::new(),
                        });
                }
            },

            // Album cover
            div { class: "aspect-square bg-gray-700 flex items-center justify-center relative",
                if let Some(cover_url) = &album.cover_art_url {
                    img {
                        src: "{cover_url}",
                        alt: "Album cover for {album.title}",
                        class: "w-full h-full object-cover",
                    }
                } else {
                    div { class: "text-gray-500 text-4xl", "🎵" }
                }

                // Hover overlay with play button
                div {
                    class: "absolute inset-0 bg-black/50 flex items-center justify-center rounded-lg opacity-0 group-hover:opacity-100 transition-opacity",
                    button {
                        class: "w-16 h-16 bg-transparent hover:bg-white/10 rounded-full flex items-center justify-center text-white text-2xl shadow-lg",
                        disabled: is_loading(),
                        onclick: move |evt| {
                            evt.stop_propagation();
                            if is_loading() {
                                return;
                            }
                            is_loading.set(true);
                            let album_id = album.id.clone();
                            let library_manager = library_manager.clone();
                            let playback = playback.clone();
                            let mut is_loading = is_loading;
                            spawn(async move {
                                match get_album_track_ids(&library_manager, &album_id).await {
                                    Ok(track_ids) => {
                                        if !track_ids.is_empty() {
                                            let first_track_id = track_ids[0].clone();
                                            playback.play_album(track_ids);

                                            // Subscribe to playback progress to clear loading when playback starts
                                            let mut progress_rx = playback.subscribe_progress();
                                            while let Some(progress) = progress_rx.recv().await {
                                                if let PlaybackProgress::StateChanged { state } = progress {
                                                    match state {
                                                        PlaybackState::Loading { track_id: loading_track_id } => {
                                                            // First track started loading - keep spinner visible
                                                            if loading_track_id == first_track_id {
                                                                continue;
                                                            }
                                                        }
                                                        PlaybackState::Playing { .. } | PlaybackState::Paused { .. } => {
                                                            // Playback started - clear loading
                                                            is_loading.set(false);
                                                            break;
                                                        }
                                                        _ => {}
                                                    }
                                                }
                                            }
                                        } else {
                                            is_loading.set(false);
                                        }
                                    }
                                    Err(e) => {
                                        tracing::warn!("Failed to get tracks for album {}: {}", album_id, e);
                                        is_loading.set(false);
                                    }
                                }
                            });
                        },
                        if is_loading() {
                            div { class: "animate-spin rounded-full h-6 w-6 border-b-2 border-white" }
                        } else {
                            span { style: "margin-left: 1px;", "▶" }
                        }
                    }
                }
            }

            // Album info
            div { class: "p-4",
                h3 {
                    class: "font-bold text-white text-lg mb-1 truncate",
                    title: "{album.title}",
                    "{album.title}"
                }
                p {
                    class: "text-gray-400 text-sm truncate",
                    title: "{artist_name}",
                    "{artist_name}"
                }
                if let Some(year) = album.year {
                    p { class: "text-gray-500 text-xs mt-1", "{year}" }
                }
            }
        }
    }
}
