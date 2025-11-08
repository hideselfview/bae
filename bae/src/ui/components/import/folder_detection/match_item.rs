use crate::import::{MatchCandidate, MatchSource};
use dioxus::prelude::*;

#[component]
pub fn MatchItem(
    candidate: MatchCandidate,
    is_selected: bool,
    on_select: EventHandler<()>,
) -> Element {
    let confidence_color = if candidate.confidence >= 90.0 {
        "bg-green-100 text-green-800"
    } else if candidate.confidence >= 70.0 {
        "bg-yellow-100 text-yellow-800"
    } else {
        "bg-gray-100 text-gray-800"
    };

    let border_class = if is_selected {
        "border-blue-500 bg-blue-50"
    } else {
        "border-gray-200"
    };

    // Extract MusicBrainz-specific info for display
    let (format_text, country_text, label_text, catalog_text) = match &candidate.source {
        MatchSource::MusicBrainz(release) => (
            release.format.as_ref().map(|f| format!("Format: {}", f)),
            release.country.as_ref().map(|c| format!("Country: {}", c)),
            release.label.as_ref().map(|l| format!("Label: {}", l)),
            release
                .catalog_number
                .as_ref()
                .map(|c| format!("Catalog: {}", c)),
        ),
        MatchSource::Discogs(_) => (None, None, None, None),
    };

    rsx! {
        div {
            class: "border rounded-lg p-4 cursor-pointer hover:bg-gray-50 transition-colors {border_class}",
            onclick: move |_| on_select.call(()),
            div { class: "flex items-start justify-between",
                div { class: "flex-1",
                    div { class: "flex items-center gap-2 mb-1",
                        h4 { class: "text-lg font-semibold text-gray-900",
                            "{candidate.title()}"
                        }
                        span { class: "text-xs bg-purple-100 text-purple-700 px-2 py-1 rounded",
                            "{candidate.source_name()}"
                        }
                    }

                    div { class: "text-sm text-gray-600 mb-2 space-y-1",
                        if let Some(ref year) = candidate.year() {
                            p { "Year: {year}" }
                        }
                        if let Some(ref fmt) = format_text {
                            p { "{fmt}" }
                        }
                        if let Some(ref country) = country_text {
                            p { "{country}" }
                        }
                        if let Some(ref label) = label_text {
                            p { "{label}" }
                        }
                        if let Some(ref catalog) = catalog_text {
                            p { class: "text-xs text-gray-500", "{catalog}" }
                        }
                    }

                    if !candidate.match_reasons.is_empty() {
                        div { class: "flex flex-wrap gap-2 mb-2",
                            for reason in candidate.match_reasons.iter() {
                                span { class: "text-xs bg-gray-100 text-gray-700 px-2 py-1 rounded",
                                    "{reason}"
                                }
                            }
                        }
                    }
                }

                div { class: "ml-4",
                    span { class: "text-xs font-semibold px-2 py-1 rounded {confidence_color}",
                        "{candidate.confidence:.0}%"
                    }
                }
            }
        }
    }
}
