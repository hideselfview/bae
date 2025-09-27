use dioxus::prelude::*;
use crate::search_context::SearchContext;
use crate::discogs::DiscogsSearchResult;

#[derive(Props, PartialEq, Clone)]
pub struct SearchItemProps {
    pub result: DiscogsSearchResult,
    pub on_import: EventHandler<String>,
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
                if let Some(year) = &props.result.year {
                    "{year}"
                } else {
                    "Unknown"
                }
            }
            td {
                class: "px-4 py-3 text-sm text-gray-500",
                if let Some(labels) = &props.result.label {
                    if let Some(first_label) = labels.first() {
                        "{first_label}"
                    } else {
                        "Unknown"
                    }
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
                        let master_id = props.result.id.to_string();
                        let master_title = props.result.title.clone();
                        let mut search_ctx = search_ctx.clone();
                        move |_| {
                            search_ctx.navigate_to_releases(master_id.clone(), master_title.clone());
                        }
                    },
                    "View Releases"
                }
                if *search_ctx.is_importing_master.read() {
                    span {
                        class: "text-gray-500",
                        "Importing..."
                    }
                } else {
                    button {
                        class: "text-green-600 hover:text-green-800 underline",
                        onclick: {
                            let master_id = props.result.id.to_string();
                            let on_import = props.on_import.clone();
                            move |_| {
                                on_import.call(master_id.clone());
                            }
                        },
                        "Add to Library"
                    }
                }
            }
        }
    }
}
