use dioxus::prelude::*;
use crate::album_import_context::AlbumImportContext;

/// Displays loading and error states for search masters operations
#[component]
pub fn SearchMastersStatus() -> Element {
    let album_import_ctx = use_context::<AlbumImportContext>();

    rsx! {
        if *album_import_ctx.is_searching_masters.read() {
            div {
                class: "text-center py-8",
                p { 
                    class: "text-gray-600",
                    "Searching..." 
                }
            }
        } else if let Some(error) = album_import_ctx.error_message.read().as_ref() {
            div {
                class: "bg-red-100 border border-red-400 text-red-700 px-4 py-3 rounded mb-4",
                "{error}"
            }
        }
    }
}
