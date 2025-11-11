use dioxus::prelude::*;

#[derive(Debug, Clone, PartialEq)]
pub enum ImportSource {
    Folder,
    Torrent,
}

#[component]
pub fn ImportSourceSelector(
    selected_source: Signal<ImportSource>,
    on_source_select: EventHandler<ImportSource>,
) -> Element {
    rsx! {
        div { class: "mb-6",
            div { class: "flex space-x-4 border-b border-gray-200",
                button {
                    class: if *selected_source.read() == ImportSource::Folder {
                        "px-4 py-2 font-medium transition-colors text-blue-600 border-b-2 border-blue-600"
                    } else {
                        "px-4 py-2 font-medium transition-colors text-gray-600 hover:text-gray-900"
                    },
                    onclick: move |_| {
                        selected_source.set(ImportSource::Folder);
                        on_source_select.call(ImportSource::Folder);
                    },
                    "Import from Folder"
                }
                button {
                    class: if *selected_source.read() == ImportSource::Torrent {
                        "px-4 py-2 font-medium transition-colors text-blue-600 border-b-2 border-blue-600"
                    } else {
                        "px-4 py-2 font-medium transition-colors text-gray-600 hover:text-gray-900"
                    },
                    onclick: move |_| {
                        selected_source.set(ImportSource::Torrent);
                        on_source_select.call(ImportSource::Torrent);
                    },
                    "Import from Torrent"
                }
            }
        }
    }
}
