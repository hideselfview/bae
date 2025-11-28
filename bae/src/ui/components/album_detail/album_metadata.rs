use crate::db::{DbAlbum, DbArtist, DbRelease};
use dioxus::prelude::*;

#[component]
pub fn AlbumMetadata(
    album: DbAlbum,
    artists: Vec<DbArtist>,
    track_count: usize,
    selected_release: Option<DbRelease>,
) -> Element {
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
            // Simple top section
            h1 { class: "text-2xl font-bold text-white mb-2", "{album.title}" }
            p { class: "text-lg text-gray-300 mb-2", "{artist_name}" }
            if let Some(year) = album.year {
                p { class: "text-gray-400 text-sm", "{year}" }
            }
        }
    }
}
