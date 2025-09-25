use dioxus::prelude::*;
use crate::{models, search_context::SearchContext};

#[derive(Props, PartialEq, Clone)]
pub struct SearchItemProps {
    pub result: models::DiscogsRelease,
    pub on_import: EventHandler<models::ImportItem>,
}

#[component]
pub fn SearchItem(props: SearchItemProps) -> Element {
    let search_ctx = use_context::<SearchContext>();

    rsx! {
        tr {
            class: "hover:bg-gray-50",
            td {
                class: "px-4 py-3",
                if let Some(thumb) = &props.result.thumb {
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
                "{props.result.title}"
            }
            td {
                class: "px-4 py-3 text-sm text-gray-500",
                if let Some(year) = props.result.year {
                    "{year}"
                } else {
                    "Unknown"
                }
            }
            td {
                class: "px-4 py-3 text-sm text-gray-500",
                if let Some(first_label) = props.result.label.first() {
                    "{first_label}"
                } else {
                    "Unknown"
                }
            }
            td {
                class: "px-4 py-3 text-sm text-gray-500",
                if let Some(country) = &props.result.country {
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
                        let master_id = props.result.id.clone();
                        let master_title = props.result.title.clone();
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
                        let result = props.result.clone();
                        let on_import = props.on_import.clone();
                        move |_| {
                            let master = models::DiscogsMaster {
                                id: result.id.clone(),
                                title: result.title.clone(),
                                year: result.year,
                                thumb: result.thumb.clone(),
                                label: result.label.clone(),
                                country: result.country.clone(),
                                tracklist: Vec::new(), // Will be populated when fetching master details
                            };
                            let import_item = models::ImportItem::Master(master);
                            on_import.call(import_item);
                        }
                    },
                    "Add to Library"
                }
            }
        }
    }
}
