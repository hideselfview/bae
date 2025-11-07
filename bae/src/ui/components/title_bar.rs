use crate::db::{DbAlbum, DbArtist};
use crate::library::use_library_manager;
use crate::ui::components::use_library_search;
use crate::ui::Route;
use dioxus::desktop::use_window;
use dioxus::prelude::*;
use std::collections::HashMap;

#[cfg(target_os = "macos")]
use cocoa::appkit::NSApplication;
#[cfg(target_os = "macos")]
use cocoa::base::{id, nil};
#[cfg(target_os = "macos")]
use objc::{msg_send, sel, sel_impl};

/// Custom title bar component with navigation (macOS: native traffic lights + nav)
#[component]
pub fn TitleBar() -> Element {
    let window = use_window();
    let current_route = use_route::<Route>();
    let library_manager = use_library_manager();
    let mut search_query = use_library_search();
    let mut show_results = use_signal(|| false);
    let mut albums = use_signal(Vec::<DbAlbum>::new);
    let mut album_artists = use_signal(HashMap::<String, Vec<DbArtist>>::new);
    let mut filtered_albums = use_signal(Vec::<DbAlbum>::new);

    // Load albums on mount
    use_effect(move || {
        let library_manager = library_manager.clone();
        spawn(async move {
            if let Ok(album_list) = library_manager.get().get_albums().await {
                let mut artists_map = HashMap::new();
                for album in &album_list {
                    if let Ok(artists) =
                        library_manager.get().get_artists_for_album(&album.id).await
                    {
                        artists_map.insert(album.id.clone(), artists);
                    }
                }
                album_artists.set(artists_map);
                albums.set(album_list);
            }
        });
    });

    // Filter albums when search query changes
    use_effect({
        move || {
            let query = search_query().to_lowercase();
            if query.is_empty() {
                filtered_albums.set(Vec::new());
                show_results.set(false);
            } else {
                let artists_map = album_artists();
                let filtered = albums()
                    .into_iter()
                    .filter(|album| {
                        if album.title.to_lowercase().contains(&query) {
                            return true;
                        }
                        if let Some(artists) = artists_map.get(&album.id) {
                            return artists
                                .iter()
                                .any(|artist| artist.name.to_lowercase().contains(&query));
                        }
                        false
                    })
                    .take(10)
                    .collect();
                filtered_albums.set(filtered);
                show_results.set(true);
            }
        }
    });

    rsx! {
        div {
            id: "title-bar",
            class: "fixed top-0 left-0 right-0 h-10 bg-[#1e222d] flex items-center pl-20 pr-2 cursor-move z-[1000] border-b border-[#2d3138]",
            onmousedown: move |_| {
                let _ = window.drag_window();
            },
            ondoubleclick: move |_| {
                perform_zoom();
            },
            div {
                class: "flex gap-2 flex-none items-center",
                style: "-webkit-app-region: no-drag;",
                NavButton {
                    route: Route::Library {},
                    label: "Library",
                    is_active: matches!(current_route, Route::Library {} | Route::AlbumDetail { .. })
                }
                NavButton {
                    route: Route::ImportWorkflowManager {},
                    label: "Import",
                    is_active: matches!(current_route, Route::ImportWorkflowManager {})
                }
                NavButton {
                    route: Route::Settings {},
                    label: "Settings",
                    is_active: matches!(current_route, Route::Settings {})
                }
            }

            // Search input on the right side
            div {
                class: "flex-1 flex justify-end items-center relative",
                style: "-webkit-app-region: no-drag;",
                div { class: "relative w-64",
                    input {
                        r#type: "text",
                        placeholder: "Search...",
                        class: "w-full h-7 px-3 pr-8 bg-[#2d3138] border border-[#3d4148] rounded text-white text-xs placeholder-gray-500 focus:outline-none focus:border-blue-500",
                        value: "{search_query()}",
                        oninput: move |evt| search_query.set(evt.value()),
                        onfocus: move |_| {
                            if !search_query().is_empty() {
                                show_results.set(true);
                            }
                        },
                        onkeydown: move |evt| {
                            if evt.key() == Key::Escape {
                                show_results.set(false);
                            }
                        },
                    }
                    div { class: "absolute right-2 top-1.5 text-gray-500 text-xs pointer-events-none",
                        "🔍"
                    }

                    // Results popover
                    if show_results() && !filtered_albums().is_empty() {
                        div {
                            class: "absolute top-full mt-2 left-0 right-0 bg-[#2d3138] border border-[#3d4148] rounded-lg shadow-lg max-h-96 overflow-y-auto z-[2000]",
                            for album in filtered_albums() {
                                {
                                    let album_id = album.id.clone();
                                    let artists = album_artists().get(&album.id).cloned().unwrap_or_default();
                                    let artist_name = if artists.is_empty() {
                                        "Unknown Artist".to_string()
                                    } else {
                                        artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")
                                    };
                                    rsx! {
                                        Link {
                                            key: "{album_id}",
                                            to: Route::AlbumDetail {
                                                album_id,
                                                release_id: String::new(),
                                            },
                                            class: "block px-3 py-2 hover:bg-[#3d4148] border-b border-[#3d4148] last:border-b-0",
                                            onclick: move |_| {
                                                show_results.set(false);
                                            },
                                            div { class: "text-white text-xs font-medium", "{album.title}" }
                                            div { class: "text-gray-400 text-xs", "{artist_name}" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Click outside to close
        if show_results() {
            div {
                class: "fixed inset-0 z-[1500]",
                onclick: move |_| {
                    show_results.set(false);
                }
            }
        }
    }
}

/// Navigation button component for titlebar
#[component]
fn NavButton(route: Route, label: &'static str, is_active: bool) -> Element {
    rsx! {
        span {
            class: "inline-block",
            onmousedown: move |evt| {
                evt.stop_propagation();
            },
            Link {
                to: route,
                class: if is_active {
                    "text-white no-underline text-[12px] cursor-pointer px-2 py-1 rounded bg-gray-700"
                } else {
                    "text-gray-400 no-underline text-[12px] cursor-pointer px-2 py-1 rounded hover:bg-gray-800 hover:text-white transition-colors"
                },
                "{label}"
            }
        }
    }
}

/// Perform window zoom (maximize/restore) using native macOS API
#[cfg(target_os = "macos")]
fn perform_zoom() {
    unsafe {
        let app = NSApplication::sharedApplication(nil);
        let window: id = msg_send![app, keyWindow];

        if window != nil {
            let _: () = msg_send![window, performSelector: sel!(performZoom:) withObject: nil];
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn perform_zoom() {
    // No-op on non-macOS platforms
}
