use dioxus::desktop::{Config, WindowBuilder};
use dioxus::prelude::*;

mod album_import_context;
mod audio_processing;
mod cache;
mod chunking;
mod cloud_storage;
mod components;
mod cue_flac;
mod database;
mod discogs;
mod encryption;
mod library;
mod library_context;
mod models;
mod secure_config;
mod subsonic;

use components::album_import::ImportWorkflowManager;
use components::*;
use library_context::SharedLibraryManager;
use std::path::PathBuf;
use subsonic::create_router;

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

/// Initialize cache manager
async fn create_cache_manager() -> cache::CacheManager {
    let cache_manager = cache::CacheManager::new()
        .await
        .expect("Failed to create cache manager");

    println!("Main: Cache manager created");
    cache_manager
}

/// Initialize cloud storage from secure config if configured (optional)
async fn create_cloud_storage(
    secure_config: &secure_config::SecureConfig,
) -> Option<cloud_storage::CloudStorageManager> {
    let config_data = match secure_config.get() {
        Ok(data) => data,
        Err(e) => {
            println!("Main: Warning - Failed to load secure config: {}", e);
            return None;
        }
    };

    let s3_config = match &config_data.s3_config {
        Some(config) => config,
        None => {
            println!("Main: Cloud storage not configured (optional)");
            return None;
        }
    };

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
}

/// Initialize database
async fn create_database() -> database::Database {
    let library_path = get_library_path();

    println!(
        "Main: Creating library directory: {}",
        library_path.display()
    );

    std::fs::create_dir_all(&library_path).expect("Failed to create library directory");

    let db_path = library_path.join("library.db");

    println!("Main: Initializing database at: {}", db_path.display());

    let database = database::Database::new(db_path.to_str().unwrap())
        .await
        .expect("Failed to create database");

    println!("Main: Database created");
    database
}

/// Initialize library manager with all dependencies
fn create_library_manager(
    database: database::Database,
    encryption_service: encryption::EncryptionService,
    cloud_storage: Option<cloud_storage::CloudStorageManager>,
) -> SharedLibraryManager {
    let chunking_service = chunking::ChunkingService::new(encryption_service.clone())
        .expect("Failed to create chunking service");

    println!("Main: Chunking service created");

    let library_manager =
        library::LibraryManager::new(database, chunking_service, cloud_storage.clone());

    println!("Main: Library manager created");

    let shared_library = SharedLibraryManager::new(library_manager);

    println!("Main: SharedLibraryManager created");

    shared_library
}

fn main() {
    // Create tokio runtime for async operations
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");

    println!("Main: Building dependencies...");

    // Create lazy secure config (no keyring access yet to avoid password prompting!)
    let secure_config = secure_config::SecureConfig::new();

    // Create encryption service
    let encryption_service = encryption::EncryptionService::new(secure_config.clone());

    // Initialize cache manager
    let cache_manager = rt.block_on(create_cache_manager());

    // Try to initialize cloud storage from secure config (optional, lazy loading)
    // This will only prompt for keyring if cloud storage is actually configured
    let cloud_storage = rt.block_on(create_cloud_storage(&secure_config));

    // Initialize database
    let database = rt.block_on(create_database());

    // Build library manager with all injected dependencies
    let library_manager =
        create_library_manager(database, encryption_service.clone(), cloud_storage.clone());

    // Create root application context
    let app_context = AppContext {
        library_manager: library_manager.clone(),
        secure_config: secure_config.clone(),
    };

    // Start Subsonic API server in background thread
    std::thread::spawn(move || {
        rt.block_on(start_subsonic_server(
            cache_manager,
            library_manager,
            encryption_service,
            cloud_storage,
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

    let app = create_router(
        library_manager,
        cache_manager,
        encryption_service,
        cloud_storage,
    );

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
