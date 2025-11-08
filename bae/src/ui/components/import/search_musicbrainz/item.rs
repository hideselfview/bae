use crate::musicbrainz::MbRelease;
use crate::ui::import_context::ImportContext;
use dioxus::prelude::*;
use std::rc::Rc;

fn extract_discogs_master_id(url: &str) -> Option<String> {
    // Extract master ID from URL like https://www.discogs.com/master/12345
    url.split("/master/")
        .nth(1)
        .and_then(|s| s.split('/').next())
        .map(|s| s.to_string())
}

fn extract_discogs_release_id(url: &str) -> Option<String> {
    // Extract release ID from URL like https://www.discogs.com/release/12345
    url.split("/release/")
        .nth(1)
        .and_then(|s| s.split('/').next())
        .map(|s| s.to_string())
}

#[derive(Props, PartialEq, Clone)]
pub struct SearchMusicBrainzItemProps {
    pub result: MbRelease,
}

#[component]
pub fn SearchMusicBrainzItem(props: SearchMusicBrainzItemProps) -> Element {
    let album_import_ctx = use_context::<Rc<ImportContext>>();

    let release_id = props.result.release_id.clone();
    let release_group_id = props.result.release_group_id.clone();

    let on_click = {
        let album_import_ctx = album_import_ctx.clone();
        let release_id = release_id.clone();
        let release_group_id = release_group_id.clone();
        move |_| {
            let album_import_ctx = album_import_ctx.clone();
            let release_id = release_id.clone();
            let release_group_id = release_group_id.clone();
            spawn(async move {
                use tracing::info;
                info!(
                    "Selected MusicBrainz release: {} (group: {})",
                    release_id, release_group_id
                );

                // Lookup the release to get external URLs (Discogs)
                use crate::musicbrainz::lookup_release_by_id;
                match lookup_release_by_id(&release_id).await {
                    Ok((_release, external_urls)) => {
                        // Try to extract Discogs master/release ID from URLs
                        if let Some(ref discogs_url) = external_urls.discogs_master_url {
                            if let Some(master_id) = extract_discogs_master_id(discogs_url) {
                                info!(
                                    "Found Discogs master_id: {} from MusicBrainz release",
                                    master_id
                                );
                                album_import_ctx.navigate_to_import_workflow(master_id, None);
                                return;
                            }
                        }
                        if let Some(ref discogs_url) = external_urls.discogs_release_url {
                            if let Some(_release_id) = extract_discogs_release_id(discogs_url) {
                                info!(
                                    "Found Discogs release_id: {} from MusicBrainz release",
                                    _release_id
                                );
                                // For release URLs, we need to lookup the master_id
                                // For now, navigate back - full implementation would lookup master from release
                                album_import_ctx.navigate_back();
                                return;
                            }
                        }
                        info!("No Discogs URLs found for MusicBrainz release");
                        album_import_ctx.navigate_back();
                    }
                    Err(e) => {
                        use tracing::warn;
                        warn!("Failed to lookup MusicBrainz release: {}", e);
                        album_import_ctx.navigate_back();
                    }
                }
            });
        }
    };

    rsx! {
        tr { class: "hover:bg-gray-50 cursor-pointer",
            onclick: on_click.clone(),
            td { class: "px-4 py-3",
                div { class: "w-12 h-12 bg-purple-100 rounded flex items-center justify-center text-purple-600 font-bold",
                    "MB"
                }
            }
            td { class: "px-4 py-3 text-sm font-medium text-gray-900",
                onclick: on_click.clone(),
                div {
                    div { class: "font-semibold", "{props.result.title}" }
                    div { class: "text-gray-500 text-xs mt-1", "{props.result.artist}" }
                }
            }
            td { class: "px-4 py-3 text-sm text-gray-500",
                onclick: on_click.clone(),
                if let Some(ref format) = props.result.format {
                    "{format}"
                } else {
                    "—"
                }
            }
            td { class: "px-4 py-3 text-sm text-gray-500",
                onclick: on_click.clone(),
                div { class: "text-xs",
                    if let Some(ref country) = props.result.country {
                        span { "{country}" }
                    } else {
                        span { "—" }
                    }
                    if let Some(ref date) = props.result.date {
                        div { class: "text-gray-400 mt-1", "{date}" }
                    }
                }
            }
            td { class: "px-4 py-3 text-sm text-gray-500",
                onclick: on_click.clone(),
                div { class: "text-xs",
                    if let Some(ref label) = props.result.label {
                        div { "{label}" }
                    } else {
                        div { "—" }
                    }
                    if let Some(ref catalog) = props.result.catalog_number {
                        div { class: "text-gray-400 mt-1", "{catalog}" }
                    }
                }
            }
            td { class: "px-4 py-3 text-sm text-gray-500",
                onclick: on_click.clone(),
                if let Some(ref barcode) = props.result.barcode {
                    span { class: "text-xs font-mono", "{barcode}" }
                } else {
                    span { "—" }
                }
            }
            td { class: "px-4 py-3 text-sm",
                onclick: on_click.clone(),
                button {
                    class: "px-4 py-2 bg-purple-600 text-white rounded hover:bg-purple-700",
                    onclick: move |e| {
                        e.stop_propagation();
                        on_click(e);
                    },
                    "Select"
                }
            }
        }
    }
}
