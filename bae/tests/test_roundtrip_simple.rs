#![cfg(feature = "test-utils")]

mod support;
use std::{fs, path::PathBuf};

use bae::models::{DiscogsAlbum, DiscogsTrack};
use tracing::info;

use crate::support::{do_roundtrip, tracing_init};

#[tokio::test]
async fn test_roundtrip_simple() {
    tracing_init();

    do_roundtrip(
        "Simple 3-Track Album Test",
        create_test_discogs_album(),
        generate_simple_test_files,
        3,
        |tracks| {
            // Verify basic track info
            info!("Tracks:");
            for track in tracks {
                info!("  - {}", track.title);
            }
        },
    )
    .await;
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
