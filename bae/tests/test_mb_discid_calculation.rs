use bae::import::calculate_mb_discid_from_cue;
use std::path::PathBuf;

#[test]
fn test_calculate_mb_discid_acdc_back_in_black() {
    // Use the actual CUE and FLAC files from the user's directory
    let cue_path = PathBuf::from("/Users/dima/Torrents/ACDC 1980 'Back In Black' (Canada Atlantic A2 16018)/ACDC - Back In Black.cue");
    let flac_path = PathBuf::from("/Users/dima/Torrents/ACDC 1980 'Back In Black' (Canada Atlantic A2 16018)/ACDC - Back In Black.flac");

    // Check if files exist
    if !cue_path.exists() {
        eprintln!("CUE file not found at: {:?}", cue_path);
        return;
    }
    if !flac_path.exists() {
        eprintln!("FLAC file not found at: {:?}", flac_path);
        return;
    }

    println!("🎵 Testing MusicBrainz DiscID calculation");
    println!("   CUE: {:?}", cue_path);
    println!("   FLAC: {:?}", flac_path);

    // Initialize tracing for debug output
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .try_init();

    match calculate_mb_discid_from_cue(&cue_path, &flac_path) {
        Ok(discid) => {
            println!("✅ Successfully calculated MusicBrainz DiscID: {}", discid);
            println!("   Expected format: 28-character base64-like string");

            // Verify format (MusicBrainz DiscIDs are 28 characters, base64-like)
            assert_eq!(discid.len(), 28, "DiscID should be 28 characters");
            assert!(
                discid
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
                "DiscID should contain only alphanumeric characters, dashes, and underscores"
            );

            println!("   ✓ DiscID format is valid");
            println!("   ✓ DiscID: {}", discid);
        }
        Err(e) => {
            panic!("Failed to calculate DiscID: {}", e);
        }
    }
}
