use dioxus::prelude::*;
use dioxus::desktop::{Config, WindowBuilder};

mod models;
mod discogs;
mod secure_config;
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
use library_context::SharedLibraryManager;
use subsonic::create_router;
use std::path::PathBuf;

/// Root application context containing all top-level dependencies
#[derive(Clone)]
pub struct AppContext {
    pub library_manager: SharedLibraryManager,
    pub secure_config: secure_config::SecureConfig,
}

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
    
    // Build dependencies
    println!("Main: Building dependencies...");
    
    // Create lazy secure config (no keyring access yet!)
    let secure_config = secure_config::SecureConfig::new();
    
    // Create encryption service (key will be lazy-loaded when first used)
    let encryption_service = encryption::EncryptionService::new(secure_config.clone());
    println!("Main: Encryption service created (lazy)");
    
    let (cache_manager, library_manager, cloud_storage) = rt.block_on(async {
        // Build cache manager
        let cache_manager = cache::CacheManager::new().await
            .expect("Failed to create cache manager");
        println!("Main: Cache manager created");
        
        // Try to initialize cloud storage from secure config (optional, lazy loading)
        // This will only prompt for keyring if cloud storage is actually configured
        let cloud_storage = match secure_config.get() {
            Ok(config_data) => {
                if let Some(s3_config) = &config_data.s3_config {
                    println!("Main: Initializing cloud storage...");
                    match cloud_storage::CloudStorageManager::new(s3_config.clone()).await {
                        Ok(cs) => {
                            println!("Main: Cloud storage initialized");
                            Some(cs)
                        }
                        Err(e) => {
                            println!("Main: Warning - Failed to initialize cloud storage: {}", e);
                            None
                        }
                    }
                } else {
                    println!("Main: Cloud storage not configured (optional)");
                    None
                }
            }
            Err(e) => {
                println!("Main: Warning - Failed to load secure config: {}", e);
                None
            }
        };
        
        // Build library path and database
        let library_path = get_library_path();
        
        // Ensure library directory exists
        println!("Main: Creating library directory: {}", library_path.display());
        tokio::fs::create_dir_all(&library_path).await
            .expect("Failed to create library directory");
        
        // Initialize database
        let db_path = library_path.join("library.db");
        println!("Main: Initializing database at: {}", db_path.display());
        let database = database::Database::new(db_path.to_str().unwrap()).await
            .expect("Failed to create database");
        println!("Main: Database created");
        
        // Initialize chunking service
        let chunking_service = chunking::ChunkingService::new(encryption_service.clone())
            .expect("Failed to create chunking service");
        println!("Main: Chunking service created");
        
        // Build library manager with all injected dependencies
        let library_manager = library::LibraryManager::new(
            database,
            chunking_service,
            cloud_storage.clone(),
        );
        println!("Main: Library manager created");
        
        // Wrap in SharedLibraryManager for thread-safe sharing
        let shared_library = SharedLibraryManager::new(library_manager);
        println!("Main: SharedLibraryManager created");
        
        (cache_manager, shared_library, cloud_storage)
    });
    
    // Create root application context
    let app_context = AppContext {
        library_manager: library_manager.clone(),
        secure_config: secure_config.clone(),
    };
    
    // Start Subsonic API server in background thread
    let cache_manager_for_subsonic = cache_manager;
    let encryption_service_for_subsonic = encryption_service;
    let cloud_storage_for_subsonic = cloud_storage;
    std::thread::spawn(move || {
        rt.block_on(start_subsonic_server(
            cache_manager_for_subsonic,
            library_manager,
            encryption_service_for_subsonic,
            cloud_storage_for_subsonic,
        ));
    });
    
    // Start the desktop app (this will run in the main thread)
    println!("Main: Starting Dioxus desktop app...");
    LaunchBuilder::desktop()
        .with_cfg(make_config())
        .with_context_provider(move || Box::new(app_context.clone()))
        .launch(App);
    println!("Main: Dioxus desktop app quit");
}

/// Start the Subsonic API server
async fn start_subsonic_server(
    cache_manager: cache::CacheManager,
    library_manager: SharedLibraryManager,
    encryption_service: encryption::EncryptionService,
    cloud_storage: Option<cloud_storage::CloudStorageManager>,
) {
    println!("Starting Subsonic API server...");
    
    let app = create_router(library_manager, cache_manager, encryption_service, cloud_storage);
    
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
