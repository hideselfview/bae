/// Unit tests specifically for chunk reading logic
use super::*;
use crate::import::pipeline::chunk_producer::produce_chunk_stream;
use crate::import::service::DiscoveredFile;
use std::fs;
use tempfile::TempDir;

#[tokio::test]
async fn test_produce_chunk_stream_exact_integration_test_scenario() {
    // Recreate the EXACT scenario from the failing integration test:
    // - 3 files with specific sizes
    // - 1MB chunk size
    // - Files with predictable byte patterns

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let temp_path = temp_dir.path();

    let chunk_size = 1024 * 1024; // 1MB

    // Generate the same 3 files as the integration test
    let pattern1: Vec<u8> = (0..=255).collect();
    let pattern2: Vec<u8> = (0..=255).rev().collect();
    let pattern3: Vec<u8> = (0..=127).map(|i| i * 2).collect();

    // File 1: 2MB
    let file1_data = pattern1.repeat(2_097_152 / 256);
    let file1_path = temp_path.join("01_file1.flac");
    fs::write(&file1_path, &file1_data).expect("Failed to write file 1");

    // File 2: 3MB
    let file2_data = pattern2.repeat(3_145_728 / 256);
    let file2_path = temp_path.join("02_file2.flac");
    fs::write(&file2_path, &file2_data).expect("Failed to write file 2");

    // File 3: 1.5MB
    let file3_data = pattern3.repeat(1_572_864 / 128);
    let file3_path = temp_path.join("03_file3.flac");
    fs::write(&file3_path, &file3_data).expect("Failed to write file 3");

    println!("Created test files:");
    println!("  File 1: {} bytes", file1_data.len());
    println!("  File 2: {} bytes", file2_data.len());
    println!("  File 3: {} bytes", file3_data.len());

    let files = vec![
        DiscoveredFile {
            path: file1_path,
            size: 2_097_152u64,
        },
        DiscoveredFile {
            path: file2_path,
            size: 3_145_728u64,
        },
        DiscoveredFile {
            path: file3_path,
            size: 1_572_864u64,
        },
    ];

    // Create channel to receive chunks
    let (chunk_tx, mut chunk_rx) = mpsc::channel(10);

    // Spawn the chunk producer
    tokio::spawn(produce_chunk_stream(files, chunk_size, chunk_tx));

    // Collect all chunks
    let mut chunks = Vec::new();
    while let Some(result) = chunk_rx.recv().await {
        match result {
            Ok(chunk) => chunks.push(chunk),
            Err(e) => panic!("Error reading chunks: {}", e),
        }
    }

    println!("\nReceived {} chunks:", chunks.len());
    for (i, chunk) in chunks.iter().enumerate() {
        println!(
            "  Chunk {}: index={}, size={} bytes",
            i,
            chunk.chunk_index,
            chunk.data.len()
        );
    }

    // Verify expectations
    assert_eq!(chunks.len(), 7, "Should have 7 chunks total");

    // Chunks 0-1: File 1 (2MB = 2 chunks)
    assert_eq!(chunks[0].chunk_index, 0);
    assert_eq!(chunks[0].data.len(), 1_048_576, "Chunk 0 should be 1MB");
    assert_eq!(chunks[1].chunk_index, 1);
    assert_eq!(chunks[1].data.len(), 1_048_576, "Chunk 1 should be 1MB");

    // Chunks 2-4: File 2 (3MB = 3 chunks)
    assert_eq!(chunks[2].chunk_index, 2);
    assert_eq!(chunks[2].data.len(), 1_048_576, "Chunk 2 should be 1MB");
    assert_eq!(chunks[3].chunk_index, 3);
    assert_eq!(chunks[3].data.len(), 1_048_576, "Chunk 3 should be 1MB");
    assert_eq!(chunks[4].chunk_index, 4);
    assert_eq!(chunks[4].data.len(), 1_048_576, "Chunk 4 should be 1MB");

    // Chunks 5-6: File 3 (1.5MB = 2 chunks)
    assert_eq!(chunks[5].chunk_index, 5);
    assert_eq!(chunks[5].data.len(), 1_048_576, "Chunk 5 should be 1MB");
    assert_eq!(chunks[6].chunk_index, 6);

    // THIS IS THE BUG: Chunk 6 should be 524,288 bytes (0.5MB)
    // But the integration test shows it's 626,688 bytes
    println!("\nChunk 6 size: {} bytes", chunks[6].data.len());
    println!("Expected: 524,288 bytes (0.5MB)");
    println!(
        "Difference: {} bytes",
        chunks[6].data.len() as i64 - 524_288
    );

    assert_eq!(
        chunks[6].data.len(),
        524_288,
        "Chunk 6 should be exactly 0.5MB (last chunk of File 3)"
    );

    // Verify total data equals sum of file sizes
    let total_bytes: usize = chunks.iter().map(|c| c.data.len()).sum();
    let expected_total = 2_097_152 + 3_145_728 + 1_572_864;
    assert_eq!(
        total_bytes, expected_total,
        "Total bytes should equal sum of file sizes"
    );

    // Verify chunk content matches original file patterns
    println!("\nVerifying chunk content integrity...");

    // Reassemble all data
    let mut reassembled = Vec::new();
    for chunk in &chunks {
        reassembled.extend_from_slice(&chunk.data);
    }

    // Verify File 1 data
    assert_eq!(
        &reassembled[0..2_097_152],
        &file1_data[..],
        "File 1 data should match"
    );

    // Verify File 2 data
    assert_eq!(
        &reassembled[2_097_152..5_242_880],
        &file2_data[..],
        "File 2 data should match"
    );

    // Verify File 3 data
    assert_eq!(
        &reassembled[5_242_880..6_815_744],
        &file3_data[..],
        "File 3 data should match"
    );

    println!("✓ All chunk data verified!");
}

#[tokio::test]
async fn test_chunk_reading_respects_file_boundaries() {
    // Simpler test: 2 small files to verify boundary handling
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let temp_path = temp_dir.path();

    let chunk_size = 1000; // 1KB chunks for easier math

    // File 1: 1500 bytes (will span 2 chunks: 1000 + 500)
    let file1_data = vec![1u8; 1500];
    let file1_path = temp_path.join("file1.dat");
    fs::write(&file1_path, &file1_data).expect("Failed to write file 1");

    // File 2: 1200 bytes (will span 2 chunks: 500 + 1000, then 200)
    let file2_data = vec![2u8; 1200];
    let file2_path = temp_path.join("file2.dat");
    fs::write(&file2_path, &file2_data).expect("Failed to write file 2");

    let files = vec![
        DiscoveredFile {
            path: file1_path,
            size: 1500u64,
        },
        DiscoveredFile {
            path: file2_path,
            size: 1200u64,
        },
    ];

    let (chunk_tx, mut chunk_rx) = mpsc::channel(10);
    tokio::spawn(produce_chunk_stream(files, chunk_size, chunk_tx));

    let mut chunks = Vec::new();
    while let Some(result) = chunk_rx.recv().await {
        chunks.push(result.expect("Should not error"));
    }

    println!("\nChunk layout:");
    for (i, chunk) in chunks.iter().enumerate() {
        println!("  Chunk {}: {} bytes", i, chunk.data.len());
    }

    // Expected chunks:
    // Chunk 0: 1000 bytes (first 1000 of file1)
    // Chunk 1: 1000 bytes (last 500 of file1 + first 500 of file2)
    // Chunk 2: 700 bytes (last 700 of file2)

    assert_eq!(chunks.len(), 3, "Should produce 3 chunks");
    assert_eq!(chunks[0].data.len(), 1000);
    assert_eq!(chunks[1].data.len(), 1000);
    assert_eq!(chunks[2].data.len(), 700);

    // Verify content
    let mut reassembled = Vec::new();
    for chunk in &chunks {
        reassembled.extend_from_slice(&chunk.data);
    }

    // First 1500 bytes should be all 1s
    assert!(reassembled[0..1500].iter().all(|&b| b == 1));
    // Next 1200 bytes should be all 2s
    assert!(reassembled[1500..2700].iter().all(|&b| b == 2));
}
