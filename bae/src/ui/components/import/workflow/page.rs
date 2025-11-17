use super::cd_import::CdImport;
use super::folder_import::FolderImport;
use super::torrent_import::TorrentImport;
use crate::ui::components::import::{ImportSource, ImportSourceSelector};
use crate::ui::import_context::ImportContext;
use dioxus::prelude::*;
use std::rc::Rc;

#[component]
pub fn ImportPage() -> Element {
    let import_context = use_context::<Rc<ImportContext>>();
    let mut selected_source = use_signal(|| ImportSource::Folder);

    let on_source_select = {
        let import_context = import_context.clone();
        move |source: ImportSource| {
            selected_source.set(source);
            // Reset import context when switching sources
            import_context.reset();
        }
    };

    rsx! {
        div { class: "max-w-4xl mx-auto p-6",
            div { class: "mb-6",
                h1 { class: "text-2xl font-bold text-white", "Import" }
            }

            // Combined source selector and import component
            div { class: "bg-gray-800 rounded-lg shadow p-4",
                ImportSourceSelector {
                    selected_source,
                    on_source_select,
                }
                match *selected_source.read() {
                    ImportSource::Folder => rsx! {
                        FolderImport {}
                    },
                    ImportSource::Torrent => rsx! {
                        TorrentImport {}
                    },
                    ImportSource::Cd => rsx! {
                        CdImport {}
                    },
                }
            }
        }
    }
}
