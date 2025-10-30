use super::release_item::ReleaseItem;
use crate::discogs::client::DiscogsError;
use crate::discogs::DiscogsMasterReleaseVersion;
use crate::ui::import_context::ImportContext;
use dioxus::prelude::*;
use std::rc::Rc;

#[component]
pub fn ReleaseList(master_id: String, master_title: String, on_back: EventHandler<()>) -> Element {
    let album_import_ctx = use_context::<Rc<ImportContext>>();
    let client = album_import_ctx.client();

    let versions_resource = {
        let master_id = master_id.clone();
        let client = client.clone();

        use_resource(move || {
            let master_id = master_id.clone();
            let client = client.clone();

            async move {
                client
                    .get_master_releases(&master_id)
                    .await
                    .map_err(|e| match e {
                        DiscogsError::RateLimit => "Rate limit exceeded".to_string(),
                        DiscogsError::InvalidApiKey => "Invalid API key".to_string(),
                        DiscogsError::NotFound => "Master not found".to_string(),
                        DiscogsError::Request(e) => format!("Request failed: {}", e),
                        DiscogsError::Serialization(e) => format!("Parsing error: {}", e),
                    })
            }
        })
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
                }
            }

            if let Some(result) = versions_resource.value().read().as_ref() {
                if let Err(error) = result {
                    div { class: "bg-red-100 border border-red-400 text-red-700 px-4 py-3 rounded mb-4",
                        "{error}"
                    }
                } else if let Ok(versions) = result {
                    if versions.is_empty() {
                        div { class: "text-center py-8",
                            p { class: "text-gray-600", "No releases found for this master." }
                        }
                    } else {
                        div { class: "overflow-x-auto",
                            table { class: "w-full border-collapse bg-white rounded-lg shadow-lg",
                                thead {
                                    tr { class: "bg-gray-50",
                                        th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider",
                                            "Cover"
                                        }
                                        th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider",
                                            "Title"
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
                                    for result in versions.iter() {
                                        ReleaseItem {
                                            key: "{result.id}",
                                            result: result.clone(),
                                            on_import: on_import_release.clone(),
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            } else {
                div { class: "text-center py-8",
                    p { class: "text-gray-600", "Loading releases..." }
                }
            }
        }
    }
}
