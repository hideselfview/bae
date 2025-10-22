use tracing::{error, info};

use crate::db::Database;

mod audio_processing;
mod cache;
mod cloud_storage;
mod config;
mod cue_flac;
mod db;
mod discogs;
mod encryption;
mod import;
mod library;
mod playback;
mod subsonic;
mod ui;

use library::SharedLibraryManager;
use subsonic::create_router;

/// Root application context containing all top-level dependencies
// Import UIContext from the ui module
pub use ui::AppContext;

/// Initialize cache manager
async fn create_cache_manager() -> cache::CacheManager {
    let cache_manager = cache::CacheManager::new()
        .await
        .expect("Failed to create cache manager");

    info!("Cache manager created");
    cache_manager
}

/// Initialize cloud storage from config
async fn create_cloud_storage_manager(
    config: &config::Config,
) -> cloud_storage::CloudStorageManager {
    info!("Initializing cloud storage...");

    cloud_storage::CloudStorageManager::new(config.s3_config.clone())
        .await
        .expect("Failed to initialize cloud storage. Please check your S3 configuration.")
}

/// Initialize database
async fn create_database(config: &config::Config) -> Database {
    let library_path = config.get_library_path();

    info!("Creating library directory: {}", library_path.display());

    std::fs::create_dir_all(&library_path).expect("Failed to create library directory");

    let db_path = library_path.join("library.db");

    info!("Initializing database at: {}", db_path.display());

    let database = Database::new(db_path.to_str().unwrap())
        .await
        .expect("Failed to create database");

    info!("Database created");

    database
}

/// Initialize library manager with all dependencies
fn create_library_manager(database: Database) -> SharedLibraryManager {
    let library_manager = library::LibraryManager::new(database);

    info!("Library manager created");

    let shared_library = SharedLibraryManager::new(library_manager);

    info!("SharedLibraryManager created");

    shared_library
}

fn configure_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::builder()
                .with_default_directive(tracing_subscriber::filter::LevelFilter::INFO.into())
                .parse_lossy(
                    "bae=info,sqlx=warn,aws_config=warn,aws_smithy=warn,aws_sdk_s3=warn,hyper=warn",
                ),
        )
        .with_line_number(true)
        .with_target(false)
        .with_file(true)
        .init();
}

fn main() {
    // Initialize logging with filters to suppress verbose debug logs
    configure_logging();

    // Shared tokio runtime
    let runtime = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    let runtime_handle = runtime.handle().clone();

    info!("Building dependencies...");

    // Load application configuration (handles .env loading in debug builds)
    let config = config::Config::load();

    // Initialize cache manager
    let cache_manager = runtime_handle.block_on(create_cache_manager());

    // Try to initialize cloud storage from config (optional, lazy loading)
    // This will only prompt for keyring if cloud storage is actually configured
    let cloud_storage = runtime_handle.block_on(create_cloud_storage_manager(&config));

    // Initialize database
    let database = runtime_handle.block_on(create_database(&config));

    // Create encryption service
    let encryption_service = encryption::EncryptionService::new(&config).expect(
        "Failed to initialize encryption service. Check your encryption key configuration.",
    );

    // Build library manager
    let library_manager = create_library_manager(database.clone());

    let import_config = import::ImportConfig {
        max_encrypt_workers: config.max_encrypt_workers,
        max_upload_workers: config.max_upload_workers,
        chunk_size_bytes: config.chunk_size_bytes,
    };

    // Create import service with shared runtime handle
    let import_handle = import::ImportService::start(
        import_config,
        runtime_handle.clone(),
        library_manager.clone(),
        encryption_service.clone(),
        cloud_storage.clone(),
    );

    // Create playback service
    let playback_handle = playback::PlaybackService::start(
        library_manager.get().clone(),
        cloud_storage.clone(),
        cache_manager.clone(),
        encryption_service.clone(),
        config.chunk_size_bytes,
        runtime_handle.clone(),
    );

    // Create UI context
    let ui_context = AppContext {
        library_manager: library_manager.clone(),
        config: config.clone(),
        import_handle,
        playback_handle,
    };

    // Start Subsonic API server as async task on shared runtime
    runtime_handle.spawn(async move {
        start_subsonic_server(
            cache_manager,
            library_manager,
            encryption_service,
            cloud_storage,
            config.chunk_size_bytes,
        )
        .await
    });

    // Start the desktop app (this will run in the main thread)
    // The runtime stays alive for the app's lifetime (Dioxus launch() blocks main thread)
    info!("Starting Dioxus desktop app...");

    ui::launch_app(ui_context);

    info!("Dioxus desktop app quit");
}

/// Start the Subsonic API server
async fn start_subsonic_server(
    cache_manager: cache::CacheManager,
    library_manager: SharedLibraryManager,
    encryption_service: encryption::EncryptionService,
    cloud_storage: cloud_storage::CloudStorageManager,
    chunk_size_bytes: usize,
) {
    info!("Starting Subsonic API server...");

    let app = create_router(
        library_manager,
        cache_manager,
        encryption_service,
        cloud_storage,
        chunk_size_bytes,
    );

    let listener = match tokio::net::TcpListener::bind("127.0.0.1:4533").await {
        Ok(listener) => {
            info!("Subsonic API server listening on http://127.0.0.1:4533");
            listener
        }
        Err(e) => {
            error!("Failed to bind Subsonic server: {}", e);
            return;
        }
    };

    if let Err(e) = axum::serve(listener, app).await {
        error!("Subsonic server error: {}", e);
    }
}
