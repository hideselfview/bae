use crate::db::DbAlbum;
use crate::library::use_library_manager;
use crate::AppContext;
use dioxus::prelude::*;
use rfd::AsyncFileDialog;
use tracing::error;

use super::super::dialog_context::DialogContext;
use super::album_art::AlbumArt;

#[component]
pub fn AlbumCoverSection(
    album: DbAlbum,
    import_progress: ReadSignal<Option<u8>>,
    is_deleting: Signal<bool>,
    is_exporting: Signal<bool>,
    export_error: Signal<Option<String>>,
    on_album_deleted: EventHandler<()>,
    first_release_id: Option<String>,
    has_single_release: bool,
) -> Element {
    let library_manager = use_library_manager();
    let app_context = use_context::<AppContext>();
    let dialog = use_context::<DialogContext>();
    let mut show_dropdown = use_signal(|| false);
    let mut hover_cover = use_signal(|| false);
    let mut show_view_files_modal = use_signal(|| None::<String>);

    rsx! {
        div {
            class: "mb-6 relative",
            onmouseenter: move |_| hover_cover.set(true),
            onmouseleave: move |_| hover_cover.set(false),
            AlbumArt {
                title: album.title.clone(),
                cover_url: album.cover_art_url.clone(),
                import_progress,
            }

            // Three dot menu button
            if hover_cover() || show_dropdown() {
                div { class: "absolute top-2 right-2 z-10",
                    button {
                        class: "w-8 h-8 bg-gray-800/40 hover:bg-gray-800/60 text-white rounded-lg flex items-center justify-center transition-colors",
                        disabled: import_progress().is_some() || is_deleting(),
                        class: if import_progress().is_some() || is_deleting() { "opacity-50 cursor-not-allowed" } else { "" },
                        onclick: move |evt| {
                            evt.stop_propagation();
                            if !is_deleting() && import_progress().is_none() {
                                show_dropdown.set(!show_dropdown());
                            }
                        },
                        div { class: "flex flex-col gap-1",
                            div { class: "w-1 h-1 bg-white rounded-full" }
                            div { class: "w-1 h-1 bg-white rounded-full" }
                            div { class: "w-1 h-1 bg-white rounded-full" }
                        }
                    }

                    // Dropdown menu
                    if show_dropdown() {
                        div {
                            class: "absolute top-full right-0 mt-2 bg-gray-700 rounded-lg shadow-lg overflow-hidden z-20 border border-gray-600 min-w-[160px]",

                            // Show release actions if there's only one release
                            if has_single_release {
                                if let Some(ref release_id) = first_release_id {
                                    button {
                                        class: "w-full px-4 py-3 text-left text-white hover:bg-gray-600 transition-colors flex items-center gap-2",
                                        disabled: is_deleting() || is_exporting(),
                                        onclick: {
                                            let release_id = release_id.clone();
                                            move |evt| {
                                                evt.stop_propagation();
                                                show_dropdown.set(false);
                                                if !is_deleting() && !is_exporting() {
                                                    show_view_files_modal.set(Some(release_id.clone()));
                                                }
                                            }
                                        },
                                        "View Files"
                                    }
                                    button {
                                        class: "w-full px-4 py-3 text-left text-white hover:bg-gray-600 transition-colors flex items-center gap-2",
                                        disabled: is_deleting() || is_exporting(),
                                        onclick: {
                                            let release_id = release_id.clone();
                                            let library_manager = library_manager.clone();
                                            let cloud_storage = app_context.cloud_storage.clone();
                                            let cache = app_context.cache.clone();
                                            let encryption_service = app_context.encryption_service.clone();
                                            let chunk_size_bytes = app_context.config.chunk_size_bytes;
                                            move |evt| {
                                                evt.stop_propagation();
                                                show_dropdown.set(false);
                                                if !is_deleting() && !is_exporting() {
                                                    let release_id = release_id.clone();
                                                    let library_manager = library_manager.clone();
                                                    let cloud_storage = cloud_storage.clone();
                                                    let cache = cache.clone();
                                                    let encryption_service = encryption_service.clone();
                                                    spawn(async move {
                                                        is_exporting.set(true);
                                                        export_error.set(None);

                                                        if let Some(folder_handle) = AsyncFileDialog::new()
                                                            .set_title("Select Export Directory")
                                                            .pick_folder()
                                                            .await
                                                        {
                                                            let target_dir = folder_handle.path().to_path_buf();

                                                            match library_manager.get().export_release(
                                                                &release_id,
                                                                &target_dir,
                                                                &cloud_storage,
                                                                &cache,
                                                                &encryption_service,
                                                                chunk_size_bytes,
                                                            ).await {
                                                                Ok(_) => {
                                                                    is_exporting.set(false);
                                                                }
                                                                Err(e) => {
                                                                    error!("Failed to export release: {}", e);
                                                                    export_error.set(Some(format!("Export failed: {}", e)));
                                                                    is_exporting.set(false);
                                                                }
                                                            }
                                                        } else {
                                                            is_exporting.set(false);
                                                        }
                                                    });
                                                }
                                            }
                                        },
                                        if is_exporting() {
                                            "Exporting..."
                                        } else {
                                            "Export"
                                        }
                                    }
                                }
                            }
                            button {
                                class: "w-full px-4 py-3 text-left text-red-400 hover:bg-gray-600 transition-colors flex items-center gap-2",
                                disabled: is_deleting(),
                                onclick: {
                                    let album_id = album.id.clone();
                                    let album_title = album.title.clone();
                                    let dialog = dialog.clone();
                                    let library_manager = library_manager.clone();
                                    move |evt| {
                                        evt.stop_propagation();
                                        show_dropdown.set(false);
                                        if is_deleting() {
                                            return;
                                        }
                                        let album_id = album_id.clone();
                                        let library_manager = library_manager.clone();
                                        dialog.show_with_callback(
                                            "Delete Album?".to_string(),
                                            format!("Are you sure you want to delete \"{}\"? This will delete all releases, tracks, and associated data. This action cannot be undone.", album_title),
                                            "Delete".to_string(),
                                            "Cancel".to_string(),
                                            move || {
                                                let album_id = album_id.clone();
                                                let library_manager = library_manager.clone();
                                                spawn(async move {
                                                    is_deleting.set(true);
                                                    match library_manager.get().delete_album(&album_id).await {
                                                        Ok(_) => {
                                                            is_deleting.set(false);
                                                            on_album_deleted.call(());
                                                        }
                                                        Err(e) => {
                                                            error!("Failed to delete album: {}", e);
                                                            is_deleting.set(false);
                                                        }
                                                    }
                                                });
                                            },
                                        );
                                    }
                                },
                                "Delete Album"
                            }
                        }
                    }
                }
            }
        }

        // Click outside to close dropdown
        if show_dropdown() {
            div {
                class: "fixed inset-0 z-[5]",
                onclick: move |_| show_dropdown.set(false),
            }
        }

        // View Files Modal
        if let Some(release_id) = show_view_files_modal() {
            super::ViewFilesModal {
                release_id: release_id.clone(),
                on_close: move |_| show_view_files_modal.set(None),
            }
        }
    }
}
