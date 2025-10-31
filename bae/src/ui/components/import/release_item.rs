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
                div { class: "w-20 h-20 aspect-square rounded overflow-hidden",
                    if let Some(thumb) = &props.result.thumb {
                        img {
                            class: "w-full h-full object-cover",
                            src: "{thumb}",
                            alt: "Album cover",
                        }
                    } else {
                        div { class: "w-full h-full bg-gray-200 flex items-center justify-center",
                            "No Image"
                        }
                    }
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
                for catno in props.result.catno.split(',').map(|s| s.trim()) {
                    div { "{catno}" }
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
                    "Import release"
                }
            }
        }
    }
}
