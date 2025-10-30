use super::item::SearchMastersItem;
use crate::ui::import_context::ImportContext;
use dioxus::prelude::*;
use std::rc::Rc;

#[derive(Props, Clone, PartialEq)]
pub struct SearchMastersListProps {
    pub on_import: EventHandler<String>,
}

#[component]
pub fn SearchMastersList(props: SearchMastersListProps) -> Element {
    let album_import_ctx = use_context::<Rc<ImportContext>>();

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
                            on_import: props.on_import,
                        }
                    }
                }
            }
        }
    }
}
