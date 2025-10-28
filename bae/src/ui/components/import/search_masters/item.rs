use crate::discogs::client::DiscogsSearchResult;
use crate::ui::import_context::ImportContext;
use dioxus::prelude::*;

#[derive(Props, PartialEq, Clone)]
pub struct SearchMastersItemProps {
    pub result: DiscogsSearchResult,
    pub on_import: EventHandler<String>,
}

#[component]
pub fn SearchMastersItem(props: SearchMastersItemProps) -> Element {
    let album_import_ctx = use_context::<ImportContext>();

    rsx! {
        tr { class: "hover:bg-gray-50",
            td { class: "px-4 py-3",
                if let Some(thumb) = &props.result.thumb {
                    img {
                        class: "w-12 h-12 object-cover rounded",
                        src: "{thumb}",
                        alt: "Album cover",
                    }
                } else {
                    div { class: "w-12 h-12 bg-gray-200 rounded flex items-center justify-center",
                        "No Image"
                    }
                }
            }
            td { class: "px-4 py-3 text-sm font-medium text-gray-900", "{props.result.title}" }
            td { class: "px-4 py-3 text-sm text-gray-500",
                if let Some(year) = &props.result.year {
                    "{year}"
                } else {
                    "Unknown"
                }
            }
            td { class: "px-4 py-3 text-sm text-gray-500",
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
            td { class: "px-4 py-3 text-sm space-x-2",
                button {
                    class: "text-blue-600 hover:text-blue-800 underline",
                    onclick: {
                        let master_id = props.result.id.to_string();
                        let master_title = props.result.title.clone();
                        let mut album_import_ctx = album_import_ctx.clone();
                        move |_| {
                            album_import_ctx
                                .navigate_to_releases(master_id.clone(), master_title.clone());
                        }
                    },
                    "View Releases"
                }
                if *album_import_ctx.is_importing_master.read() {
                    span { class: "text-gray-500", "Importing..." }
                } else {
                    button {
                        class: "text-green-600 hover:text-green-800 underline",
                        onclick: {
                            let master_id = props.result.id.to_string();
                            let on_import = props.on_import;
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
