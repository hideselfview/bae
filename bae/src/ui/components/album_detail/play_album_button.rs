use crate::db::DbTrack;
use crate::library::use_library_manager;
use dioxus::prelude::*;

use super::super::use_playback_service;
use super::utils::get_album_track_ids;

#[component]
pub fn PlayAlbumButton(
    album_id: String,
    tracks: Vec<DbTrack>,
    import_progress: ReadSignal<Option<u8>>,
    is_deleting: ReadSignal<bool>,
) -> Element {
    let playback = use_playback_service();
    let library_manager = use_library_manager();
    let mut show_play_menu = use_signal(|| false);

    rsx! {
        div { class: "relative mt-6",
            div { class: "flex rounded-lg overflow-hidden",
                // Main play button (left side)
                button {
                    class: "flex-1 px-6 py-3 bg-blue-600 hover:bg-blue-500 text-white font-semibold transition-colors flex items-center justify-center gap-2",
                    disabled: import_progress().is_some() || is_deleting(),
                    class: if import_progress().is_some() || is_deleting() { "opacity-50 cursor-not-allowed" } else { "" },
                    onclick: {
                        let tracks = tracks.clone();
                        let playback = playback.clone();
                        move |_| {
                            let track_ids: Vec<String> = tracks.iter().map(|t| t.id.clone()).collect();
                            playback.play_album(track_ids);
                        }
                    },
                    if import_progress().is_some() {
                        "Importing..."
                    } else {
                        "▶ Play Album"
                    }
                }
                // Divider and dropdown trigger (right side)
                div { class: "border-l border-blue-500",
                    button {
                        class: "px-3 py-3 bg-blue-600 hover:bg-blue-500 text-white transition-colors flex items-center justify-center",
                        disabled: import_progress().is_some() || is_deleting(),
                        class: if import_progress().is_some() || is_deleting() { "opacity-50 cursor-not-allowed" } else { "" },
                        onclick: move |evt| {
                            evt.stop_propagation();
                            if !is_deleting() && import_progress().is_none() {
                                show_play_menu.set(!show_play_menu());
                            }
                        },
                        "▼"
                    }
                }
            }

            // Play menu dropdown
            if show_play_menu() {
                div {
                    class: "absolute top-full left-0 right-0 mt-2 bg-gray-700 rounded-lg shadow-lg overflow-hidden z-10 border border-gray-600",
                    button {
                        class: "w-full px-4 py-3 text-left text-white hover:bg-gray-600 transition-colors flex items-center gap-2",
                        disabled: import_progress().is_some() || is_deleting(),
                        onclick: {
                            let album_id = album_id.clone();
                            let library_manager = library_manager.clone();
                            let playback = playback.clone();
                            move |evt| {
                                evt.stop_propagation();
                                show_play_menu.set(false);
                                if !is_deleting() && import_progress().is_none() {
                                    let album_id = album_id.clone();
                                    let library_manager = library_manager.clone();
                                    let playback = playback.clone();
                                    spawn(async move {
                                        if let Ok(track_ids) = get_album_track_ids(&library_manager, &album_id).await {
                                            playback.add_to_queue(track_ids);
                                        }
                                    });
                                }
                            }
                        },
                        "➕ Add Album to Queue"
                    }
                }
            }
        }

        // Click outside to close dropdown
        if show_play_menu() {
            div {
                class: "fixed inset-0 z-[5]",
                onclick: move |_| show_play_menu.set(false),
            }
        }
    }
}
