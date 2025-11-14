use crate::import::{MatchCandidate, MatchSource};
use dioxus::prelude::*;

#[component]
pub fn Confirmation(
    confirmed_candidate: ReadSignal<Option<MatchCandidate>>,
    on_edit: EventHandler<()>,
    on_confirm: EventHandler<()>,
) -> Element {
    if let Some(candidate) = confirmed_candidate.read().as_ref() {
        rsx! {
            div { class: "space-y-4",
                div { class: "bg-blue-50 border-2 border-blue-500 rounded-lg p-6",
                    div { class: "flex items-start justify-between mb-4",
                        div { class: "flex-1",
                            h3 { class: "text-lg font-semibold text-gray-900 mb-2",
                                "Selected Release"
                            }
                            div { class: "text-sm text-gray-600 space-y-1",
                                p { class: "text-lg font-medium text-gray-900", "{candidate.title()}" }
                                if let Some(ref year) = candidate.year() {
                                    p { "Year: {year}" }
                                }
                                {
                                    let (format_text, country_text, label_text) = match &candidate.source {
                                        MatchSource::MusicBrainz(release) => (
                                            release.format.as_ref().map(|f| format!("Format: {}", f)),
                                            release.country.as_ref().map(|c| format!("Country: {}", c)),
                                            release.label.as_ref().map(|l| format!("Label: {}", l)),
                                        ),
                                        MatchSource::Discogs(_) => (None, None, None),
                                    };
                                    rsx! {
                                        if let Some(ref fmt) = format_text {
                                            p { "{fmt}" }
                                        }
                                        if let Some(ref country) = country_text {
                                            p { "{country}" }
                                        }
                                        if let Some(ref label) = label_text {
                                            p { "{label}" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                div { class: "flex justify-end gap-3",
                    button {
                        class: "px-6 py-2 bg-gray-600 text-white rounded hover:bg-gray-700",
                        onclick: move |_| on_edit.call(()),
                        "Edit"
                    }
                    button {
                        class: "px-6 py-2 bg-green-600 text-white rounded hover:bg-green-700",
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

