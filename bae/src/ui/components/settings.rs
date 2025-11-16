use crate::config::use_config;
use dioxus::prelude::*;

/// Settings page
#[component]
pub fn Settings() -> Element {
    let config = use_config();

    rsx! {
        div { class: "max-w-4xl mx-auto p-6",
            h1 { class: "text-2xl font-bold text-white mb-6", "Settings" }

            div { class: "bg-white rounded-lg shadow p-6",
                h2 { class: "text-xl font-semibold text-gray-900 mb-4", "Configuration" }

                div { class: "space-y-4",
                    div { class: "border-b border-gray-200 pb-3",
                        div { class: "text-sm font-medium text-gray-500 mb-1", "S3 Bucket" }
                        div { class: "text-base text-gray-900", "{config.s3_config.bucket_name}" }
                    }

                    div { class: "border-b border-gray-200 pb-3",
                        div { class: "text-sm font-medium text-gray-500 mb-1", "S3 Region" }
                        div { class: "text-base text-gray-900", "{config.s3_config.region}" }
                    }

                    if let Some(endpoint) = &config.s3_config.endpoint_url {
                        div { class: "border-b border-gray-200 pb-3",
                            div { class: "text-sm font-medium text-gray-500 mb-1", "S3 Endpoint" }
                            div { class: "text-base text-gray-900", "{endpoint}" }
                        }
                    }

                    div { class: "border-b border-gray-200 pb-3",
                        div { class: "text-sm font-medium text-gray-500 mb-1", "Discogs API Key" }
                        div { class: "text-base text-gray-900 flex items-center gap-2",
                            "Configured"
                            span { class: "text-green-600", "✓" }
                }
            }

                    div { class: "pb-3",
                        div { class: "text-sm font-medium text-gray-500 mb-1", "Torrent Bind Interface" }
                        div { class: "text-base text-gray-900",
                            if let Some(interface) = &config.torrent_bind_interface {
                                "{interface}"
                            } else {
                                span { class: "text-gray-400 italic", "Not set (uses default)" }
                            }
                        }
                    }
                }
            }
        }
    }
}
