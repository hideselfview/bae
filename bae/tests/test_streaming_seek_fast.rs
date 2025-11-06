//! Fast streaming seek test harness
//!
//! This test uses a pre-existing database and S3 bucket to quickly test seek functionality
//! without running a full import every time.
//!
//! Environment variables (all required):
//! - `BAE_TEST_DB_PATH`: Path to existing test database (defaults to `test_streaming_seek_fast.db` in temp dir)
//! - `BAE_TEST_S3_BUCKET`: S3 bucket name with existing data
//! - `BAE_TEST_TRACK_ID`: Track ID to test
//!
//! Example:
//! ```bash
//! export BAE_TEST_DB_PATH="/path/to/test.db"
//! export BAE_TEST_S3_BUCKET="bae-test-abc123"
//! export BAE_TEST_TRACK_ID="some-track-id"
//! cargo test --test test_streaming_seek_fast --release -- --nocapture
//! ```
//!
//! To get a track ID, run the full import test first, or query your database:
//! ```sql
//! SELECT id, title FROM tracks LIMIT 1;
//! ```

use bae::cache::CacheManager;
use bae::cloud_storage::{CloudStorageManager, S3Config};
use bae::config::Config;
use bae::db::Database;
use bae::encryption::EncryptionService;
use bae::library::{LibraryManager, SharedLibraryManager};
use bae::playback::{PlaybackProgress, PlaybackService, PlaybackState};
use std::time::Duration;

#[tokio::test]
async fn test_seek_fast() {
    // Setup logging
    tracing_log::LogTracer::init().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("debug")),
        )
        .try_init()
        .ok();

    tracing::info!("Starting fast streaming seek test");

    // Get or create database path
    let db_path = std::env::var("BAE_TEST_DB_PATH").unwrap_or_else(|_| {
        std::env::temp_dir()
            .join("test_streaming_seek_fast.db")
            .to_str()
            .unwrap()
            .to_string()
    });

    tracing::info!("Using database: {}", db_path);

    // Open existing database
    let database = Database::new(&db_path)
        .await
        .expect("Failed to open database");

    let library_manager = SharedLibraryManager::new(LibraryManager::new(database));

    // Get S3 bucket from env (required)
    let bucket_name = std::env::var("BAE_TEST_S3_BUCKET")
        .expect("BAE_TEST_S3_BUCKET environment variable must be set");

    tracing::info!("Using S3 bucket: {}", bucket_name);

    // Setup config
    let config = Config::load();

    // Create S3 config with specified bucket
    let test_s3_config = S3Config {
        bucket_name: bucket_name.clone(),
        ..config.s3_config.clone()
    };

    // Setup cloud storage
    let cloud_storage = CloudStorageManager::new(test_s3_config)
        .await
        .expect("Failed to create cloud storage");

    // Setup encryption
    let encryption_service =
        EncryptionService::new(&config).expect("Failed to create encryption service");

    // Setup cache manager
    let cache_manager = CacheManager::new()
        .await
        .expect("Failed to create cache manager");

    // Get track to test (required)
    let track_id = std::env::var("BAE_TEST_TRACK_ID")
        .expect("BAE_TEST_TRACK_ID environment variable must be set");

    tracing::info!("Using track ID: {}", track_id);

    // Verify track exists and is complete
    let track = library_manager
        .get()
        .get_track(&track_id)
        .await
        .expect("Failed to get track")
        .expect("Track not found");

    if !matches!(track.import_status, bae::db::ImportStatus::Complete) {
        panic!("Track is not complete. Run the full import test first.");
    }

    let track_duration = Duration::from_millis(track.duration_ms.unwrap_or(0) as u64);
    tracing::info!("Track duration: {:?}", track_duration);

    // Setup playback service
    let runtime = tokio::runtime::Handle::current();
    let chunk_size_bytes = 1024 * 1024; // 1MB

    // Set test mode to use mock audio output
    std::env::set_var("BAE_TEST_MODE", "1");

    let playback_handle = PlaybackService::start(
        library_manager.get().clone(),
        cloud_storage.clone(),
        cache_manager.clone(),
        encryption_service.clone(),
        chunk_size_bytes,
        runtime.clone(),
    );

    let mut progress_rx = playback_handle.subscribe_progress();

    // Start playback
    tracing::info!("Starting playback of track: {}", track_id);
    playback_handle.play(track_id.clone());

    // Wait for playback to start
    let mut playback_started = false;
    let start_timeout = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match progress_rx.recv().await {
                Some(PlaybackProgress::StateChanged { state }) => {
                    if matches!(state, PlaybackState::Playing { .. }) {
                        tracing::info!("Playback started");
                        playback_started = true;
                        break;
                    }
                }
                Some(PlaybackProgress::SeekError { .. }) => {
                    panic!("Playback error during start");
                }
                Some(_) => {
                    // Ignore other progress updates
                }
                None => {
                    panic!("Progress channel closed");
                }
            }
        }
    })
    .await;

    if start_timeout.is_err() || !playback_started {
        panic!("Playback did not start within 10 seconds");
    }

    // Wait a bit for playback to stabilize
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Test a single seek
    let seek_pos = Duration::from_secs(120);
    tracing::info!("Testing seek to {}s", seek_pos.as_secs());

    playback_handle.seek(seek_pos);

    // Wait for seek to complete
    let seek_timeout = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            match progress_rx.recv().await {
                Some(PlaybackProgress::Seeking { .. }) => {
                    tracing::info!("Seek in progress...");
                }
                Some(PlaybackProgress::Seeked { position, .. }) => {
                    tracing::info!("Seek completed to position {}s", position.as_secs());

                    let diff = position.abs_diff(seek_pos);

                    if diff > Duration::from_secs(5) {
                        panic!(
                            "Seek position mismatch: requested {}s, got {}s (diff: {}s)",
                            seek_pos.as_secs(),
                            position.as_secs(),
                            diff.as_secs()
                        );
                    } else {
                        tracing::info!("✅ Seek position verified (within 5s tolerance)");
                    }
                    break;
                }
                Some(PlaybackProgress::SeekSkipped {
                    current_position, ..
                }) => {
                    tracing::info!(
                        "Seek skipped - already at position {}s",
                        current_position.as_secs()
                    );
                    break;
                }
                Some(PlaybackProgress::SeekError { .. }) => {
                    panic!("Seek error");
                }
                Some(_) => {
                    // Ignore other progress updates
                }
                None => {
                    panic!("Progress channel closed");
                }
            }
        }
    })
    .await;

    if seek_timeout.is_err() {
        panic!("Seek did not complete within 30 seconds");
    }

    tracing::info!("✅ Fast seek test completed successfully");
}
