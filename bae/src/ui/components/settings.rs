use crate::config::use_config;
use dioxus::prelude::*;

/// Settings page
/// TODO: Fully implement with new unified Config system
#[component]
pub fn Settings() -> Element {
    let config = use_config();

    rsx! {
        div { class: "p-6",
            h1 { class: "text-2xl font-bold mb-4", "Settings" }

            div { class: "mb-6",
                h2 { class: "text-xl font-semibold mb-2", "Configuration" }

                div { class: "bg-gray-100 p-4 rounded",
                    p { class: "mb-2",
                        strong { "S3 Bucket: " }
                        "{config.s3_config.bucket_name}"
                    }
                    p { class: "mb-2",
                        strong { "S3 Region: " }
                        "{config.s3_config.region}"
                    }
                    if let Some(endpoint) = &config.s3_config.endpoint_url {
                        p { class: "mb-2",
                            strong { "S3 Endpoint: " }
                            "{endpoint}"
                        }
                    }
                    p { class: "mb-2",
                        strong { "Discogs API Key: " }
                        "Configured ✓"
                    }
                }
            }

            p { class: "text-sm text-gray-500 mt-4", "Settings management UI coming soon..." }
        }
    }
}
