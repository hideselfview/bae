use super::{release_list::ReleaseList, search_masters::SearchMastersPage};
use crate::ui::import_context::{ImportContext, SearchView};
use dioxus::prelude::*;

/// Manages the import workflow and navigation between search and releases views
#[component]
pub fn ImportWorkflowManager() -> Element {
    let album_import_ctx = use_context::<ImportContext>();
    let album_import_ctx_clone = album_import_ctx.clone();

    let on_release_back = {
        let mut album_import_ctx = album_import_ctx_clone.clone();
        move |_| album_import_ctx.navigate_back_to_search()
    };

    let current_view = album_import_ctx.current_view.read().clone();

    match current_view {
        SearchView::SearchResults => {
            rsx! {
                SearchMastersPage {}
            }
        }
        SearchView::ReleaseDetails {
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
    }
}
