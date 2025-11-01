use crate::db::DbTrack;
use crate::library::use_library_manager;
use crate::playback::{PlaybackProgress, PlaybackState};
use crate::ui::Route;
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
    is_loading: ReadOnlySignal<bool>,
    is_stopped: ReadOnlySignal<bool>,
) -> Element {
    rsx! {
        div { class: "flex items-center gap-2",
            button {
                class: if is_loading() {
                    "px-3 py-2 bg-gray-700 rounded opacity-50"
                } else {
                    "px-3 py-2 bg-gray-700 rounded hover:bg-gray-600"
                },
                disabled: is_loading(),
                onclick: move |_| on_previous.call(()),
                "⏮"
            }
            if is_playing() {
                if is_loading() {
                    button {
                        class: "px-4 py-2 bg-blue-600 rounded opacity-50 flex items-center justify-center",
                        disabled: true,
                        div { class: "animate-spin rounded-full h-4 w-4 border-b-2 border-white" }
                    }
                } else {
                    button {
                        class: "px-4 py-2 bg-blue-600 rounded hover:bg-blue-500",
                        onclick: move |_| on_pause.call(()),
                        "⏸"
                    }
                }
            } else {
                if is_stopped() {
                    button {
                        class: "px-4 py-2 bg-gray-700 rounded opacity-50",
                        disabled: true,
                        "▶"
                    }
                } else if is_loading() {
                    button {
                        class: "px-4 py-2 bg-green-600 rounded opacity-50 flex items-center justify-center",
                        disabled: true,
                        div { class: "animate-spin rounded-full h-4 w-4 border-b-2 border-white" }
                    }
                } else {
                    button {
                        class: "px-4 py-2 bg-green-600 rounded hover:bg-green-500",
                        onclick: move |_| on_resume.call(()),
                        "▶"
                    }
                }
            }
            button {
                class: if is_loading() {
                    "px-3 py-2 bg-gray-700 rounded opacity-50"
                } else {
                    "px-3 py-2 bg-gray-700 rounded hover:bg-gray-600"
                },
                disabled: is_loading(),
                onclick: move |_| on_next.call(()),
                "⏭"
            }
        }
    }
}

#[component]
fn TrackInfoZone(
    track: ReadOnlySignal<Option<DbTrack>>,
    artist_name: ReadOnlySignal<String>,
    is_loading: ReadOnlySignal<bool>,
) -> Element {
    let library_manager = use_library_manager();
    // let navigator = navigator();

    rsx! {
        div { class: "flex-1",
            if let Some(track) = track() {
                div {
                    class: "font-semibold cursor-pointer hover:text-blue-300 transition-colors",
                    onclick: {
                        let track = track.clone();
                        let library_manager = library_manager.clone();
                        let navigator = navigator();
                        move |_| {
                            let track = track.clone();
                            let library_manager = library_manager.clone();
                            spawn(async move {
                                if let Ok(album_id) = library_manager.get().get_album_id_for_release(&track.release_id).await {
                                    navigator.push(Route::AlbumDetail {
                                        album_id,
                                        release_id: track.release_id.clone(),
                                    });
                                }
                            });
                        }
                    },
                    "{track.title}"
                }
                div { class: "text-sm text-gray-400", "{artist_name()}" }
            } else if is_loading() {
                div { class: "font-semibold text-gray-400", "Loading..." }
                div { class: "text-sm text-gray-500", "Loading" }
            } else {
                div { class: "font-semibold text-gray-400", "No track playing" }
                div { class: "text-sm text-gray-500", "" }
            }
        }
    }
}

fn format_duration(duration: std::time::Duration) -> String {
    let total_secs = duration.as_secs();
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    format!("{:02}:{:02}", mins, secs)
}

#[component]
fn PositionZone(
    position: ReadOnlySignal<Option<std::time::Duration>>,
    duration: ReadOnlySignal<Option<std::time::Duration>>,
    is_paused: ReadOnlySignal<bool>,
    on_seek: EventHandler<std::time::Duration>,
    is_seeking: Signal<bool>,
) -> Element {
    let mut local_position = use_signal(|| *position.read());

    // Sync local_position with position when not seeking
    // Don't read local_position in the effect to avoid reactive loop
    use_effect(move || {
        if !is_seeking() {
            local_position.set(*position.read());
        }
    });

    rsx! {
        if let Some(pos) = local_position() {
            div { class: "flex items-center gap-2 text-sm text-gray-400",
                span { class: "w-12 text-right", "{format_duration(pos)}" }
                if let Some(duration) = duration() {
                    input {
                        r#type: "range",
                        class: "w-64 h-2 bg-gray-700 rounded-lg appearance-none cursor-pointer",
                        style: "background: linear-gradient(to right, #3b82f6 0%, #3b82f6 {(pos.as_secs_f64() / duration.as_secs_f64().max(1.0) * 100.0)}%, #374151 {(pos.as_secs_f64() / duration.as_secs_f64().max(1.0) * 100.0)}%, #374151 100%);",
                        min: "0",
                        max: "{duration.as_secs()}",
                        value: "{pos.as_secs()}",
                        onmousedown: move |_| {
                            is_seeking.set(true);
                        },
                        onmouseup: move |_| {
                            // If user releases without moving, clear is_seeking
                            // This prevents position updates from being blocked forever
                            if is_seeking() {
                                // Small delay to let onchange fire first if it will
                                spawn(async move {
                                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                                    if is_seeking() {
                                        is_seeking.set(false);
                                    }
                                });
                            }
                        },
                        oninput: move |evt| {
                            if let Ok(secs) = evt.value().parse::<u64>() {
                                local_position.set(Some(std::time::Duration::from_secs(secs)));
                            }
                        },
                        onchange: move |evt| {
                            if let Ok(secs) = evt.value().parse::<u64>() {
                                on_seek.call(std::time::Duration::from_secs(secs));
                                // is_seeking will be cleared when StateChanged event arrives
                            }
                        },
                    }
                    span { class: "w-12", "{format_duration(duration)}" }
                } else {
                    div { class: "w-64 h-2 bg-gray-700 rounded-lg",
                        div {
                            class: "h-full bg-blue-600 rounded-lg",
                            style: "width: 50%;",
                        }
                    }
                    span { class: "w-12", "--:--" }
                }
            }
        } else {
            div { class: "w-72" }
        }
    }
}

#[component]
pub fn NowPlayingBar() -> Element {
    let playback = use_playback_service();
    let library_manager = use_library_manager();
    let mut state = use_signal(|| PlaybackState::Stopped);
    let mut current_artist = use_signal(|| "Unknown Artist".to_string());
    let mut is_seeking = use_signal(|| false);

    // Subscribe to playback progress updates
    use_effect({
        let playback = playback.clone();
        let library_manager = library_manager.clone();
        move || {
            let playback = playback.clone();
            let library_manager = library_manager.clone();
            spawn(async move {
                let mut progress_rx = playback.subscribe_progress();
                while let Some(progress) = progress_rx.recv().await {
                    match progress {
                        PlaybackProgress::SeekError {
                            requested_position: _,
                            track_duration: _,
                        } => {
                            // Seek error - could show user notification, but for now just ignore
                            tracing::warn!("Seek failed: requested position past track end");
                        }
                        PlaybackProgress::Seeked {
                            position,
                            track_id: _,
                            was_paused,
                        } => {
                            // Seek completed - update position in current state
                            if is_seeking() {
                                is_seeking.set(false);
                            }

                            // Update position in state, preserving track and duration
                            match state() {
                                PlaybackState::Playing {
                                    ref track,
                                    duration,
                                    ..
                                }
                                | PlaybackState::Paused {
                                    ref track,
                                    duration,
                                    ..
                                } => {
                                    let new_state = if was_paused {
                                        PlaybackState::Paused {
                                            track: track.clone(),
                                            position,
                                            duration,
                                        }
                                    } else {
                                        PlaybackState::Playing {
                                            track: track.clone(),
                                            position,
                                            duration,
                                        }
                                    };
                                    state.set(new_state);
                                }
                                _ => {}
                            }
                        }
                        PlaybackProgress::SeekSkipped {
                            requested_position: _,
                            current_position: _,
                        } => {
                            // Seek was skipped (position difference < 100ms)
                            // Clear is_seeking flag so position updates resume
                            if is_seeking() {
                                is_seeking.set(false);
                            }
                        }
                        PlaybackProgress::StateChanged { state: new_state } => {
                            // Update state first
                            state.set(new_state.clone());

                            // If we were seeking, clear it now that state changed
                            // The position in new_state should match where we seeked to
                            if is_seeking() {
                                is_seeking.set(false);
                            }

                            // Fetch artist for current track
                            if let PlaybackState::Playing { ref track, .. }
                            | PlaybackState::Paused { ref track, .. } = new_state
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
                        }
                        PlaybackProgress::PositionUpdate { position, .. } => {
                            // Ignore position updates while user is seeking
                            if is_seeking() {
                                continue;
                            }

                            // Update position in state, preserving duration
                            if let PlaybackState::Playing {
                                ref track,
                                duration,
                                ..
                            } = state()
                            {
                                state.set(PlaybackState::Playing {
                                    track: track.clone(),
                                    position,
                                    duration,
                                });
                            } else if let PlaybackState::Paused {
                                ref track,
                                duration,
                                ..
                            } = state()
                            {
                                state.set(PlaybackState::Paused {
                                    track: track.clone(),
                                    position,
                                    duration,
                                });
                            }

                            // After a seek completes, the first PositionUpdate will sync local_position
                            // via the use_effect that watches position()
                        }
                        PlaybackProgress::TrackCompleted { .. } => {
                            // Track finished - could auto-advance here if needed
                        }
                    }
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
    let duration = use_memo(move || match state() {
        PlaybackState::Playing { duration, .. } | PlaybackState::Paused { duration, .. } => {
            duration
        }
        _ => None,
    });
    let is_playing = use_memo(move || matches!(state(), PlaybackState::Playing { .. }));
    let is_paused = use_memo(move || matches!(state(), PlaybackState::Paused { .. }));
    let is_loading = use_memo(move || matches!(state(), PlaybackState::Loading { .. }));
    let is_stopped = use_memo(move || matches!(state(), PlaybackState::Stopped));

    let artist_name = use_memo(move || current_artist.read().clone());

    let playback_prev = playback.clone();
    let playback_pause = playback.clone();
    let playback_resume = playback.clone();
    let playback_next = playback.clone();
    let playback_seek = playback.clone();

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
                    is_loading,
                    is_stopped,
                }
                TrackInfoZone {
                    track,
                    artist_name,
                    is_loading,
                }
                PositionZone {
                    position,
                    duration,
                    is_paused,
                    on_seek: move |duration| playback_seek.seek(duration),
                    is_seeking,
                }
            }
        }
    }
}
