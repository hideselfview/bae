use dioxus::prelude::*;
use crate::search_context::SearchContext;
use super::search_item::SearchItem;

#[component]
pub fn SearchList() -> Element {
    let search_ctx = use_context::<SearchContext>();

    if search_ctx.search_results.read().is_empty() {
        return rsx! { div {} };
    }

    rsx! {
        div {
            class: "overflow-x-auto",
            table {
                class: "w-full border-collapse bg-white rounded-lg shadow-lg",
                thead {
                    tr {
                        class: "bg-gray-50",
                        th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider", "Cover" }
                        th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider", "Title" }
                        th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider", "Year" }
                        th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider", "Label" }
                        th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider", "Country" }
                        th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider", "Actions" }
                    }
                }
                tbody {
                    class: "divide-y divide-gray-200",
                    for result in search_ctx.search_results.read().iter() {
                        SearchItem {
                            key: "{result.id}",
                            result: result.clone()
                        }
                    }
                }
            }
        }
    }
}
