use dioxus::prelude::*;
use crate::library::{LibraryManager, LibraryError};
use crate::database::DbAlbum;
use crate::Route;
use std::path::PathBuf;

/// Get the library path (same as import workflow)
fn get_library_path() -> PathBuf {
    let home_dir = dirs::home_dir().expect("Failed to get home directory");
    home_dir.join("Music").join("bae")
}

/// Library browser page
#[component]
pub fn Library() -> Element {
    let mut albums = use_signal(|| Vec::<DbAlbum>::new());
    let mut loading = use_signal(|| true);
    let mut error = use_signal(|| None::<String>);

    // Load albums on component mount
    use_effect(move || {
        spawn(async move {
            loading.set(true);
            error.set(None);
            
            match load_albums().await {
                Ok(album_list) => {
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
        div {
            class: "container mx-auto p-6",
            h1 { 
                class: "text-3xl font-bold mb-6 text-white",
                "Music Library" 
            }
            
            if loading() {
                div {
                    class: "flex justify-center items-center py-12",
                    div {
                        class: "animate-spin rounded-full h-12 w-12 border-b-2 border-blue-500"
                    }
                    p {
                        class: "ml-4 text-gray-300",
                        "Loading your music library..."
                    }
                }
            } else if let Some(err) = error() {
                div {
                    class: "bg-red-900 border border-red-700 text-red-100 px-4 py-3 rounded mb-4",
                    p { "{err}" }
                    p {
                        class: "text-sm mt-2",
                        "Make sure you've imported some albums first!"
                    }
                }
            } else if albums().is_empty() {
                div {
                    class: "text-center py-12",
                    div {
                        class: "text-gray-400 text-6xl mb-4",
                        "🎵"
                    }
                    h2 {
                        class: "text-2xl font-bold text-gray-300 mb-2",
                        "No albums in your library yet"
                    }
                    p {
                        class: "text-gray-500 mb-4",
                        "Import your first album to get started!"
                    }
                    Link {
                        to: Route::ImportWorkflowManager {},
                        class: "inline-block bg-blue-600 hover:bg-blue-700 text-white font-bold py-2 px-4 rounded",
                        "Import Album"
                    }
                }
            } else {
                AlbumGrid { albums: albums() }
            }
        }
    }
}

/// Grid component to display albums
#[component]
fn AlbumGrid(albums: Vec<DbAlbum>) -> Element {
    rsx! {
        div {
            class: "grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5 gap-6",
            for album in albums {
                AlbumCard { album }
            }
        }
    }
}

/// Individual album card component
#[component]
fn AlbumCard(album: DbAlbum) -> Element {
    rsx! {
        div {
            class: "bg-gray-800 rounded-lg overflow-hidden shadow-lg hover:shadow-xl transition-shadow duration-300 cursor-pointer",
            onclick: {
                let album_title = album.title.clone();
                move |_| {
                    println!("Clicked album: {}", album_title);
                    // TODO: Navigate to album detail view
                }
            },
            
            // Album cover
            div {
                class: "aspect-square bg-gray-700 flex items-center justify-center",
                if let Some(cover_url) = &album.cover_art_url {
                    img {
                        src: "{cover_url}",
                        alt: "Album cover for {album.title}",
                        class: "w-full h-full object-cover"
                    }
                } else {
                    div {
                        class: "text-gray-500 text-4xl",
                        "🎵"
                    }
                }
            }
            
            // Album info
            div {
                class: "p-4",
                h3 {
                    class: "font-bold text-white text-lg mb-1 truncate",
                    title: "{album.title}",
                    "{album.title}"
                }
                p {
                    class: "text-gray-400 text-sm truncate",
                    title: "{album.artist_name}",
                    "{album.artist_name}"
                }
                if let Some(year) = album.year {
                    p {
                        class: "text-gray-500 text-xs mt-1",
                        "{year}"
                    }
                }
            }
        }
    }
}

/// Load albums from the library database
async fn load_albums() -> Result<Vec<DbAlbum>, LibraryError> {
    let library_path = get_library_path();
    let library_manager = LibraryManager::new(library_path).await?;
    library_manager.get_albums().await
}
