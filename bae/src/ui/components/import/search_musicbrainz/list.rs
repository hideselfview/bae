use super::item::SearchMusicBrainzItem;
use crate::ui::import_context::ImportContext;
use dioxus::prelude::*;
use std::rc::Rc;

#[component]
pub fn SearchMusicBrainzList() -> Element {
    let album_import_ctx = use_context::<Rc<ImportContext>>();
    let results = album_import_ctx.mb_search_results.read().clone();

    if results.is_empty() {
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
                            "Title / Artist"
                        }
                        th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider",
                            "Format"
                        }
                        th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider",
                            "Country / Date"
                        }
                        th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider",
                            "Label / Catalog #"
                        }
                        th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider",
                            "Barcode"
                        }
                        th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider",
                            "Actions"
                        }
                    }
                }
                tbody { class: "divide-y divide-gray-200",
                    for result in results.iter() {
                        SearchMusicBrainzItem {
                            key: "{result.release_id}",
                            result: result.clone(),
                        }
                    }
                }
            }
        }
    }
}
