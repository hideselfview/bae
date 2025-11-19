use crate::import::{MatchCandidate, MatchSource};
use dioxus::prelude::*;

#[component]
pub fn MatchItem(
    candidate: MatchCandidate,
    is_selected: bool,
    on_select: EventHandler<()>,
) -> Element {
    let border_class = if is_selected {
        "border-blue-500 bg-blue-900/30"
    } else {
        "border-gray-700"
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
            class: "border rounded-lg p-4 cursor-pointer hover:bg-gray-700 transition-colors {border_class}",
            onclick: move |_| on_select.call(()),
            div { class: "flex items-start gap-4",
                // Album cover
                div { class: "w-16 h-16 flex-shrink-0 bg-gray-700 rounded overflow-hidden",
                    if let Some(cover_url) = candidate.cover_art_url() {
                        img {
                            src: "{cover_url}",
                            alt: "Album cover",
                            class: "w-full h-full object-cover",
                        }
                    } else {
                        div { class: "w-full h-full flex items-center justify-center text-gray-500 text-2xl", "🎵" }
                    }
                }

                div { class: "flex-1 min-w-0",
                    div { class: "flex items-center gap-2 mb-1",
                        h4 { class: "text-lg font-semibold text-white",
                            "{candidate.title()}"
                        }
                    }

                    div { class: "text-sm text-gray-400 mb-2 space-y-1",
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
                }
            }
        }
    }
}
