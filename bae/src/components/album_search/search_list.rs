use dioxus::prelude::*;
use crate::{search_context::SearchContext, models::ImportItem};
use super::{search_item::SearchItem, import_workflow::ImportWorkflow};

#[component]
pub fn SearchList() -> Element {
    let search_ctx = use_context::<SearchContext>();
    let selected_import_item = use_signal(|| None::<ImportItem>);

    let on_import_item = {
        let selected_import_item = selected_import_item;
        let search_ctx = search_ctx.clone();
        move |master_id: String| {
            let mut selected_import_item = selected_import_item.clone();
            let mut search_ctx = search_ctx.clone();
            spawn(async move {
                // Fetch full master details using the search context
                match search_ctx.import_master(master_id).await {
                    Ok(import_item) => {
                        selected_import_item.set(Some(import_item));
                    }
                    Err(_) => {
                        // Error is already handled by search_ctx
                    }
                }
            });
        }
    };

    let on_back_from_import = {
        let mut selected_import_item = selected_import_item;
        move |_| {
            selected_import_item.set(None);
        }
    };

    // If an item is selected for import, show the import workflow
    if let Some(item) = selected_import_item.read().as_ref() {
        return rsx! {
            ImportWorkflow {
                item: item.clone(),
                on_back: on_back_from_import
            }
        };
    }

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
                            result: result.clone(),
                            on_import: on_import_item.clone()
                        }
                    }
                }
            }
        }
    }
}
