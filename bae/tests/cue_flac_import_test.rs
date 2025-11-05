use bae::cache::CacheManager;
use bae::cloud_storage::{CloudStorageManager, S3Config};
use bae::config::Config;
use bae::db::Database;
use bae::discogs::client::DiscogsClient;
use bae::encryption::EncryptionService;
use bae::import::{ImportConfig, ImportRequestParams, ImportService};
use bae::library::{LibraryManager, SharedLibraryManager};
use bae::playback::reassembly::reassemble_track;
use std::path::PathBuf;
use tokio::time::{sleep, Duration};
use uuid::Uuid;

#[tokio::test]
#[ignore] // Requires Discogs API and actual files
async fn test_black_sabbath_cue_flac_import() {
    // Setup logging (reads from RUST_LOG env var, defaults to warn,bae=debug)
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn,bae=debug")),
        )
        .with_test_writer()
        .init();

    // Setup test environment
    let test_dir = std::env::temp_dir().join("bae_test_black_sabbath");
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
    println!("Using test bucket: {}", test_bucket_name);

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
    let folder_path = PathBuf::from("/Users/dima/Torrents/1970. Black Sabbath - Black Sabbath ( Creative Sounds,6006,USA (red))");
    if !folder_path.exists() {
        eprintln!("Test folder not found: {}", folder_path.display());
        eprintln!("Skipping test");
        return;
    }

    // Fetch Discogs release
    let discogs_client = DiscogsClient::new(std::env::var("DISCOGS_API_KEY").unwrap_or_default());

    let discogs_release = discogs_client
        .get_release("2270893")
        .await
        .expect("Failed to fetch Discogs release");

    println!("Fetched Discogs release: {}", discogs_release.title);

    // Import
    let params = ImportRequestParams::FromFolder {
        discogs_release,
        folder: folder_path,
        master_year: 1970,
    };

    println!("Sending import request...");
    let (album_id, release_id) = import_handle
        .send_request(params)
        .await
        .expect("Import request failed");

    println!("Import queued: album={}, release={}", album_id, release_id);

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

        println!(
            "Import status: {} tracks, all_complete={}, any_failed={} (attempt {})",
            tracks.len(),
            all_complete,
            any_failed,
            attempts
        );

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

    println!("Import completed!");

    // Verify tracks
    let tracks = library_manager
        .get()
        .get_tracks(&release_id)
        .await
        .expect("Failed to get tracks");

    assert_eq!(tracks.len(), 5, "Should have 5 tracks");

    println!("\n=== Track Details ===");
    for track in &tracks {
        println!(
            "Track {}: {} - duration: {:?}ms",
            track.track_number.unwrap_or(0),
            track.title,
            track.duration_ms
        );
        assert!(track.duration_ms.is_some(), "Track should have duration");

        // Get chunk coordinates
        if let Some(coords) = library_manager
            .get()
            .get_track_chunk_coords(&track.id)
            .await
            .expect("Failed to get chunk coords")
        {
            println!(
                "  Chunks: {}-{}, Byte offsets: {}-{}",
                coords.start_chunk_index,
                coords.end_chunk_index,
                coords.start_byte_offset,
                coords.end_byte_offset
            );
        }
    }

    // Extract tracks and write FLAC files
    println!("\n=== Extracting Tracks ===");
    let output_dir = test_dir.join("extracted_tracks");
    std::fs::create_dir_all(&output_dir).unwrap();

    for track in &tracks {
        let track_num = track.track_number.unwrap_or(0);
        let filename = format!("{:02}.flac", track_num);
        let output_path = output_dir.join(&filename);

        println!("Extracting track {}: {}...", track_num, track.title);

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

        println!(
            "  ✓ Wrote {} bytes to {}",
            audio_data.len(),
            output_path.display()
        );
    }

    println!(
        "\n✓ Test passed! Extracted {} tracks to {}",
        tracks.len(),
        output_dir.display()
    );

    // Cleanup
    // Note: We keep the test bucket for debugging (can be manually cleaned up later)
    // std::fs::remove_dir_all(&test_dir).ok();
}
