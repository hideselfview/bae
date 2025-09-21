use dioxus::prelude::*;
use crate::{models, Route};

/// Individual album search result component
#[component]
pub fn AlbumSearchResult(release: models::DiscogsRelease) -> Element {
    rsx! {
        div {
            class: "bg-white rounded-lg shadow-md p-4 hover:shadow-lg transition-shadow",
            
            if let Some(thumb) = &release.thumb {
                img {
                    src: "{thumb}",
                    alt: "Album cover",
                    class: "w-full h-48 object-cover rounded mb-3"
                }
            } else {
                div {
                    class: "w-full h-48 bg-gray-200 rounded mb-3 flex items-center justify-center",
                    span {
                        class: "text-gray-500",
                        "No Image"
                    }
                }
            }

            h3 {
                class: "font-bold text-lg mb-2",
                "{release.title}"
            }

            if let Some(year) = release.year {
                p {
                    class: "text-gray-600 mb-2",
                    "Year: {year}"
                }
            }

            if !release.genre.is_empty() {
                p {
                    class: "text-gray-600 mb-2",
                    "Genre: {release.genre.join(\", \")}"
                }
            }

            if let Some(country) = &release.country {
                p {
                    class: "text-gray-600 mb-2",
                    "Country: {country}"
                }
            }

            div {
                class: "mt-4",
                Link {
                    to: Route::AlbumImport {},
                    class: "bg-blue-500 text-white px-4 py-2 rounded hover:bg-blue-600 transition-colors",
                    "Add to Library"
                }
            }
        }
    }
}
