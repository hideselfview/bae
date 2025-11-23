use crate::db::{DbAlbum, DbArtist};
use crate::library::use_library_manager;
use crate::ui::Route;
use dioxus::prelude::*;

use super::dropdown_menu::AlbumDropdownMenu;

/// Individual album card component
///
/// Note: Albums now represent logical albums that can have multiple releases.
/// For now, we show albums without import status (which moved to releases).
/// Future enhancement: Show all releases for an album in the detail view.
#[component]
pub fn AlbumCard(album: DbAlbum, artists: Vec<DbArtist>) -> Element {
    let library_manager = use_library_manager();
    let is_loading = use_signal(|| false);
    let mut hover_cover = use_signal(|| false);
    let mut show_dropdown = use_signal(|| false);
    let mut releases_signal = use_signal(Vec::new);

    // Extract album fields to avoid move issues
    let album_id = album.id.clone();
    let album_title = album.title.clone();
    let album_year = album.year;
    let cover_art_url = album.cover_art_url.clone();

    // Format artist names
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

    let card_class = "bg-gray-800 rounded-lg overflow-hidden shadow-lg hover:shadow-xl transition-shadow duration-300 cursor-pointer group";

    rsx! {
        div {
            class: "{card_class}",
            onclick: {
                let album_id_clone = album_id;
                let navigator = navigator();
                move |_| {
                    navigator.push(Route::AlbumDetail {
                        album_id: album_id_clone.clone(),
                        release_id: String::new(),
                    });
                }
            },

            // Album cover
            div {
                class: "aspect-square bg-gray-700 flex items-center justify-center relative",
                onmouseenter: move |_| hover_cover.set(true),
                onmouseleave: move |_| {
                    if !show_dropdown() {
                        hover_cover.set(false);
                    }
                },

                if let Some(cover_url) = &cover_art_url {
                    img {
                        src: "{cover_url}",
                        alt: "Album cover for {album_title}",
                        class: "w-full h-full object-cover",
                    }
                } else {
                    div { class: "text-gray-500 text-4xl", "🎵" }
                }

                // Three dot menu button - appears on hover or when dropdown is open
                if hover_cover() || show_dropdown() {
                    div { class: "absolute top-2 right-2 z-10",
                        button {
                            class: "w-8 h-8 bg-gray-800/40 hover:bg-gray-800/60 text-white rounded-lg flex items-center justify-center transition-colors",
                            onclick: {
                                let album_id_clone = album_id.clone();
                                move |evt| {
                                evt.stop_propagation();
                                let was_open = show_dropdown();
                                show_dropdown.set(!was_open);
                                if !was_open {
                                    let library_manager = library_manager.clone();
                                    let album_id_for_spawn = album_id_clone.clone();
                                    spawn(async move {
                                        if let Ok(releases) = library_manager
                                            .get()
                                            .get_releases_for_album(&album_id_for_spawn)
                                            .await
                                        {
                                            releases_signal.set(releases);
                                        }
                                    });
                                }
                            }},
                            div { class: "flex flex-col gap-1",
                                div { class: "w-1 h-1 bg-white rounded-full" }
                                div { class: "w-1 h-1 bg-white rounded-full" }
                                div { class: "w-1 h-1 bg-white rounded-full" }
                            }
                        }

                        // Dropdown menu
                        if show_dropdown() {
                            AlbumDropdownMenu {
                                album_id: album_id.clone(),
                                releases: ReadSignal::from(releases_signal),
                                library_manager: library_manager.clone(),
                                is_loading,
                                on_close: move |_| {
                                    show_dropdown.set(false);
                                    hover_cover.set(false);
                                }
                            }
                        }
                    }
                }
            }

            // Album info
            div { class: "p-4",
                h3 {
                    class: "font-bold text-white text-lg mb-1 truncate",
                    title: "{album_title}",
                    "{album_title}"
                }
                p {
                    class: "text-gray-400 text-sm truncate",
                    title: "{artist_name}",
                    "{artist_name}"
                }
                YearDisplay {
                    album_id: album_id.clone(),
                    album_year: album_year,
                }
            }

            // Click outside to close dropdown
            if show_dropdown() {
                div {
                    class: "fixed inset-0 z-[5]",
                    onclick: move |_| {
                        show_dropdown.set(false);
                        hover_cover.set(false);
                    }
                }
            }
        }
    }
}

/// Display year information for an album, showing both original and release year if different
#[component]
fn YearDisplay(album_id: String, album_year: Option<i32>) -> Element {
    let library_manager = use_library_manager();
    let mut release_year = use_signal(|| None::<i32>);

    // Fetch the first release's year to show release-specific year
    use_effect({
        let library_manager = library_manager.clone();
        let album_id = album_id.clone();
        move || {
            let library_manager = library_manager.clone();
            let album_id = album_id.clone();
            spawn(async move {
                if let Ok(releases) = library_manager
                    .get()
                    .get_releases_for_album(&album_id)
                    .await
                {
                    if let Some(first_release) = releases.first() {
                        release_year.set(first_release.year);
                    }
                }
            });
        }
    });

    let release_yr = *release_year.read();

    rsx! {
        div { class: "text-gray-500 text-xs mt-1",
            // Show both years if they differ, otherwise just show one
            if let Some(album_yr) = album_year {
                if let Some(rel_yr) = release_yr {
                    if album_yr != rel_yr {
                        // Different years - show both
                        span { "Original: {album_yr} • This Release: {rel_yr}" }
                    } else {
                        // Same year - show once
                        span { "{album_yr}" }
                    }
                } else {
                    // Only album year available
                    span { "{album_yr}" }
                }
            } else if let Some(rel_yr) = release_yr {
                // Only release year available
                span { "{rel_yr}" }
            }
        }
    }
}
