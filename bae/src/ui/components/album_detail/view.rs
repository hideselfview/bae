use crate::db::{DbAlbum, DbArtist, DbRelease, DbTrack};
use crate::library::use_library_manager;
use dioxus::prelude::*;
use tracing::error;

use super::super::use_playback_service;
use super::album_art::AlbumArt;
use super::track_row::TrackRow;
use super::utils::get_album_track_ids;

/// Album detail view component
#[component]
pub fn AlbumDetailView(
    album: DbAlbum,
    releases: Vec<DbRelease>,
    artists: Vec<DbArtist>,
    selected_release_id: Option<String>,
    on_release_select: EventHandler<String>,
    tracks: Vec<DbTrack>,
    import_progress: ReadSignal<Option<u8>>,
    on_album_deleted: EventHandler<()>,
) -> Element {
    let playback = use_playback_service();
    let library_manager = use_library_manager();
    let mut show_delete_confirm = use_signal(|| false);
    let mut show_release_delete_confirm = use_signal(|| None::<String>);
    let mut is_deleting = use_signal(|| false);
    let mut show_dropdown = use_signal(|| false);
    let mut show_release_dropdown = use_signal(|| None::<String>);

    let artist_name = if artists.is_empty() {
        "Unknown Artist".to_string()
    } else if artists.len() == 1 {
        artists[0].name.clone()
    } else {
        artists
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };

    rsx! {
        div { class: "grid grid-cols-1 lg:grid-cols-3 gap-8",

            // Album artwork and info
            div { class: "lg:col-span-1",
                div { class: "bg-gray-800 rounded-lg p-6",

                    // Album cover
                    div { class: "mb-6",
                        AlbumArt {
                            title: album.title.clone(),
                            cover_url: album.cover_art_url.clone(),
                            import_progress,
                        }
                    }

                    // Album metadata
                    div {
                        h1 { class: "text-2xl font-bold text-white mb-2", "{album.title}" }
                        p { class: "text-lg text-gray-300 mb-4", "{artist_name}" }

                        div { class: "space-y-2 text-sm text-gray-400",
                            if let Some(year) = album.year {
                                div {
                                    span { class: "font-medium", "Year: " }
                                    span { "{year}" }
                                }
                            }
                            if let Some(discogs_release) = &album.discogs_release {
                                div {
                                    span { class: "font-medium", "Discogs Master ID: " }
                                    span { "{discogs_release.master_id}" }
                                }
                            }
                            div {
                                span { class: "font-medium", "Tracks: " }
                                span { "{tracks.len()}" }
                            }
                        }
                    }

                    // Play Album button
                    button {
                        class: "w-full mt-6 px-6 py-3 bg-blue-600 hover:bg-blue-500 text-white font-semibold rounded-lg transition-colors flex items-center justify-center gap-2",
                        disabled: import_progress().is_some() || is_deleting(),
                        class: if import_progress().is_some() || is_deleting() { "opacity-50 cursor-not-allowed" } else { "" },
                        onclick: {
                            let tracks = tracks.clone();
                            let playback_clone = playback.clone();
                            move |_| {
                                let track_ids: Vec<String> = tracks.iter().map(|t| t.id.clone()).collect();
                                playback_clone.play_album(track_ids);
                            }
                        },
                        if import_progress().is_some() {
                            "Importing..."
                        } else {
                            "▶ Play Album"
                        }
                    }

                    // Add Album to Queue button
                    button {
                        class: "w-full mt-3 px-6 py-3 bg-gray-700 hover:bg-gray-600 text-white font-semibold rounded-lg transition-colors flex items-center justify-center gap-2",
                        disabled: import_progress().is_some() || is_deleting(),
                        class: if import_progress().is_some() || is_deleting() { "opacity-50 cursor-not-allowed" } else { "" },
                        onclick: {
                            let album_id = album.id.clone();
                            let library_manager = library_manager.clone();
                            let playback = playback.clone();
                            move |_| {
                                let album_id = album_id.clone();
                                let library_manager = library_manager.clone();
                                let playback = playback.clone();
                                spawn(async move {
                                    if let Ok(track_ids) = get_album_track_ids(&library_manager, &album_id).await {
                                        playback.add_to_queue(track_ids);
                                    }
                                });
                            }
                        },
                        "➕ Add Album to Queue"
                    }

                    // Actions dropdown menu
                    div { class: "relative mt-3",
                        button {
                            class: "w-full px-6 py-3 bg-gray-700 hover:bg-gray-600 text-white font-semibold rounded-lg transition-colors flex items-center justify-center gap-2",
                            disabled: import_progress().is_some() || is_deleting(),
                            class: if import_progress().is_some() || is_deleting() { "opacity-50 cursor-not-allowed" } else { "" },
                            onclick: move |_| {
                                if !is_deleting() && import_progress().is_none() {
                                    show_dropdown.set(!show_dropdown());
                                }
                            },
                            "⋮ More"
                        }

                        // Dropdown menu
                        if show_dropdown() {
                            div {
                                class: "absolute top-full left-0 right-0 mt-2 bg-gray-700 rounded-lg shadow-lg overflow-hidden z-10 border border-gray-600",
                                button {
                                    class: "w-full px-4 py-3 text-left text-red-400 hover:bg-gray-600 transition-colors flex items-center gap-2",
                                    disabled: is_deleting(),
                                    onclick: move |evt| {
                                        evt.stop_propagation();
                                        show_dropdown.set(false);
                                        if !is_deleting() {
                                            show_delete_confirm.set(true);
                                        }
                                    },
                                    "Delete Album"
                                }
                            }
                        }
                    }

                    // Click outside to close dropdowns
                    if show_dropdown() {
                        div {
                            class: "fixed inset-0 z-[5]",
                            onclick: move |_| {
                                show_dropdown.set(false);
                            }
                        }
                    }
                    if show_release_dropdown().is_some() {
                        div {
                            class: "fixed inset-0 z-[5]",
                            onclick: move |_| {
                                show_release_dropdown.set(None);
                            }
                        }
                    }

                    // Delete confirmation dialog
                    if show_delete_confirm() {
                        div {
                            class: "fixed inset-0 bg-black/50 flex items-center justify-center z-50",
                            onclick: move |_| {
                                if !is_deleting() {
                                    show_delete_confirm.set(false);
                                }
                            },
                            div {
                                class: "bg-gray-800 rounded-lg p-6 max-w-md w-full mx-4",
                                onclick: move |evt| evt.stop_propagation(),
                                h2 { class: "text-xl font-bold text-white mb-4", "Delete Album?" }
                                p { class: "text-gray-300 mb-6",
                                    "Are you sure you want to delete \"{album.title}\"? This will delete all releases, tracks, and associated data. This action cannot be undone."
                                }
                                div { class: "flex gap-3 justify-end",
                                    button {
                                        class: "px-4 py-2 bg-gray-700 hover:bg-gray-600 text-white rounded-lg",
                                        disabled: is_deleting(),
                                        onclick: move |_| {
                                            if !is_deleting() {
                                                show_delete_confirm.set(false);
                                            }
                                        },
                                        "Cancel"
                                    }
                                    button {
                                        class: "px-4 py-2 bg-red-600 hover:bg-red-500 text-white rounded-lg",
                                        disabled: is_deleting(),
                                        onclick: {
                                            let album_id = album.id.clone();
                                            let library_manager = library_manager.clone();
                                            move |_| {
                                                if is_deleting() {
                                                    return;
                                                }
                                                is_deleting.set(true);
                                                let album_id = album_id.clone();
                                                let library_manager = library_manager.clone();
                                                let mut is_deleting = is_deleting;
                                                let mut show_delete_confirm = show_delete_confirm;
                                                spawn(async move {
                                                    match library_manager.get().delete_album(&album_id).await {
                                                        Ok(_) => {
                                                            show_delete_confirm.set(false);
                                                            is_deleting.set(false);
                                                            on_album_deleted.call(());
                                                        }
                                                        Err(e) => {
                                                            error!("Failed to delete album: {}", e);
                                                            is_deleting.set(false);
                                                        }
                                                    }
                                                });
                                            }
                                        },
                                        if is_deleting() {
                                            "Deleting..."
                                        } else {
                                            "Delete"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Tracklist
            div { class: "lg:col-span-2",
                div { class: "bg-gray-800 rounded-lg p-6",

                    // Release tabs (if multiple releases exist)
                    if releases.len() > 1 {
                        div { class: "mb-6 border-b border-gray-700",
                            div { class: "flex gap-2 overflow-x-auto",
                                for release in releases.iter() {
                                    {
                                        let is_selected = selected_release_id.as_ref() == Some(&release.id);
                                        let release_id = release.id.clone();
                                        let release_id_for_delete = release.id.clone();
                                        rsx! {
                                            div {
                                                key: "{release.id}",
                                                class: "flex items-center gap-2 relative",
                                                button {
                                                    class: if is_selected { "px-4 py-2 text-sm font-medium text-blue-400 border-b-2 border-blue-400 whitespace-nowrap" } else { "px-4 py-2 text-sm font-medium text-gray-400 hover:text-gray-300 border-b-2 border-transparent whitespace-nowrap" },
                                                    onclick: move |_| {
                                                        on_release_select.call(release_id.clone());
                                                    },
                                                    {
                                                        if let Some(ref name) = release.release_name {
                                                            name.clone()
                                                        } else if let Some(year) = release.year {
                                                            format!("Release ({})", year)
                                                        } else {
                                                            "Release".to_string()
                                                        }
                                                    }
                                                }
                                                div { class: "relative",
                                                    {
                                                        let release_id_for_dropdown = release_id_for_delete.clone();
                                                        rsx! {
                                                            button {
                                                                class: "px-2 py-1 text-sm text-gray-400 hover:text-gray-300 hover:bg-gray-700 rounded",
                                                                disabled: is_deleting(),
                                                                onclick: move |evt| {
                                                                    evt.stop_propagation();
                                                                    if !is_deleting() {
                                                                        let current = show_release_dropdown();
                                                                        if current.as_ref() == Some(&release_id_for_dropdown) {
                                                                            show_release_dropdown.set(None);
                                                                        } else {
                                                                            show_release_dropdown.set(Some(release_id_for_dropdown.clone()));
                                                                        }
                                                                    }
                                                                },
                                                                "⋮"
                                                            }

                                                            // Release dropdown menu
                                                            if show_release_dropdown().as_ref() == Some(&release_id_for_dropdown) {
                                                                {
                                                                    let release_id_for_delete_action = release_id_for_delete.clone();
                                                                    rsx! {
                                                                        div {
                                                                            class: "absolute right-0 top-full mt-1 bg-gray-700 rounded-lg shadow-lg overflow-hidden z-10 border border-gray-600 min-w-[160px]",
                                                                            button {
                                                                                class: "w-full px-4 py-2 text-left text-red-400 hover:bg-gray-600 transition-colors flex items-center gap-2 text-sm",
                                                                                disabled: is_deleting(),
                                                                                onclick: move |evt| {
                                                                                    evt.stop_propagation();
                                                                                    show_release_dropdown.set(None);
                                                                                    if !is_deleting() {
                                                                                        show_release_delete_confirm.set(Some(release_id_for_delete_action.clone()));
                                                                                    }
                                                                                },
                                                                                "Delete Release"
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // Release delete confirmation dialog
                        if let Some(release_id_to_delete) = show_release_delete_confirm() {
                            if releases.iter().any(|r| r.id == release_id_to_delete) {
                                div {
                                    class: "fixed inset-0 bg-black/50 flex items-center justify-center z-50",
                                    onclick: move |_| {
                                        if !is_deleting() {
                                            show_release_delete_confirm.set(None);
                                        }
                                    },
                                    div {
                                        class: "bg-gray-800 rounded-lg p-6 max-w-md w-full mx-4",
                                        onclick: move |evt| evt.stop_propagation(),
                                        h2 { class: "text-xl font-bold text-white mb-4", "Delete Release?" }
                                        p { class: "text-gray-300 mb-6",
                                            "Are you sure you want to delete this release? This will delete all tracks and associated data for this release."
                                            if releases.len() == 1 {
                                                " Since this is the only release, the album will also be deleted."
                                            } else {
                                                ""
                                            }
                                        }
                                        div { class: "flex gap-3 justify-end",
                                            button {
                                                class: "px-4 py-2 bg-gray-700 hover:bg-gray-600 text-white rounded-lg",
                                                disabled: is_deleting(),
                                                onclick: move |_| {
                                                    if !is_deleting() {
                                                        show_release_delete_confirm.set(None);
                                                    }
                                                },
                                                "Cancel"
                                            }
                                            button {
                                                class: "px-4 py-2 bg-red-600 hover:bg-red-500 text-white rounded-lg",
                                                disabled: is_deleting(),
                                                onclick: {
                                                    let release_id = release_id_to_delete.clone();
                                                    let library_manager = library_manager.clone();
                                                    let releases_count = releases.len();
                                                    move |_| {
                                                        if is_deleting() {
                                                            return;
                                                        }
                                                        is_deleting.set(true);
                                                        let release_id = release_id.clone();
                                                        let library_manager = library_manager.clone();
                                                        spawn(async move {
                                                            match library_manager.get().delete_release(&release_id).await {
                                                                Ok(_) => {
                                                                    show_release_delete_confirm.set(None);
                                                                    is_deleting.set(false);
                                                                    // If this was the last release, album was deleted too
                                                                    if releases_count == 1 {
                                                                        on_album_deleted.call(());
                                                                    } else {
                                                                        // Refresh the page to show updated releases
                                                                        on_album_deleted.call(());
                                                                    }
                                                                }
                                                                Err(e) => {
                                                                    error!("Failed to delete release: {}", e);
                                                                    is_deleting.set(false);
                                                                }
                                                            }
                                                        });
                                                    }
                                                },
                                                if is_deleting() {
                                                    "Deleting..."
                                                } else {
                                                    "Delete"
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    h2 { class: "text-xl font-bold text-white mb-4", "Tracklist" }

                    if tracks.is_empty() {
                        div { class: "text-center py-8 text-gray-400",
                            p { "No tracks found for this album." }
                        }
                    } else {
                        div { class: "space-y-2",
                            for track in &tracks {
                                TrackRow {
                                    track: track.clone(),
                                    release_id: selected_release_id.clone().unwrap_or_default(),
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
