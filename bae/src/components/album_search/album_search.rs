use dioxus::prelude::*;
use crate::{models, discogs, api_keys};

/// Album search page  
#[component]
pub fn AlbumSearch() -> Element {
    let mut search_query = use_signal(|| String::new());
    let mut search_results = use_signal(|| Vec::<models::DiscogsRelease>::new());
    let mut is_loading = use_signal(|| false);
    let mut error_message = use_signal(|| None::<String>);

    let search_albums = move |query: String| {
        spawn(async move {
            if query.trim().is_empty() {
                search_results.set(Vec::new());
                return;
            }

            is_loading.set(true);
            error_message.set(None);

            // Get API key from secure storage
            match api_keys::retrieve_api_key() {
                Ok(api_key) => {
                    let client = discogs::DiscogsClient::new(api_key);
                    
                    match client.search_masters(&query, "").await {
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
            h1 { 
                class: "text-3xl font-bold mb-6",
                "Search Albums" 
            }
            
            div {
                class: "mb-6 flex gap-2",
                input {
                    class: "flex-1 p-3 border border-gray-300 rounded-lg text-lg",
                    placeholder: "Search for albums, artists, or releases...",
                    value: "{search_query}",
                    oninput: move |event| {
                        search_query.set(event.value());
                    },
                    onkeydown: move |event| {
                        if event.key() == Key::Enter {
                            search_albums(search_query.read().clone());
                        }
                    }
                }
                button {
                    class: "px-6 py-3 bg-blue-600 text-white rounded-lg hover:bg-blue-700 font-medium",
                    onclick: move |_| {
                        search_albums(search_query.read().clone());
                    },
                    "Search"
                }
            }

            if *is_loading.read() {
                div {
                    class: "text-center py-8",
                    p { 
                        class: "text-gray-600",
                        "Searching..." 
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
                                                class: "w-12 h-12 object-cover rounded",
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
                                            "Unknown"
                                        }
                                    }
                                    td {
                                        class: "px-4 py-3 text-sm text-gray-500",
                                        if let Some(first_label) = result.label.first() {
                                            "{first_label}"
                                        } else {
                                            "Unknown"
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
                                        class: "px-4 py-3 text-sm space-x-2",
                                        Link {
                                            to: "/releases/{result.id}/{result.title}",
                                            class: "text-blue-600 hover:text-blue-800 underline",
                                            "View Releases"
                                        }
                                        button {
                                            class: "text-green-600 hover:text-green-800 underline",
                                            "Add to Library"
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
