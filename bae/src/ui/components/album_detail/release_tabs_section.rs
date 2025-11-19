use crate::db::{DbRelease, DbTorrent};
use dioxus::prelude::*;

use super::release_action_menu::ReleaseActionMenu;

#[component]
pub fn ReleaseTabsSection(
    releases: Vec<DbRelease>,
    selected_release_id: Option<String>,
    on_release_select: EventHandler<String>,
    is_deleting: ReadSignal<bool>,
    is_exporting: Signal<bool>,
    export_error: Signal<Option<String>>,
    on_view_files: EventHandler<String>,
    on_delete_release: EventHandler<String>,
    torrents_resource: Resource<
        Result<std::collections::HashMap<String, DbTorrent>, crate::library::LibraryError>,
    >,
) -> Element {
    let mut show_release_dropdown = use_signal(|| None::<String>);

    rsx! {
        div { class: "mb-6 border-b border-gray-700",
            div { class: "flex gap-2 overflow-x-auto",
                for release in releases.iter() {
                    {
                        let is_selected = selected_release_id.as_ref() == Some(&release.id);
                        let release_id = release.id.clone();
                        let release_id_for_menu = release.id.clone();
                        rsx! {
                            div {
                                key: "{release.id}",
                                class: "flex items-center gap-2 relative",
                                button {
                                    class: if is_selected { "px-4 py-2 text-sm font-medium text-blue-400 border-b-2 border-blue-400 whitespace-nowrap" } else { "px-4 py-2 text-sm font-medium text-gray-400 hover:text-gray-300 border-b-2 border-transparent whitespace-nowrap" },
                                    onclick: {
                                        let release_id = release_id.clone();
                                        move |_| on_release_select.call(release_id.clone())
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
                                    button {
                                        class: "px-2 py-1 text-sm text-gray-400 hover:text-gray-300 hover:bg-gray-700 rounded",
                                        disabled: is_deleting(),
                                        onclick: {
                                            let release_id = release_id_for_menu.clone();
                                            move |evt| {
                                                evt.stop_propagation();
                                                if !is_deleting() {
                                                    let current = show_release_dropdown();
                                                    if current.as_ref() == Some(&release_id) {
                                                        show_release_dropdown.set(None);
                                                    } else {
                                                        show_release_dropdown.set(Some(release_id.clone()));
                                                    }
                                                }
                                            }
                                        },
                                        "⋮"
                                    }

                                    // Release dropdown menu
                                    if show_release_dropdown().as_ref() == Some(&release_id_for_menu) {
                                        {
                                            let torrents = torrents_resource
                                                .value()
                                                .read()
                                                .as_ref()
                                                .and_then(|r| r.as_ref().ok())
                                                .cloned()
                                                .unwrap_or_default();
                                            let torrent = torrents.get(&release_id_for_menu);
                                            let has_torrent = torrent.is_some();
                                            let is_seeding = torrent.map(|t| t.is_seeding).unwrap_or(false);
                                            rsx! {
                                                ReleaseActionMenu {
                                                    release_id: release_id_for_menu.clone(),
                                                    has_torrent,
                                                    is_seeding,
                                                    is_deleting,
                                                    is_exporting,
                                                    export_error,
                                                    on_view_files: move |id| {
                                                        show_release_dropdown.set(None);
                                                        on_view_files.call(id);
                                                    },
                                                    on_delete: move |id| {
                                                        show_release_dropdown.set(None);
                                                        on_delete_release.call(id);
                                                    },
                                                    torrents_resource,
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

        // Click outside to close dropdown
        if show_release_dropdown().is_some() {
            div {
                class: "fixed inset-0 z-[5]",
                onclick: move |_| show_release_dropdown.set(None),
            }
        }
    }
}
