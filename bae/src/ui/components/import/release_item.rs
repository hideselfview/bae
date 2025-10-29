use crate::discogs::DiscogsMasterReleaseVersion;
use dioxus::prelude::*;

#[derive(Props, PartialEq, Clone)]
pub struct ReleaseItemProps {
    pub result: DiscogsMasterReleaseVersion,
    pub on_import: EventHandler<DiscogsMasterReleaseVersion>,
}

#[component]
pub fn ReleaseItem(props: ReleaseItemProps) -> Element {
    rsx! {
        tr { class: "hover:bg-gray-50",
            td {
                class: "px-4 py-3 cursor-pointer",
                onclick: {
                    let on_import = props.on_import;
                    let result = props.result.clone();
                    move |_| {
                        on_import.call(result.clone());
                    }
                },
                if let Some(thumb) = &props.result.thumb {
                    img {
                        class: "w-10 h-10 object-cover rounded",
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
                    let on_import = props.on_import;
                    let result = props.result.clone();
                    move |_| {
                        on_import.call(result.clone());
                    }
                },
                "{props.result.title}"
            }
            td {
                class: "px-4 py-3 text-sm text-gray-500 cursor-pointer",
                onclick: {
                    let on_import = props.on_import;
                    let result = props.result.clone();
                    move |_| {
                        on_import.call(result.clone());
                    }
                },
                if !props.result.label.is_empty() {
                    "{props.result.label}"
                } else {
                    "-"
                }
            }
            td {
                class: "px-4 py-3 text-sm text-gray-500 cursor-pointer",
                onclick: {
                    let on_import = props.on_import;
                    let result = props.result.clone();
                    move |_| {
                        on_import.call(result.clone());
                    }
                },
                "{props.result.catno}"
            }
            td {
                class: "px-4 py-3 text-sm text-gray-500 cursor-pointer",
                onclick: {
                    let on_import = props.on_import;
                    let result = props.result.clone();
                    move |_| {
                        on_import.call(result.clone());
                    }
                },
                "{props.result.country}"
            }
            td {
                class: "px-4 py-3 text-sm text-gray-500 cursor-pointer",
                onclick: {
                    let on_import = props.on_import;
                    let result = props.result.clone();
                    move |_| {
                        on_import.call(result.clone());
                    }
                },
                if !props.result.format.is_empty() {
                    "{props.result.format}"
                } else {
                    "-"
                }
            }
            td {
                class: "px-4 py-3 text-sm text-gray-500 cursor-pointer",
                onclick: {
                    let on_import = props.on_import;
                    let result = props.result.clone();
                    move |_| {
                        on_import.call(result.clone());
                    }
                },
                if let Some(released) = &props.result.released {
                    "{released}"
                } else {
                    "-"
                }
            }
            td { class: "px-4 py-3 text-sm",
                button {
                    class: "text-green-600 hover:text-green-800 underline whitespace-nowrap",
                    onclick: move |_| {
                        props.on_import.call(props.result.clone());
                    },
                    "Add Release"
                }
            }
        }
    }
}
