use crate::db::DbRelease;
use crate::library::SharedLibraryManager;
use crate::playback::{PlaybackProgress, PlaybackState};
use crate::ui::components::album_detail::utils::get_album_track_ids;
use crate::ui::components::use_playback_service;
use dioxus::prelude::*;

use super::release_submenu::{ReleaseAction, ReleaseSubmenu};

#[component]
pub fn AlbumDropdownMenu(
    album_id: ReadSignal<String>,
    releases: ReadSignal<Vec<DbRelease>>,
    library_manager: SharedLibraryManager,
    is_loading: Signal<bool>,
    on_close: EventHandler<()>,
) -> Element {
    let playback = use_playback_service();
    let mut show_play_release_menu = use_signal(|| false);
    let mut show_queue_release_menu = use_signal(|| false);
    let has_multiple_releases = releases().len() > 1;

    rsx! {
        div {
            class: "absolute top-full right-0 mt-2 bg-gray-700 rounded-lg shadow-lg overflow-hidden z-20 border border-gray-600 min-w-[160px]",

            if has_multiple_releases {
                // Play with release selection
                div { class: "relative",
                    button {
                        class: "w-full px-4 py-3 text-left text-white hover:bg-gray-600 transition-colors flex items-center gap-2 justify-between",
                        disabled: is_loading(),
                        onclick: move |evt| {
                            evt.stop_propagation();
                            show_play_release_menu.set(!show_play_release_menu());
                            show_queue_release_menu.set(false);
                        },
                        span { "▶ Play Album" }
                        span { "▶" }
                    }

                    if show_play_release_menu() {
                        ReleaseSubmenu {
                            releases,
                            action: ReleaseAction::Play,
                            library_manager: library_manager.clone(),
                            is_loading,
                            on_close: move |_| {
                                show_play_release_menu.set(false);
                                on_close.call(());
                            }
                        }
                    }
                }

                // Queue with release selection
                div { class: "relative",
                    button {
                        class: "w-full px-4 py-3 text-left text-white hover:bg-gray-600 transition-colors flex items-center gap-2 justify-between",
                        disabled: is_loading(),
                        onclick: move |evt| {
                            evt.stop_propagation();
                            show_queue_release_menu.set(!show_queue_release_menu());
                            show_play_release_menu.set(false);
                        },
                        span { "➕ Add to Queue" }
                        span { "▶" }
                    }

                    if show_queue_release_menu() {
                        ReleaseSubmenu {
                            releases,
                            action: ReleaseAction::Queue,
                            library_manager: library_manager.clone(),
                            is_loading,
                            on_close: move |_| {
                                show_queue_release_menu.set(false);
                                on_close.call(());
                            }
                        }
                    }
                }
            } else {
                // Single release - direct actions
                button {
                    class: "w-full px-4 py-3 text-left text-white hover:bg-gray-600 transition-colors flex items-center gap-2",
                    disabled: is_loading(),
                    onclick: {
                        let album_id_value = album_id();
                        let library_manager_clone = library_manager.clone();
                        let playback_clone = playback.clone();
                        let mut is_loading_clone = is_loading;
                        move |evt| {
                            evt.stop_propagation();
                            on_close.call(());

                            if is_loading_clone() {
                                return;
                            }

                            is_loading_clone.set(true);
                            let album_id = album_id_value.clone();
                            let library_manager = library_manager_clone.clone();
                            let playback = playback_clone.clone();
                            let mut is_loading = is_loading_clone;

                            spawn(async move {
                                match get_album_track_ids(&library_manager, &album_id).await {
                                    Ok(track_ids) => {
                                        if !track_ids.is_empty() {
                                            let first_track_id = track_ids[0].clone();
                                            playback.play_album(track_ids);

                                            let mut progress_rx = playback.subscribe_progress();
                                            while let Some(progress) = progress_rx.recv().await {
                                                if let PlaybackProgress::StateChanged { state } = progress {
                                                    match state {
                                                        PlaybackState::Loading { track_id: loading_track_id } => {
                                                            if loading_track_id == first_track_id {
                                                                continue;
                                                            }
                                                        }
                                                        PlaybackState::Playing { .. }
                                                        | PlaybackState::Paused { .. } => {
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
                        }
                    },
                    if is_loading() {
                        "Loading..."
                    } else {
                        "▶ Play Album"
                    }
                }

                button {
                    class: "w-full px-4 py-3 text-left text-white hover:bg-gray-600 transition-colors flex items-center gap-2",
                    disabled: is_loading(),
                    onclick: {
                        let album_id_value = album_id();
                        let library_manager_clone = library_manager.clone();
                        let playback_clone = playback.clone();
                        move |evt| {
                            evt.stop_propagation();
                            on_close.call(());

                            if is_loading() {
                                return;
                            }

                            let album_id = album_id_value.clone();
                            let library_manager = library_manager_clone.clone();
                            let playback = playback_clone.clone();

                            spawn(async move {
                                if let Ok(track_ids) = get_album_track_ids(&library_manager, &album_id).await {
                                    playback.add_to_queue(track_ids);
                                }
                            });
                        }
                    },
                    "➕ Add to Queue"
                }
            }
        }
    }
}
