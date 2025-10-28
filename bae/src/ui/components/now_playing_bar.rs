use crate::library::use_library_manager;
use crate::playback::PlaybackState;
use dioxus::prelude::*;

use super::use_playback_service;

#[component]
pub fn NowPlayingBar() -> Element {
    let playback = use_playback_service();
    let library_manager = use_library_manager();
    let mut state = use_signal(|| PlaybackState::Stopped);
    let mut current_artist = use_signal(|| "Unknown Artist".to_string());

    // Poll playback state periodically and fetch artist
    use_effect({
        let playback = playback.clone();
        let library_manager = library_manager.clone();
        move || {
            let playback = playback.clone();
            let library_manager = library_manager.clone();
            spawn(async move {
                loop {
                    let current_state = playback.get_state().await;

                    // Fetch artist for current track
                    if let PlaybackState::Playing { ref track, .. }
                    | PlaybackState::Paused { ref track, .. } = current_state
                    {
                        if let Ok(artists) =
                            library_manager.get().get_artists_for_track(&track.id).await
                        {
                            if !artists.is_empty() {
                                let artist_names: Vec<_> =
                                    artists.iter().map(|a| a.name.as_str()).collect();
                                current_artist.set(artist_names.join(", "));
                            } else {
                                current_artist.set("Unknown Artist".to_string());
                            }
                        }
                    }

                    state.set(current_state);
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
            });
        }
    });

    match state() {
        PlaybackState::Stopped => rsx! {
            div { class: "fixed bottom-0 left-0 right-0 bg-gray-800 text-white p-4 border-t border-gray-700",
                div { class: "flex items-center justify-between",
                    div { class: "text-gray-400", "No track playing" }
                }
            }
        },
        PlaybackState::Loading { track_id } => rsx! {
            div { class: "fixed bottom-0 left-0 right-0 bg-gray-800 text-white p-4 border-t border-gray-700",
                div { class: "flex items-center justify-between",
                    div { class: "text-gray-400", "Loading {track_id}..." }
                }
            }
        },
        PlaybackState::Playing { track, position } => {
            let artist_name = current_artist();
            rsx! {
                div { class: "fixed bottom-0 left-0 right-0 bg-gray-800 text-white p-4 border-t border-gray-700",
                    div { class: "flex items-center gap-4",
                        // Playback controls
                        div { class: "flex items-center gap-2",
                            button {
                                class: "px-3 py-2 bg-gray-700 rounded hover:bg-gray-600",
                                onclick: {
                                    let playback = playback.clone();
                                    move |_| playback.previous()
                                },
                                "⏮"
                            }
                            button {
                                class: "px-4 py-2 bg-blue-600 rounded hover:bg-blue-500",
                                onclick: {
                                    let playback = playback.clone();
                                    move |_| playback.pause()
                                },
                                "⏸"
                            }
                            button {
                                class: "px-3 py-2 bg-gray-700 rounded hover:bg-gray-600",
                                onclick: {
                                    let playback = playback.clone();
                                    move |_| playback.next()
                                },
                                "⏭"
                            }
                        }

                        // Track info
                        div { class: "flex-1",
                            div { class: "font-semibold", "{track.title}" }
                            div { class: "text-sm text-gray-400", "{artist_name}" }
                        }

                        // Position display
                        div { class: "text-sm text-gray-400", "{position.as_secs()}s" }
                    }
                }
            }
        }
        PlaybackState::Paused { track, position } => {
            let artist_name = current_artist();
            rsx! {
                div { class: "fixed bottom-0 left-0 right-0 bg-gray-800 text-white p-4 border-t border-gray-700",
                    div { class: "flex items-center gap-4",
                        // Playback controls
                        div { class: "flex items-center gap-2",
                            button {
                                class: "px-3 py-2 bg-gray-700 rounded hover:bg-gray-600",
                                onclick: {
                                    let playback = playback.clone();
                                    move |_| playback.previous()
                                },
                                "⏮"
                            }
                            button {
                                class: "px-4 py-2 bg-green-600 rounded hover:bg-green-500",
                                onclick: {
                                    let playback = playback.clone();
                                    move |_| playback.resume()
                                },
                                "▶"
                            }
                            button {
                                class: "px-3 py-2 bg-gray-700 rounded hover:bg-gray-600",
                                onclick: {
                                    let playback = playback.clone();
                                    move |_| playback.next()
                                },
                                "⏭"
                            }
                        }

                        // Track info
                        div { class: "flex-1",
                            div { class: "font-semibold", "{track.title}" }
                            div { class: "text-sm text-gray-400", "{artist_name}" }
                        }

                        // Position display
                        div { class: "text-sm text-gray-400", "⏸ {position.as_secs()}s" }
                    }
                }
            }
        }
    }
}
