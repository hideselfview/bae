use crate::db::DbTrack;
use crate::library::use_library_manager;
use crate::playback::PlaybackState;
use dioxus::prelude::*;

use super::use_playback_service;

#[component]
fn PlaybackControlsZone(
    on_previous: EventHandler<()>,
    on_pause: EventHandler<()>,
    on_resume: EventHandler<()>,
    on_next: EventHandler<()>,
    is_playing: ReadOnlySignal<bool>,
    is_paused: ReadOnlySignal<bool>,
) -> Element {
    rsx! {
        div { class: "flex items-center gap-2",
            if is_playing() || is_paused() {
                button {
                    class: "px-3 py-2 bg-gray-700 rounded hover:bg-gray-600",
                    onclick: move |_| on_previous.call(()),
                    "⏮"
                }
                if is_playing() {
                    button {
                        class: "px-4 py-2 bg-blue-600 rounded hover:bg-blue-500",
                        onclick: move |_| on_pause.call(()),
                        "⏸"
                    }
                } else {
                    button {
                        class: "px-4 py-2 bg-green-600 rounded hover:bg-green-500",
                        onclick: move |_| on_resume.call(()),
                        "▶"
                    }
                }
                button {
                    class: "px-3 py-2 bg-gray-700 rounded hover:bg-gray-600",
                    onclick: move |_| on_next.call(()),
                    "⏭"
                }
            } else {
                div { class: "w-24" }
            }
        }
    }
}

#[component]
fn TrackInfoZone(
    track: ReadOnlySignal<Option<DbTrack>>,
    artist_name: ReadOnlySignal<String>,
    loading_track_id: ReadOnlySignal<Option<String>>,
) -> Element {
    rsx! {
        div { class: "flex-1",
            if let Some(track) = track() {
                div { class: "font-semibold", "{track.title}" }
                div { class: "text-sm text-gray-400", "{artist_name()}" }
            } else if let Some(track_id) = loading_track_id() {
                div { class: "text-gray-400", "Loading {track_id}..." }
            } else {
                div { class: "text-gray-400", "No track playing" }
            }
        }
    }
}

#[component]
fn PositionZone(
    position: ReadOnlySignal<Option<std::time::Duration>>,
    is_paused: ReadOnlySignal<bool>,
) -> Element {
    rsx! {
        if let Some(position) = position() {
            div { class: "text-sm text-gray-400",
                if is_paused() {
                    "⏸ {position.as_secs()}s"
                } else {
                    "{position.as_secs()}s"
                }
            }
        } else {
            div { class: "w-16" }
        }
    }
}

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

    // Derive reactive signals from state
    let track = use_memo(move || match state() {
        PlaybackState::Playing { ref track, .. } | PlaybackState::Paused { ref track, .. } => {
            Some(track.clone())
        }
        _ => None,
    });
    let position = use_memo(move || match state() {
        PlaybackState::Playing { position, .. } | PlaybackState::Paused { position, .. } => {
            Some(position)
        }
        _ => None,
    });
    let is_playing = use_memo(move || matches!(state(), PlaybackState::Playing { .. }));
    let is_paused = use_memo(move || matches!(state(), PlaybackState::Paused { .. }));
    let loading_track_id = use_memo(move || match state() {
        PlaybackState::Loading { track_id } => Some(track_id.clone()),
        _ => None,
    });

    let artist_name = use_memo(move || current_artist.read().clone());

    let playback_prev = playback.clone();
    let playback_pause = playback.clone();
    let playback_resume = playback.clone();
    let playback_next = playback.clone();

    rsx! {
        div { class: "fixed bottom-0 left-0 right-0 bg-gray-800 text-white p-4 border-t border-gray-700",
            div { class: "flex items-center gap-4",
                PlaybackControlsZone {
                    on_previous: move |_| playback_prev.previous(),
                    on_pause: move |_| playback_pause.pause(),
                    on_resume: move |_| playback_resume.resume(),
                    on_next: move |_| playback_next.next(),
                    is_playing,
                    is_paused,
                }
                TrackInfoZone {
                    track,
                    artist_name,
                    loading_track_id,
                }
                PositionZone {
                    position,
                    is_paused,
                }
            }
        }
    }
}
