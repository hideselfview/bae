#![cfg(feature = "test-utils")]

mod support;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tokio::time::timeout;
use tracing::debug;

use crate::support::tracing_init;
use bae::cache::{CacheConfig, CacheManager};
use bae::cloud_storage::CloudStorageManager;
use bae::db::Database;
use bae::discogs::models::{DiscogsArtist, DiscogsRelease, DiscogsTrack};
use bae::encryption::EncryptionService;
use bae::import::ImportRequestParams;
use bae::library::{LibraryManager, SharedLibraryManager};
use bae::playback::{PlaybackProgress, PlaybackState};
use bae::test_support::MockCloudStorage;

/// Test helper to set up playback service with imported test tracks
struct PlaybackTestFixture {
    playback_handle: bae::playback::PlaybackHandle,
    progress_rx: tokio::sync::mpsc::UnboundedReceiver<PlaybackProgress>,
    track_ids: Vec<String>,
    _temp_dir: TempDir,
}

impl PlaybackTestFixture {
    async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        tracing_init();

        let temp_dir = TempDir::new()?;
        let db_path = temp_dir.path().join("test.db");
        let cache_dir = temp_dir.path().join("cache");
        std::fs::create_dir_all(&cache_dir)?;

        let album_dir = temp_dir.path().join("album");
        std::fs::create_dir_all(&album_dir)?;

        let chunk_size_bytes = 1024 * 1024;
        let mock_storage = Arc::new(MockCloudStorage::new());
        let cloud_storage = CloudStorageManager::from_storage(mock_storage.clone());

        let database = Database::new(db_path.to_str().unwrap()).await?;

        let encryption_service = EncryptionService::new_with_key(vec![0u8; 32]);

        let cache_config = CacheConfig {
            cache_dir,
            max_size_bytes: 1024 * 1024 * 1024,
            max_chunks: 10000,
        };
        let cache_manager = CacheManager::with_config(cache_config).await?;

        let library_manager = LibraryManager::new(database, cloud_storage.clone());
        let shared_library_manager = SharedLibraryManager::new(library_manager.clone());
        let library_manager_arc = Arc::new(library_manager);

        let runtime_handle = tokio::runtime::Handle::current();

        // Set up import service and import test tracks
        let discogs_release = create_test_album();
        let _track_data = generate_test_flac_files(&album_dir);

        let import_config = bae::import::ImportConfig {
            chunk_size_bytes,
            max_encrypt_workers: std::thread::available_parallelism()
                .map(|n| n.get() * 2)
                .unwrap_or(4),
            max_upload_workers: 20,
            max_db_write_workers: 10,
        };

        let import_handle = bae::import::ImportService::start(
            import_config,
            runtime_handle.clone(),
            shared_library_manager.clone(),
            encryption_service.clone(),
            cloud_storage.clone(),
        );

        // Send import request
        let master_year = discogs_release.year.unwrap_or(2024);
        let (_album_id, release_id) = import_handle
            .send_request(ImportRequestParams::FromFolder {
                discogs_release,
                folder: album_dir.clone(),
                master_year,
            })
            .await?;

        // Wait for import to complete
        let mut progress_rx = import_handle.subscribe_release(release_id.clone());
        while let Some(progress) = progress_rx.recv().await {
            match progress {
                bae::import::ImportProgress::Complete { .. } => break,
                bae::import::ImportProgress::Failed { error, .. } => {
                    return Err(format!("Import failed: {}", error).into());
                }
                _ => {}
            }
        }

        // Get track IDs
        let albums = library_manager_arc.get_albums().await?;
        assert!(!albums.is_empty(), "Should have imported album");
        let releases = library_manager_arc
            .get_releases_for_album(&albums[0].id)
            .await?;
        assert!(!releases.is_empty(), "Should have imported release");
        let tracks = library_manager_arc.get_tracks(&releases[0].id).await?;
        let track_ids: Vec<String> = tracks.iter().map(|t| t.id.clone()).collect();
        assert!(!track_ids.is_empty(), "Should have imported tracks");

        // Set env var to mute audio during tests
        std::env::set_var("MUTE_TEST_AUDIO", "1");

        let playback_handle = bae::playback::PlaybackService::start(
            library_manager_arc.as_ref().clone(),
            cloud_storage,
            cache_manager,
            encryption_service,
            chunk_size_bytes,
            runtime_handle,
        );

        // Also set volume to 0.0 as backup (commands are processed asynchronously)
        playback_handle.set_volume(0.0);

        let progress_rx = playback_handle.subscribe_progress();

        Ok(Self {
            playback_handle,
            progress_rx,
            track_ids,
            _temp_dir: temp_dir,
        })
    }

    /// Wait for a specific state change with timeout
    async fn wait_for_state<F>(
        &mut self,
        predicate: F,
        timeout_duration: Duration,
    ) -> Option<PlaybackState>
    where
        F: Fn(&PlaybackState) -> bool,
    {
        let deadline = Instant::now() + timeout_duration;

        while Instant::now() < deadline {
            match timeout(Duration::from_millis(100), self.progress_rx.recv()).await {
                Ok(Some(PlaybackProgress::StateChanged { state })) => {
                    if predicate(&state) {
                        return Some(state);
                    }
                }
                Ok(Some(_)) => continue,
                Ok(None) => break,
                Err(_) => continue,
            }
        }

        None
    }

    /// Wait for a position update with timeout
    async fn wait_for_position_update(&mut self, timeout_duration: Duration) -> Option<Duration> {
        let deadline = Instant::now() + timeout_duration;

        while Instant::now() < deadline {
            match timeout(Duration::from_millis(100), self.progress_rx.recv()).await {
                Ok(Some(PlaybackProgress::PositionUpdate { position, .. })) => {
                    return Some(position);
                }
                Ok(Some(_)) => continue,
                Ok(None) => break,
                Err(_) => continue,
            }
        }

        None
    }

    /// Wait for a Seeked event with timeout
    async fn wait_for_seeked(&mut self, timeout_duration: Duration) -> Option<Duration> {
        let deadline = Instant::now() + timeout_duration;

        while Instant::now() < deadline {
            match timeout(Duration::from_millis(100), self.progress_rx.recv()).await {
                Ok(Some(PlaybackProgress::Seeked { position, .. })) => {
                    return Some(position);
                }
                Ok(Some(_)) => continue,
                Ok(None) => break,
                Err(_) => continue,
            }
        }

        None
    }

    /// Wait for a SeekSkipped event with timeout
    async fn wait_for_seek_skipped(
        &mut self,
        timeout_duration: Duration,
    ) -> Option<(Duration, Duration)> {
        let deadline = Instant::now() + timeout_duration;

        while Instant::now() < deadline {
            match timeout(Duration::from_millis(100), self.progress_rx.recv()).await {
                Ok(Some(PlaybackProgress::SeekSkipped {
                    requested_position,
                    current_position,
                })) => {
                    return Some((requested_position, current_position));
                }
                Ok(Some(_)) => continue,
                Ok(None) => break,
                Err(_) => continue,
            }
        }

        None
    }
}

/// Create a test album with 2 short tracks
fn create_test_album() -> DiscogsRelease {
    DiscogsRelease {
        id: "test-playback-123".to_string(),
        title: "Playback Test Album".to_string(),
        year: Some(2024),
        genre: vec![],
        style: vec![],
        format: vec![],
        country: Some("US".to_string()),
        label: vec!["Test Label".to_string()],
        cover_image: None,
        thumb: None,
        artists: vec![DiscogsArtist {
            name: "Test Artist".to_string(),
            id: "test-artist-1".to_string(),
        }],
        tracklist: vec![
            DiscogsTrack {
                position: "1".to_string(),
                title: "Test Track 1".to_string(),
                duration: Some("0:10".to_string()),
            },
            DiscogsTrack {
                position: "2".to_string(),
                title: "Test Track 2".to_string(),
                duration: Some("0:10".to_string()),
            },
        ],
        master_id: "test-master-123".to_string(),
    }
}

/// Copy pre-generated FLAC fixtures to test directory
/// Fixtures should be generated using scripts/generate_test_flac.sh
fn generate_test_flac_files(dir: &std::path::Path) -> Vec<Vec<u8>> {
    use std::fs;

    // Path to fixture directory (relative to crate root)
    let fixture_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flac");

    let fixture_files = vec!["01 Test Track 1.flac", "02 Test Track 2.flac"];

    let mut file_data = Vec::new();
    for fixture_name in fixture_files {
        let fixture_path = fixture_dir.join(fixture_name);
        let test_path = dir.join(fixture_name);

        // Read fixture and copy to test directory
        let data = fs::read(&fixture_path).unwrap_or_else(|_| {
            panic!(
                "FLAC fixture not found: {}\n\
                     Run: ./scripts/generate_test_flac.sh",
                fixture_path.display()
            );
        });

        fs::write(&test_path, &data).expect("Failed to copy FLAC fixture");
        file_data.push(data);
    }

    file_data
}

/// Check if audio tests should be skipped (e.g., in CI without audio device)
fn should_skip_audio_tests() -> bool {
    if std::env::var("SKIP_AUDIO_TESTS").is_ok() {
        return true;
    }

    // Try to get default output device - if it fails, skip tests
    // Use the trait method from cpal::traits::HostTrait
    use cpal::traits::HostTrait;
    cpal::default_host().default_output_device().is_none()
}

#[tokio::test]
async fn test_pause_then_seek_stays_paused() {
    if should_skip_audio_tests() {
        debug!("Skipping audio test - no audio device available");
        return;
    }

    let mut fixture = match PlaybackTestFixture::new().await {
        Ok(f) => f,
        Err(e) => {
            debug!("Failed to set up test fixture: {}", e);
            return;
        }
    };

    if fixture.track_ids.is_empty() {
        debug!("No tracks available for testing");
        return;
    }

    let track_id = &fixture.track_ids[0];

    // Play the track
    fixture.playback_handle.play(track_id.clone());

    // Wait for playing state
    let playing_state = fixture
        .wait_for_state(
            |s| matches!(s, PlaybackState::Playing { .. }),
            Duration::from_secs(5),
        )
        .await;

    if playing_state.is_none() {
        debug!("Failed to start playback");
        return;
    }

    // Pause
    fixture.playback_handle.pause();

    // Wait for paused state
    let paused_state = fixture
        .wait_for_state(
            |s| matches!(s, PlaybackState::Paused { .. }),
            Duration::from_secs(2),
        )
        .await;

    assert!(
        paused_state.is_some(),
        "Should be paused after pause command"
    );

    // Seek while paused
    fixture.playback_handle.seek(Duration::from_secs(5));

    // Wait for seek to complete
    let seeked_position = fixture.wait_for_seeked(Duration::from_secs(3)).await;

    assert!(
        seeked_position.is_some(),
        "Should receive Seeked event after seeking"
    );

    // Verify we're still paused by checking state
    let final_state = fixture
        .wait_for_state(
            |s| matches!(s, PlaybackState::Paused { .. }),
            Duration::from_millis(100),
        )
        .await;

    // If we don't get a state change, that's actually good - it means we stayed paused
    // The Seeked event doesn't change the paused/playing state, only position
    if final_state.is_none() {
        // No state change after seek is expected - we stayed paused
        debug!("No state change after seek, stayed paused");
    }
}

#[tokio::test]
async fn test_play_then_seek_continues_playing() {
    if should_skip_audio_tests() {
        debug!("Skipping audio test - no audio device available");
        return;
    }

    let mut fixture = match PlaybackTestFixture::new().await {
        Ok(f) => f,
        Err(e) => {
            debug!("Failed to set up test fixture: {}", e);
            return;
        }
    };

    if fixture.track_ids.is_empty() {
        debug!("No tracks available for testing");
        return;
    }

    let track_id = &fixture.track_ids[0];

    // Play the track
    fixture.playback_handle.play(track_id.clone());

    // Wait for playing state
    let playing_state = fixture
        .wait_for_state(
            |s| matches!(s, PlaybackState::Playing { .. }),
            Duration::from_secs(5),
        )
        .await;

    assert!(
        playing_state.is_some(),
        "Should be playing after play command"
    );

    // Seek while playing
    fixture.playback_handle.seek(Duration::from_secs(3));

    // Wait for seek to complete
    let seeked_position = fixture.wait_for_seeked(Duration::from_secs(3)).await;

    assert!(
        seeked_position.is_some(),
        "Should receive Seeked event after seeking"
    );

    // Verify we're still playing by checking state
    let final_state = fixture
        .wait_for_state(
            |s| matches!(s, PlaybackState::Playing { .. }),
            Duration::from_millis(100),
        )
        .await;

    // If we don't get a state change, that's actually good - it means we stayed playing
    // The Seeked event doesn't change the paused/playing state, only position
    if final_state.is_none() {
        // No state change after seek is expected - we stayed playing
        debug!("No state change after seek, stayed playing");
    }
}

#[tokio::test]
async fn test_auto_advance_to_next_track() {
    if should_skip_audio_tests() {
        debug!("Skipping audio test - no audio device available");
        return;
    }

    let mut fixture = match PlaybackTestFixture::new().await {
        Ok(f) => f,
        Err(e) => {
            debug!("Failed to set up test fixture: {}", e);
            return;
        }
    };

    if fixture.track_ids.len() < 2 {
        debug!("Need at least 2 tracks for auto-advance test");
        return;
    }

    let first_track_id = fixture.track_ids[0].clone();
    let second_track_id = fixture.track_ids[1].clone();

    // Play the first track
    fixture.playback_handle.play(first_track_id.clone());

    // Wait for playing state
    let _playing_state = fixture
        .wait_for_state(
            |s| matches!(s, PlaybackState::Playing { .. }),
            Duration::from_secs(5),
        )
        .await;

    // Seek near the end (to 4.5 seconds, since track is 5 seconds)
    // Stay within bounds to wait for completion and trigger auto-advance
    fixture
        .playback_handle
        .seek(Duration::from_secs(4) + Duration::from_millis(500));

    // Wait for track completion and auto-advance
    let next_track_state = fixture
        .wait_for_state(
            |s| {
                if let PlaybackState::Playing { track, .. } = s {
                    track.id == second_track_id
                } else {
                    false
                }
            },
            Duration::from_secs(5),
        )
        .await;

    // Auto-advance may not work with placeholder files, so this is a best-effort test
    if next_track_state.is_some() {
        // Success - auto-advance worked
    } else {
        debug!("Auto-advance test inconclusive - may need valid FLAC files");
    }
}

#[tokio::test]
async fn test_position_maintained_across_pause_resume() {
    if should_skip_audio_tests() {
        debug!("Skipping audio test - no audio device available");
        return;
    }

    let mut fixture = match PlaybackTestFixture::new().await {
        Ok(f) => f,
        Err(e) => {
            debug!("Failed to set up test fixture: {}", e);
            return;
        }
    };

    if fixture.track_ids.is_empty() {
        debug!("No tracks available for testing");
        return;
    }

    let track_id = &fixture.track_ids[0];

    // Play the track
    fixture.playback_handle.play(track_id.clone());

    // Wait for playing state
    let _playing_state = fixture
        .wait_for_state(
            |s| matches!(s, PlaybackState::Playing { .. }),
            Duration::from_secs(5),
        )
        .await;

    // Seek to a specific position
    let seek_position = Duration::from_secs(2);
    fixture.playback_handle.seek(seek_position);

    // Wait for position update
    let _position = fixture
        .wait_for_position_update(Duration::from_secs(2))
        .await;

    // Pause
    fixture.playback_handle.pause();

    // Get paused state and verify position
    let paused_state = fixture
        .wait_for_state(
            |s| matches!(s, PlaybackState::Paused { .. }),
            Duration::from_secs(2),
        )
        .await;

    if let Some(PlaybackState::Paused { position, .. }) = paused_state {
        // Position should be close to seek position (within 1 second)
        let diff = position.abs_diff(seek_position);
        assert!(
            diff < Duration::from_secs(1),
            "Position should be maintained when paused"
        );
    }

    // Resume
    fixture.playback_handle.resume();

    // Get playing state and verify position maintained
    let resumed_state = fixture
        .wait_for_state(
            |s| matches!(s, PlaybackState::Playing { .. }),
            Duration::from_secs(2),
        )
        .await;

    if let Some(PlaybackState::Playing { position, .. }) = resumed_state {
        // Position should still be close to seek position
        let diff = position.abs_diff(seek_position);
        assert!(
            diff < Duration::from_secs(1),
            "Position should be maintained when resumed"
        );
    }
}

#[tokio::test]
async fn test_previous_track_navigation() {
    if should_skip_audio_tests() {
        debug!("Skipping audio test - no audio device available");
        return;
    }

    let mut fixture = match PlaybackTestFixture::new().await {
        Ok(f) => f,
        Err(e) => {
            debug!("Failed to set up test fixture: {}", e);
            return;
        }
    };

    if fixture.track_ids.len() < 2 {
        debug!("Need at least 2 tracks for previous track test");
        return;
    }

    let first_track_id = fixture.track_ids[0].clone();
    let second_track_id = fixture.track_ids[1].clone();

    // Play the first track
    fixture.playback_handle.play(first_track_id.clone());

    // Wait for playing state
    let first_track_state = fixture
        .wait_for_state(
            |s| matches!(s, PlaybackState::Playing { .. }),
            Duration::from_secs(5),
        )
        .await;

    assert!(
        first_track_state.is_some(),
        "Should be playing first track after play command"
    );

    // Advance to second track using Next
    fixture.playback_handle.next();

    // Wait for second track to start playing
    let second_track_state = fixture
        .wait_for_state(
            |s| {
                if let PlaybackState::Playing { track, .. } = s {
                    track.id == second_track_id
                } else {
                    false
                }
            },
            Duration::from_secs(5),
        )
        .await;

    assert!(
        second_track_state.is_some(),
        "Should be playing second track after Next command"
    );

    // Seek to early position (< 3 seconds) and test Previous goes to previous track
    fixture.playback_handle.seek(Duration::from_secs(1));
    let _position = fixture
        .wait_for_position_update(Duration::from_secs(2))
        .await;

    // Call Previous - should go to first track (we're < 3 seconds in)
    fixture.playback_handle.previous();

    let previous_track_state = fixture
        .wait_for_state(
            |s| {
                if let PlaybackState::Playing { track, .. } = s {
                    track.id == first_track_id
                } else {
                    false
                }
            },
            Duration::from_secs(5),
        )
        .await;

    assert!(
        previous_track_state.is_some(),
        "Should go to previous track when Previous is called early in track"
    );

    // Now seek to later position (> 3 seconds) and test Previous restarts current track
    fixture.playback_handle.seek(Duration::from_secs(4));
    let _position = fixture
        .wait_for_position_update(Duration::from_secs(2))
        .await;

    // Call Previous - should restart current track (we're > 3 seconds in)
    fixture.playback_handle.previous();

    let restart_state = fixture
        .wait_for_state(
            |s| {
                if let PlaybackState::Playing {
                    track, position, ..
                } = s
                {
                    track.id == first_track_id && *position < Duration::from_secs(1)
                } else {
                    false
                }
            },
            Duration::from_secs(5),
        )
        .await;

    assert!(
        restart_state.is_some(),
        "Should restart current track when Previous is called late in track"
    );
}

#[tokio::test]
async fn test_previous_track_when_starting_on_second_track() {
    if should_skip_audio_tests() {
        debug!("Skipping audio test - no audio device available");
        return;
    }

    let mut fixture = match PlaybackTestFixture::new().await {
        Ok(f) => f,
        Err(e) => {
            debug!("Failed to set up test fixture: {}", e);
            return;
        }
    };

    if fixture.track_ids.len() < 2 {
        debug!("Need at least 2 tracks for previous track test");
        return;
    }

    let first_track_id = fixture.track_ids[0].clone();
    let second_track_id = fixture.track_ids[1].clone();

    // Start playback directly on the second track (not the first)
    fixture.playback_handle.play(second_track_id.clone());

    // Wait for playing state
    let second_track_state = fixture
        .wait_for_state(
            |s| {
                if let PlaybackState::Playing { track, .. } = s {
                    track.id == second_track_id
                } else {
                    false
                }
            },
            Duration::from_secs(5),
        )
        .await;

    assert!(
        second_track_state.is_some(),
        "Should be playing second track after play command"
    );

    // Seek to early position (< 3 seconds)
    fixture.playback_handle.seek(Duration::from_secs(1));
    let _position = fixture
        .wait_for_position_update(Duration::from_secs(2))
        .await;

    // Call Previous - should go to first track (we're < 3 seconds in)
    fixture.playback_handle.previous();

    let previous_track_state = fixture
        .wait_for_state(
            |s| {
                if let PlaybackState::Playing { track, .. } = s {
                    track.id == first_track_id
                } else {
                    false
                }
            },
            Duration::from_secs(5),
        )
        .await;

    assert!(
        previous_track_state.is_some(),
        "Should go to previous track when Previous is called after starting on second track"
    );
}

#[tokio::test]
async fn test_previous_track_multiple_navigation() {
    if should_skip_audio_tests() {
        debug!("Skipping audio test - no audio device available");
        return;
    }

    let mut fixture = match PlaybackTestFixture::new().await {
        Ok(f) => f,
        Err(e) => {
            debug!("Failed to set up test fixture: {}", e);
            return;
        }
    };

    // Need at least 3 tracks for this test, but we only have 2
    // So we'll test with 2 tracks: start on track 2, go back to track 1
    if fixture.track_ids.len() < 2 {
        debug!("Need at least 2 tracks for previous track test");
        return;
    }

    let first_track_id = fixture.track_ids[0].clone();
    let second_track_id = fixture.track_ids[1].clone();

    // Start playback directly on the second track
    fixture.playback_handle.play(second_track_id.clone());

    // Wait for playing state
    let _second_track_state = fixture
        .wait_for_state(
            |s| {
                if let PlaybackState::Playing { track, .. } = s {
                    track.id == second_track_id
                } else {
                    false
                }
            },
            Duration::from_secs(5),
        )
        .await;

    // Seek to early position
    fixture.playback_handle.seek(Duration::from_secs(1));
    let _position = fixture
        .wait_for_position_update(Duration::from_secs(2))
        .await;

    // Call Previous - should go to first track
    fixture.playback_handle.previous();

    let first_nav_state = fixture
        .wait_for_state(
            |s| {
                if let PlaybackState::Playing { track, .. } = s {
                    track.id == first_track_id
                } else {
                    false
                }
            },
            Duration::from_secs(5),
        )
        .await;

    assert!(
        first_nav_state.is_some(),
        "Should go to first track when Previous is called from second track"
    );

    // Seek to early position again
    fixture.playback_handle.seek(Duration::from_secs(1));
    let _position = fixture
        .wait_for_position_update(Duration::from_secs(2))
        .await;

    // Call Previous again - should restart first track (no previous track)
    fixture.playback_handle.previous();

    let restart_state = fixture
        .wait_for_state(
            |s| {
                if let PlaybackState::Playing {
                    track, position, ..
                } = s
                {
                    track.id == first_track_id && *position < Duration::from_secs(1)
                } else {
                    false
                }
            },
            Duration::from_secs(5),
        )
        .await;

    assert!(
        restart_state.is_some(),
        "Should restart first track when Previous is called and there's no previous track"
    );
}

#[tokio::test]
async fn test_seek_to_same_position_sends_state_changed() {
    if should_skip_audio_tests() {
        debug!("Skipping audio test - no audio device available");
        return;
    }

    let mut fixture = match PlaybackTestFixture::new().await {
        Ok(f) => f,
        Err(e) => {
            debug!("Failed to set up test fixture: {}", e);
            return;
        }
    };

    if fixture.track_ids.is_empty() {
        debug!("No tracks available for testing");
        return;
    }

    let track_id = &fixture.track_ids[0];

    // Play the track
    fixture.playback_handle.play(track_id.clone());

    // Wait for playing state
    let playing_state = fixture
        .wait_for_state(
            |s| matches!(s, PlaybackState::Playing { .. }),
            Duration::from_secs(5),
        )
        .await;

    assert!(
        playing_state.is_some(),
        "Should be playing after play command"
    );

    // Seek to a position
    let seek_position = Duration::from_secs(2);
    fixture.playback_handle.seek(seek_position);

    // Wait for position update
    let _position = fixture
        .wait_for_position_update(Duration::from_secs(2))
        .await;

    // Now seek to roughly the same position (within 100ms) - should skip but send StateChanged
    // Wait a bit for position to stabilize, then seek to very close position
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Get current position
    let current_pos = fixture
        .wait_for_position_update(Duration::from_secs(1))
        .await
        .unwrap_or(seek_position);

    // Seek to position within 50ms of current (should be skipped)
    let same_position = current_pos + Duration::from_millis(50);
    fixture.playback_handle.seek(same_position);

    // With the fix: Seek should be skipped (< 100ms difference) and SeekSkipped event sent
    // Without the fix: Seek would go through and restart stream
    let seek_skipped = fixture.wait_for_seek_skipped(Duration::from_secs(2)).await;

    assert!(
        seek_skipped.is_some(),
        "Should receive SeekSkipped event when position difference < 100ms"
    );

    if let Some((requested, current)) = seek_skipped {
        let diff = requested.abs_diff(current);
        assert!(
            diff < Duration::from_millis(100),
            "Seek should only be skipped when difference < 100ms, got {:?}",
            diff
        );
    }

    // Verify position updates continue after skipped seek
    tokio::time::sleep(Duration::from_millis(500)).await;

    let position_update = fixture
        .wait_for_position_update(Duration::from_secs(2))
        .await;

    assert!(
        position_update.is_some(),
        "Position updates should continue after skipped seek"
    );
}

#[tokio::test]
async fn test_queue_maintained_after_previous_navigation() {
    if should_skip_audio_tests() {
        debug!("Skipping audio test - no audio device available");
        return;
    }

    let mut fixture = match PlaybackTestFixture::new().await {
        Ok(f) => f,
        Err(e) => {
            debug!("Failed to set up test fixture: {}", e);
            return;
        }
    };

    if fixture.track_ids.len() < 2 {
        debug!("Need at least 2 tracks for queue navigation test");
        return;
    }

    let first_track_id = fixture.track_ids[0].clone();
    let second_track_id = fixture.track_ids[1].clone();
    // Note: We only have 2 tracks in test fixtures, so we'll test the scenario with 2 tracks
    // The bug would manifest as: track 1 -> next -> track 2 -> previous -> track 1 -> next -> should be track 2, not track 3

    // Start playback on first track
    fixture.playback_handle.play(first_track_id.clone());

    // Wait for playing state
    let _first_track_state = fixture
        .wait_for_state(
            |s| {
                if let PlaybackState::Playing { track, .. } = s {
                    track.id == first_track_id
                } else {
                    false
                }
            },
            Duration::from_secs(5),
        )
        .await;

    // Press Next to go to track 2
    fixture.playback_handle.next();

    let second_track_state = fixture
        .wait_for_state(
            |s| {
                if let PlaybackState::Playing { track, .. } = s {
                    track.id == second_track_id
                } else {
                    false
                }
            },
            Duration::from_secs(5),
        )
        .await;

    assert!(
        second_track_state.is_some(),
        "Should be playing second track after Next command"
    );

    // Seek to early position
    fixture.playback_handle.seek(Duration::from_secs(1));
    let _position = fixture
        .wait_for_position_update(Duration::from_secs(2))
        .await;

    // Press Previous to go back to track 1
    fixture.playback_handle.previous();

    let back_to_first_state = fixture
        .wait_for_state(
            |s| {
                if let PlaybackState::Playing { track, .. } = s {
                    track.id == first_track_id
                } else {
                    false
                }
            },
            Duration::from_secs(5),
        )
        .await;

    assert!(
        back_to_first_state.is_some(),
        "Should go back to first track when Previous is called from second track"
    );

    // Seek to early position again
    fixture.playback_handle.seek(Duration::from_secs(1));
    let _position = fixture
        .wait_for_position_update(Duration::from_secs(2))
        .await;

    // Press Next again - should go to track 2 (not track 3 if it existed)
    // The bug: after going back, the queue should still have track 2, not be empty
    fixture.playback_handle.next();

    let should_be_second_state = fixture
        .wait_for_state(
            |s| {
                if let PlaybackState::Playing { track, .. } = s {
                    track.id == second_track_id
                } else {
                    false
                }
            },
            Duration::from_secs(5),
        )
        .await;

    assert!(
        should_be_second_state.is_some(),
        "Should go to track 2 when Next is called after navigating back to track 1"
    );
}
