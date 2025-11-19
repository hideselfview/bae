use dioxus::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ImportSource {
    Folder,
    Torrent,
    Cd,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TorrentInputMode {
    File,
    Magnet,
}

#[component]
pub fn ImportSourceSelector(
    selected_source: Signal<ImportSource>,
    on_source_select: EventHandler<ImportSource>,
) -> Element {
    rsx! {
        div { class: "mb-4",
            div { class: "flex space-x-4 border-b border-gray-600",
                button {
                    class: if *selected_source.read() == ImportSource::Folder {
                        "px-4 py-2 font-medium transition-colors text-blue-400 border-b-2 border-blue-400 -mb-px"
                    } else {
                        "px-4 py-2 font-medium transition-colors text-gray-400 hover:text-gray-300"
                    },
                    onclick: move |_| {
                        on_source_select.call(ImportSource::Folder);
                    },
                    "Folder"
                }
                button {
                    class: if *selected_source.read() == ImportSource::Torrent {
                        "px-4 py-2 font-medium transition-colors text-blue-400 border-b-2 border-blue-400 -mb-px"
                    } else {
                        "px-4 py-2 font-medium transition-colors text-gray-400 hover:text-gray-300"
                    },
                    onclick: move |_| {
                        on_source_select.call(ImportSource::Torrent);
                    },
                    "Torrent"
                }
                button {
                    class: if *selected_source.read() == ImportSource::Cd {
                        "px-4 py-2 font-medium transition-colors text-blue-400 border-b-2 border-blue-400 -mb-px"
                    } else {
                        "px-4 py-2 font-medium transition-colors text-gray-400 hover:text-gray-300"
                    },
                    onclick: move |_| {
                        on_source_select.call(ImportSource::Cd);
                    },
                    "CD"
                }
            }
        }
    }
}
