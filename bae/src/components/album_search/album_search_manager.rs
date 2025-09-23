use dioxus::prelude::*;
use crate::search_context::{SearchContext, SearchView};
use super::{search_form::SearchForm, release_list::ReleaseList};


/// Manages the album search state and navigation between search and releases views
#[component]
pub fn AlbumSearchManager() -> Element {
    let search_ctx = use_context::<SearchContext>();
    let search_ctx_clone = search_ctx.clone();

    let on_release_back = {
        let mut search_ctx = search_ctx_clone.clone();
        move |_| search_ctx.navigate_back_to_search()
    };

    let current_view = search_ctx.current_view.read().clone();

    match current_view {
        SearchView::SearchResults => {
            rsx! {
                SearchForm {}
            }
        }
        SearchView::ReleaseDetails { master_id, master_title } => {
            rsx! {
                ReleaseList {
                    master_id: master_id.clone(),
                    master_title: master_title.clone(),
                    on_back: on_release_back
                }
            }
        }
    }
}
