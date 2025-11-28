use crate::db::DbTorrent;
use crate::library::use_library_manager;
use crate::AppContext;
use dioxus::prelude::*;
use rfd::AsyncFileDialog;
use tracing::error;

use super::super::use_torrent_manager;

#[component]
pub fn ReleaseActionMenu(
    release_id: String,
    has_torrent: bool,
    is_seeding: bool,
    is_deleting: ReadSignal<bool>,
    is_exporting: Signal<bool>,
    export_error: Signal<Option<String>>,
    on_view_files: EventHandler<String>,
    on_delete: EventHandler<String>,
    torrents_resource: Resource<
        Result<std::collections::HashMap<String, DbTorrent>, crate::library::LibraryError>,
    >,
) -> Element {
    let library_manager = use_library_manager();
    let app_context = use_context::<AppContext>();
    let torrent_manager = use_torrent_manager();

    rsx! {
        div {
            class: "absolute right-0 top-full mt-1 bg-gray-700 rounded-lg shadow-lg overflow-hidden z-10 border border-gray-600 min-w-[160px]",
            button {
                class: "w-full px-4 py-2 text-left text-white hover:bg-gray-600 transition-colors flex items-center gap-2 text-sm",
                disabled: is_deleting() || is_exporting(),
                onclick: {
                    let release_id = release_id.clone();
                    move |evt| {
                        evt.stop_propagation();
                        if !is_deleting() && !is_exporting() {
                            on_view_files.call(release_id.clone());
                        }
                    }
                },
                "Release Info"
            }
            if has_torrent {
                if is_seeding {
                    button {
                        class: "w-full px-4 py-2 text-left text-white hover:bg-gray-600 transition-colors flex items-center gap-2 text-sm",
                        disabled: is_deleting() || is_exporting(),
                        onclick: {
                            let release_id = release_id.clone();
                            let manager = torrent_manager.clone();
                            let mut torrents_resource = torrents_resource;
                            move |evt| {
                                evt.stop_propagation();
                                if !is_deleting() && !is_exporting() {
                                    let release_id = release_id.clone();
                                    let manager = manager.clone();
                                    spawn(async move {
                                        let _ = manager.stop_seeding(release_id).await;
                                        torrents_resource.restart();
                                    });
                                }
                            }
                        },
                        "Stop Seeding"
                    }
                } else {
                    button {
                        class: "w-full px-4 py-2 text-left text-white hover:bg-gray-600 transition-colors flex items-center gap-2 text-sm",
                        disabled: is_deleting() || is_exporting(),
                        onclick: {
                            let release_id = release_id.clone();
                            let manager = torrent_manager.clone();
                            let mut torrents_resource = torrents_resource;
                            move |evt| {
                                evt.stop_propagation();
                                if !is_deleting() && !is_exporting() {
                                    let release_id = release_id.clone();
                                    let manager = manager.clone();
                                    spawn(async move {
                                        let _ = manager.start_seeding(release_id).await;
                                        torrents_resource.restart();
                                    });
                                }
                            }
                        },
                        "Start Seeding"
                    }
                }
            }
            button {
                class: "w-full px-4 py-2 text-left text-white hover:bg-gray-600 transition-colors flex items-center gap-2 text-sm",
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
            button {
                class: "w-full px-4 py-2 text-left text-red-400 hover:bg-gray-600 transition-colors flex items-center gap-2 text-sm",
                disabled: is_deleting() || is_exporting(),
                onclick: {
                    let release_id = release_id.clone();
                    move |evt| {
                        evt.stop_propagation();
                        if !is_deleting() && !is_exporting() {
                            on_delete.call(release_id.clone());
                        }
                    }
                },
                "Delete Release"
            }
        }
    }
}
