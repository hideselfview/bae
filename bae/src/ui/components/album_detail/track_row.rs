use crate::db::{DbArtist, DbTrack, ImportStatus};
use crate::library::use_library_manager;
use dioxus::prelude::*;

use super::super::use_playback_service;
use super::utils::format_duration;

/// Individual track row component
#[component]
pub fn TrackRow(track: DbTrack, is_completed: bool) -> Element {
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

            // Completion indicator or play button
            if track.import_status == ImportStatus::Importing && !is_completed {
                div {
                    class: "w-6 text-gray-500 text-sm",
                    "⏳"
                }
            } else if is_completed || track.import_status == ImportStatus::Complete {
                button {
                    class: "opacity-0 group-hover:opacity-100 transition-opacity text-blue-400 hover:text-blue-300",
                    onclick: move |_| {
                        playback.play(track.id.clone());
                    },
                    "▶"
                }
            } else {
                button {
                    class: "opacity-0 group-hover:opacity-100 transition-opacity text-blue-400 hover:text-blue-300",
                    onclick: move |_| {
                        playback.play(track.id.clone());
                    },
                    "▶"
                }
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
