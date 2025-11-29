use crate::import::{MatchCandidate, MatchSource};
use crate::ui::import_context::ImportContext;
use crate::ui::local_file_url;
use dioxus::prelude::*;
use std::rc::Rc;

#[component]
pub fn Confirmation(
    confirmed_candidate: ReadSignal<Option<MatchCandidate>>,
    on_edit: EventHandler<()>,
    on_confirm: EventHandler<()>,
) -> Element {
    let import_context = use_context::<Rc<ImportContext>>();
    let is_importing = import_context.is_importing();
    let folder_files = import_context.folder_files();
    let folder_path = import_context.folder_path();

    // Use context signal for selected cover image index (None = use remote URL from candidate)
    let mut selected_cover_index = import_context.selected_cover_index();

    if let Some(candidate) = confirmed_candidate.read().as_ref() {
        let remote_cover_url = candidate.cover_art_url();
        let release_year = candidate.year();
        let original_year = match &candidate.source {
            MatchSource::MusicBrainz(release) => release.first_release_date.clone(),
            MatchSource::Discogs(_) => None,
        };

        // Get local artwork files
        let artwork_files = folder_files.read().artwork.clone();
        let folder_path_str = folder_path.read().clone();

        // Determine the cover URL to display
        let display_cover_url = if let Some(idx) = *selected_cover_index.read() {
            // User selected a local image
            if let Some(img) = artwork_files.get(idx) {
                let path = format!("{}/{}", folder_path_str, img.name);
                Some(local_file_url(&path))
            } else {
                remote_cover_url.clone()
            }
        } else {
            // Default: use remote URL if available
            remote_cover_url.clone()
        };

        rsx! {
            div { class: "bg-gray-800 rounded-lg shadow p-6",
                h3 { class: "text-sm font-semibold text-gray-300 uppercase tracking-wide mb-4",
                    "Selected Release"
                }
                div { class: "bg-gray-900 rounded-lg p-5 mb-4 border border-gray-700",
                    div { class: "flex gap-6",
                        // Album art
                        if let Some(ref url) = display_cover_url {
                            div { class: "flex-shrink-0 w-32 h-32 rounded-lg border border-gray-600 shadow-lg overflow-hidden",
                                img {
                                    src: "{url}",
                                    alt: "Album cover",
                                    class: "w-full h-full object-cover",
                                }
                            }
                        } else {
                            div { class: "flex-shrink-0 w-32 h-32 rounded-lg border border-gray-600 shadow-lg bg-gray-700 flex items-center justify-center",
                                span { class: "text-gray-500 text-4xl", "🎵" }
                            }
                        }
                        // Release info
                        div { class: "flex-1 space-y-3",
                            p { class: "text-xl font-semibold text-white", "{candidate.title()}" }
                            div { class: "space-y-1 text-sm text-gray-300",
                                // Original album year (MusicBrainz only)
                                if let Some(ref orig_year) = original_year {
                                    p {
                                        span { class: "text-gray-400", "Original: " }
                                        span { class: "text-white", "{orig_year}" }
                                    }
                                }
                                // This release year
                                if let Some(ref year) = release_year {
                                    p {
                                        span { class: "text-gray-400", "This Release: " }
                                        span { class: "text-white", "{year}" }
                                    }
                                }
                                {
                                    let (format_text, country_text, label_text) = match &candidate.source {
                                        MatchSource::MusicBrainz(release) => (
                                            release.format.as_ref().map(|f| f.clone()),
                                            release.country.as_ref().map(|c| c.clone()),
                                            release.label.as_ref().map(|l| l.clone()),
                                        ),
                                        MatchSource::Discogs(_) => (None, None, None),
                                    };
                                    rsx! {
                                        if let Some(ref fmt) = format_text {
                                            p {
                                                span { class: "text-gray-400", "Format: " }
                                                span { class: "text-white", "{fmt}" }
                                            }
                                        }
                                        if let Some(ref country) = country_text {
                                            p {
                                                span { class: "text-gray-400", "Country: " }
                                                span { class: "text-white", "{country}" }
                                            }
                                        }
                                        if let Some(ref label) = label_text {
                                            p {
                                                span { class: "text-gray-400", "Label: " }
                                                span { class: "text-white", "{label}" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Cover art selection (if there are local images to choose from)
                if !artwork_files.is_empty() || remote_cover_url.is_some() {
                    div { class: "mb-4",
                        h4 { class: "text-sm font-medium text-gray-400 mb-2", "Cover Art" }
                        div { class: "flex flex-wrap gap-2",
                            // Remote cover option (if available)
                            if let Some(ref url) = remote_cover_url {
                                {
                                    let is_selected = selected_cover_index.read().is_none();
                                    rsx! {
                                        button {
                                            class: if is_selected {
                                                "relative w-16 h-16 rounded border-2 border-green-500 overflow-hidden"
                                            } else {
                                                "relative w-16 h-16 rounded border-2 border-gray-600 hover:border-gray-500 overflow-hidden"
                                            },
                                            onclick: move |_| selected_cover_index.set(None),
                                            img {
                                                src: "{url}",
                                                alt: "Remote cover",
                                                class: "w-full h-full object-cover",
                                            }
                                            if is_selected {
                                                div { class: "absolute top-0 right-0 bg-green-500 text-white text-xs px-1 rounded-bl",
                                                    "✓"
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            // Local image options
                            for (idx, img) in artwork_files.iter().enumerate() {
                                {
                                    let is_selected = *selected_cover_index.read() == Some(idx);
                                    let img_path = format!("{}/{}", folder_path_str, img.name);
                                    let img_url = local_file_url(&img_path);
                                    rsx! {
                                        button {
                                            class: if is_selected {
                                                "relative w-16 h-16 rounded border-2 border-green-500 overflow-hidden"
                                            } else {
                                                "relative w-16 h-16 rounded border-2 border-gray-600 hover:border-gray-500 overflow-hidden"
                                            },
                                            onclick: move |_| selected_cover_index.set(Some(idx)),
                                            img {
                                                src: "{img_url}",
                                                alt: "{img.name}",
                                                class: "w-full h-full object-cover",
                                            }
                                            if is_selected {
                                                div { class: "absolute top-0 right-0 bg-green-500 text-white text-xs px-1 rounded-bl",
                                                    "✓"
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        p { class: "text-xs text-gray-500 mt-1",
                            "Click an image to set it as the album cover"
                        }
                    }
                }

                div { class: "flex justify-end gap-3",
                    button {
                        class: "px-6 py-2 bg-gray-700 text-white rounded-lg hover:bg-gray-600 transition-colors border border-gray-600",
                        disabled: *is_importing.read(),
                        onclick: move |_| on_edit.call(()),
                        "Edit"
                    }
                    button {
                        class: if *is_importing.read() {
                            "px-6 py-2 bg-green-600 text-white rounded-lg transition-colors opacity-75 cursor-not-allowed flex items-center gap-2"
                        } else {
                            "px-6 py-2 bg-green-600 text-white rounded-lg hover:bg-green-700 transition-colors flex items-center gap-2"
                        },
                        disabled: *is_importing.read(),
                        onclick: move |_| on_confirm.call(()),
                        if *is_importing.read() {
                            div { class: "animate-spin rounded-full h-4 w-4 border-b-2 border-white" }
                        }
                        "Import"
                    }
                }
            }
        }
    } else {
        rsx! { div {} }
    }
}
