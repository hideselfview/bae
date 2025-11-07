use crate::db::{DbAlbum, DbArtist};
use crate::library::use_library_manager;
use crate::ui::components::album_card::AlbumCard;
use crate::ui::Route;
use dioxus::prelude::*;
use std::collections::HashMap;
use tracing::debug;

/// Library browser page
#[component]
pub fn Library() -> Element {
    debug!("Component rendering");
    let library_manager = use_library_manager();
    let mut albums = use_signal(Vec::<DbAlbum>::new);
    let mut album_artists = use_signal(HashMap::<String, Vec<DbArtist>>::new);
    let mut loading = use_signal(|| true);
    let mut error = use_signal(|| None::<String>);

    // Load albums and their artists on component mount
    use_effect(move || {
        debug!("Starting load_albums effect");
        let library_manager = library_manager.clone();
        spawn(async move {
            debug!("Inside async spawn, fetching albums");
            loading.set(true);
            error.set(None);

            match library_manager.get().get_albums().await {
                Ok(album_list) => {
                    // Load artists for each album
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
                    loading.set(false);
                }
                Err(e) => {
                    error.set(Some(format!("Failed to load library: {}", e)));
                    loading.set(false);
                }
            }
        });
    });

    rsx! {
        div { class: "container mx-auto p-6",
            h1 { class: "text-3xl font-bold text-white mb-6", "Music Library" }

            if loading() {
                div { class: "flex justify-center items-center py-12",
                    div { class: "animate-spin rounded-full h-12 w-12 border-b-2 border-blue-500" }
                    p { class: "ml-4 text-gray-300", "Loading your music library..." }
                }
            } else if let Some(err) = error() {
                div { class: "bg-red-900 border border-red-700 text-red-100 px-4 py-3 rounded mb-4",
                    p { "{err}" }
                    p { class: "text-sm mt-2", "Make sure you've imported some albums first!" }
                }
            } else if albums().is_empty() {
                div { class: "text-center py-12",
                    div { class: "text-gray-400 text-6xl mb-4", "🎵" }
                    h2 { class: "text-2xl font-bold text-gray-300 mb-2",
                        "No albums in your library yet"
                    }
                    p { class: "text-gray-500 mb-4", "Import your first album to get started!" }
                    Link {
                        to: Route::ImportWorkflowManager {},
                        class: "inline-block bg-blue-600 hover:bg-blue-700 text-white font-bold py-2 px-4 rounded",
                        "Import Album"
                    }
                }
            } else {
                AlbumGrid {
                    albums: albums(),
                    album_artists: album_artists(),
                }
            }
        }
    }
}

/// Grid component to display albums
#[component]
fn AlbumGrid(albums: Vec<DbAlbum>, album_artists: HashMap<String, Vec<DbArtist>>) -> Element {
    rsx! {
        div { class: "grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5 gap-6",
            for album in albums {
                AlbumCard {
                    album: album.clone(),
                    artists: album_artists.get(&album.id).cloned().unwrap_or_default(),
                }
            }
        }
    }
}
