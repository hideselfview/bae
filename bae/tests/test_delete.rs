#![cfg(feature = "test-utils")]

mod support;
use std::sync::Arc;
use tempfile::TempDir;

use crate::support::tracing_init;
use bae::cloud_storage::CloudStorageManager;
use bae::db::{Database, DbAlbum, DbChunk, DbRelease, DbTrack, ImportStatus};
use bae::library::{LibraryManager, SharedLibraryManager};
use bae::test_support::MockCloudStorage;
use chrono::Utc;
use uuid::Uuid;

async fn setup_test_environment() -> (
    SharedLibraryManager,
    CloudStorageManager,
    Database,
    TempDir,
    Arc<MockCloudStorage>,
) {
    tracing_init();

    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");

    let database = Database::new(db_path.to_str().unwrap())
        .await
        .expect("Failed to create database");

    let mock_storage = Arc::new(MockCloudStorage::new());
    let cloud_storage = CloudStorageManager::from_storage(mock_storage.clone());

    let library_manager = LibraryManager::new(database.clone(), cloud_storage.clone());
    let shared_library_manager = SharedLibraryManager::new(library_manager);

    (
        shared_library_manager,
        cloud_storage,
        database,
        temp_dir,
        mock_storage,
    )
}

fn create_test_album() -> DbAlbum {
    DbAlbum {
        id: Uuid::new_v4().to_string(),
        title: "Test Album".to_string(),
        year: Some(2024),
        discogs_release: None,
        bandcamp_album_id: None,
        cover_art_url: None,
        is_compilation: false,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn create_test_release(album_id: &str) -> DbRelease {
    DbRelease {
        id: Uuid::new_v4().to_string(),
        album_id: album_id.to_string(),
        release_name: None,
        year: Some(2024),
        discogs_release_id: None,
        bandcamp_release_id: None,
        import_status: ImportStatus::Complete,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn create_test_track(release_id: &str, track_number: i32) -> DbTrack {
    DbTrack {
        id: Uuid::new_v4().to_string(),
        release_id: release_id.to_string(),
        title: format!("Track {}", track_number),
        track_number: Some(track_number),
        duration_ms: Some(180000),
        discogs_position: None,
        import_status: ImportStatus::Complete,
        created_at: Utc::now(),
    }
}

async fn create_test_chunk(
    release_id: &str,
    chunk_index: i32,
    cloud_storage: &CloudStorageManager,
) -> DbChunk {
    let chunk_id = Uuid::new_v4().to_string();
    let storage_location = cloud_storage
        .upload_chunk_data(&chunk_id, &[0u8; 1024])
        .await
        .unwrap();
    DbChunk {
        id: chunk_id,
        release_id: release_id.to_string(),
        chunk_index,
        encrypted_size: 1024,
        storage_location,
        last_accessed: None,
        created_at: Utc::now(),
    }
}

#[tokio::test]
async fn test_delete_album_integration() {
    let (library_manager, cloud_storage, database, _temp_dir, _mock_storage) =
        setup_test_environment().await;

    // Create album with release, tracks, and chunks
    let album = create_test_album();
    let release = create_test_release(&album.id);
    let track1 = create_test_track(&release.id, 1);
    let track2 = create_test_track(&release.id, 2);
    let chunk1 = create_test_chunk(&release.id, 0, &cloud_storage).await;
    let chunk2 = create_test_chunk(&release.id, 1, &cloud_storage).await;

    // Insert into database
    database.insert_album(&album).await.unwrap();
    database.insert_release(&release).await.unwrap();
    database.insert_track(&track1).await.unwrap();
    database.insert_track(&track2).await.unwrap();
    database.insert_chunk(&chunk1).await.unwrap();
    database.insert_chunk(&chunk2).await.unwrap();

    // Chunks are already uploaded in create_test_chunk
    let location1 = chunk1.storage_location.clone();
    let location2 = chunk2.storage_location.clone();

    // Verify chunks exist
    assert!(cloud_storage.download_chunk(&location1).await.is_ok());
    assert!(cloud_storage.download_chunk(&location2).await.is_ok());

    // Delete album
    library_manager.get().delete_album(&album.id).await.unwrap();

    // Verify album is deleted
    let album_result = library_manager
        .get()
        .get_album_by_id(&album.id)
        .await
        .unwrap();
    assert!(album_result.is_none());

    // Verify releases are deleted
    let releases = library_manager
        .get()
        .get_releases_for_album(&album.id)
        .await
        .unwrap();
    assert!(releases.is_empty());

    // Verify tracks are deleted
    let tracks = library_manager.get().get_tracks(&release.id).await.unwrap();
    assert!(tracks.is_empty());

    // Verify chunks are deleted from cloud storage
    assert!(cloud_storage.download_chunk(&location1).await.is_err());
    assert!(cloud_storage.download_chunk(&location2).await.is_err());
}

#[tokio::test]
async fn test_delete_release_integration() {
    let (library_manager, cloud_storage, database, _temp_dir, _mock_storage) =
        setup_test_environment().await;

    // Create album with two releases
    let album = create_test_album();
    let release1 = create_test_release(&album.id);
    let release2 = create_test_release(&album.id);
    let track1 = create_test_track(&release1.id, 1);
    let track2 = create_test_track(&release2.id, 1);
    let chunk1 = create_test_chunk(&release1.id, 0, &cloud_storage).await;
    let chunk2 = create_test_chunk(&release2.id, 0, &cloud_storage).await;

    // Insert into database
    database.insert_album(&album).await.unwrap();
    database.insert_release(&release1).await.unwrap();
    database.insert_release(&release2).await.unwrap();
    database.insert_track(&track1).await.unwrap();
    database.insert_track(&track2).await.unwrap();
    database.insert_chunk(&chunk1).await.unwrap();
    database.insert_chunk(&chunk2).await.unwrap();

    // Chunks are already uploaded in create_test_chunk
    let location1 = chunk1.storage_location.clone();
    let location2 = chunk2.storage_location.clone();

    // Delete first release
    library_manager
        .get()
        .delete_release(&release1.id)
        .await
        .unwrap();

    // Verify album still exists
    let album_result = library_manager
        .get()
        .get_album_by_id(&album.id)
        .await
        .unwrap();
    assert!(album_result.is_some());

    // Verify only release2 remains
    let releases = library_manager
        .get()
        .get_releases_for_album(&album.id)
        .await
        .unwrap();
    assert_eq!(releases.len(), 1);
    assert_eq!(releases[0].id, release2.id);

    // Verify track1 is deleted, track2 still exists
    let tracks1 = library_manager
        .get()
        .get_tracks(&release1.id)
        .await
        .unwrap();
    assert!(tracks1.is_empty());
    let tracks2 = library_manager
        .get()
        .get_tracks(&release2.id)
        .await
        .unwrap();
    assert_eq!(tracks2.len(), 1);

    // Verify chunk1 is deleted from cloud storage, chunk2 still exists
    assert!(cloud_storage.download_chunk(&location1).await.is_err());
    assert!(cloud_storage.download_chunk(&location2).await.is_ok());
}

#[tokio::test]
async fn test_delete_last_release_deletes_album() {
    let (library_manager, cloud_storage, database, _temp_dir, _mock_storage) =
        setup_test_environment().await;

    // Create album with single release
    let album = create_test_album();
    let release = create_test_release(&album.id);
    let chunk = create_test_chunk(&release.id, 0, &cloud_storage).await;

    // Insert into database
    database.insert_album(&album).await.unwrap();
    database.insert_release(&release).await.unwrap();
    database.insert_chunk(&chunk).await.unwrap();

    // Delete release (should also delete album)
    library_manager
        .get()
        .delete_release(&release.id)
        .await
        .unwrap();

    // Verify album is deleted
    let album_result = library_manager
        .get()
        .get_album_by_id(&album.id)
        .await
        .unwrap();
    assert!(album_result.is_none());

    // Verify releases are deleted
    let releases = library_manager
        .get()
        .get_releases_for_album(&album.id)
        .await
        .unwrap();
    assert!(releases.is_empty());
}
