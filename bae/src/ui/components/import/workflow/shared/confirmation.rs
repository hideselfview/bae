use crate::import::{MatchCandidate, MatchSource};
use dioxus::prelude::*;

#[component]
pub fn Confirmation(
    confirmed_candidate: ReadSignal<Option<MatchCandidate>>,
    on_edit: EventHandler<()>,
    on_confirm: EventHandler<()>,
) -> Element {
    if let Some(candidate) = confirmed_candidate.read().as_ref() {
        let cover_url = candidate.cover_art_url();
        let release_year = candidate.year();
        let original_year = match &candidate.source {
            MatchSource::MusicBrainz(release) => release.first_release_date.clone(),
            MatchSource::Discogs(_) => None,
        };

        rsx! {
            div { class: "bg-gray-800 rounded-lg shadow p-6",
                h3 { class: "text-sm font-semibold text-gray-300 uppercase tracking-wide mb-4",
                    "Selected Release"
                }
                div { class: "bg-gray-900 rounded-lg p-5 mb-4 border border-gray-700",
                    div { class: "flex gap-6",
                        // Album art
                        if let Some(ref url) = cover_url {
                            div { class: "flex-shrink-0 w-32 h-32 rounded-lg border border-gray-600 shadow-lg overflow-hidden",
                                img {
                                    src: "{url}",
                                    alt: "Album cover",
                                    class: "w-full h-full object-cover",
                                }
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
                div { class: "flex justify-end gap-3",
                    button {
                        class: "px-6 py-2 bg-gray-700 text-white rounded-lg hover:bg-gray-600 transition-colors border border-gray-600",
                        onclick: move |_| on_edit.call(()),
                        "Edit"
                    }
                    button {
                        class: "px-6 py-2 bg-green-600 text-white rounded-lg hover:bg-green-700 transition-colors",
                        onclick: move |_| on_confirm.call(()),
                        "Import"
                    }
                }
            }
        }
    } else {
        rsx! { div {} }
    }
}
