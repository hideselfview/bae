use dioxus::prelude::*;
use super::{search_form::SearchForm, search_status::SearchStatus, search_list::SearchList};

/// Main search page that orchestrates the search UI components
#[component]
pub fn SearchPage() -> Element {
    rsx! {
        div {
            class: "container mx-auto p-6",
            h1 { 
                class: "text-3xl font-bold mb-6",
                "Search Albums" 
            }
            
            SearchForm {}
            SearchStatus {}
            SearchList {}
        }
    }
}
