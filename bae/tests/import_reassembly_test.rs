use bae::cache::CacheManager;
use bae::cloud_storage::{CloudStorage, CloudStorageError, CloudStorageManager};
use bae::database::Database;
use bae::encryption::EncryptionService;
use bae::import::{ImportConfig, ImportRequest, ImportService};
use bae::library::LibraryManager;
use bae::models::{DiscogsAlbum, DiscogsTrack};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

/// Mock cloud storage for integration tests
struct MockCloudStorage {
    chunks: Mutex<HashMap<String, Vec<u8>>>,
}

impl MockCloudStorage {
    fn new() -> Self {
        MockCloudStorage {
            chunks: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait::async_trait]
impl CloudStorage for MockCloudStorage {
    async fn upload_chunk(&self, chunk_id: &str, data: &[u8]) -> Result<String, CloudStorageError> {
        let location = format!(
            "s3://test-bucket/chunks/{}/{}/{}.enc",
            &chunk_id[0..2],
            &chunk_id[2..4],
            chunk_id
        );

        self.chunks
            .lock()
            .unwrap()
            .insert(location.clone(), data.to_vec());

        Ok(location)
    }

    async fn download_chunk(&self, storage_location: &str) -> Result<Vec<u8>, CloudStorageError> {
        self.chunks
            .lock()
            .unwrap()
            .get(storage_location)
            .cloned()
            .ok_or_else(|| {
                CloudStorageError::Download(format!("Chunk not found: {}", storage_location))
            })
    }
}

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

/// Load vinyl album fixture with vinyl side notation (A1-A7, B1-B9)
fn load_vinyl_album_fixture() -> DiscogsAlbum {
    use bae::models::DiscogsMaster;

    let json = std::fs::read_to_string("tests/fixtures/vinyl_master_test.json")
        .expect("Failed to read fixture");
    let master: DiscogsMaster = serde_json::from_str(&json).expect("Failed to parse fixture");

    DiscogsAlbum::Master(master)
}

/// Generate simple test files
fn generate_simple_test_files(dir: &std::path::Path) -> Vec<Vec<u8>> {
    let pattern_ascending: Vec<u8> = (0..=255).collect();
    let pattern_descending: Vec<u8> = (0..=255).rev().collect();
    let pattern_evens: Vec<u8> = (0..=127).map(|i| i * 2).collect();

    vec![
        generate_test_file(
            dir,
            "01 Track 1 - Pattern 0-255.flac",
            &pattern_ascending,
            2 * 1024 * 1024,
        )
        .1,
        generate_test_file(
            dir,
            "02 Track 2 - Pattern 255-0.flac",
            &pattern_descending,
            3 * 1024 * 1024,
        )
        .1,
        generate_test_file(
            dir,
            "03 Track 3 - Pattern Evens.flac",
            &pattern_evens,
            1536 * 1024,
        )
        .1,
    ]
}

/// Generate vinyl album test files (16 files with varied sizes + non-audio files)
fn generate_vinyl_album_files(dir: &std::path::Path) -> Vec<Vec<u8>> {
    let files = vec![
        ("01 Track A1.flac", 14_832_725),
        ("02 Track A2.flac", 36_482_083),
        ("03 Track A3.flac", 30_521_871),
        ("04 Track A4.flac", 33_719_395),
        ("05 Track A5.flac", 29_026_016),
        ("06 Track A6.flac", 35_828_979),
        ("07 Track A7.flac", 38_103_336),
        ("08 Track B1.flac", 28_602_917),
        ("09 Track B2.flac", 27_651_815),
        ("10 Track B3.flac", 17_568_354),
        ("11 Track B4.flac", 29_874_467),
        ("12 Track B5.flac", 20_314_862),
        ("13 Track B6.flac", 7_204_911),
        ("14 Track B7.flac", 32_466_724),
        ("15 Track B8.flac", 31_995_657),
        ("16 Track B9.flac", 31_599_774),
    ];

    let mut file_data = Vec::new();
    for (name, size) in files {
        let pattern: Vec<u8> = (0..=255).collect();
        let data = pattern.repeat((size / 256) + 1);
        let data = &data[0..size];
        fs::write(dir.join(name), data).unwrap();
        file_data.push(data.to_vec());
    }

    // Add non-audio files (to verify they're chunked but not mapped to tracks)
    fs::write(dir.join("album.log"), b"log data").unwrap();
    fs::write(dir.join("info.txt"), b"text data").unwrap();
    fs::write(dir.join("album.cue"), b"cue data").unwrap();
    fs::create_dir(dir.join("Artwork")).unwrap();
    fs::write(dir.join("Artwork/cover.jpg"), b"jpg data").unwrap();

    file_data
}

/// Parameterized test runner for import and reassembly
///
/// This function handles the complete import+reassembly flow and can be customized
/// with closures for verification at each stage.
async fn do_import_and_reassembly<F, G>(
    test_name: &str,
    discogs_album: DiscogsAlbum,
    generate_files: F,
    expected_track_count: usize,
    verify_tracks: G,
) where
    F: FnOnce(&std::path::Path) -> Vec<Vec<u8>>,
    G: FnOnce(&[bae::database::DbTrack]),
{
    println!("\n=== {} ===\n", test_name);

    // Setup directories
    println!("Creating temp directories...");
    let temp_root = TempDir::new().expect("Failed to create temp root");
    let album_dir = temp_root.path().join("album");
    let db_dir = temp_root.path().join("db");
    let cache_dir_path = temp_root.path().join("cache");

    std::fs::create_dir_all(&album_dir).expect("Failed to create album dir");
    std::fs::create_dir_all(&db_dir).expect("Failed to create db dir");
    std::fs::create_dir_all(&cache_dir_path).expect("Failed to create cache dir");
    println!("Directories created");

    // Generate test files
    println!("Generating test files...");
    let file_data = generate_files(&album_dir);
    println!("Generated {} files", file_data.len());

    // Setup services
    println!("Setting up services...");
    let chunk_size_bytes = 1024 * 1024;
    let mock_storage = Arc::new(MockCloudStorage::new());
    let cloud_storage = CloudStorageManager::from_storage(mock_storage.clone());

    println!("Creating database...");
    let db_file = db_dir.join("test.db");
    let database = Database::new(&format!("sqlite://{}", db_file.display()))
        .await
        .expect("Failed to create database");

    println!("Creating encryption service...");
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
    let shared_library_manager =
        bae::library_context::SharedLibraryManager::new(library_manager.clone());
    let library_manager = Arc::new(library_manager);

    let runtime_handle = tokio::runtime::Handle::current();
    let import_config = ImportConfig {
        chunk_size_bytes,
        max_encrypt_workers: 4,
        max_upload_workers: 4,
    };

    let import_handle = ImportService::start(
        runtime_handle,
        shared_library_manager,
        encryption_service.clone(),
        cloud_storage.clone(),
        import_config,
    );

    println!("Services initialized");

    // Import
    println!("Starting import...");
    println!("Sending import request...");
    let album_id = import_handle
        .send_request(ImportRequest::FromFolder {
            album: discogs_album,
            folder: album_dir.clone(),
        })
        .await
        .expect("Failed to send import request");
    println!("Request sent, got album_id: {}", album_id);

    println!("Subscribing to album progress...");
    let mut progress_rx = import_handle.subscribe_album(album_id);

    // Wait for completion
    println!("Waiting for import to complete...");
    let mut progress_count = 0;
    while let Some(progress) = progress_rx.recv().await {
        progress_count += 1;
        println!("[Progress {}] {:?}", progress_count, progress);
        if matches!(progress, bae::import::ImportProgress::Complete { .. }) {
            println!("✅ Import completed!");
            break;
        }
        if let bae::import::ImportProgress::Failed { error, .. } = progress {
            panic!("Import failed: {}", error);
        }
    }
    println!(
        "Progress monitoring ended (received {} events)",
        progress_count
    );

    // Verify database state
    println!("Verifying database...");
    let albums = library_manager
        .get_albums()
        .await
        .expect("Failed to get albums");
    assert_eq!(albums.len(), 1, "Expected 1 album");

    let tracks = library_manager
        .get_tracks(&albums[0].id)
        .await
        .expect("Failed to get tracks");

    assert_eq!(tracks.len(), expected_track_count);
    assert!(
        tracks
            .iter()
            .all(|t| t.import_status == bae::database::ImportStatus::Complete),
        "Not all tracks have Complete status"
    );

    // Run custom track verification
    verify_tracks(&tracks);

    // Verify reassembly (spot check up to first 3 tracks)
    println!("Verifying reassembly...");
    for (i, (track, expected_data)) in tracks.iter().zip(&file_data).take(3).enumerate() {
        let files = library_manager
            .get_files_for_track(&track.id)
            .await
            .expect("Failed to get files");
        assert_eq!(files.len(), 1);

        let chunks = library_manager
            .get_chunks_for_file(&files[0].id)
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

    println!("✅ Test passed!\n");
}

#[tokio::test]
async fn test_import_and_reassembly() {
    do_import_and_reassembly(
        "Simple 3-Track Album Test",
        create_test_discogs_album(),
        generate_simple_test_files,
        3,
        |tracks| {
            // Verify basic track info
            println!("Tracks:");
            for track in tracks {
                println!("  - {}", track.title);
            }
        },
    )
    .await;
}

#[tokio::test]
async fn test_vinyl_side_notation() {
    do_import_and_reassembly(
        "Vinyl Album with Side Notation (A1-A7, B1-B9)",
        load_vinyl_album_fixture(),
        generate_vinyl_album_files,
        16,
        |tracks| {
            println!("Verifying NO duplicate track numbers...");
            let mut numbers: Vec<i32> = tracks.iter().filter_map(|t| t.track_number).collect();
            numbers.sort();

            println!("Track numbers:");
            for n in &numbers {
                println!("  {}", n);
            }

            let has_dupes = numbers.windows(2).any(|w| w[0] == w[1]);

            if has_dupes {
                println!("\n⚠️  DUPLICATE TRACK NUMBERS DETECTED!");
                let mut seen = std::collections::HashSet::new();
                for num in &numbers {
                    if !seen.insert(num) {
                        println!("  Duplicate: {}", num);
                    }
                }
                panic!("FAILED: Duplicate track numbers! Vinyl sides (A1, B1) both became #1");
            }

            println!("✅ All track numbers are unique despite vinyl side notation!");
        },
    )
    .await;
}
