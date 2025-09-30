use dioxus::prelude::*;
use crate::library_context::use_library_manager;
use crate::database::DbAlbum;
use crate::Route;

/// Library browser page
#[component]
pub fn Library() -> Element {
    println!("Library: Component rendering");
    let library_manager = use_library_manager();
    let mut albums = use_signal(|| Vec::<DbAlbum>::new());
    let mut filtered_albums = use_signal(|| Vec::<DbAlbum>::new());
    let mut loading = use_signal(|| true);
    let mut error = use_signal(|| None::<String>);
    let mut search_query = use_signal(|| String::new());

    // Load albums on component mount
    use_effect(move || {
        println!("Library: Starting load_albums effect");
        let library_manager = library_manager.clone();
        spawn(async move {
            println!("Library: Inside async spawn, fetching albums");
            loading.set(true);
            error.set(None);
            
            match library_manager.read().await.get_albums().await {
                Ok(album_list) => {
                    albums.set(album_list.clone());
                    filtered_albums.set(album_list);
                    loading.set(false);
                }
                Err(e) => {
                    error.set(Some(format!("Failed to load library: {}", e)));
                    loading.set(false);
                }
            }
        });
    });

    // Filter albums when search query changes
    use_effect({
        let albums = albums.clone();
        let search_query = search_query.clone();
        move || {
            let query = search_query().to_lowercase();
            if query.is_empty() {
                filtered_albums.set(albums());
            } else {
                let filtered = albums().into_iter()
                    .filter(|album| {
                        album.title.to_lowercase().contains(&query) ||
                        album.artist_name.to_lowercase().contains(&query)
                    })
                    .collect();
                filtered_albums.set(filtered);
            }
        }
    });

    rsx! {
        div {
            class: "container mx-auto p-6",
            div {
                class: "flex flex-col sm:flex-row sm:items-center sm:justify-between mb-6",
                h1 { 
                    class: "text-3xl font-bold text-white mb-4 sm:mb-0",
                    "Music Library" 
                }
                
                // Search bar
                div {
                    class: "relative",
                    input {
                        r#type: "text",
                        placeholder: "Search albums or artists...",
                        class: "w-full sm:w-80 px-4 py-2 bg-gray-800 border border-gray-600 rounded-lg text-white placeholder-gray-400 focus:outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500",
                        value: "{search_query()}",
                        oninput: move |evt| search_query.set(evt.value()),
                    }
                    div {
                        class: "absolute right-3 top-2.5 text-gray-400",
                        "🔍"
                    }
                }
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
            } else if filtered_albums().is_empty() {
                div {
                    class: "text-center py-12",
                    div {
                        class: "text-gray-400 text-6xl mb-4",
                        "🔍"
                    }
                    h2 {
                        class: "text-2xl font-bold text-gray-300 mb-2",
                        "No albums found"
                    }
                    p {
                        class: "text-gray-500 mb-4",
                        "Try a different search term or browse all albums"
                    }
                    button {
                        class: "bg-blue-600 hover:bg-blue-700 text-white font-bold py-2 px-4 rounded",
                        onclick: move |_| search_query.set(String::new()),
                        "Clear Search"
                    }
                }
            } else {
                div {
                    // Results counter
                    if !search_query().is_empty() {
                        div {
                            class: "mb-4 text-gray-400 text-sm",
                            {format!("Found {} album{} matching \"{}\"", 
                                filtered_albums().len(),
                                if filtered_albums().len() == 1 { "" } else { "s" },
                                search_query()
                            )}
                        }
                    }
                    AlbumGrid { albums: filtered_albums() }
                }
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
                let album_id = album.id.clone();
                let navigator = navigator();
                move |_| {
                    navigator.push(Route::AlbumDetail { album_id: album_id.clone() });
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

