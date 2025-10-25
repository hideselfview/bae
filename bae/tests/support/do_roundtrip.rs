#![cfg(feature = "test-utils")]

use bae::cache::CacheManager;
use bae::cloud_storage::CloudStorageManager;
use bae::db::Database;
use bae::discogs::DiscogsAlbum;
use bae::encryption::EncryptionService;
use bae::import::{ImportConfig, ImportRequestParams, ImportService};
use bae::library::LibraryManager;
use std::sync::Arc;
use tempfile::TempDir;
use tracing::info;

use super::MockCloudStorage;

/// Parameterized test runner for import and reassembly
///
/// This function handles the complete import+reassembly flow and can be customized
/// with closures for verification at each stage.
pub async fn do_roundtrip<F, G>(
    test_name: &str,
    discogs_album: DiscogsAlbum,
    generate_files: F,
    expected_track_count: usize,
    verify_tracks: G,
) where
    F: FnOnce(&std::path::Path) -> Vec<Vec<u8>>,
    G: FnOnce(&[bae::db::DbTrack]),
{
    info!("\n=== {} ===\n", test_name);

    // Setup directories
    //////////////////////////////////////////////////////////////
    info!("Creating temp directories...");

    let temp_root = TempDir::new().expect("Failed to create temp root");
    let album_dir = temp_root.path().join("album");
    let db_dir = temp_root.path().join("db");
    let cache_dir_path = temp_root.path().join("cache");

    std::fs::create_dir_all(&album_dir).expect("Failed to create album dir");
    std::fs::create_dir_all(&db_dir).expect("Failed to create db dir");
    std::fs::create_dir_all(&cache_dir_path).expect("Failed to create cache dir");

    info!("Directories created");

    // Generate test files
    //////////////////////////////////////////////////////////////
    info!("Generating test files...");

    let file_data = generate_files(&album_dir);

    info!("Generated {} files", file_data.len());

    // Setup services
    //////////////////////////////////////////////////////////////
    info!("Setting up services...");

    let chunk_size_bytes = 1024 * 1024;
    let mock_storage = Arc::new(MockCloudStorage::new());
    let cloud_storage = CloudStorageManager::from_storage(mock_storage.clone());

    info!("Creating database...");

    let db_file = db_dir.join("test.db");
    let database = Database::new(db_file.to_str().unwrap())
        .await
        .expect("Failed to create database");

    info!("Creating encryption service...");

    let encryption_service = EncryptionService::new_with_key(vec![0u8; 32]);

    let cache_config = bae::cache::CacheConfig {
        cache_dir: cache_dir_path,
        max_size_bytes: 1024 * 1024 * 1024,
        max_chunks: 10000,
    };
    let _cache_manager = CacheManager::with_config(cache_config)
        .await
        .expect("Failed to create cache manager");

    let library_manager = LibraryManager::new(database.clone());
    let shared_library_manager = bae::library::SharedLibraryManager::new(library_manager.clone());
    let library_manager = Arc::new(library_manager);

    let runtime_handle = tokio::runtime::Handle::current();

    let import_config = ImportConfig {
        chunk_size_bytes,
        max_encrypt_workers: std::thread::available_parallelism()
            .map(|n| n.get() * 2)
            .unwrap_or(4),
        max_upload_workers: 20,
        max_db_write_workers: 10,
    };

    info!("Starting import service...");

    let import_handle = ImportService::start(
        import_config,
        runtime_handle,
        shared_library_manager,
        encryption_service.clone(),
        cloud_storage.clone(),
    );

    info!("Services initialized");

    // Send import request and subscribe for updates
    //////////////////////////////////////////////////////////////
    info!("Starting import...");
    info!("Sending import request...");

    let release_id = import_handle
        .send_request(ImportRequestParams::FromFolder {
            discogs_album,
            folder: album_dir.clone(),
        })
        .await
        .expect("Failed to send import request");

    info!("Request sent, got release_id: {}", release_id);
    info!("Subscribing to release progress...");

    let mut progress_rx = import_handle.subscribe_release(release_id);

    // Wait for completion
    info!("Waiting for import to complete...");

    let mut progress_count = 0;
    while let Some(progress) = progress_rx.recv().await {
        progress_count += 1;

        info!("[Progress {}] {:?}", progress_count, progress);

        if matches!(progress, bae::import::ImportProgress::Complete { .. }) {
            info!("✅ Import completed!");
            break;
        }
        if let bae::import::ImportProgress::Failed { error, .. } = progress {
            panic!("Import failed: {}", error);
        }
    }

    info!(
        "Progress monitoring ended (received {} events)",
        progress_count
    );

    // Verify database state
    info!("Verifying database...");

    let albums = library_manager
        .get_albums()
        .await
        .expect("Failed to get albums");
    assert_eq!(albums.len(), 1, "Expected 1 album");

    let releases = library_manager
        .get_releases_for_album(&albums[0].id)
        .await
        .expect("Failed to get releases");
    assert_eq!(releases.len(), 1, "Expected 1 release");

    let tracks = library_manager
        .get_tracks(&releases[0].id)
        .await
        .expect("Failed to get tracks");

    assert_eq!(tracks.len(), expected_track_count);
    assert!(
        tracks
            .iter()
            .all(|t| t.import_status == bae::db::ImportStatus::Complete),
        "Not all tracks have Complete status"
    );

    // Run custom track verification
    verify_tracks(&tracks);

    // Verify reassembly (spot check up to first 3 tracks)
    info!("Verifying reassembly...");

    for (i, (track, expected_data)) in tracks.iter().zip(&file_data).take(3).enumerate() {
        // Get track position to find the file
        let track_position = library_manager
            .get_track_position(&track.id)
            .await
            .expect("Failed to get track position")
            .expect("No track position found");

        let file = library_manager
            .get_file_by_id(&track_position.file_id)
            .await
            .expect("Failed to get file")
            .expect("No file found");

        let chunks = library_manager
            .get_chunks_for_file(&file.id)
            .await
            .expect("Failed to get chunks");

        let mut reassembled = Vec::new();
        for chunk in chunks {
            let encrypted = cloud_storage
                .download_chunk(&chunk.storage_location)
                .await
                .expect("Failed to download");
            let decrypted = encryption_service
                .decrypt_chunk(&encrypted)
                .expect("Failed to decrypt");
            reassembled.extend_from_slice(&decrypted);
        }

        assert_eq!(
            reassembled.len(),
            expected_data.len(),
            "Track {} size mismatch",
            i + 1
        );
        assert_eq!(
            reassembled,
            *expected_data,
            "Track {} content mismatch",
            i + 1
        );
    }

    info!("✅ Test passed!\n");
}
