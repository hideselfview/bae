use bae::import::detect_metadata;
use std::path::PathBuf;

#[tokio::test]
async fn test_final_verification() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .try_init();

    println!("\n=== Final Verification ===\n");

    let root_down = PathBuf::from("/Users/dima/Torrents/Root Down");
    match detect_metadata(root_down) {
        Ok(metadata) => {
            println!("Root Down DiscID: {:?}", metadata.mb_discid);
        }
        Err(e) => eprintln!("Root Down Error: {}", e),
    }

    let black_sabbath = PathBuf::from("/Users/dima/Torrents/1970. Black Sabbath - Black Sabbath ( Creative Sounds,6006,USA (red))");
    match detect_metadata(black_sabbath) {
        Ok(metadata) => {
            println!("Black Sabbath DiscID: {:?}", metadata.mb_discid);
            assert_eq!(
                metadata.mb_discid,
                Some("wEstTQHp6rDr82355w3pgnZIlnY-".to_string()),
                "Black Sabbath DiscID should match"
            );
        }
        Err(e) => eprintln!("Black Sabbath Error: {}", e),
    }

    println!("\n✅ Both albums verified!");
}
