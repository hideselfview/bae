use dioxus::prelude::*;
use crate::models;

#[component]
pub fn ReleaseItem(result: models::DiscogsRelease) -> Element {
    rsx! {
        tr {
            class: "hover:bg-gray-50",
            td {
                class: "px-4 py-3",
                if let Some(thumb) = &result.thumb {
                    img {
                        class: "w-10 h-10 object-cover rounded",
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
                    "-"
                }
            }
            td {
                class: "px-4 py-3 text-sm text-gray-500",
                if let Some(first_label) = result.label.first() {
                    "{first_label}"
                } else {
                    "-"
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
                class: "px-4 py-3 text-sm text-gray-500",
                if !result.format.is_empty() {
                    "{result.format.join(\", \")}"
                } else {
                    "-"
                }
            }
            td {
                class: "px-4 py-3 text-sm",
                button {
                    class: "text-green-600 hover:text-green-800 underline",
                    onclick: {
                        let release_title = result.title.clone();
                        let release_id = result.id.clone();
                        let release_format = result.format.clone();
                        let release_year = result.year;
                        move |_| {
                            // TODO: Implement actual library storage
                            let formats = if !release_format.is_empty() {
                                release_format.join(", ")
                            } else {
                                "Unknown".to_string()
                            };
                            println!("Adding release to library: {} (ID: {}, Format: {}, Year: {:?})", 
                                     release_title, release_id, formats, release_year);
                        }
                    },
                    "Add to Library"
                }
            }
        }
    }
}
