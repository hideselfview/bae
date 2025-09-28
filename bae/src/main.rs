use dioxus::prelude::*;
use dioxus::desktop::{Config, WindowBuilder};

mod models;
mod discogs;
mod api_keys;
mod components;
mod album_import_context;
mod database;
mod library;
mod chunking;
mod encryption;
mod cloud_storage;
mod cache;
mod subsonic;

use components::*;
use components::album_import::ImportWorkflowManager;
use album_import_context::AlbumImportContextProvider;
use library::LibraryManager;
use subsonic::{SubsonicState, create_router};
use std::path::PathBuf;

#[derive(Debug, Clone, Routable, PartialEq)]
#[rustfmt::skip]
enum Route {
    #[layout(Navbar)]
    #[route("/")]
    Library {},
    #[route("/album/:album_id")]
    AlbumDetail { album_id: String },
    #[route("/import")]
    ImportWorkflowManager {},
    #[route("/settings")]
    Settings {},
}

const FAVICON: Asset = asset!("/assets/favicon.ico");
const MAIN_CSS: Asset = asset!("/assets/main.css");
const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");

/// Get the library path
fn get_library_path() -> PathBuf {
    let home_dir = dirs::home_dir().expect("Failed to get home directory");
    home_dir.join("Music").join("bae")
}

#[tokio::main]
async fn main() {
    // Start Subsonic API server in background
    tokio::spawn(start_subsonic_server());
    
    // Start the desktop app
    LaunchBuilder::desktop()
        .with_cfg(make_config())
        .launch(App);
}

/// Start the Subsonic API server
async fn start_subsonic_server() {
    println!("Starting Subsonic API server...");
    
    match LibraryManager::new(get_library_path()).await {
        Ok(mut library_manager) => {
            // Try to configure cloud storage
            if let Err(e) = library_manager.try_configure_cloud_storage().await {
                println!("Warning: Cloud storage not configured for Subsonic server: {}", e);
            }
            
            let state = SubsonicState::new(library_manager);
            let app = create_router(state);
            
            let listener = match tokio::net::TcpListener::bind("127.0.0.1:4533").await {
                Ok(listener) => {
                    println!("Subsonic API server listening on http://127.0.0.1:4533");
                    listener
                }
                Err(e) => {
                    println!("Failed to bind Subsonic server: {}", e);
                    return;
                }
            };
            
            if let Err(e) = axum::serve(listener, app).await {
                println!("Subsonic server error: {}", e);
            }
        }
        Err(e) => {
            println!("Failed to initialize library for Subsonic server: {}", e);
        }
    }
}

fn make_config() -> Config {
    Config::default().with_window(make_window())
}

fn make_window() -> WindowBuilder {
    WindowBuilder::new()
        .with_title("bae")
        .with_always_on_top(false)
        .with_inner_size(dioxus::desktop::LogicalSize::new(1200, 800))
}

#[component]
fn App() -> Element {
    rsx! {
        document::Link { rel: "icon", href: FAVICON }
        document::Link { rel: "stylesheet", href: MAIN_CSS } 
        document::Link { rel: "stylesheet", href: TAILWIND_CSS }
        AlbumImportContextProvider {
            Router::<Route> {}
        }
    }
}