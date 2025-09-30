use dioxus::prelude::*;
use dioxus::desktop::{Config, WindowBuilder};

mod models;
mod discogs;
mod api_keys;
mod s3_config;
mod components;
mod album_import_context;
mod database;
mod library;
mod library_context;
mod chunking;
mod encryption;
mod cloud_storage;
mod cache;
mod cue_flac;
mod audio_processing;
mod subsonic;

use components::*;
use components::album_import::ImportWorkflowManager;
use album_import_context::AlbumImportContextProvider;
use library_context::{initialize_library, get_library, LibraryContextProvider};
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

fn main() {
    // Create tokio runtime for async operations
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    
    // Initialize global singletons
    println!("Main: Initializing global singletons...");
    rt.block_on(async {
        // Initialize cache manager
        crate::cache::initialize_cache().await
            .expect("Failed to initialize cache manager");
        println!("Main: Cache manager initialized");
        
        // Initialize library manager
        let library_path = get_library_path();
        initialize_library(library_path).await
            .expect("Failed to initialize library manager");
        println!("Main: Library manager initialized");
    });
    
    // Start Subsonic API server in background thread
    std::thread::spawn(move || {
        rt.block_on(start_subsonic_server());
    });
    
    // Start the desktop app (this will run in the main thread)
    println!("Main: Starting Dioxus desktop app...");
    LaunchBuilder::desktop()
        .with_cfg(make_config())
        .launch(App);
    println!("Main: Dioxus app launched");
}

/// Start the Subsonic API server
async fn start_subsonic_server() {
    println!("Starting Subsonic API server...");
    
    let state = SubsonicState::new_shared(get_library());
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
    println!("App: Rendering app component");
    let library_manager = get_library();
    
    rsx! {
        document::Link { rel: "icon", href: FAVICON }
        document::Link { rel: "stylesheet", href: MAIN_CSS } 
        document::Link { rel: "stylesheet", href: TAILWIND_CSS }
        LibraryContextProvider {
            library_manager: library_manager,
            AlbumImportContextProvider {
                Router::<Route> {}
            }
        }
    }
}