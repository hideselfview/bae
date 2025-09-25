use dioxus::prelude::*;
use crate::models;

#[derive(Props, PartialEq, Clone)]
pub struct ReleaseItemProps {
    pub result: models::DiscogsRelease,
    pub on_import: EventHandler<models::DiscogsRelease>,
}

#[component]
pub fn ReleaseItem(props: ReleaseItemProps) -> Element {
    rsx! {
        tr {
            class: "hover:bg-gray-50",
            td {
                class: "px-4 py-3",
                if let Some(thumb) = &props.result.thumb {
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
                "{props.result.title}"
            }
            td {
                class: "px-4 py-3 text-sm text-gray-500",
                if let Some(year) = props.result.year {
                    "{year}"
                } else {
                    "-"
                }
            }
            td {
                class: "px-4 py-3 text-sm text-gray-500",
                if let Some(first_label) = props.result.label.first() {
                    "{first_label}"
                } else {
                    "-"
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
                class: "px-4 py-3 text-sm text-gray-500",
                if !props.result.format.is_empty() {
                    "{props.result.format.join(\", \")}"
                } else {
                    "-"
                }
            }
            td {
                class: "px-4 py-3 text-sm",
                button {
                    class: "text-green-600 hover:text-green-800 underline",
                    onclick: move |_| {
                        props.on_import.call(props.result.clone());
                    },
                    "Add to Library"
                }
            }
        }
    }
}
