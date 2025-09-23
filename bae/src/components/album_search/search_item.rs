use dioxus::prelude::*;
use crate::{models, search_context::SearchContext};

#[component]
pub fn SearchItem(result: models::DiscogsRelease) -> Element {
    let search_ctx = use_context::<SearchContext>();

    rsx! {
        tr {
            class: "hover:bg-gray-50",
            td {
                class: "px-4 py-3",
                if let Some(thumb) = &result.thumb {
                    img {
                        class: "w-12 h-12 object-cover rounded",
                        src: "{thumb}",
                        alt: "Album cover"
                    }
                } else {
                    div {
                        class: "w-12 h-12 bg-gray-200 rounded flex items-center justify-center",
                        "No Image"
                    }
                }
            }
            td {
                class: "px-4 py-3 text-sm font-medium text-gray-900",
                "{result.title}"
            }
            td {
                class: "px-4 py-3 text-sm text-gray-500",
                if let Some(year) = result.year {
                    "{year}"
                } else {
                    "Unknown"
                }
            }
            td {
                class: "px-4 py-3 text-sm text-gray-500",
                if let Some(first_label) = result.label.first() {
                    "{first_label}"
                } else {
                    "Unknown"
                }
            }
            td {
                class: "px-4 py-3 text-sm text-gray-500",
                if let Some(country) = &result.country {
                    "{country}"
                } else {
                    "-"
                }
            }
            td {
                class: "px-4 py-3 text-sm space-x-2",
                button {
                    class: "text-blue-600 hover:text-blue-800 underline",
                    onclick: {
                        let master_id = result.id.clone();
                        let master_title = result.title.clone();
                        let mut search_ctx = search_ctx.clone();
                        move |_| {
                            search_ctx.navigate_to_releases(master_id.clone(), master_title.clone());
                        }
                    },
                    "View Releases"
                }
                button {
                    class: "text-green-600 hover:text-green-800 underline",
                    onclick: {
                        let album_title = result.title.clone();
                        let album_id = result.id.clone();
                        move |_| {
                            // TODO: Implement actual library storage
                            println!("Adding master album to library: {} (ID: {})", album_title, album_id);
                        }
                    },
                    "Add to Library"
                }
            }
        }
    }
}
