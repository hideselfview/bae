use crate::ui::import_context::ImportContext;
use dioxus::prelude::*;
use std::rc::Rc;

#[component]
pub fn SearchMusicBrainzStatus() -> Element {
    let album_import_ctx = use_context::<Rc<ImportContext>>();
    let is_searching = album_import_ctx.is_searching_mb;
    let mb_error = album_import_ctx.mb_error_message;

    rsx! {
        if *is_searching.read() {
            div { class: "text-center py-8",
                p { class: "text-gray-600", "Searching MusicBrainz..." }
            }
        } else if let Some(error) = mb_error.read().as_ref() {
            div { class: "bg-red-100 border border-red-400 text-red-700 px-4 py-3 rounded mb-4",
                "{error}"
            }
        }
    }
}
