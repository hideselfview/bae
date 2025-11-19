use crate::db::{DbAlbum, DbArtist};
use dioxus::prelude::*;

#[component]
pub fn AlbumMetadata(album: DbAlbum, artists: Vec<DbArtist>, track_count: usize) -> Element {
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

    rsx! {
        div {
            h1 { class: "text-2xl font-bold text-white mb-2", "{album.title}" }
            p { class: "text-lg text-gray-300 mb-4", "{artist_name}" }

            div { class: "space-y-2 text-sm text-gray-400",
                if let Some(year) = album.year {
                    div {
                        span { class: "font-medium", "Year: " }
                        span { "{year}" }
                    }
                }
                if let Some(discogs_release) = &album.discogs_release {
                    div {
                        span { class: "font-medium", "Discogs Master ID: " }
                        span { "{discogs_release.master_id}" }
                    }
                }
                div {
                    span { class: "font-medium", "Tracks: " }
                    span { "{track_count}" }
                }
            }
        }
    }
}
