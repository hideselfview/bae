use super::{
    form::SearchMusicBrainzForm, list::SearchMusicBrainzList, status::SearchMusicBrainzStatus,
};
use crate::ui::import_context::ImportContext;
use dioxus::prelude::*;
use std::rc::Rc;

/// Main MusicBrainz search page
#[component]
pub fn SearchMusicBrainzPage() -> Element {
    let album_import_ctx = use_context::<Rc<ImportContext>>();

    rsx! {
        div { class: "container mx-auto p-6",
            div { class: "flex items-center justify-between mb-6",
                h1 { class: "text-3xl font-bold", "Search MusicBrainz" }
                button {
                    class: "px-4 py-2 bg-gray-600 text-white rounded-lg hover:bg-gray-700 font-medium",
                    onclick: move |_| { album_import_ctx.navigate_back(); },
                    "Back"
                }
            }

            SearchMusicBrainzForm {}
            SearchMusicBrainzStatus {}
            SearchMusicBrainzList {}
        }
    }
}
