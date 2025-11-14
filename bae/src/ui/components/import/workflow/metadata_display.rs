use crate::import::FolderMetadata;
use dioxus::prelude::*;

#[component]
pub fn MetadataDisplay(metadata: FolderMetadata) -> Element {
    let confidence_color = if metadata.confidence >= 70.0 {
        "text-green-600"
    } else if metadata.confidence >= 40.0 {
        "text-yellow-600"
    } else {
        "text-red-600"
    };

    rsx! {
        div { class: "bg-white rounded-lg shadow p-6 mb-6",
            h3 { class: "text-lg font-semibold text-gray-900 mb-4", "Detected Metadata" }

            div { class: "space-y-2",
                if let Some(ref artist) = metadata.artist {
                    div { class: "flex items-center",
                        span { class: "text-sm font-medium text-gray-600 w-24", "Artist:" }
                        span { class: "text-sm text-gray-900", "{artist}" }
                    }
                }

                if let Some(ref album) = metadata.album {
                    div { class: "flex items-center",
                        span { class: "text-sm font-medium text-gray-600 w-24", "Album:" }
                        span { class: "text-sm text-gray-900", "{album}" }
                    }
                }

                if let Some(year) = metadata.year {
                    div { class: "flex items-center",
                        span { class: "text-sm font-medium text-gray-600 w-24", "Year:" }
                        span { class: "text-sm text-gray-900", "{year}" }
                    }
                }

                if let Some(ref discid) = metadata.discid {
                    div { class: "flex items-center",
                        span { class: "text-sm font-medium text-gray-600 w-24", "DISCID:" }
                        span { class: "text-sm text-gray-900 font-mono", "{discid}" }
                    }
                }

                if let Some(track_count) = metadata.track_count {
                    div { class: "flex items-center",
                        span { class: "text-sm font-medium text-gray-600 w-24", "Tracks:" }
                        span { class: "text-sm text-gray-900", "{track_count}" }
                    }
                }
            }

            div { class: "mt-4 pt-4 border-t border-gray-200",
                div { class: "flex items-center",
                    span { class: "text-sm font-medium text-gray-600", "Confidence: " }
                    span { class: "text-sm font-semibold {confidence_color}",
                        "{metadata.confidence:.0}%"
                    }
                }
            }
        }
    }
}
