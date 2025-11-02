use crate::discogs::{DiscogsMasterReleaseVersion, DiscogsTrack};
use crate::ui::import_context::ImportContext;
use dioxus::prelude::*;
use std::rc::Rc;

#[derive(Props, PartialEq, Clone)]
pub struct ReleaseItemProps {
    pub result: DiscogsMasterReleaseVersion,
    pub on_import: EventHandler<DiscogsMasterReleaseVersion>,
}

#[component]
pub fn ReleaseItem(props: ReleaseItemProps) -> Element {
    let album_import_ctx = use_context::<Rc<ImportContext>>();
    let client = album_import_ctx.client();

    let is_expanded = use_signal(|| false);
    let tracks = use_signal(|| Option::<Vec<DiscogsTrack>>::None);
    let is_loading_tracks = use_signal(|| false);

    let release_id = props.result.id.to_string();

    rsx! {
        Fragment {
            tr {
                class: "hover:bg-gray-50",
                onclick: {
                    let mut is_expanded = is_expanded;
                    let mut is_loading_tracks = is_loading_tracks;
                    let client = client.clone();
                    let release_id = release_id.clone();

                    move |_| {
                        if is_expanded() {
                            is_expanded.set(false);
                        } else {
                            is_expanded.set(true);
                            // Lazy load tracks if not already loaded
                            if tracks.read().is_none() {
                                is_loading_tracks.set(true);
                                let client = client.clone();
                                let release_id = release_id.clone();
                                let mut tracks = tracks;
                                let mut is_loading_tracks = is_loading_tracks;

                                spawn(async move {
                                    match client.get_release(&release_id).await {
                                        Ok(release) => {
                                            tracks.set(Some(release.tracklist));
                                        }
                                        Err(_) => {
                                            tracks.set(Some(Vec::new()));
                                        }
                                    }
                                    is_loading_tracks.set(false);
                                });
                            }
                        }
                    }
                },
                td {
                    class: "px-4 py-3 cursor-pointer",
                    div { class: "w-20 h-20 aspect-square rounded overflow-hidden",
                        if let Some(thumb) = &props.result.thumb {
                            img {
                                class: "w-full h-full object-cover",
                                src: "{thumb}",
                                alt: "Album cover",
                            }
                        } else {
                            div { class: "w-full h-full bg-gray-200 flex items-center justify-center",
                                "No Image"
                            }
                        }
                    }
                }
                td {
                    class: "px-4 py-3 text-sm text-gray-500 cursor-pointer",
                    if !props.result.label.is_empty() {
                        "{props.result.label}"
                    } else {
                        "-"
                    }
                }
                td {
                    class: "px-4 py-3 text-sm text-gray-500 cursor-pointer",
                    for catno in props.result.catno.split(',').map(|s| s.trim()) {
                        div { "{catno}" }
                    }
                }
                td {
                    class: "px-4 py-3 text-sm text-gray-500 cursor-pointer",
                    "{props.result.country}"
                }
                td {
                    class: "px-4 py-3 text-sm text-gray-500 cursor-pointer",
                    if !props.result.format.is_empty() {
                        "{props.result.format}"
                    } else {
                        "-"
                    }
                }
                td {
                    class: "px-4 py-3 text-sm text-gray-500 cursor-pointer",
                    if let Some(released) = &props.result.released {
                        "{released}"
                    } else {
                        "-"
                    }
                }
                td {
                    class: "px-4 py-3 text-sm",
                    onclick: move |e: MouseEvent| {
                        e.stop_propagation();
                    },
                    button {
                        class: "text-green-600 hover:text-green-800 underline whitespace-nowrap",
                        onclick: move |_| {
                            props.on_import.call(props.result.clone());
                        },
                        "Import release"
                    }
                }
            }
            if is_expanded() {
                tr {
                    td {
                        colspan: 7,
                        class: "px-4 py-4 bg-gray-50",
                        if is_loading_tracks() {
                            div { class: "text-center text-gray-600 py-4",
                                "Loading tracks..."
                            }
                        } else if let Some(track_list) = tracks.read().as_ref() {
                            if track_list.is_empty() {
                                div { class: "text-center text-gray-600 py-4",
                                    "No tracks available"
                                }
                            } else {
                                div { class: "space-y-1",
                                    for track in track_list.iter() {
                                        div { class: "flex items-center gap-3 text-sm",
                                            div { class: "w-16 text-gray-500 font-mono text-xs",
                                                "{track.position}"
                                            }
                                            div { class: "flex-1 text-gray-700",
                                                "{track.title}"
                                            }
                                            if let Some(duration) = &track.duration {
                                                div { class: "text-gray-500 text-xs",
                                                    "{duration}"
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        } else {
                            div { class: "text-center text-gray-600 py-4",
                                "No tracks available"
                            }
                        }
                    }
                }
            }
        }
    }
}
