use super::{
    import_workflow::ImportWorkflow, release_list::ReleaseList, search_masters::SearchMastersPage,
};
use crate::ui::import_context::{ImportContext, ImportStep};
use dioxus::prelude::*;

/// Manages the import workflow and navigation between search and releases views
#[component]
pub fn ImportWorkflowManager() -> Element {
    let album_import_ctx = use_context::<ImportContext>();

    let on_release_back = {
        let mut album_import_ctx = album_import_ctx.clone();
        move |_| album_import_ctx.navigate_back_to_search()
    };

    let current_step = album_import_ctx.current_step.read().clone();

    match current_step {
        ImportStep::SearchResults => {
            rsx! {
                SearchMastersPage {}
            }
        }
        ImportStep::ReleaseDetails {
            master_id,
            master_title,
        } => {
            rsx! {
                ReleaseList {
                    master_id: master_id.clone(),
                    master_title: master_title.clone(),
                    on_back: on_release_back,
                }
            }
        }
        ImportStep::ImportWorkflow {
            master_id,
            release_id,
        } => {
            rsx! {
                ImportWorkflow {
                    master_id: master_id.clone(),
                    release_id: release_id.clone(),
                }
            }
        }
    }
}
