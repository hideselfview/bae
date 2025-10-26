use crate::db::{DbAlbum, DbArtist};
use crate::ui::Route;
use dioxus::prelude::*;

/// Individual album card component
///
/// Note: Albums now represent logical albums that can have multiple releases.
/// For now, we show albums without import status (which moved to releases).
/// Future enhancement: Show all releases for an album in the detail view.
#[component]
pub fn AlbumCard(album: DbAlbum, artists: Vec<DbArtist>) -> Element {
    // Format artist names
    let artist_name = if artists.is_empty() {
        "Unknown Artist".to_string()
    } else if artists.len() == 1 {
        artists[0].name.clone()
    } else {
        // Multiple artists: join with commas
        artists
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };

    let card_class = "bg-gray-800 rounded-lg overflow-hidden shadow-lg hover:shadow-xl transition-shadow duration-300 cursor-pointer";

    rsx! {
        div {
            class: "{card_class}",
            onclick: {
                let album_id = album.id.clone();
                let navigator = navigator();
                move |_| {
                    navigator.push(Route::AlbumDetail {
                    album_id: album_id.clone(),
                    release_id: String::new(),
                });
                }
            },

            // Album cover
            div {
                class: "aspect-square bg-gray-700 flex items-center justify-center relative",
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
                    title: "{artist_name}",
                    "{artist_name}"
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
