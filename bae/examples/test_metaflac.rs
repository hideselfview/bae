/// Test metaflac library to see if it can help us

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let flac_path = std::env::args()
        .nth(1)
        .expect("Usage: test_metaflac <flac_file>");

    // Try to read FLAC metadata
    let mut tag = metaflac::Tag::read_from_path(&flac_path)?;

    // Get STREAMINFO
    if let Some(streaminfo) = tag.get_streaminfo() {
        println!("STREAMINFO:");
        println!("  Sample rate: {}", streaminfo.sample_rate);
        println!("  Channels: {}", streaminfo.num_channels);
        println!("  Bits per sample: {}", streaminfo.bits_per_sample);
        println!("  Total samples: {}", streaminfo.total_samples);
        println!(
            "  Duration: {:.2}s",
            streaminfo.total_samples as f64 / streaminfo.sample_rate as f64
        );
    }

    // Check for seektable
    let blocks: Vec<_> = tag.blocks().collect();
    println!("\nMetadata blocks: {}", blocks.len());
    for (i, block) in blocks.iter().enumerate() {
        match block {
            metaflac::Block::StreamInfo(si) => println!("  Block {}: STREAMINFO", i),
            metaflac::Block::SeekTable(st) => {
                println!("  Block {}: SEEKTABLE ({} points)", i, st.seekpoints.len())
            }
            metaflac::Block::VorbisComment(_) => println!("  Block {}: VORBIS_COMMENT", i),
            metaflac::Block::Picture(_) => println!("  Block {}: PICTURE", i),
            _ => println!("  Block {}: Other", i),
        }
    }

    Ok(())
}
