use super::release_item::ReleaseItem;
use crate::discogs::client::DiscogsError;
use crate::discogs::{DiscogsMasterReleaseVersion, PaginationInfo, SortOrder};
use crate::ui::import_context::ImportContext;
use dioxus::prelude::*;
use std::rc::Rc;

#[component]
pub fn ReleaseList(master_id: String, master_title: String, on_back: EventHandler<()>) -> Element {
    let album_import_ctx = use_context::<Rc<ImportContext>>();
    let client = album_import_ctx.client();

    let sort_order = use_signal(|| SortOrder::Ascending);
    let current_page = use_signal(|| 1u32);
    let is_loading = use_signal(|| false);
    let error_message = use_signal(|| Option::<String>::None);
    let all_versions = use_signal(|| Vec::<DiscogsMasterReleaseVersion>::new());
    let pagination_info = use_signal(|| Option::<PaginationInfo>::None);

    // Load initial page
    {
        let master_id = master_id.clone();
        let client = client.clone();
        let is_loading = is_loading;
        let error_message = error_message;
        let all_versions = all_versions;
        let pagination_info = pagination_info;

        use_effect(move || {
            let master_id = master_id.clone();
            let client = client.clone();
            let sort_order_val = *sort_order.read();
            let mut is_loading = is_loading;
            let mut error_message = error_message;
            let mut all_versions = all_versions;
            let mut pagination_info = pagination_info;

            spawn(async move {
                is_loading.set(true);
                error_message.set(None);
                all_versions.set(Vec::new());
                pagination_info.set(None);

                match client
                    .get_master_releases(&master_id, Some(sort_order_val), 1)
                    .await
                {
                    Ok(result) => {
                        all_versions.set(result.versions.clone());
                        pagination_info.set(Some(result.pagination));
                        is_loading.set(false);
                    }
                    Err(e) => {
                        let error = match e {
                            DiscogsError::RateLimit => "Rate limit exceeded".to_string(),
                            DiscogsError::InvalidApiKey => "Invalid API key".to_string(),
                            DiscogsError::NotFound => "Master not found".to_string(),
                            DiscogsError::Request(e) => format!("Request failed: {}", e),
                            DiscogsError::Serialization(e) => format!("Parsing error: {}", e),
                            DiscogsError::InvalidInput(msg) => msg,
                        };
                        error_message.set(Some(error));
                        is_loading.set(false);
                    }
                }
            });
        });
    }

    let load_more = {
        let master_id = master_id.clone();
        let client = client.clone();
        let mut current_page = current_page;
        let mut is_loading = is_loading;
        let mut error_message = error_message;
        let mut all_versions = all_versions;
        let mut pagination_info = pagination_info;

        move || {
            let master_id = master_id.clone();
            let client = client.clone();
            let sort_order_val = *sort_order.read();
            let next_page = *current_page.read() + 1;

            spawn(async move {
                is_loading.set(true);
                error_message.set(None);

                match client
                    .get_master_releases(&master_id, Some(sort_order_val), next_page)
                    .await
                {
                    Ok(result) => {
                        let mut versions = all_versions.read().clone();
                        versions.extend(result.versions);
                        all_versions.set(versions);
                        pagination_info.set(Some(result.pagination));
                        current_page.set(next_page);
                        is_loading.set(false);
                    }
                    Err(e) => {
                        let error = match e {
                            DiscogsError::RateLimit => "Rate limit exceeded".to_string(),
                            DiscogsError::InvalidApiKey => "Invalid API key".to_string(),
                            DiscogsError::NotFound => "Master not found".to_string(),
                            DiscogsError::Request(e) => format!("Request failed: {}", e),
                            DiscogsError::Serialization(e) => format!("Parsing error: {}", e),
                            DiscogsError::InvalidInput(msg) => msg,
                        };
                        error_message.set(Some(error));
                        is_loading.set(false);
                    }
                }
            });
        }
    };

    let on_import_release = {
        let master_id_for_import = master_id.clone();
        let album_import_ctx = album_import_ctx.clone();

        move |version: DiscogsMasterReleaseVersion| {
            let release_id = version.id.to_string();
            album_import_ctx
                .navigate_to_import_workflow(master_id_for_import.clone(), Some(release_id));
        }
    };

    rsx! {
        div { class: "container mx-auto p-6",
            div { class: "mb-6",
                div { class: "flex items-center gap-4 mb-4",
                    button {
                        class: "px-4 py-2 bg-gray-600 text-white rounded-lg hover:bg-gray-700 font-medium flex items-center gap-2",
                        onclick: move |_| on_back.call(()),
                        "← Back to Search"
                    }
                    h1 { class: "text-3xl font-bold", "Releases for: {master_title}" }
                    div { class: "ml-auto flex items-center gap-2",
                        label { class: "text-sm font-medium text-gray-400", "Sort:" }
                        select {
                            class: "px-3 py-2 border border-gray-300 rounded-lg bg-white text-gray-900 text-sm",
                            value: match *sort_order.read() {
                                SortOrder::Ascending => "asc",
                                SortOrder::Descending => "desc",
                            },
                            onchange: move |e: FormEvent| {
                                let mut sort_order = sort_order;
                                let mut current_page = current_page;
                                sort_order
                                    .set(
                                        if e.value() == "asc" {
                                            SortOrder::Ascending
                                        } else {
                                            SortOrder::Descending
                                        },
                                    );
                                current_page.set(1);
                            },
                            option { value: "asc", "Oldest" }
                            option { value: "desc", "Newest" }
                        }
                    }
                }
            }

            if let Some(error) = error_message.read().as_ref() {
                div { class: "bg-red-100 border border-red-400 text-red-700 px-4 py-3 rounded mb-4",
                    "{error}"
                }
            } else {
                div { class: "relative",
                    if all_versions.read().is_empty() && is_loading() {
                        div { class: "text-center py-8",
                            p { class: "text-gray-600", "Loading releases..." }
                        }
                    } else if all_versions.read().is_empty() {
                        div { class: "text-center py-8",
                            p { class: "text-gray-600", "No releases found for this master." }
                        }
                    } else {
                        div { class: "space-y-4",
                            div { class: "overflow-x-auto",
                                table { class: "w-full border-collapse bg-white rounded-lg shadow-lg",
                                    thead {
                                        tr { class: "bg-gray-50",
                                            th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider",
                                                "Cover"
                                            }
                                            th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider",
                                                "Label"
                                            }
                                            th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider",
                                                "Catalog #"
                                            }
                                            th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider",
                                                "Country"
                                            }
                                            th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider",
                                                "Format"
                                            }
                                            th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider",
                                                "Released"
                                            }
                                            th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider",
                                                "Actions"
                                            }
                                        }
                                    }
                                    tbody { class: "divide-y divide-gray-200",
                                        for version in all_versions.read().iter() {
                                            ReleaseItem {
                                                key: "{version.id}",
                                                result: version.clone(),
                                                on_import: on_import_release.clone(),
                                            }
                                        }
                                    }
                                }
                            }
                            if let Some(pagination) = pagination_info.read().as_ref() {
                                if pagination.page < pagination.pages {
                                    div { class: "flex justify-center",
                                        button {
                                            class: "px-6 py-3 bg-gray-600 text-white rounded-lg hover:bg-gray-700 disabled:opacity-50 disabled:cursor-not-allowed font-medium",
                                            disabled: is_loading(),
                                            onclick: move |_| load_more(),
                                            if is_loading() {
                                                "Loading..."
                                            } else {
                                                "Load More"
                                            }
                                        }
                                    }
                                } else {
                                    div { class: "text-center text-sm text-gray-600 py-4",
                                        "Showing all {pagination.items} releases"
                                    }
                                }
                            }
                        }
                    }
                    if is_loading() && !all_versions.read().is_empty() {
                        div { class: "absolute inset-0 bg-white/50 flex items-center justify-center",
                            div { class: "text-gray-600", "Loading..." }
                        }
                    }
                }
            }
        }
    }
}
