use crate::db::{DbAlbum, DbArtist, DbRelease, DbTrack};
use crate::library::use_library_manager;
use dioxus::prelude::*;

use super::album_cover_section::AlbumCoverSection;
use super::album_metadata::AlbumMetadata;
use super::delete_release_dialog::DeleteReleaseDialog;
use super::export_error_toast::ExportErrorToast;
use super::play_album_button::PlayAlbumButton;
use super::release_tabs_section::ReleaseTabsSection;
use super::track_row::TrackRow;
use super::ViewFilesModal;

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
    let library_manager = use_library_manager();
    let is_deleting = use_signal(|| false);
    let mut show_release_delete_confirm = use_signal(|| None::<String>);
    let mut show_view_files_modal = use_signal(|| None::<String>);
    let is_exporting = use_signal(|| false);
    let mut export_error = use_signal(|| None::<String>);

    // Load torrent info for all releases
    let library_manager_for_torrents = library_manager.clone();
    let releases_for_torrents = releases.clone();
    let torrents_resource = use_resource(move || {
        let library_manager = library_manager_for_torrents.clone();
        let releases = releases_for_torrents.clone();
        async move {
            let mut torrents = std::collections::HashMap::new();
            for release in &releases {
                if let Ok(Some(torrent)) = library_manager
                    .get()
                    .database()
                    .get_torrent_by_release(&release.id)
                    .await
                {
                    torrents.insert(release.id.clone(), torrent);
                }
            }
            Ok::<_, crate::library::LibraryError>(torrents)
        }
    });

    rsx! {
        div { class: "grid grid-cols-1 lg:grid-cols-3 gap-8",

            // Album artwork and info
            div { class: "lg:col-span-1",
                div { class: "bg-gray-800 rounded-lg p-6",

                    AlbumCoverSection {
                        album: album.clone(),
                        import_progress,
                        is_deleting,
                        is_exporting,
                        export_error,
                        on_album_deleted,
                        first_release_id: releases.first().map(|r| r.id.clone()),
                        has_single_release: releases.len() == 1,
                    }

                    AlbumMetadata {
                        album: album.clone(),
                        artists: artists.clone(),
                        track_count: tracks.len(),
                        selected_release: releases.iter().find(|r| Some(r.id.clone()) == selected_release_id).cloned(),
                    }

                    PlayAlbumButton {
                        album_id: album.id.clone(),
                        tracks: tracks.clone(),
                        import_progress,
                        is_deleting,
                    }
                }
            }

            // Tracklist
            div { class: "lg:col-span-2",
                div { class: "bg-gray-800 rounded-lg p-6",

                    // Release tabs (show only if multiple releases)
                    if releases.len() > 1 {
                        ReleaseTabsSection {
                            releases: releases.clone(),
                            selected_release_id: selected_release_id.clone(),
                            on_release_select,
                            is_deleting,
                            is_exporting,
                            export_error,
                            on_view_files: move |id| show_view_files_modal.set(Some(id)),
                            on_delete_release: move |id| show_release_delete_confirm.set(Some(id)),
                            torrents_resource,
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

        // Release delete confirmation dialog
        if let Some(release_id_to_delete) = show_release_delete_confirm() {
            if releases.iter().any(|r| r.id == release_id_to_delete) {
                DeleteReleaseDialog {
                    release_id: release_id_to_delete.clone(),
                    is_last_release: releases.len() == 1,
                    is_deleting,
                    on_confirm: move |_| {
                        show_release_delete_confirm.set(None);
                        on_album_deleted.call(());
                    },
                    on_cancel: move |_| show_release_delete_confirm.set(None),
                }
            }
        }

        // View Files Modal
        if let Some(release_id) = show_view_files_modal() {
            ViewFilesModal {
                release_id: release_id.clone(),
                on_close: move |_| show_view_files_modal.set(None),
            }
        }

        // Export Error Display
        if let Some(ref error) = export_error() {
            ExportErrorToast {
                error: error.clone(),
                on_dismiss: move |_| export_error.set(None),
            }
        }
    }
}
