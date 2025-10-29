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
            td {
                class: "px-4 py-3 cursor-pointer",
                onclick: {
                    let mut ctx = album_import_ctx.clone();
                    let master_id = props.result.id.to_string();
                    let master_title = props.result.title.clone();
                    move |_| {
                        ctx.navigate_to_releases(master_id.clone(), master_title.clone());
                    }
                },
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
            td {
                class: "px-4 py-3 text-sm font-medium text-gray-900 cursor-pointer",
                onclick: {
                    let mut ctx = album_import_ctx.clone();
                    let master_id = props.result.id.to_string();
                    let master_title = props.result.title.clone();
                    move |_| {
                        ctx.navigate_to_releases(master_id.clone(), master_title.clone());
                    }
                },
                "{props.result.title}"
            }
            td {
                class: "px-4 py-3 text-sm text-gray-500 cursor-pointer",
                onclick: {
                    let mut ctx = album_import_ctx.clone();
                    let master_id = props.result.id.to_string();
                    let master_title = props.result.title.clone();
                    move |_| {
                        ctx.navigate_to_releases(master_id.clone(), master_title.clone());
                    }
                },
                if let Some(year) = &props.result.year {
                    "{year}"
                } else {
                    "Unknown"
                }
            }
            td {
                class: "px-4 py-3 text-sm text-gray-500 cursor-pointer",
                onclick: {
                    let mut ctx = album_import_ctx.clone();
                    let master_id = props.result.id.to_string();
                    let master_title = props.result.title.clone();
                    move |_| {
                        ctx.navigate_to_releases(master_id.clone(), master_title.clone());
                    }
                },
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
            td { class: "px-4 py-3 text-sm",
                button {
                    class: "text-green-600 hover:text-green-800 underline whitespace-nowrap",
                    onclick: {
                        let master_id = props.result.id.to_string();
                        let on_import = props.on_import;
                        move |_| {
                            on_import.call(master_id.clone());
                        }
                    },
                    "Add Album"
                }
            }
        }
    }
}
