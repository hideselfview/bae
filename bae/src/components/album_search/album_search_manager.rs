use dioxus::prelude::*;
use crate::{models, discogs, api_keys, search_context::{SearchContext, SearchView}};


/// Manages the album search state and navigation between search and releases views
#[component]
pub fn AlbumSearchManager() -> Element {
    let search_ctx = use_context::<SearchContext>();
    let search_ctx_clone = search_ctx.clone();

    let view = search_ctx.current_view.read().clone();
    match view {
        SearchView::SearchResults => {
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
                            value: "{search_ctx.search_query}",
                            oninput: {
                                let mut search_ctx = search_ctx_clone.clone();
                                move |event: FormEvent| {
                                    search_ctx.search_query.set(event.value());
                                }
                            },
                            onkeydown: {
                                let mut search_ctx = search_ctx_clone.clone();
                                move |event: KeyboardEvent| {
                                    if event.key() == Key::Enter {
                                        let query = search_ctx.search_query.read().clone();
                                        search_ctx.search_albums(query);
                                    }
                                }
                            }
                        }
                        button {
                            class: "px-6 py-3 bg-blue-600 text-white rounded-lg hover:bg-blue-700 font-medium",
                            onclick: {
                                let mut search_ctx = search_ctx_clone.clone();
                                move |_| {
                                    let query = search_ctx.search_query.read().clone();
                                    search_ctx.search_albums(query);
                                }
                            },
                            "Search"
                        }
                    }

                    if *search_ctx.is_loading.read() {
                        div {
                            class: "text-center py-8",
                            p { 
                                class: "text-gray-600",
                                "Searching..." 
                            }
                        }
                    } else if let Some(error) = search_ctx.error_message.read().as_ref() {
                        div {
                            class: "bg-red-100 border border-red-400 text-red-700 px-4 py-3 rounded mb-4",
                            "{error}"
                        }
                    }

                    if !search_ctx.search_results.read().is_empty() {
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
                                    for result in search_ctx.search_results.read().iter() {
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
                                                button {
                                                    class: "text-blue-600 hover:text-blue-800 underline",
                                                    onclick: {
                                                        let master_id = result.id.clone();
                                                        let master_title = result.title.clone();
                                                        let mut search_ctx = search_ctx_clone.clone();
                                                        move |_| {
                                                            search_ctx.navigate_to_releases(master_id.clone(), master_title.clone());
                                                        }
                                                    },
                                                    "View Releases"
                                                }
                                                button {
                                                    class: "text-green-600 hover:text-green-800 underline",
                                                    onclick: {
                                                        let album_title = result.title.clone();
                                                        let album_id = result.id.clone();
                                                        move |_| {
                                                            // TODO: Implement actual library storage
                                                            println!("Adding master album to library: {} (ID: {})", album_title, album_id);
                                                        }
                                                    },
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
        SearchView::ReleaseDetails { master_id, master_title } => {
            rsx! {
                AlbumReleasesWithBack {
                    master_id: master_id.clone(),
                    master_title: master_title.clone(),
                    on_back: {
                        let mut search_ctx = search_ctx_clone.clone();
                        move |_| search_ctx.navigate_back_to_search()
                    }
                }
            }
        }
    }
}

/// Album releases component with back navigation
#[component]
fn AlbumReleasesWithBack(
    master_id: String,
    master_title: String,
    on_back: EventHandler<()>
) -> Element {
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
                    button {
                        class: "px-4 py-2 bg-gray-600 text-white rounded-lg hover:bg-gray-700 font-medium flex items-center gap-2",
                        onclick: move |_| on_back.call(()),
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
                                            onclick: {
                                                let release_title = result.title.clone();
                                                let release_id = result.id.clone();
                                                let release_format = result.format.clone();
                                                let release_year = result.year;
                                                move |_| {
                                                    // TODO: Implement actual library storage
                                                    let formats = if !release_format.is_empty() {
                                                        release_format.join(", ")
                                                    } else {
                                                        "Unknown".to_string()
                                                    };
                                                    println!("Adding release to library: {} (ID: {}, Format: {}, Year: {:?})", 
                                                             release_title, release_id, formats, release_year);
                                                }
                                            },
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