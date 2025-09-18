use dioxus::prelude::*;
use crate::{models, discogs, api_keys};

/// Album releases page - shows all releases for a specific master
#[component]
pub fn AlbumReleases(master_id: String, master_title: String) -> Element {
    let mut search_format = use_signal(|| "".to_string());
    let mut search_results = use_signal(|| Vec::<models::DiscogsRelease>::new());
    let mut is_loading = use_signal(|| false);
    let mut error_message = use_signal(|| None::<String>);

    let master_id_clone1 = master_id.clone();
    let master_id_clone2 = master_id.clone();
    
    // Load releases on component mount
    use_effect(move || {
        let master_id = master_id_clone1.clone();
        spawn(async move {
            is_loading.set(true);
            error_message.set(None);

            // Get API key from secure storage
            match api_keys::retrieve_api_key() {
                Ok(api_key) => {
                    let client = discogs::DiscogsClient::new(api_key);
                    
                    match client.search_releases_for_master(&master_id, "").await {
                        Ok(results) => {
                            search_results.set(results);
                        }
                        Err(e) => {
                            error_message.set(Some(format!("Search failed: {}", e)));
                        }
                    }
                }
                Err(_) => {
                    error_message.set(Some("No API key configured. Please go to Settings to add your Discogs API key.".to_string()));
                }
            }
            
            is_loading.set(false);
        });
    });

    let on_format_change = move |event: FormEvent| {
        let master_id = master_id_clone2.clone();
        search_format.set(event.value());
        spawn(async move {
            is_loading.set(true);
            error_message.set(None);

            // Get API key from secure storage
            match api_keys::retrieve_api_key() {
                Ok(api_key) => {
                    let client = discogs::DiscogsClient::new(api_key);
                    
                    match client.search_releases_for_master(&master_id, &event.value()).await {
                        Ok(results) => {
                            search_results.set(results);
                        }
                        Err(e) => {
                            error_message.set(Some(format!("Search failed: {}", e)));
                        }
                    }
                }
                Err(_) => {
                    error_message.set(Some("No API key configured. Please go to Settings to add your Discogs API key.".to_string()));
                }
            }
            
            is_loading.set(false);
        });
    };

    rsx! {
        div {
            class: "container mx-auto p-6",
            div {
                class: "mb-6",
                div {
                    class: "flex items-center gap-4 mb-4",
                    Link {
                        to: "/search",
                        class: "px-4 py-2 bg-gray-600 text-white rounded-lg hover:bg-gray-700 font-medium flex items-center gap-2",
                        "← Back to Search"
                    }
                    h1 { 
                        class: "text-3xl font-bold",
                        "Releases for: {master_title}"
                    }
                }
            }
            
            div {
                class: "mb-6 flex gap-2",
                select {
                    class: "px-3 py-3 border border-gray-300 rounded-lg text-lg bg-white",
                    value: "{search_format}",
                    onchange: on_format_change,
                    option { value: "", "All Formats" }
                    option { value: "Vinyl", "Vinyl" }
                    option { value: "CD", "CD" }
                    option { value: "Cassette", "Cassette" }
                    option { value: "Digital", "Digital" }
                    option { value: "DVD", "DVD" }
                    option { value: "Blu-ray", "Blu-ray" }
                }
            }

            if *is_loading.read() {
                div {
                    class: "text-center py-8",
                    p { 
                        class: "text-gray-600",
                        "Loading releases..." 
                    }
                }
            } else if let Some(error) = error_message.read().as_ref() {
                div {
                    class: "bg-red-100 border border-red-400 text-red-700 px-4 py-3 rounded mb-4",
                    "{error}"
                }
            }

            if !search_results.read().is_empty() {
                div {
                    class: "overflow-x-auto",
                    table {
                        class: "w-full border-collapse bg-white rounded-lg shadow-lg",
                        thead {
                            tr {
                                class: "bg-gray-50",
                                th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider", "Cover" }
                                th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider", "Title" }
                                th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider", "Year" }
                                th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider", "Label" }
                                th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider", "Country" }
                                th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider", "Format" }
                                th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider", "Actions" }
                            }
                        }
                        tbody {
                            class: "divide-y divide-gray-200",
                            for result in search_results.read().iter() {
                                tr {
                                    class: "hover:bg-gray-50",
                                    td {
                                        class: "px-4 py-3",
                                        if let Some(thumb) = &result.thumb {
                                            img {
                                                class: "w-10 h-10 object-cover rounded",
                                                src: "{thumb}",
                                                alt: "Album cover"
                                            }
                                        } else {
                                            div {
                                                class: "w-12 h-12 bg-gray-200 rounded flex items-center justify-center",
                                                "No Image"
                                            }
                                        }
                                    }
                                    td {
                                        class: "px-4 py-3 text-sm font-medium text-gray-900",
                                        "{result.title}"
                                    }
                                    td {
                                        class: "px-4 py-3 text-sm text-gray-500",
                                        if let Some(year) = result.year {
                                            "{year}"
                                        } else {
                                            "-"
                                        }
                                    }
                                    td {
                                        class: "px-4 py-3 text-sm text-gray-500",
                                        if let Some(first_label) = result.label.first() {
                                            "{first_label}"
                                        } else {
                                            "-"
                                        }
                                    }
                                    td {
                                        class: "px-4 py-3 text-sm text-gray-500",
                                        if let Some(country) = &result.country {
                                            "{country}"
                                        } else {
                                            "-"
                                        }
                                    }
                                    td {
                                        class: "px-4 py-3 text-sm text-gray-500",
                                        if !result.format.is_empty() {
                                            "{result.format.join(\", \")}"
                                        } else {
                                            "-"
                                        }
                                    }
                                    td {
                                        class: "px-4 py-3 text-sm",
                                        button {
                                            class: "text-green-600 hover:text-green-800 underline",
                                            "Import Album"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
