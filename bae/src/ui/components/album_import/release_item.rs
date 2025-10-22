use crate::discogs::DiscogsMasterReleaseVersion;
use crate::ui::album_import_context::AlbumImportContext;
use dioxus::prelude::*;

#[derive(Props, PartialEq, Clone)]
pub struct ReleaseItemProps {
    pub result: DiscogsMasterReleaseVersion,
    pub on_import: EventHandler<DiscogsMasterReleaseVersion>,
}

#[component]
pub fn ReleaseItem(props: ReleaseItemProps) -> Element {
    let album_import_ctx = use_context::<AlbumImportContext>();
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
                if !props.result.label.is_empty() {
                    "{props.result.label}"
                } else {
                    "-"
                }
            }
            td {
                class: "px-4 py-3 text-sm text-gray-500",
                "{props.result.catno}"
            }
            td {
                class: "px-4 py-3 text-sm text-gray-500",
                "{props.result.country}"
            }
            td {
                class: "px-4 py-3 text-sm text-gray-500",
                if !props.result.format.is_empty() {
                    "{props.result.format}"
                } else {
                    "-"
                }
            }
            td {
                class: "px-4 py-3 text-sm text-gray-500",
                if let Some(released) = &props.result.released {
                    "{released}"
                } else {
                    "-"
                }
            }
            td {
                class: "px-4 py-3 text-sm",
                if *album_import_ctx.is_importing_release.read() {
                    span {
                        class: "text-gray-500",
                        "Importing..."
                    }
                } else {
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
}
