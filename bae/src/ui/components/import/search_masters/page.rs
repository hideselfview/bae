use super::{form::SearchMastersForm, list::SearchMastersList, status::SearchMastersStatus};
use dioxus::prelude::*;

/// Main search masters page that orchestrates the search UI components
#[component]
pub fn SearchMastersPage() -> Element {
    rsx! {
        div { class: "container mx-auto p-6",
            h1 { class: "text-3xl font-bold mb-6", "Search Albums" }

            SearchMastersForm {}
            SearchMastersStatus {}
            SearchMastersList {}
        }
    }
}
