use crate::db::{DbAlbum, DbTrack};
use crate::library::use_library_manager;
use crate::playback::PlaybackState;
use crate::ui::Route;
use dioxus::prelude::*;

use super::album_detail::utils::format_duration;
use super::playback_hooks::{use_playback_queue, use_playback_service, use_playback_state};

/// Shared state for queue sidebar visibility
#[derive(Clone)]
pub struct QueueSidebarState {
    pub is_open: Signal<bool>,
}

/// Track info with album details
#[derive(Clone, Debug)]
struct TrackWithAlbum {
    track: DbTrack,
    album: Option<DbAlbum>,
}

/// Queue sidebar component - toggleable right sidebar showing playback queue
#[component]
pub fn QueueSidebar() -> Element {
    let sidebar_state = use_context::<QueueSidebarState>();
    let mut is_open = sidebar_state.is_open;
    let queue_hook = use_playback_queue();
    let playback_state = use_playback_state();
    let library_manager = use_library_manager();

    // Get current playing track
    let current_track = use_memo(move || match playback_state() {
        PlaybackState::Playing { ref track, .. } | PlaybackState::Paused { ref track, .. } => {
            Some(track.clone())
        }
        _ => None,
    });

    // Fetch track details for each queue item
    let queue = queue_hook.tracks;
    let playback = use_playback_service();
    let clear_fn = queue_hook.clear;
    let track_details = use_signal(std::collections::HashMap::<String, TrackWithAlbum>::new);

    // Fetch track and album details when queue or current track changes
    use_effect({
        let library_manager = library_manager.clone();
        move || {
            let library_manager = library_manager.clone();
            let queue_val = queue.read().clone();
            let mut track_details = track_details;
            spawn(async move {
                let mut new_details = std::collections::HashMap::<String, TrackWithAlbum>::new();

                // Fetch details for queue items
                for track_id in queue_val.iter() {
                    if let Ok(Some(track)) = library_manager.get().get_track(track_id).await {
                        new_details.insert(track_id.clone(), TrackWithAlbum { track, album: None });
                    }
                }

                // Fetch album details for all tracks
                for (track_id, track_with_album) in new_details.iter_mut() {
                    if let Ok(album_id) =
                        library_manager.get().get_album_id_for_track(track_id).await
                    {
                        if let Ok(Some(album)) =
                            library_manager.get().get_album_by_id(&album_id).await
                        {
                            track_with_album.album = Some(album);
                        }
                    }
                }

                track_details.set(new_details);
            });
        }
    });

    // Fetch album for current track
    let mut current_track_album = use_signal(|| Option::<DbAlbum>::None);
    use_effect({
        let library_manager = library_manager.clone();
        move || {
            if let Some(track) = current_track() {
                let track_id = track.id.clone();
                let library_manager = library_manager.clone();
                let mut current_track_album = current_track_album;
                spawn(async move {
                    if let Ok(album_id) = library_manager
                        .get()
                        .get_album_id_for_track(&track_id)
                        .await
                    {
                        if let Ok(Some(album)) =
                            library_manager.get().get_album_by_id(&album_id).await
                        {
                            current_track_album.set(Some(album));
                        }
                    }
                });
            } else {
                current_track_album.set(None);
            }
        }
    });

    rsx! {
        if is_open() {
            div {
                class: "fixed top-0 right-0 h-full w-80 bg-gray-900 border-l border-gray-700 z-50 flex flex-col shadow-2xl",
                // Content
                div {
                    class: "flex-1 overflow-y-auto",
                    // Now playing section
                    div {
                        div { class: "px-4 pt-4 pb-2",
                            h3 { class: "text-sm font-semibold text-gray-400 uppercase tracking-wide", "Now playing" }
                        }
                        if let Some(ref current_track_val) = current_track() {
                            QueueItem {
                                track_id: current_track_val.id.clone(),
                                index: 0,
                                is_current: true,
                                track: current_track_val.clone(),
                                album: current_track_album(),
                                on_remove: {
                                    // Can't remove current track from queue (it's not in queue)
                                    move |_| {}
                                },
                            }
                        } else {
                            div { class: "px-4 py-3 text-gray-500 text-sm",
                                "Nothing playing"
                            }
                        }
                    }
                    // Up next section
                    div {
                        div { class: "px-4 pt-4 pb-2",
                            h3 { class: "text-sm font-semibold text-gray-400 uppercase tracking-wide", "Up next" }
                        }
                        if !queue.read().is_empty() {
                            for (index, track_id) in queue.read().iter().enumerate() {
                                if let Some(track_with_album) = track_details.read().get(track_id).cloned() {
                                    QueueItem {
                                        track_id: track_id.clone(),
                                        index: index,
                                        is_current: false,
                                        track: track_with_album.track,
                                        album: track_with_album.album,
                                        on_remove: {
                                            let playback_clone = playback.clone();
                                            move |idx| {
                                                playback_clone.remove_from_queue(idx);
                                            }
                                        },
                                    }
                                }
                            }
                        } else {
                            div { class: "px-4 py-3 text-gray-500 text-sm",
                                "No tracks queued"
                            }
                        }
                    }
                }
                // Footer with buttons
                div { class: "flex items-center justify-between p-4 border-t border-gray-700",
                    button {
                        class: "px-3 py-2 bg-gray-700 rounded hover:bg-gray-600 text-sm",
                        onclick: move |_| (clear_fn)(),
                        "Clear"
                    }
                    button {
                        class: "px-3 py-2 bg-gray-700 rounded hover:bg-gray-600",
                        onclick: move |_| {
                            is_open.set(false);
                        },
                        "☰"
                    }
                }
            }
        }
    }
}

/// Individual queue item component
#[component]
fn QueueItem(
    track_id: String,
    index: usize,
    is_current: bool,
    track: DbTrack,
    album: Option<DbAlbum>,
    on_remove: EventHandler<usize>,
) -> Element {
    rsx! {
        div {
            class: if is_current {
                "flex items-center gap-3 p-3 border-b border-gray-700 bg-blue-500/10 hover:bg-blue-500/15 group"
            } else {
                "flex items-center gap-3 p-3 border-b border-gray-700 hover:bg-gray-800 group"
            },
            // Album cover
            div { class: "w-12 h-12 flex-shrink-0 bg-gray-700 rounded overflow-hidden",
                if let Some(album) = &album {
                    if let Some(cover_url) = &album.cover_art_url {
                        img {
                            src: "{cover_url}",
                            alt: "Album cover",
                            class: "w-full h-full object-cover",
                        }
                    } else {
                        div { class: "w-full h-full flex items-center justify-center text-gray-500 text-xl", "🎵" }
                    }
                } else {
                    div { class: "w-full h-full flex items-center justify-center text-gray-500 text-xl", "🎵" }
                }
            }
            // Track info
            div { class: "flex-1 min-w-0",
                // Title and duration on same line
                div { class: "flex items-center gap-2",
                    button {
                        class: if is_current { "font-medium text-blue-300 hover:text-blue-200 text-left truncate flex-1" } else { "font-medium text-white hover:text-blue-300 text-left truncate flex-1" },
                        onclick: {
                            let album_id = album.as_ref().map(|a| a.id.clone());
                            let navigator = navigator();
                            move |_| {
                                if let Some(album_id_clone) = album_id.clone() {
                                    navigator.push(Route::AlbumDetail {
                                        album_id: album_id_clone,
                                        release_id: String::new(),
                                    });
                                }
                            }
                        },
                        "{track.title}"
                    }
                    span { class: "text-sm text-gray-400 flex-shrink-0",
                        if let Some(duration_ms) = track.duration_ms {
                            {format_duration(duration_ms)}
                        } else {
                            "—:—"
                        }
                    }
                }
                // Album title on second line
                if let Some(album) = &album {
                    div { class: "text-sm text-gray-400 truncate",
                        "{album.title}"
                    }
                } else {
                    div { class: "text-sm text-gray-400 truncate",
                        "Loading..."
                    }
                }
            }
            // Remove button (only show for queue items, not current track)
            if !is_current {
                button {
                    class: "px-2 py-1 text-sm text-gray-400 hover:text-red-400 rounded opacity-0 group-hover:opacity-100 transition-opacity",
                    onclick: move |_| on_remove.call(index),
                    "✕"
                }
            }
        }
    }
}
