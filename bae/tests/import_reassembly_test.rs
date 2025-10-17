use bae::cache::CacheManager;
use bae::cloud_storage::mock::MockCloudStorage;
use bae::cloud_storage::CloudStorageManager;
use bae::database::Database;
use bae::encryption::EncryptionService;
use bae::import::{ImportConfig, ImportRequest, ImportService};
use bae::library::LibraryManager;
use bae::models::{DiscogsAlbum, DiscogsTrack};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;

/// Generate a test file with a repeating byte pattern
fn generate_test_file(
    dir: &std::path::Path,
    filename: &str,
    pattern: &[u8],
    size: usize,
) -> (PathBuf, Vec<u8>) {
    let file_path = dir.join(filename);
    let mut data = Vec::with_capacity(size);

    while data.len() < size {
        for &byte in pattern {
            if data.len() >= size {
                break;
            }
            data.push(byte);
        }
    }

    fs::write(&file_path, &data).expect("Failed to write test file");
    (file_path, data)
}

/// Create mock Discogs metadata for testing
fn create_test_discogs_album() -> DiscogsAlbum {
    use bae::models::DiscogsMaster;

    DiscogsAlbum::Master(DiscogsMaster {
        id: "test-master-123".to_string(),
        title: "Test Album".to_string(),
        year: Some(2024),
        thumb: None,
        label: vec!["Test Label".to_string()],
        country: Some("US".to_string()),
        tracklist: vec![
            DiscogsTrack {
                position: "1".to_string(),
                title: "Track 1 - Pattern 0-255".to_string(),
                duration: Some("3:00".to_string()),
            },
            DiscogsTrack {
                position: "2".to_string(),
                title: "Track 2 - Pattern 255-0".to_string(),
                duration: Some("4:00".to_string()),
            },
            DiscogsTrack {
                position: "3".to_string(),
                title: "Track 3 - Pattern Evens".to_string(),
                duration: Some("2:30".to_string()),
            },
        ],
    })
}

#[tokio::test]
async fn test_import_and_reassembly() {
    println!("\n=== Starting Import and Reassembly Integration Test ===\n");

    // Step 1: Generate test data
    println!("Step 1: Generating test data...");

    // Create separate directories: one for album files, one for infrastructure
    let temp_root = TempDir::new().expect("Failed to create temp root");
    let album_dir = temp_root.path().join("album");
    let db_dir = temp_root.path().join("db");
    let cache_dir_path = temp_root.path().join("cache");

    std::fs::create_dir_all(&album_dir).expect("Failed to create album dir");
    std::fs::create_dir_all(&db_dir).expect("Failed to create db dir");
    std::fs::create_dir_all(&cache_dir_path).expect("Failed to create cache dir");

    let temp_path = &album_dir;

    let pattern_ascending: Vec<u8> = (0..=255).collect();
    let pattern_descending: Vec<u8> = (0..=255).rev().collect();
    let pattern_evens: Vec<u8> = (0..=127).map(|i| i * 2).collect();

    let (_file1_path, file1_data) = generate_test_file(
        temp_path,
        "01 Track 1 - Pattern 0-255.flac",
        &pattern_ascending,
        2 * 1024 * 1024, // 2MB
    );
    let (_file2_path, file2_data) = generate_test_file(
        temp_path,
        "02 Track 2 - Pattern 255-0.flac",
        &pattern_descending,
        3 * 1024 * 1024, // 3MB
    );
    let (_file3_path, file3_data) = generate_test_file(
        temp_path,
        "03 Track 3 - Pattern Evens.flac",
        &pattern_evens,
        1536 * 1024, // 1.5MB
    );

    println!("  Generated file 1: {} bytes", file1_data.len());
    println!("  Generated file 2: {} bytes", file2_data.len());
    println!("  Generated file 3: {} bytes", file3_data.len());

    // Step 2: Setup test infrastructure
    println!("\nStep 2: Setting up test infrastructure...");

    // Test configuration
    let chunk_size_bytes = 1024 * 1024; // 1MB chunks
    let max_encrypt_workers = 4;
    let max_upload_workers = 4;

    // Create mock cloud storage
    let mock_storage = Arc::new(MockCloudStorage::new());
    let cloud_storage = CloudStorageManager::from_storage(mock_storage.clone());

    // Create temp database (in separate directory from album files)
    let db_file = db_dir.join("test.db");
    let database = Database::new(&format!("sqlite://{}", db_file.display()))
        .await
        .expect("Failed to create database");

    // Create encryption service with fixed key
    let encryption_service = EncryptionService::new_with_key(
        hex::decode("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
            .expect("Invalid hex key"),
    );

    // Create cache manager (in separate directory from album files)
    let cache_dir = cache_dir_path.clone();
    let cache_config = bae::cache::CacheConfig {
        cache_dir,
        max_size_bytes: 1024 * 1024 * 1024, // 1GB for testing
        max_chunks: 10000,
    };
    let _cache_manager = CacheManager::with_config(cache_config)
        .await
        .expect("Failed to create cache manager");

    // Create library manager
    let library_manager = LibraryManager::new(database.clone());

    // Wrap in SharedLibraryManager for ImportService
    let shared_library_manager =
        bae::library_context::SharedLibraryManager::new(library_manager.clone());

    // Also keep Arc version for direct access in test
    let library_manager = Arc::new(library_manager);

    // Create import service
    let runtime_handle = tokio::runtime::Handle::current();
    let import_config = ImportConfig {
        chunk_size_bytes,
        max_encrypt_workers,
        max_upload_workers,
    };

    let import_handle = ImportService::start(
        runtime_handle,
        shared_library_manager,
        encryption_service.clone(),
        cloud_storage.clone(),
        import_config,
    );

    println!("  All services initialized");

    // Step 3: Import test album
    println!("\nStep 3: Importing test album...");
    let discogs_album = create_test_discogs_album();

    let request = ImportRequest::FromFolder {
        album: discogs_album,
        folder: temp_path.to_path_buf(),
    };

    import_handle
        .send_request(request)
        .await
        .expect("Failed to send import request");

    // Wait a bit for import to complete
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

    println!("  Import request completed");

    // Step 4: Verify database state
    println!("\nStep 4: Verifying database state...");

    let albums = library_manager
        .get_albums()
        .await
        .expect("Failed to get albums");

    assert_eq!(albums.len(), 1, "Expected 1 album");
    let album = &albums[0];
    println!("  ✓ Album: {} by {}", album.title, album.artist_name);

    let tracks = library_manager
        .get_tracks(&album.id)
        .await
        .expect("Failed to get tracks");

    assert_eq!(tracks.len(), 3, "Expected 3 tracks");
    for (i, track) in tracks.iter().enumerate() {
        println!(
            "  ✓ Track {}: {} (status: {:?})",
            i + 1,
            track.title,
            track.import_status
        );
        assert_eq!(
            track.import_status,
            bae::database::ImportStatus::Complete,
            "Track should be complete"
        );
    }

    // Verify files and print file_chunks mappings
    for (i, track) in tracks.iter().enumerate() {
        let files = library_manager
            .get_files_for_track(&track.id)
            .await
            .expect("Failed to get files");
        assert_eq!(files.len(), 1, "Expected 1 file per track");
        println!(
            "    File {}: {} ({} bytes)",
            i + 1,
            files[0].original_filename,
            files[0].file_size
        );

        // Get chunks for this file to see what's mapped
        let file_chunks = library_manager
            .get_chunks_for_file(&files[0].id)
            .await
            .expect("Failed to get chunks for file");

        println!(
            "      → {} chunks mapped (indices: {}-{})",
            file_chunks.len(),
            file_chunks.first().map(|c| c.chunk_index).unwrap_or(-1),
            file_chunks.last().map(|c| c.chunk_index).unwrap_or(-1)
        );
    }

    // Verify chunks
    let all_chunks = library_manager
        .get_chunks_for_album(&album.id)
        .await
        .expect("Failed to get chunks");

    let total_file_size = file1_data.len() + file2_data.len() + file3_data.len();
    let expected_chunks = (total_file_size as f64 / chunk_size_bytes as f64).ceil() as usize;

    println!(
        "  Total chunks in database: {} (expected: {})",
        all_chunks.len(),
        expected_chunks
    );
    assert_eq!(all_chunks.len(), expected_chunks, "Chunk count mismatch");

    // Verify chunks are consecutive
    let mut chunk_indices: Vec<i32> = all_chunks.iter().map(|c| c.chunk_index).collect();
    chunk_indices.sort();
    for (i, &index) in chunk_indices.iter().enumerate() {
        assert_eq!(
            index, i as i32,
            "Chunk indices should be consecutive from 0"
        );
    }
    println!(
        "  ✓ Chunks are consecutively numbered from 0 to {}",
        chunk_indices.len() - 1
    );

    // Step 5: Verify MockCloudStorage contents
    println!("\nStep 5: Verifying MockCloudStorage contents...");

    let stored_chunk_count = mock_storage.chunk_count();
    println!("  Chunks in storage: {}", stored_chunk_count);
    assert_eq!(
        stored_chunk_count, expected_chunks,
        "Storage chunk count should match database"
    );

    // Verify each database chunk exists in storage
    for chunk in &all_chunks {
        let data = mock_storage.get_chunk_by_location(&chunk.storage_location);
        assert!(
            data.is_some(),
            "Chunk {} should exist in storage at {}",
            chunk.id,
            chunk.storage_location
        );
        let encrypted_data = data.unwrap();
        assert_eq!(
            encrypted_data.len() as i64,
            chunk.encrypted_size,
            "Encrypted size should match"
        );
    }
    println!("  ✓ All database chunks exist in storage with correct sizes");

    // Step 6: Reassemble and verify files
    println!("\nStep 6: Reassembling and verifying files...");

    let test_cases = vec![
        (&tracks[0], &file1_data, "Track 1"),
        (&tracks[1], &file2_data, "Track 2"),
        (&tracks[2], &file3_data, "Track 3"),
    ];

    for (track, expected_data, track_name) in test_cases {
        println!("  Reassembling {}...", track_name);

        // Get files for this track
        let files = library_manager
            .get_files_for_track(&track.id)
            .await
            .expect("Failed to get files");

        assert_eq!(files.len(), 1, "Expected 1 file per track");
        let file = &files[0];

        // Get chunks for this file
        let chunks = library_manager
            .get_chunks_for_file(&file.id)
            .await
            .expect("Failed to get chunks for file");

        println!("    File has {} chunks", chunks.len());

        // Manually reassemble (simulating playback logic)
        let mut reassembled = Vec::new();
        for (i, chunk) in chunks.iter().enumerate() {
            // Download encrypted chunk
            let encrypted_data = cloud_storage
                .download_chunk(&chunk.storage_location)
                .await
                .expect("Failed to download chunk");

            // Decrypt chunk
            let decrypted_data = encryption_service
                .decrypt_chunk(&encrypted_data)
                .expect("Failed to decrypt chunk");

            println!(
                "      Chunk {} (album index {}): {} bytes decrypted",
                i,
                chunk.chunk_index,
                decrypted_data.len()
            );
            reassembled.extend_from_slice(&decrypted_data);
        }

        println!(
            "    Reassembled {} bytes (expected {})",
            reassembled.len(),
            expected_data.len()
        );

        // Verify byte-for-byte match
        if reassembled != *expected_data {
            // Find first mismatch
            for (i, (got, expected)) in reassembled.iter().zip(expected_data.iter()).enumerate() {
                if got != expected {
                    let chunk_size = chunk_size_bytes;
                    let chunk_index = i / chunk_size;
                    let offset_in_chunk = i % chunk_size;
                    panic!(
                        "{}: First mismatch at byte {} (chunk {}, offset {}): got {}, expected {}",
                        track_name, i, chunk_index, offset_in_chunk, got, expected
                    );
                }
            }
            if reassembled.len() != expected_data.len() {
                panic!(
                    "{}: Size mismatch: got {} bytes, expected {} bytes",
                    track_name,
                    reassembled.len(),
                    expected_data.len()
                );
            }
        }

        println!("    ✓ {} matches original data perfectly", track_name);
    }

    println!("\n=== Test Passed! ===\n");
    println!("The chunking, encryption, storage, and reassembly pipeline works correctly.");
    println!("If FLAC playback is still broken, the issue is likely FLAC-specific (headers, format, etc).");
}
