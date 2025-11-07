//! Streaming playback and seeking integration test
//!
//! This test requires:
//! - `BAE_TEST_CUE_FLAC_FOLDER`: Path to a folder containing a CUE/FLAC album
//! - `BAE_TEST_DISCOGS_RELEASE_ID`: Discogs release ID for the album metadata
//! - `DISCOGS_API_KEY`: Discogs API key (optional, defaults to empty string)
//!
//! Example:
//! ```bash
//! export BAE_TEST_CUE_FLAC_FOLDER="/path/to/album/folder"
//! export BAE_TEST_DISCOGS_RELEASE_ID="2270893"
//! export DISCOGS_API_KEY="your-api-key"
//! cargo test --test test_streaming_seek --release -- --ignored --nocapture
//! ```

use bae::cache::CacheManager;
use bae::cloud_storage::{CloudStorageManager, S3Config};
use bae::config::Config;
use bae::db::Database;
use bae::discogs::client::DiscogsClient;
use bae::encryption::EncryptionService;
use bae::import::{ImportConfig, ImportRequestParams, ImportService};
use bae::library::{LibraryManager, SharedLibraryManager};
use bae::playback::{PlaybackHandle, PlaybackProgress, PlaybackService, PlaybackState};
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;
use uuid::Uuid;

/// Test a single seek operation and verify completion
async fn test_single_seek(
    playback_handle: &PlaybackHandle,
    progress_rx: &mut mpsc::UnboundedReceiver<PlaybackProgress>,
    seek_pos: Duration,
    label: &str,
) -> bool {
    tracing::info!("Seeking to {}s ({})...", seek_pos.as_secs(), label);

    playback_handle.seek(seek_pos);

    // Wait for seek to complete (or timeout after 30 seconds)
    let mut seek_completed = false;
    let seek_timeout = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            match progress_rx.recv().await {
                Some(PlaybackProgress::Seeking {
                    requested_position, ..
                }) => {
                    tracing::info!(
                        "Seeking state received for position {}s ({})",
                        requested_position.as_secs(),
                        label
                    );
                }
                Some(PlaybackProgress::Seeked { position, .. }) => {
                    tracing::info!(
                        "Seek completed to position {}s ({})",
                        position.as_secs(),
                        label
                    );

                    // Verify we're close to the requested position (within 5 seconds)
                    let diff = if position > seek_pos {
                        position - seek_pos
                    } else {
                        seek_pos - position
                    };

                    if diff > Duration::from_secs(5) {
                        tracing::error!(
                            "Seek position mismatch: requested {}s, got {}s (diff: {}s) [{}]",
                            seek_pos.as_secs(),
                            position.as_secs(),
                            diff.as_secs(),
                            label
                        );
                    } else {
                        tracing::info!("Seek position verified (within 5s tolerance) [{}]", label);
                    }

                    seek_completed = true;
                    break;
                }
                Some(progress) => {
                    tracing::debug!("Received progress during seek: {:?}", progress);
                }
                None => {
                    tracing::error!("Progress channel closed during seek");
                    break;
                }
            }
        }
    })
    .await;

    if !seek_timeout.is_ok() || !seek_completed {
        tracing::error!(
            "Seek to {}s did not complete within 30 seconds [{}]",
            seek_pos.as_secs(),
            label
        );
        return false;
    }

    true
}

#[tokio::test]
#[ignore] // Requires Discogs API and actual files
async fn test_streaming_seek() {
    // Setup logging (reads from RUST_LOG env var, defaults to info level)
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!("Starting streaming seek test");

    // Setup test environment
    let test_dir = std::env::temp_dir().join("bae_test_streaming_seek");
    std::fs::create_dir_all(&test_dir).unwrap();

    let db_path = test_dir.join("test.db");
    let database = Database::new(db_path.to_str().unwrap())
        .await
        .expect("Failed to create DB");

    let library_manager = SharedLibraryManager::new(LibraryManager::new(database));

    // Setup config
    let config = Config::load();

    // Generate unique bucket name for this test run
    let test_bucket_name = format!("bae-test-{}", Uuid::new_v4().to_string().replace("-", ""));

    // Create modified S3 config with unique bucket
    let test_s3_config = S3Config {
        bucket_name: test_bucket_name.clone(),
        ..config.s3_config.clone()
    };

    // Setup cloud storage with unique bucket (will be created automatically)
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

    // Setup import service
    let runtime = tokio::runtime::Handle::current();
    let chunk_size_bytes = 1024 * 1024; // 1MB
    let import_config = ImportConfig {
        chunk_size_bytes,
        max_encrypt_workers: 4,
        max_upload_workers: 4,
        max_db_write_workers: 2,
    };
    let import_handle = ImportService::start(
        import_config,
        runtime.clone(),
        library_manager.clone(),
        encryption_service.clone(),
        cloud_storage.clone(),
    );

    // Check if test folder exists
    let folder_path = std::env::var("BAE_TEST_CUE_FLAC_FOLDER")
        .map(PathBuf::from)
        .expect("BAE_TEST_CUE_FLAC_FOLDER environment variable must be set");
    if !folder_path.exists() {
        tracing::warn!("Test folder does not exist, skipping test");
        return;
    }

    // Fetch Discogs release
    let discogs_client = DiscogsClient::new(std::env::var("DISCOGS_API_KEY").unwrap_or_default());
    let discogs_release_id = std::env::var("BAE_TEST_DISCOGS_RELEASE_ID")
        .expect("BAE_TEST_DISCOGS_RELEASE_ID environment variable must be set");
    let discogs_release = discogs_client
        .get_release(&discogs_release_id)
        .await
        .expect("Failed to fetch Discogs release");

    tracing::info!("Fetched release: {}", discogs_release.title);

    // Import
    let params = ImportRequestParams::FromFolder {
        discogs_release,
        folder: folder_path,
        master_year: 1970,
    };

    let (_album_id, release_id) = import_handle
        .send_request(params)
        .await
        .expect("Import request failed");

    tracing::info!("Import started, waiting for completion...");

    // Wait for import to complete (poll status via tracks)
    let mut attempts = 0;
    loop {
        sleep(Duration::from_secs(2)).await;
        attempts += 1;

        let tracks = library_manager
            .get()
            .get_tracks(&release_id)
            .await
            .expect("Failed to get tracks");

        let all_complete = tracks
            .iter()
            .all(|t| matches!(t.import_status, bae::db::ImportStatus::Complete));
        let any_failed = tracks
            .iter()
            .any(|t| matches!(t.import_status, bae::db::ImportStatus::Failed));

        if all_complete {
            tracing::info!("✅ Import completed!");
            break;
        }
        if any_failed {
            panic!("Import failed");
        }
        if attempts > 120 {
            panic!("Import timed out after 4 minutes");
        }
    }

    // Get tracks
    let tracks = library_manager
        .get()
        .get_tracks(&release_id)
        .await
        .expect("Failed to get tracks");

    tracing::info!("Found {} tracks", tracks.len());
    assert!(!tracks.is_empty(), "No tracks imported");

    // Get first track with sufficient duration for testing
    let track = tracks
        .iter()
        .find(|t| t.duration_ms.unwrap_or(0) > 240_000) // At least 4 minutes
        .or_else(|| tracks.first())
        .unwrap();

    let track_duration = Duration::from_millis(track.duration_ms.unwrap_or(0) as u64);
    tracing::info!(
        "Testing with track: {} (duration: {}s)",
        track.title,
        track_duration.as_secs()
    );

    // Start playback service
    // PlaybackService needs LibraryManager (not Arc), so we clone it
    let playback_handle = PlaybackService::start(
        library_manager.get().clone(),
        cloud_storage.clone(),
        cache_manager.clone(),
        encryption_service.clone(),
        chunk_size_bytes,
        runtime.clone(),
    );

    // Subscribe to progress updates
    let mut progress_rx = playback_handle.subscribe_progress();

    // Play the track
    tracing::info!("Starting playback...");
    playback_handle.play(track.id.clone());

    // Wait for playback to start
    let mut playback_started = false;
    let timeout = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            match progress_rx.recv().await {
                Some(PlaybackProgress::StateChanged { state }) => {
                    tracing::debug!("State changed: {:?}", state);
                    if matches!(state, PlaybackState::Playing { .. }) {
                        playback_started = true;
                        break;
                    }
                }
                Some(progress) => {
                    tracing::debug!("Received progress: {:?}", progress);
                }
                None => {
                    tracing::error!("Progress channel closed");
                    break;
                }
            }
        }
    })
    .await;

    assert!(
        timeout.is_ok() && playback_started,
        "Playback did not start within 30 seconds"
    );
    tracing::info!("✅ Playback started successfully");

    // Wait a bit for initial buffering
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Test 1: Basic sequential seeks
    tracing::info!("\n=== Test 1: Sequential seeks ===");
    let seek_positions = vec![
        Duration::from_secs(30),
        Duration::from_secs(60),
        Duration::from_secs(120),
        Duration::from_secs(track_duration.as_secs() / 2), // Middle
        Duration::from_secs(track_duration.as_secs().saturating_sub(30)), // Near end
    ];

    for seek_pos in seek_positions {
        if seek_pos >= track_duration {
            tracing::info!(
                "Skipping seek to {}s (beyond track duration)",
                seek_pos.as_secs()
            );
            continue;
        }

        if !test_single_seek(&playback_handle, &mut progress_rx, seek_pos, "sequential").await {
            panic!("Sequential seek to {}s failed", seek_pos.as_secs());
        }

        // Wait between seeks to let playback stabilize
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    tracing::info!("✅ Sequential seeks completed successfully");

    // Test 2: Rapid consecutive seeks (seeking while seeking)
    tracing::info!("\n=== Test 2: Rapid consecutive seeks (seeking while seeking) ===");
    let rapid_seeks = vec![
        Duration::from_secs(100),
        Duration::from_secs(200),
        Duration::from_secs(300),
    ];

    for seek_pos in rapid_seeks {
        if seek_pos >= track_duration {
            continue;
        }

        tracing::info!(
            "Rapid seek to {}s (not waiting for completion)...",
            seek_pos.as_secs()
        );
        playback_handle.seek(seek_pos);
        // Don't wait - immediately queue next seek
    }

    // Wait for final seek to complete
    let final_pos = Duration::from_secs(300.min(track_duration.as_secs().saturating_sub(10)));
    let mut final_seek_completed = false;
    let timeout = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            match progress_rx.recv().await {
                Some(PlaybackProgress::Seeked { position, .. }) => {
                    tracing::info!("Rapid seek completed at position {}s", position.as_secs());
                    // Check if we're near the final position
                    let diff = if position > final_pos {
                        position - final_pos
                    } else {
                        final_pos - position
                    };
                    if diff < Duration::from_secs(10) {
                        final_seek_completed = true;
                        break;
                    }
                }
                Some(progress) => {
                    tracing::debug!("Progress during rapid seek: {:?}", progress);
                }
                None => break,
            }
        }
    })
    .await;

    assert!(
        timeout.is_ok() && final_seek_completed,
        "Rapid seeks did not stabilize within 30 seconds"
    );
    tracing::info!("✅ Rapid consecutive seeks completed");

    // Test 3: Backward seeks
    tracing::info!("\n=== Test 3: Backward seeks ===");
    let backward_seeks = vec![
        Duration::from_secs(200),
        Duration::from_secs(100),
        Duration::from_secs(50),
        Duration::from_secs(10),
    ];

    for seek_pos in backward_seeks {
        if seek_pos >= track_duration {
            continue;
        }

        if !test_single_seek(&playback_handle, &mut progress_rx, seek_pos, "backward").await {
            panic!("Backward seek to {}s failed", seek_pos.as_secs());
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    tracing::info!("✅ Backward seeks completed successfully");

    // Test 4: Seek to same position twice
    tracing::info!("\n=== Test 4: Seek to same position twice ===");
    let same_pos = Duration::from_secs(150.min(track_duration.as_secs().saturating_sub(30)));

    if !test_single_seek(&playback_handle, &mut progress_rx, same_pos, "first").await {
        panic!("First seek to {}s failed", same_pos.as_secs());
    }

    tokio::time::sleep(Duration::from_millis(500)).await;

    if !test_single_seek(&playback_handle, &mut progress_rx, same_pos, "duplicate").await {
        panic!("Duplicate seek to {}s failed", same_pos.as_secs());
    }

    tracing::info!("✅ Duplicate seek test completed");

    // Test 5: Seek far ahead to position where chunks aren't loaded
    // This reproduces the issue where Symphonia probe reads past the seek target
    tracing::info!("\n=== Test 5: Seek far ahead (chunks not loaded) ===");

    // Seek very far ahead from current position (should require loading many chunks we don't have yet)
    let far_ahead_pos = Duration::from_secs((track_duration.as_secs() * 2 / 3).min(300));
    tracing::info!(
        "Seeking far ahead to {}s from current position (chunks not yet loaded)...",
        far_ahead_pos.as_secs()
    );

    if !test_single_seek(
        &playback_handle,
        &mut progress_rx,
        far_ahead_pos,
        "far-ahead",
    )
    .await
    {
        panic!("Far ahead seek to {}s failed", far_ahead_pos.as_secs());
    }

    tracing::info!("✅ Far ahead seek test completed");

    // Test 6: Seek backward to beginning
    tracing::info!("\n=== Test 6: Seek backward to beginning ===");

    if !test_single_seek(
        &playback_handle,
        &mut progress_rx,
        Duration::from_secs(0),
        "to-beginning",
    )
    .await
    {
        panic!("Seek to beginning (0s) failed");
    }

    tracing::info!("✅ Seek to beginning test completed");

    tracing::info!("\n✅ All seek tests completed successfully!");

    // Stop playback
    playback_handle.stop();
    tracing::info!("Test completed");
}
