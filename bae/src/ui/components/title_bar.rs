use crate::db::{DbAlbum, DbArtist};
use crate::library::use_library_manager;
use crate::ui::components::use_library_search;
use crate::ui::Route;
use dioxus::desktop::use_window;
use dioxus::prelude::*;
use std::collections::HashMap;
use tracing::info;

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
        // Click outside to close - render BEFORE title-bar
        if show_results() {
            div {
                class: "fixed inset-0 z-[1500]",
                onclick: move |evt| {
                    info!("Click-outside handler fired");
                    show_results.set(false);
                }
            }
        }

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
                div {
                    class: "relative w-64",
                    id: "search-container",
                    input {
                        r#type: "text",
                        placeholder: "Search...",
                        autocomplete: "off",
                        class: "w-full h-7 px-3 bg-[#2d3138] border border-[#3d4148] rounded text-white text-xs placeholder-gray-500 focus:outline-none focus:border-blue-500",
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

                    // Results popover - z-2000 to be above overlay (z-1500) and title-bar (z-1000)
                    if show_results() && !filtered_albums().is_empty() {
                        div {
                            class: "absolute top-full mt-2 left-0 right-0 bg-[#2d3138] border border-[#3d4148] rounded-lg shadow-lg max-h-96 overflow-y-auto",
                            style: "z-index: 2000;",
                            id: "search-popover",
                            onclick: move |evt| {
                                info!("Popover container clicked - stopping propagation");
                                evt.stop_propagation();
                            },
                            for album in filtered_albums() {
                                {
                                    let album_id = album.id.clone();
                                    let album_title = album.title.clone();
                                    let album_year = album.year;
                                    let cover_art = album.cover_art_url.clone();
                                    let artists = album_artists().get(&album.id).cloned().unwrap_or_default();
                                    let artist_name = if artists.is_empty() {
                                        "Unknown Artist".to_string()
                                    } else {
                                        artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")
                                    };
                                    rsx! {
                                        div {
                                            key: "{album_id}",
                                            class: "flex items-center gap-3 px-3 py-2 hover:bg-[#3d4148] border-b border-[#3d4148] last:border-b-0 cursor-pointer",
                                            onclick: {
                                                let album_id = album_id.clone();
                                                let navigator = navigator();
                                                move |evt| {
                                                    info!("Search result onclick fired for album_id: {}", album_id);
                                                    evt.stop_propagation();
                                                    info!("Stopped propagation, closing popover and clearing search");
                                                    show_results.set(false);
                                                    search_query.set(String::new());
                                                    let route = Route::AlbumDetail {
                                                        album_id: album_id.clone(),
                                                        release_id: String::new(),
                                                    };
                                                    info!("Navigating to route: {:?}", route);
                                                    navigator.push(route);
                                                    info!("Navigator.push called for album_id: {}", album_id);
                                                }
                                            },
                                            if let Some(cover_url) = cover_art {
                                                img {
                                                    src: "{cover_url}",
                                                    class: "w-10 h-10 rounded object-cover flex-shrink-0",
                                                    alt: "{album_title}",
                                                }
                                            } else {
                                                div {
                                                    class: "w-10 h-10 bg-gray-700 rounded flex items-center justify-center flex-shrink-0",
                                                    div { class: "text-gray-500 text-xs", "🎵" }
                                                }
                                            }
                                            div { class: "flex-1 min-w-0",
                                                div { class: "text-white text-xs font-medium truncate", "{album_title}" }
                                                div { class: "text-gray-400 text-xs truncate",
                                                    "{artist_name}"
                                                    if let Some(year) = album_year {
                                                        span { class: "text-gray-500", " • {year}" }
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
