use super::{form::SearchMastersForm, list::SearchMastersList, status::SearchMastersStatus};
use crate::ui::import_context::ImportContext;
use dioxus::prelude::*;
use std::rc::Rc;

/// Main search masters page that orchestrates the search UI components
#[component]
pub fn SearchMastersPage() -> Element {
    let album_import_ctx = use_context::<Rc<ImportContext>>();

    let on_import_item = {
        let album_import_ctx = album_import_ctx.clone();
        move |master_id: String| {
            album_import_ctx.navigate_to_import_workflow(master_id, None);
        }
    };

    rsx! {
        div { class: "container mx-auto p-6",
            h1 { class: "text-3xl font-bold mb-6", "Search Albums" }

            SearchMastersForm {}
            SearchMastersStatus {}
            SearchMastersList { on_import: on_import_item }
        }
    }
}
