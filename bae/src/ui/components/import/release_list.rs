use super::{import_workflow::ImportWorkflow, release_item::ReleaseItem};
use crate::discogs::DiscogsMasterReleaseVersion;
use crate::ui::import_context::ImportContext;
use dioxus::prelude::*;

#[component]
pub fn ReleaseList(master_id: String, master_title: String, on_back: EventHandler<()>) -> Element {
    let album_import_ctx = use_context::<ImportContext>();
    let mut release_results = use_signal(Vec::<DiscogsMasterReleaseVersion>::new);
    let mut selected_import_item = use_signal(|| None::<crate::discogs::DiscogsAlbum>);

    let master_id_for_effect = master_id.clone();

    // Load releases on component mount
    use_effect({
        let album_import_ctx = album_import_ctx.clone();

        move || {
            let master_id = master_id_for_effect.clone();
            let mut album_import_ctx = album_import_ctx.clone();

            spawn(async move {
                match album_import_ctx.get_master_versions(master_id).await {
                    Ok(versions) => {
                        release_results.set(versions);
                    }
                    Err(_) => {
                        // Error is already handled by album_import_ctx
                    }
                }
            });
        }
    });

    let on_import_release = {
        let master_id_for_import = master_id.clone();
        let album_import_ctx = album_import_ctx.clone();

        move |version: DiscogsMasterReleaseVersion| {
            let release_id = version.id.to_string();
            let master_id = master_id_for_import.clone();
            let mut album_import_ctx = album_import_ctx.clone();

            spawn(async move {
                match album_import_ctx.import_release(release_id, master_id).await {
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

    let on_back_from_import = {
        move |_| {
            selected_import_item.set(None);
        }
    };

    // If an item is selected for import, show the import workflow
    if let Some(item) = selected_import_item.read().as_ref() {
        return rsx! {
            ImportWorkflow {
                discogs_album: item.clone(),
                on_back: on_back_from_import
            }
        };
    }

    rsx! {
        div {
            class: "container mx-auto p-6",
            div {
                class: "mb-6",
                div {
                    class: "flex items-center gap-4 mb-4",
                    button {
                        class: "px-4 py-2 bg-gray-600 text-white rounded-lg hover:bg-gray-700 font-medium flex items-center gap-2",
                        onclick: move |_| on_back.call(()),
                        "← Back to Search"
                    }
                    h1 {
                        class: "text-3xl font-bold",
                        "Releases for: {master_title}"
                    }
                }
            }


            if *album_import_ctx.is_loading_versions.read() {
                div {
                    class: "text-center py-8",
                    p {
                        class: "text-gray-600",
                        "Loading releases..."
                    }
                }
            } else if let Some(error) = album_import_ctx.error_message.read().as_ref() {
                div {
                    class: "bg-red-100 border border-red-400 text-red-700 px-4 py-3 rounded mb-4",
                    "{error}"
                }
            }

            if !release_results.read().is_empty() {
                div {
                    class: "overflow-x-auto",
                    table {
                        class: "w-full border-collapse bg-white rounded-lg shadow-lg",
                        thead {
                            tr {
                                class: "bg-gray-50",
                                th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider", "Cover" }
                                th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider", "Title" }
                                th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider", "Label" }
                                th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider", "Catalog #" }
                                th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider", "Country" }
                                th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider", "Format" }
                                th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider", "Released" }
                                th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider", "Actions" }
                            }
                        }
                        tbody {
                            class: "divide-y divide-gray-200",
                            for result in release_results.read().iter() {
                                ReleaseItem {
                                    key: "{result.id}",
                                    result: result.clone(),
                                    on_import: on_import_release.clone()
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
