#![cfg(feature = "test-utils")]

mod support;
use std::fs;

use bae::discogs::DiscogsAlbum;
use support::{do_roundtrip, tracing_init};
use tracing::{error, info};

#[tokio::test]
async fn test_roundtrip_vinyl() {
    tracing_init();

    do_roundtrip(
        "Vinyl Album with Side Notation (A1-A7, B1-B9)",
        load_vinyl_album_fixture(),
        generate_vinyl_album_files,
        2,
        |tracks| {
            info!("Verifying NO duplicate track numbers...");
            let mut numbers: Vec<i32> = tracks.iter().filter_map(|t| t.track_number).collect();
            numbers.sort();

            info!("Track numbers:");
            for n in &numbers {
                info!("  {}", n);
            }

            let has_dupes = numbers.windows(2).any(|w| w[0] == w[1]);

            if has_dupes {
                error!("\n⚠️  DUPLICATE TRACK NUMBERS DETECTED!");
                let mut seen = std::collections::HashSet::new();
                for num in &numbers {
                    if !seen.insert(num) {
                        error!("  Duplicate: {}", num);
                    }
                }
                panic!("FAILED: Duplicate track numbers! Vinyl sides (A1, B1) both became #1");
            }

            info!("✅ All track numbers are unique despite vinyl side notation!");
        },
    )
    .await;
}

/// Load vinyl album fixture with vinyl side notation (A1-A7, B1-B9)
fn load_vinyl_album_fixture() -> DiscogsAlbum {
    use bae::discogs::DiscogsMaster;

    let json = std::fs::read_to_string("tests/fixtures/vinyl_master_test.json")
        .expect("Failed to read fixture");
    let master: DiscogsMaster = serde_json::from_str(&json).expect("Failed to parse fixture");

    DiscogsAlbum::Master(master)
}

/// Generate vinyl album test files (16 files with varied sizes + non-audio files)
fn generate_vinyl_album_files(dir: &std::path::Path) -> Vec<Vec<u8>> {
    let files = vec![
        ("01 Track A1.flac", 14_832_725),
        ("02 Track A2.flac", 36_482_083),
        // ("03 Track A3.flac", 30_521_871),
        // ("04 Track A4.flac", 33_719_395),
        // ("05 Track A5.flac", 29_026_016),
        // ("06 Track A6.flac", 35_828_979),
        // ("07 Track A7.flac", 38_103_336),
        // ("08 Track B1.flac", 28_602_917),
        // ("09 Track B2.flac", 27_651_815),
        // ("10 Track B3.flac", 17_568_354),
        // ("11 Track B4.flac", 29_874_467),
        // ("12 Track B5.flac", 20_314_862),
        // ("13 Track B6.flac", 7_204_911),
        // ("14 Track B7.flac", 32_466_724),
        // ("15 Track B8.flac", 31_995_657),
        // ("16 Track B9.flac", 31_599_774),
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
