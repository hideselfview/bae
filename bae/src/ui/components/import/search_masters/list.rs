use super::{super::import_workflow::ImportWorkflow, item::SearchMastersItem};
use crate::{discogs::DiscogsAlbum, ui::import_context::ImportContext};
use dioxus::prelude::*;

#[component]
pub fn SearchMastersList() -> Element {
    let album_import_ctx = use_context::<ImportContext>();
    let mut selected_import_item = use_signal(|| None::<DiscogsAlbum>);

    let on_import_item = {
        let album_import_ctx = album_import_ctx.clone();

        move |master_id: String| {
            let mut album_import_ctx = album_import_ctx.clone();

            spawn(async move {
                // Fetch full master details using the search context
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

    // If an item is selected for import, show the import workflow
    if let Some(item) = selected_import_item.read().as_ref() {
        return rsx! {
            ImportWorkflow {
                discogs_album: item.clone(),
                on_back: move |_| selected_import_item.set(None),
            }
        };
    }

    if album_import_ctx.search_results.read().is_empty() {
        return rsx! {
            div {}
        };
    }

    rsx! {
        div { class: "overflow-x-auto",
            table { class: "w-full border-collapse bg-white rounded-lg shadow-lg text-left",
                thead {
                    tr { class: "bg-gray-50",
                        th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider",
                            "Cover"
                        }
                        th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider",
                            "Title"
                        }
                        th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider",
                            "Year"
                        }
                        th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider",
                            "Label"
                        }
                        th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider",
                            "Actions"
                        }
                    }
                }
                tbody { class: "divide-y divide-gray-200",
                    for result in album_import_ctx.search_results.read().iter() {
                        SearchMastersItem {
                            key: "{result.id}",
                            result: result.clone(),
                            on_import: on_import_item.clone(),
                        }
                    }
                }
            }
        }
    }
}
