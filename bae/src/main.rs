use dioxus::prelude::*;

mod models;
mod discogs;
mod api_keys;

#[derive(Debug, Clone, Routable, PartialEq)]
#[rustfmt::skip]
enum Route {
    #[layout(Navbar)]
    #[route("/")]
    Home {},
    #[route("/library")]
    Library {},
    #[route("/search")]
    AlbumSearch {},
    #[route("/import")]
    AlbumImport {},
    #[route("/settings")]
    Settings {},
    #[route("/blog/:id")]
    Blog { id: i32 },
}

const FAVICON: Asset = asset!("/assets/favicon.ico");
const MAIN_CSS: Asset = asset!("/assets/main.css");
const HEADER_SVG: Asset = asset!("/assets/header.svg");
const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    rsx! {
        document::Link { rel: "icon", href: FAVICON }
        document::Link { rel: "stylesheet", href: MAIN_CSS } document::Link { rel: "stylesheet", href: TAILWIND_CSS }
        Router::<Route> {}
    }
}

#[component]
pub fn Hero() -> Element {
    rsx! {
        div {
            id: "hero",
            img { src: HEADER_SVG, id: "header" }
            div { id: "links",
                a { href: "https://dioxuslabs.com/learn/0.6/", "📚 Learn Dioxus" }
                a { href: "https://dioxuslabs.com/awesome", "🚀 Awesome Dioxus" }
                a { href: "https://github.com/dioxus-community/", "📡 Community Libraries" }
                a { href: "https://github.com/DioxusLabs/sdk", "⚙️ Dioxus Development Kit" }
                a { href: "https://marketplace.visualstudio.com/items?itemName=DioxusLabs.dioxus", "💫 VSCode Extension" }
                a { href: "https://discord.gg/XgGxMSkvUM", "👋 Community Discord" }
            }
        }
    }
}

/// Home page
#[component]
fn Home() -> Element {
    rsx! {
        div {
            class: "container mx-auto p-6",
            div {
                class: "text-center py-12",
                h1 {
                    class: "text-4xl font-bold mb-4",
                    "Welcome to bae"
                }
                p {
                    class: "text-xl text-gray-600 mb-8",
                    "Your personal music library manager"
                }
                div {
                    class: "flex justify-center space-x-4",
                    Link {
                        to: Route::AlbumSearch {},
                        class: "bg-blue-500 text-white px-6 py-3 rounded-lg hover:bg-blue-600 transition-colors",
                        "Search Albums"
                    }
                    Link {
                        to: Route::Library {},
                        class: "bg-gray-500 text-white px-6 py-3 rounded-lg hover:bg-gray-600 transition-colors",
                        "Browse Library"
                    }
                }
            }
            
            div {
                class: "grid grid-cols-1 md:grid-cols-3 gap-8 mt-12",
                div {
                    class: "text-center p-6",
                    h3 {
                        class: "text-xl font-bold mb-3",
                        "Search & Discover"
                    }
                    p {
                        class: "text-gray-600",
                        "Find albums using the Discogs database with detailed metadata and artwork"
                    }
                }
                div {
                    class: "text-center p-6",
                    h3 {
                        class: "text-xl font-bold mb-3",
                        "Import & Organize"
                    }
                    p {
                        class: "text-gray-600",
                        "Import your music collection from local files or remote sources"
                    }
                }
                div {
                    class: "text-center p-6",
                    h3 {
                        class: "text-xl font-bold mb-3",
                        "Stream & Enjoy"
                    }
                    p {
                        class: "text-gray-600",
                        "Access your music anywhere with built-in streaming capabilities"
                    }
                }
            }
        }
    }
}

/// Blog page
#[component]
pub fn Blog(id: i32) -> Element {
    rsx! {
        div {
            id: "blog",

            // Content
            h1 { "This is blog #{id}!" }
            p { "In blog #{id}, we show how the Dioxus router works and how URL parameters can be passed as props to our route components." }

            // Navigation links
            Link {
                to: Route::Blog { id: id - 1 },
                "Previous"
            }
            span { " <---> " }
            Link {
                to: Route::Blog { id: id + 1 },
                "Next"
            }
        }
    }
}

/// Shared navbar component.
#[component]
fn Navbar() -> Element {
    rsx! {
        div {
            id: "navbar",
            class: "bg-gray-800 text-white p-4 flex space-x-6",
            Link {
                to: Route::Home {},
                class: "hover:text-blue-300 transition-colors",
                "Home"
            }
            Link {
                to: Route::Library {},
                class: "hover:text-blue-300 transition-colors",
                "Library"
            }
            Link {
                to: Route::AlbumSearch {},
                class: "hover:text-blue-300 transition-colors",
                "Search"
            }
            Link {
                to: Route::AlbumImport {},
                class: "hover:text-blue-300 transition-colors",
                "Import"
            }
            Link {
                to: Route::Settings {},
                class: "hover:text-blue-300 transition-colors",
                "Settings"
            }
        }

        Outlet::<Route> {}
    }
}

/// Library browser page
#[component]
fn Library() -> Element {
    rsx! {
        div {
            class: "container mx-auto p-6",
            h1 { 
                class: "text-3xl font-bold mb-6",
                "Music Library" 
            }
            p { 
                class: "text-gray-600",
                "Your music library will appear here." 
            }
        }
    }
}

/// Album search page  
#[component]
fn AlbumSearch() -> Element {
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
            match get_api_key().await {
                Ok(Some(api_key)) => {
                    let client = discogs::DiscogsClient::new(api_key);
                    
                    match client.search_releases(&query).await {
                        Ok(results) => {
                            search_results.set(results);
                        }
                        Err(e) => {
                            error_message.set(Some(format!("Search failed: {}", e)));
                        }
                    }
                }
                Ok(None) => {
                    error_message.set(Some("No API key configured. Please go to Settings to add your Discogs API key.".to_string()));
                }
                Err(e) => {
                    error_message.set(Some(format!("Failed to retrieve API key: {}", e)));
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
                class: "mb-6",
                input {
                    class: "w-full p-3 border border-gray-300 rounded-lg text-lg",
                    placeholder: "Search for albums, artists, or releases...",
                    value: "{search_query}",
                    oninput: move |event| {
                        let query = event.value();
                        search_query.set(query.clone());
                        search_albums(query);
                    }
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

            div {
                class: "grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6",
                for result in search_results.read().iter() {
                    AlbumSearchResult { release: result.clone() }
                }
            }
        }
    }
}

/// Individual album search result component
#[component]
fn AlbumSearchResult(release: models::DiscogsRelease) -> Element {
    rsx! {
        div {
            class: "bg-white rounded-lg shadow-md p-4 hover:shadow-lg transition-shadow",
            
            if let Some(thumb) = &release.thumb {
                img {
                    src: "{thumb}",
                    alt: "Album cover",
                    class: "w-full h-48 object-cover rounded mb-3"
                }
            } else {
                div {
                    class: "w-full h-48 bg-gray-200 rounded mb-3 flex items-center justify-center",
                    span {
                        class: "text-gray-500",
                        "No Image"
                    }
                }
            }

            h3 {
                class: "font-bold text-lg mb-2",
                "{release.title}"
            }

            if let Some(year) = release.year {
                p {
                    class: "text-gray-600 mb-2",
                    "Year: {year}"
                }
            }

            if !release.genre.is_empty() {
                p {
                    class: "text-gray-600 mb-2",
                    "Genre: {release.genre.join(\", \")}"
                }
            }

            if let Some(country) = &release.country {
                p {
                    class: "text-gray-600 mb-2",
                    "Country: {country}"
                }
            }

            div {
                class: "mt-4",
                Link {
                    to: Route::AlbumImport {},
                    class: "bg-blue-500 text-white px-4 py-2 rounded hover:bg-blue-600 transition-colors",
                    "Import Album"
                }
            }
        }
    }
}

/// Album import page
#[component]
fn AlbumImport() -> Element {
    rsx! {
        div {
            class: "container mx-auto p-6",
            h1 { 
                class: "text-3xl font-bold mb-6",
                "Import Album" 
            }
            p { 
                class: "text-gray-600",
                "Album import functionality will be implemented here." 
            }
        }
    }
}

/// Settings page
#[component]
fn Settings() -> Element {
    let mut api_key_input = use_signal(|| String::new());
    let mut is_saving = use_signal(|| false);
    let mut save_message = use_signal(|| None::<String>);
    let mut has_api_key = use_signal(|| false);
    let mut is_loading = use_signal(|| true);

    // Check if API key exists on component load
    use_effect(move || {
        spawn(async move {
            match check_api_key().await {
                Ok(exists) => has_api_key.set(exists),
                Err(_) => has_api_key.set(false),
            }
            is_loading.set(false);
        });
    });

    let mut save_api_key_action = move || {
        let key = api_key_input.read().clone();
        if key.trim().is_empty() {
            save_message.set(Some("Please enter an API key".to_string()));
            return;
        }

        spawn(async move {
            is_saving.set(true);
            save_message.set(None);

            match save_api_key(key).await {
                Ok(_) => {
                    save_message.set(Some("API key saved and validated successfully!".to_string()));
                    has_api_key.set(true);
                    api_key_input.set(String::new()); // Clear the input for security
                }
                Err(e) => {
                    save_message.set(Some(format!("Error: {}", e)));
                }
            }

            is_saving.set(false);
        });
    };

    let delete_api_key_action = move || {
        spawn(async move {
            match delete_api_key().await {
                Ok(_) => {
                    save_message.set(Some("API key deleted successfully".to_string()));
                    has_api_key.set(false);
                }
                Err(e) => {
                    save_message.set(Some(format!("Error deleting API key: {}", e)));
                }
            }
        });
    };

    rsx! {
        div {
            class: "container mx-auto p-6 max-w-2xl",
            h1 { 
                class: "text-3xl font-bold mb-6",
                "Settings" 
            }

            // API Key Management Section
            div {
                class: "bg-white rounded-lg shadow-md p-6 mb-6",
                h2 {
                    class: "text-xl font-bold mb-4",
                    "Discogs API Key"
                }
                
                p {
                    class: "text-gray-600 mb-4",
                    "To search and import albums, you need a Discogs API key. "
                    a {
                        href: "https://www.discogs.com/settings/developers",
                        target: "_blank",
                        class: "text-blue-500 hover:text-blue-700 underline",
                        "Get your API key here"
                    }
                }

                if *is_loading.read() {
                    p { class: "text-gray-500", "Checking API key status..." }
                } else if *has_api_key.read() {
                    div {
                        class: "flex items-center justify-between bg-green-50 border border-green-200 rounded p-4 mb-4",
                        div {
                            class: "flex items-center",
                            span {
                                class: "text-green-600 font-medium",
                                "✓ API key configured and valid"
                            }
                        }
                        button {
                            class: "bg-red-500 text-white px-4 py-2 rounded hover:bg-red-600 transition-colors",
                            onclick: move |_| delete_api_key_action(),
                            "Remove Key"
                        }
                    }
                } else {
                    div {
                        class: "space-y-4",
                        div {
                            label {
                                class: "block text-sm font-medium text-gray-700 mb-2",
                                "API Key"
                            }
                            input {
                                r#type: "password",
                                class: "w-full p-3 border border-gray-300 rounded-lg",
                                placeholder: "Enter your Discogs API key",
                                value: "{api_key_input}",
                                oninput: move |event| {
                                    api_key_input.set(event.value());
                                    save_message.set(None); // Clear any previous messages
                                }
                            }
                        }
                        
                        button {
                            class: "bg-blue-500 text-white px-6 py-2 rounded-lg hover:bg-blue-600 transition-colors disabled:bg-gray-400",
                            disabled: *is_saving.read(),
                            onclick: move |_| save_api_key_action(),
                            if *is_saving.read() {
                                "Validating..."
                            } else {
                                "Save & Validate"
                            }
                        }
                    }
                }

                if let Some(message) = save_message.read().as_ref() {
                    div {
                        class: if message.contains("successfully") {
                            "mt-4 p-3 bg-green-100 border border-green-400 text-green-700 rounded"
                        } else {
                            "mt-4 p-3 bg-red-100 border border-red-400 text-red-700 rounded"
                        },
                        "{message}"
                    }
                }
            }

            // Future settings sections can be added here
            div {
                class: "bg-gray-50 rounded-lg p-6",
                h2 {
                    class: "text-xl font-bold mb-4 text-gray-600",
                    "More Settings"
                }
                p {
                    class: "text-gray-500",
                    "Additional settings will be available here in future updates."
                }
            }
        }
    }
}

/// Echo component that demonstrates fullstack server functions.
#[component]
fn Echo() -> Element {
    let mut response = use_signal(|| String::new());

    rsx! {
        div {
            id: "echo",
            h4 { "ServerFn Echo" }
            input {
                placeholder: "Type here to echo...",
                oninput:  move |event| async move {
                    let data = echo_server(event.value()).await.unwrap();
                    response.set(data);
                },
            }

            if !response().is_empty() {
                p {
                    "Server echoed: "
                    i { "{response}" }
                }
            }
        }
    }
}

/// Echo the user input on the server.
#[server(EchoServer)]
async fn echo_server(input: String) -> Result<String, ServerFnError> {
    Ok(input)
}

/// Server function to save and validate API key
#[server(SaveApiKey)]
async fn save_api_key(api_key: String) -> Result<(), ServerFnError> {
    use crate::api_keys::ApiKeyManager;
    
    let manager = ApiKeyManager::new();
    manager.set_and_validate_api_key(&api_key)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    
    Ok(())
}

/// Server function to check if API key exists and is valid
#[server(CheckApiKey)]
async fn check_api_key() -> Result<bool, ServerFnError> {
    use crate::api_keys::ApiKeyManager;
    
    let manager = ApiKeyManager::new();
    Ok(manager.has_valid_api_key().await)
}

/// Server function to delete stored API key
#[server(DeleteApiKey)]
async fn delete_api_key() -> Result<(), ServerFnError> {
    use crate::api_keys::ApiKeyManager;
    
    let manager = ApiKeyManager::new();
    manager.delete_api_key()
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    
    Ok(())
}

/// Server function to get API key for internal use
#[server(GetApiKey)]
async fn get_api_key() -> Result<Option<String>, ServerFnError> {
    use crate::api_keys::ApiKeyManager;
    
    let manager = ApiKeyManager::new();
    match manager.get_api_key() {
        Ok(key) => Ok(Some(key)),
        Err(_) => Ok(None),
    }
}
