//! CUE/FLAC import integration test
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
//! cargo test --test cue_flac_import_test --release -- --ignored --nocapture
//! ```

use bae::cache::CacheManager;
use bae::cloud_storage::{CloudStorageManager, S3Config};
use bae::config::Config;
use bae::db::Database;
use bae::discogs::client::DiscogsClient;
use bae::encryption::EncryptionService;
use bae::import::{ImportConfig, ImportRequest, ImportService};
use bae::library::{LibraryManager, SharedLibraryManager};
use bae::playback::reassembly::reassemble_track;
use std::path::PathBuf;
use tokio::time::{sleep, Duration};
use uuid::Uuid;

#[tokio::test]
#[ignore] // Requires Discogs API and actual files
async fn test_cue_flac_import() {
    // Setup logging (reads from RUST_LOG env var, defaults to info level)
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // Setup test environment
    let test_dir = std::env::temp_dir().join("bae_test_cue_flac");
    std::fs::create_dir_all(&test_dir).unwrap();

    let db_path = test_dir.join("test.db");
    let database = Database::new(db_path.to_str().unwrap())
        .await
        .expect("Failed to create DB");

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

    let library_manager =
        SharedLibraryManager::new(LibraryManager::new(database, cloud_storage.clone()));

    // Setup encryption
    let encryption_service =
        EncryptionService::new(&config).expect("Failed to create encryption service");

    // Setup cache manager (for reassembly)
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

    // Import
    let params = ImportRequest::Folder {
        discogs_release,
        folder: folder_path,
        master_year: 1970,
    };

    let (_album_id, release_id) = import_handle
        .send_request(params)
        .await
        .expect("Import request failed");

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
            break;
        }
        if any_failed {
            panic!("Import failed");
        }
        if attempts > 120 {
            panic!("Import timed out after 4 minutes");
        }
    }

    // Verify tracks
    let tracks = library_manager
        .get()
        .get_tracks(&release_id)
        .await
        .expect("Failed to get tracks");

    assert_eq!(tracks.len(), 5, "Should have 5 tracks");

    for track in &tracks {
        assert!(track.duration_ms.is_some(), "Track should have duration");
    }

    // Extract tracks and write FLAC files
    let output_dir = test_dir.join("extracted_tracks");
    std::fs::create_dir_all(&output_dir).unwrap();

    for track in &tracks {
        let track_num = track.track_number.unwrap_or(0);
        let filename = format!("{:02}.flac", track_num);
        let output_path = output_dir.join(&filename);

        // Reassemble track using the same logic as playback
        let audio_data = reassemble_track(
            &track.id,
            library_manager.get(),
            &cloud_storage,
            &cache_manager,
            &encryption_service,
            chunk_size_bytes,
        )
        .await
        .unwrap_or_else(|e| panic!("Failed to reassemble track {}: {}", track_num, e));

        // Validate FLAC header
        assert!(
            audio_data.len() >= 4,
            "Audio data too small: {} bytes",
            audio_data.len()
        );
        assert_eq!(
            &audio_data[0..4],
            b"fLaC",
            "Invalid FLAC header for track {}",
            track_num
        );

        // Write FLAC file
        std::fs::write(&output_path, &audio_data).unwrap_or_else(|e| {
            panic!("Failed to write FLAC file {}: {}", output_path.display(), e)
        });
    }

    // Cleanup
    // Note: We keep the test bucket for debugging (can be manually cleaned up later)
    // std::fs::remove_dir_all(&test_dir).ok();
}
