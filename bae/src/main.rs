use dioxus::desktop::{Config as DioxusConfig, WindowBuilder};
use dioxus::prelude::*;

mod album_import_context;
mod audio_processing;
mod cache;
mod chunking;
mod cloud_storage;
mod components;
mod config;
mod cue_flac;
mod database;
mod discogs;
mod encryption;
mod import_service;
mod library;
mod library_context;
mod models;
mod progress_service; // Used internally by import_service
mod subsonic;

use components::album_import::ImportWorkflowManager;
use components::*;
use library_context::SharedLibraryManager;
use subsonic::create_router;

/// Root application context containing all top-level dependencies
#[derive(Clone)]
pub struct AppContext {
    pub library_manager: SharedLibraryManager,
    pub config: config::Config,
    pub import_service_handle: import_service::ImportServiceHandle,
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

/// Initialize cache manager
async fn create_cache_manager() -> cache::CacheManager {
    let cache_manager = cache::CacheManager::new()
        .await
        .expect("Failed to create cache manager");

    println!("Main: Cache manager created");
    cache_manager
}

/// Initialize cloud storage from config
async fn create_cloud_storage(config: &config::Config) -> cloud_storage::CloudStorageManager {
    println!("Main: Initializing cloud storage...");

    cloud_storage::CloudStorageManager::new(config.s3_config.clone())
        .await
        .expect("Failed to initialize cloud storage. Please check your S3 configuration.")
}

/// Initialize database
async fn create_database(config: &config::Config) -> database::Database {
    let library_path = config.get_library_path();

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
fn create_library_manager(database: database::Database) -> SharedLibraryManager {
    let library_manager = library::LibraryManager::new(database);

    println!("Main: Library manager created");

    let shared_library = SharedLibraryManager::new(library_manager);

    println!("Main: SharedLibraryManager created");

    shared_library
}

fn main() {
    // Initialize logging with filters to suppress verbose debug logs
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::builder()
                .with_default_directive(tracing_subscriber::filter::LevelFilter::INFO.into())
                .parse_lossy(
                    "bae=info,sqlx=warn,aws_config=warn,aws_smithy=warn,aws_sdk_s3=warn,hyper=warn",
                ),
        )
        .init();

    // Create tokio runtime for async operations
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");

    println!("Main: Building dependencies...");

    // Load application configuration (handles .env loading in debug builds)
    let config = config::Config::load();

    // Initialize cache manager
    let cache_manager = rt.block_on(create_cache_manager());

    // Try to initialize cloud storage from config (optional, lazy loading)
    // This will only prompt for keyring if cloud storage is actually configured
    let cloud_storage = rt.block_on(create_cloud_storage(&config));

    // Initialize database
    let database = rt.block_on(create_database(&config));

    // Create encryption service
    let encryption_service = encryption::EncryptionService::new(&config).expect(
        "Failed to initialize encryption service. Check your encryption key configuration.",
    );

    // Create shared chunking service
    let chunking_service = chunking::ChunkingService::new(encryption_service.clone())
        .expect("Failed to create chunking service");

    println!("Main: Chunking service created");

    // Build library manager
    let library_manager = create_library_manager(database.clone());

    // Create import service on dedicated thread
    let import_service = import_service::ImportService::new(
        library_manager.clone(),
        chunking_service.clone(),
        cloud_storage.clone(),
    );
    let import_service_handle = import_service.start();

    // Create root application context
    let app_context = AppContext {
        library_manager: library_manager.clone(),
        config: config.clone(),
        import_service_handle,
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
    cloud_storage: cloud_storage::CloudStorageManager,
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

fn make_config() -> DioxusConfig {
    DioxusConfig::default().with_window(make_window())
}

fn make_window() -> WindowBuilder {
    WindowBuilder::new()
        .with_title("bae")
        .with_always_on_top(false)
        .with_inner_size(dioxus::desktop::LogicalSize::new(1200, 800))
}
