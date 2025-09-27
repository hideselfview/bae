use dioxus::prelude::*;
use super::{search_masters_form::SearchMastersForm, search_masters_status::SearchMastersStatus, search_masters_list::SearchMastersList};

/// Main search masters page that orchestrates the search UI components
#[component]
pub fn SearchMastersPage() -> Element {
    rsx! {
        div {
            class: "container mx-auto p-6",
            h1 { 
                class: "text-3xl font-bold mb-6",
                "Search Albums" 
            }
            
            SearchMastersForm {}
            SearchMastersStatus {}
            SearchMastersList {}
        }
    }
}
