use crate::db::DbRelease;
use crate::library::SharedLibraryManager;
use crate::playback::{PlaybackProgress, PlaybackState};
use crate::ui::components::use_playback_service;
use dioxus::prelude::*;

use super::utils::format_release_display;

#[derive(Clone, Copy, PartialEq)]
pub enum ReleaseAction {
    Play,
    Queue,
}

#[component]
pub fn ReleaseSubmenu(
    releases: ReadSignal<Vec<DbRelease>>,
    action: ReleaseAction,
    library_manager: SharedLibraryManager,
    is_loading: Signal<bool>,
    on_close: EventHandler<()>,
) -> Element {
    let playback = use_playback_service();
    rsx! {
        div {
            class: "absolute left-full top-0 ml-2 bg-gray-700 rounded-lg shadow-lg overflow-hidden z-30 border border-gray-600 min-w-[200px]",
            for release in releases().iter() {
                {
                    let release_id = release.id.clone();
                    let release_display = format_release_display(release);

                    rsx! {
                        button {
                            class: "w-full px-4 py-2 text-left text-white hover:bg-gray-600 transition-colors text-sm",
                            disabled: is_loading(),
                            onclick: {
                                let release_id_clone = release_id.clone();
                                let library_manager_clone = library_manager.clone();
                                let playback_clone = playback.clone();
                                let mut is_loading_clone = is_loading;
                                move |evt| {
                                    evt.stop_propagation();
                                    on_close.call(());

                                    if is_loading_clone() {
                                        return;
                                    }

                                    let release_id = release_id_clone.clone();
                                    let library_manager = library_manager_clone.clone();
                                    let playback = playback_clone.clone();
                                    let action = action;

                                    if action == ReleaseAction::Play {
                                        is_loading_clone.set(true);
                                        let mut is_loading = is_loading_clone;
                                        spawn(async move {
                                            match library_manager.get().get_tracks(&release_id).await {
                                                Ok(mut tracks) => {
                                                    tracks.sort_by(|a, b| match (a.track_number, b.track_number) {
                                                        (Some(a_num), Some(b_num)) => a_num.cmp(&b_num),
                                                        (Some(_), None) => std::cmp::Ordering::Less,
                                                        (None, Some(_)) => std::cmp::Ordering::Greater,
                                                        (None, None) => std::cmp::Ordering::Equal,
                                                    });
                                                    let track_ids: Vec<String> = tracks.iter().map(|t| t.id.clone()).collect();
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
                                                    tracing::warn!("Failed to get tracks for release {}: {}", release_id, e);
                                                    is_loading.set(false);
                                                }
                                            }
                                        });
                                    } else {
                                        spawn(async move {
                                            match library_manager.get().get_tracks(&release_id).await {
                                                Ok(mut tracks) => {
                                                    tracks.sort_by(|a, b| match (a.track_number, b.track_number) {
                                                        (Some(a_num), Some(b_num)) => a_num.cmp(&b_num),
                                                        (Some(_), None) => std::cmp::Ordering::Less,
                                                        (None, Some(_)) => std::cmp::Ordering::Greater,
                                                        (None, None) => std::cmp::Ordering::Equal,
                                                    });
                                                    let track_ids: Vec<String> = tracks.iter().map(|t| t.id.clone()).collect();
                                                    playback.add_to_queue(track_ids);
                                                }
                                                Err(e) => {
                                                    tracing::warn!("Failed to get tracks for release {}: {}", release_id, e);
                                                }
                                            }
                                        });
                                    }
                                }
                            },
                            "{release_display}"
                        }
                    }
                }
            }
        }
    }
}
