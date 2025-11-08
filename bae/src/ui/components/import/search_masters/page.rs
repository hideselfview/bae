use super::{form::SearchMastersForm, list::SearchMastersList, status::SearchMastersStatus};
use crate::ui::import_context::ImportContext;
use dioxus::prelude::*;
use std::rc::Rc;

/// Main search masters page that orchestrates the search UI components
#[component]
pub fn SearchMastersPage() -> Element {
    let album_import_ctx = use_context::<Rc<ImportContext>>();

    let on_folder_import = {
        let album_import_ctx = album_import_ctx.clone();
        move |_| {
            album_import_ctx.navigate_to_folder_detection();
        }
    };

    let on_musicbrainz_search = {
        let album_import_ctx = album_import_ctx.clone();
        move |_| {
            album_import_ctx.navigate_to_musicbrainz_search();
        }
    };

    rsx! {
        div { class: "container mx-auto p-6",
            div { class: "flex items-center justify-between mb-6",
                h1 { class: "text-3xl font-bold", "Search Albums" }
                div { class: "flex gap-2",
                    button {
                        class: "px-4 py-2 bg-purple-600 text-white rounded-lg hover:bg-purple-700 font-medium",
                        onclick: on_musicbrainz_search,
                        "Search MusicBrainz"
                    }
                    button {
                        class: "px-4 py-2 bg-green-600 text-white rounded-lg hover:bg-green-700 font-medium",
                        onclick: on_folder_import,
                        "Import from Folder"
                    }
                }
            }

            SearchMastersForm {}
            SearchMastersStatus {}
            SearchMastersList {}
        }
    }
}
