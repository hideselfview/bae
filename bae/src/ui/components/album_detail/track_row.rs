use crate::db::{DbArtist, DbTrack};
use crate::library::use_library_manager;
use crate::playback::{PlaybackProgress, PlaybackState};
use dioxus::prelude::*;

use super::super::import_hooks::TrackImportState;
use super::super::use_track_progress;
use super::super::{use_playback_service, use_playback_state};
use super::utils::format_duration;

/// Individual track row component
#[component]
pub fn TrackRow(track: DbTrack, release_id: String) -> Element {
    let library_manager = use_library_manager();
    let playback = use_playback_service();
    let playback_state = use_playback_state();
    let mut track_artists = use_signal(Vec::<DbArtist>::new);
    let track_progress = use_track_progress(track.id.clone(), track.import_status);
    // let track_progress = use_signal(|| TrackImportState::Importing { percent: 12 });

    // Track playback state for this track
    // Initialize synchronously from shared playback state
    let track_id = track.id.clone();
    let is_currently_playing = use_signal(move || {
        matches!(
            playback_state(),
            PlaybackState::Playing {
                track: ref playing_track,
                ..
            } if playing_track.id == track_id
        )
    });
    let track_id = track.id.clone();
    let is_currently_paused = use_signal(move || {
        matches!(
            playback_state(),
            PlaybackState::Paused {
                track: ref paused_track,
                ..
            } if paused_track.id == track_id
        )
    });
    let track_id = track.id.clone();
    let is_loading = use_signal(move || {
        matches!(
            playback_state(),
            PlaybackState::Loading {
                track_id: ref loading_track_id,
            } if loading_track_id == &track_id
        )
    });

    // Subscribe to playback progress to track if this track is playing
    use_effect({
        let track_id = track.id.clone();
        let playback = playback.clone();
        move || {
            let mut progress_rx = playback.subscribe_progress();
            let track_id = track_id.clone();
            let mut is_currently_playing = is_currently_playing;
            let mut is_currently_paused = is_currently_paused;
            let mut is_loading = is_loading;
            spawn(async move {
                while let Some(progress) = progress_rx.recv().await {
                    if let PlaybackProgress::StateChanged { state } = progress {
                        match state {
                            PlaybackState::Loading {
                                track_id: loading_track_id,
                            } => {
                                is_loading.set(loading_track_id == track_id);
                            }
                            PlaybackState::Playing {
                                track: playing_track,
                                ..
                            } => {
                                is_currently_playing.set(playing_track.id == track_id);
                                is_currently_paused.set(false);
                                is_loading.set(false);
                            }
                            PlaybackState::Paused {
                                track: paused_track,
                                ..
                            } => {
                                is_currently_playing.set(false);
                                is_currently_paused.set(paused_track.id == track_id);
                                is_loading.set(false);
                            }
                            _ => {
                                is_currently_playing.set(false);
                                is_currently_paused.set(false);
                                is_loading.set(false);
                            }
                        }
                    }
                }
            });
        }
    });

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

    let progress_state = track_progress();
    let is_importing = matches!(
        progress_state,
        TrackImportState::Queued | TrackImportState::Importing { .. }
    );
    let is_complete = matches!(progress_state, TrackImportState::Complete);
    let is_failed = matches!(progress_state, TrackImportState::Failed);

    let progress_percent = if let TrackImportState::Importing { percent } = progress_state {
        percent
    } else {
        0
    };

    rsx! {
        div {
            class: "relative flex items-center py-3 px-4 rounded-lg group overflow-hidden",
            class: if is_complete { "hover:bg-gray-700 transition-colors" } else { "" },

            // Progress bar background (only when importing/queued)
            if is_importing {
                div {
                    class: "absolute inset-0 bg-blue-500 opacity-10 transition-all duration-300",
                    style: "width: {progress_percent}%",
                }
            }

            // Failed state background
            if is_failed {
                div {
                    class: "absolute inset-0 bg-red-500 opacity-10",
                }
            }

            // Content (with relative positioning to stay above progress bar)
            div { class: "relative flex items-center w-full",

                // Play/Pause button (only show when complete)
                if is_complete {
                    if is_loading() {
                        div {
                            class: "w-6 flex items-center justify-center",
                            div { class: "animate-spin rounded-full h-4 w-4 border-b-2 border-blue-400" }
                        }
                    } else if is_currently_playing() {
                        button {
                            class: "w-6 text-blue-400 hover:text-blue-300",
                            onclick: move |_| {
                                playback.pause();
                            },
                            "⏸"
                        }
                    } else if is_currently_paused() {
                        button {
                            class: "w-6 text-blue-400 hover:text-blue-300",
                            onclick: move |_| {
                                playback.resume();
                            },
                            "▶"
                        }
                    } else {
                        button {
                            class: "w-6 opacity-0 group-hover:opacity-100 transition-opacity text-blue-400 hover:text-blue-300",
                            onclick: move |_| {
                                playback.play(track.id.clone());
                            },
                            "▶"
                        }
                    }
                } else {
                    div { class: "w-6" }
                }

                // Track number
                div {
                    class: "w-12 text-right text-sm font-mono",
                    class: if is_failed {
                        "text-red-400"
                    } else if is_importing {
                        "text-gray-600"
                    } else {
                        "text-gray-400"
                    },
                    if let Some(track_num) = track.track_number {
                        "{track_num}."
                    } else {
                        "—"
                    }
                }

                // Track info
                div { class: "flex-1 ml-4",
                    h3 {
                        class: "font-medium transition-colors",
                        class: if is_failed {
                            "text-red-300"
                        } else if is_importing {
                            "text-gray-500"
                        } else {
                            "text-white group-hover:text-blue-300"
                        },
                        "{track.title}"
                    }
                    if !track_artists().is_empty() {
                        p {
                            class: "text-sm",
                            class: if is_failed {
                                "text-red-400"
                            } else if is_importing {
                                "text-gray-600"
                            } else {
                                "text-gray-400"
                            },
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
                    class: "text-sm font-mono",
                    class: if is_failed {
                        "text-red-400"
                    } else if is_importing {
                        "text-gray-600"
                    } else {
                        "text-gray-400"
                    },
                    if let Some(duration_ms) = track.duration_ms {
                        {format_duration(duration_ms)}
                    } else {
                        "—:—"
                    }
                }
            }
        }
    }
}
