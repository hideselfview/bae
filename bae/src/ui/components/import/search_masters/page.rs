use super::{
    super::import_workflow::ImportWorkflow, form::SearchMastersForm, list::SearchMastersList,
    status::SearchMastersStatus,
};
use crate::{discogs::DiscogsAlbum, ui::import_context::ImportContext};
use dioxus::prelude::*;

/// Main search masters page that orchestrates the search UI components
#[component]
pub fn SearchMastersPage() -> Element {
    let album_import_ctx = use_context::<ImportContext>();
    let mut selected_import_item = use_signal(|| None::<DiscogsAlbum>);

    let on_import_item = {
        let album_import_ctx = album_import_ctx.clone();

        move |master_id: String| {
            let mut album_import_ctx = album_import_ctx.clone();

            spawn(async move {
                match album_import_ctx.import_master(master_id).await {
                    Ok(import_item) => {
                        selected_import_item.set(Some(import_item));
                    }
                    Err(_) => {
                        // Error is already handled by album_import_ctx
                    }
                }
            });
        }
    };

    // If an item is selected for import, show the import workflow without search UI
    if let Some(item) = selected_import_item.read().as_ref() {
        return rsx! {
            ImportWorkflow {
                discogs_album: item.clone(),
                on_back: move |_| selected_import_item.set(None),
            }
        };
    }

    rsx! {
        div { class: "container mx-auto p-6",
            h1 { class: "text-3xl font-bold mb-6", "Search Albums" }

            SearchMastersForm {}
            SearchMastersStatus {}
            SearchMastersList { on_import: on_import_item }
        }
    }
}
