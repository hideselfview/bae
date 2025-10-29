use super::{
    super::import_workflow::ImportWorkflow, form::SearchMastersForm, list::SearchMastersList,
    status::SearchMastersStatus,
};
use dioxus::prelude::*;

/// Main search masters page that orchestrates the search UI components
#[component]
pub fn SearchMastersPage() -> Element {
    let mut selected_master_id = use_signal(|| None::<String>);

    let on_import_item = move |master_id: String| {
        selected_master_id.set(Some(master_id));
    };

    // If an item is selected for import, show the import workflow without search UI
    if let Some(master_id) = selected_master_id.read().as_ref() {
        return rsx! {
            ImportWorkflow {
                master_id: master_id.clone(),
                release_id: None,
                on_back: move |_| selected_master_id.set(None),
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
