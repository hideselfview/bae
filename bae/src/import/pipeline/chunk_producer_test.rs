// Tests for chunk producer module
//
// These tests verify the chunk reading and production logic in isolation.
// They test file reading, chunking, and error handling scenarios.

use super::chunk_producer::*;
use crate::import::types::DiscoveredFile;
use std::fs;
use tempfile::TempDir;
use tokio::sync::mpsc;

#[tokio::test]
async fn test_produce_chunk_stream_single_file_exact_size() {
    // Test reading a single file that's exactly the chunk size
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let temp_path = temp_dir.path();

    let chunk_size = 1024; // 1KB chunks
    let file_data = vec![42u8; chunk_size]; // Exactly 1KB of data
    let file_path = temp_path.join("test.dat");
    fs::write(&file_path, &file_data).expect("Failed to write test file");

    let files = vec![DiscoveredFile {
        path: file_path,
        size: chunk_size as u64,
    }];

    let (chunk_tx, mut chunk_rx) = mpsc::channel(10);
    tokio::spawn(produce_chunk_stream(files, chunk_size, chunk_tx));

    let mut chunks = Vec::new();
    while let Some(result) = chunk_rx.recv().await {
        chunks.push(result.expect("Should not error"));
    }

    assert_eq!(chunks.len(), 1, "Should produce exactly 1 chunk");
    assert_eq!(chunks[0].chunk_index, 0);
    assert_eq!(chunks[0].data.len(), chunk_size);
    assert_eq!(chunks[0].data, file_data);
    assert!(!chunks[0].chunk_id.is_empty(), "Chunk should have an ID");
}

#[tokio::test]
async fn test_produce_chunk_stream_single_file_smaller_than_chunk() {
    // Test reading a single file smaller than chunk size
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let temp_path = temp_dir.path();

    let chunk_size = 1024;
    let file_data = vec![99u8; 500]; // 500 bytes, smaller than chunk
    let file_path = temp_path.join("small.dat");
    fs::write(&file_path, &file_data).expect("Failed to write test file");

    let files = vec![DiscoveredFile {
        path: file_path,
        size: 500u64,
    }];

    let (chunk_tx, mut chunk_rx) = mpsc::channel(10);
    tokio::spawn(produce_chunk_stream(files, chunk_size, chunk_tx));

    let mut chunks = Vec::new();
    while let Some(result) = chunk_rx.recv().await {
        chunks.push(result.expect("Should not error"));
    }

    assert_eq!(chunks.len(), 1, "Should produce exactly 1 chunk");
    assert_eq!(chunks[0].chunk_index, 0);
    assert_eq!(chunks[0].data.len(), 500);
    assert_eq!(chunks[0].data, file_data);
}

#[tokio::test]
async fn test_produce_chunk_stream_single_file_larger_than_chunk() {
    // Test reading a single file larger than chunk size
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let temp_path = temp_dir.path();

    let chunk_size = 1024;
    let file_data = vec![77u8; 2500]; // 2.5KB, spans 3 chunks
    let file_path = temp_path.join("large.dat");
    fs::write(&file_path, &file_data).expect("Failed to write test file");

    let files = vec![DiscoveredFile {
        path: file_path,
        size: 2500u64,
    }];

    let (chunk_tx, mut chunk_rx) = mpsc::channel(10);
    tokio::spawn(produce_chunk_stream(files, chunk_size, chunk_tx));

    let mut chunks = Vec::new();
    while let Some(result) = chunk_rx.recv().await {
        chunks.push(result.expect("Should not error"));
    }

    assert_eq!(chunks.len(), 3, "Should produce 3 chunks");

    // First two chunks should be full size
    assert_eq!(chunks[0].chunk_index, 0);
    assert_eq!(chunks[0].data.len(), chunk_size);
    assert_eq!(chunks[1].chunk_index, 1);
    assert_eq!(chunks[1].data.len(), chunk_size);

    // Last chunk should be remainder
    assert_eq!(chunks[2].chunk_index, 2);
    assert_eq!(chunks[2].data.len(), 452); // 2500 - 2*1024 = 452

    // Verify data integrity
    let mut reassembled = Vec::new();
    for chunk in &chunks {
        reassembled.extend_from_slice(&chunk.data);
    }
    assert_eq!(reassembled, file_data);
}

#[tokio::test]
async fn test_produce_chunk_stream_multiple_files() {
    // Test reading multiple files as continuous stream
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let temp_path = temp_dir.path();

    let chunk_size = 1000; // 1KB chunks for easier math

    // File 1: 1500 bytes (spans 2 chunks: 1000 + 500)
    let file1_data = vec![1u8; 1500];
    let file1_path = temp_path.join("file1.dat");
    fs::write(&file1_path, &file1_data).expect("Failed to write file 1");

    // File 2: 1200 bytes (spans 2 chunks: 500 + 1000, then 200)
    let file2_data = vec![2u8; 1200];
    let file2_path = temp_path.join("file2.dat");
    fs::write(&file2_path, &file2_data).expect("Failed to write file 2");

    // File 3: 500 bytes (fits in 1 chunk)
    let file3_data = vec![3u8; 500];
    let file3_path = temp_path.join("file3.dat");
    fs::write(&file3_path, &file3_data).expect("Failed to write file 3");

    let files = vec![
        DiscoveredFile {
            path: file1_path,
            size: 1500u64,
        },
        DiscoveredFile {
            path: file2_path,
            size: 1200u64,
        },
        DiscoveredFile {
            path: file3_path,
            size: 500u64,
        },
    ];

    let (chunk_tx, mut chunk_rx) = mpsc::channel(10);
    tokio::spawn(produce_chunk_stream(files, chunk_size, chunk_tx));

    let mut chunks = Vec::new();
    while let Some(result) = chunk_rx.recv().await {
        chunks.push(result.expect("Should not error"));
    }

    // Expected: 4 chunks total
    // Chunk 0: 1000 bytes (first 1000 of file1)
    // Chunk 1: 1000 bytes (last 500 of file1 + first 500 of file2)
    // Chunk 2: 1000 bytes (last 700 of file2 + first 300 of file3)
    // Chunk 3: 200 bytes (last 200 of file3)
    assert_eq!(chunks.len(), 4, "Should produce 4 chunks");

    // Verify chunk indices
    for (i, chunk) in chunks.iter().enumerate() {
        assert_eq!(chunk.chunk_index, i as i32);
    }

    // Verify chunk sizes
    assert_eq!(chunks[0].data.len(), 1000);
    assert_eq!(chunks[1].data.len(), 1000);
    assert_eq!(chunks[2].data.len(), 1000);
    assert_eq!(chunks[3].data.len(), 200);

    // Verify data integrity by reassembling
    let mut reassembled = Vec::new();
    for chunk in &chunks {
        reassembled.extend_from_slice(&chunk.data);
    }

    // First 1500 bytes should be all 1s (file1)
    assert!(reassembled[0..1500].iter().all(|&b| b == 1));
    // Next 1200 bytes should be all 2s (file2)
    assert!(reassembled[1500..2700].iter().all(|&b| b == 2));
    // Last 500 bytes should be all 3s (file3)
    assert!(reassembled[2700..3200].iter().all(|&b| b == 3));
}

#[tokio::test]
async fn test_produce_chunk_stream_empty_file() {
    // Test reading an empty file
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let temp_path = temp_dir.path();

    let file_path = temp_path.join("empty.dat");
    fs::write(&file_path, b"").expect("Failed to write empty file");

    let files = vec![DiscoveredFile {
        path: file_path,
        size: 0u64,
    }];

    let (chunk_tx, mut chunk_rx) = mpsc::channel(10);
    tokio::spawn(produce_chunk_stream(files, 1024, chunk_tx));

    let mut chunks = Vec::new();
    while let Some(result) = chunk_rx.recv().await {
        chunks.push(result.expect("Should not error"));
    }

    assert_eq!(chunks.len(), 0, "Empty file should produce no chunks");
}

#[tokio::test]
async fn test_produce_chunk_stream_nonexistent_file() {
    // Test error handling for nonexistent file
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let temp_path = temp_dir.path();

    let nonexistent_path = temp_path.join("nonexistent.dat");

    let files = vec![DiscoveredFile {
        path: nonexistent_path,
        size: 1000u64,
    }];

    let (chunk_tx, mut chunk_rx) = mpsc::channel(10);
    tokio::spawn(produce_chunk_stream(files, 1024, chunk_tx));

    let mut chunks = Vec::new();
    while let Some(result) = chunk_rx.recv().await {
        match result {
            Ok(chunk) => chunks.push(chunk),
            Err(e) => {
                assert!(e.contains("Failed to open file"));
                return; // Expected error, test passes
            }
        }
    }

    panic!("Should have received an error for nonexistent file");
}

#[tokio::test]
async fn test_produce_chunk_stream_channel_closed() {
    // Test behavior when receiver is dropped (channel closed)
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let temp_path = temp_dir.path();

    let file_data = vec![42u8; 2000];
    let file_path = temp_path.join("test.dat");
    fs::write(&file_path, &file_data).expect("Failed to write test file");

    let files = vec![DiscoveredFile {
        path: file_path,
        size: 2000u64,
    }];

    let (chunk_tx, chunk_rx) = mpsc::channel(10);

    // Drop the receiver immediately
    drop(chunk_rx);

    // This should not panic and should exit gracefully
    produce_chunk_stream(files, 1024, chunk_tx).await;
}

#[tokio::test]
async fn test_finalize_chunk_creates_unique_ids() {
    // Test that each chunk gets a unique UUID
    let data1 = vec![1, 2, 3, 4, 5];
    let data2 = vec![6, 7, 8, 9, 10];

    let chunk1 = finalize_chunk(0, data1);
    let chunk2 = finalize_chunk(1, data2);

    assert_ne!(
        chunk1.chunk_id, chunk2.chunk_id,
        "Chunk IDs should be unique"
    );
    assert_eq!(chunk1.chunk_index, 0);
    assert_eq!(chunk2.chunk_index, 1);
    assert_eq!(chunk1.data, vec![1, 2, 3, 4, 5]);
    assert_eq!(chunk2.data, vec![6, 7, 8, 9, 10]);
}

#[tokio::test]
async fn test_finalize_chunk_id_format() {
    // Test that chunk IDs are valid UUIDs
    let data = vec![42u8; 100];
    let chunk = finalize_chunk(0, data);

    // UUIDs should be 36 characters long (with hyphens)
    assert_eq!(chunk.chunk_id.len(), 36);

    // Should contain hyphens at expected positions
    assert_eq!(chunk.chunk_id.chars().nth(8), Some('-'));
    assert_eq!(chunk.chunk_id.chars().nth(13), Some('-'));
    assert_eq!(chunk.chunk_id.chars().nth(18), Some('-'));
    assert_eq!(chunk.chunk_id.chars().nth(23), Some('-'));
}

#[tokio::test]
async fn test_produce_chunk_stream_large_file() {
    // Test with a larger file to ensure memory efficiency
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let temp_path = temp_dir.path();

    let chunk_size = 1024 * 1024; // 1MB chunks
    let file_size = 5 * 1024 * 1024; // 5MB file

    // Create a file with predictable pattern
    let pattern: Vec<u8> = (0..=255).collect();
    let file_data = pattern.repeat(file_size / 256);
    let actual_file_size = file_data.len();
    let file_path = temp_path.join("large.dat");
    fs::write(&file_path, &file_data).expect("Failed to write large file");

    let files = vec![DiscoveredFile {
        path: file_path,
        size: actual_file_size as u64,
    }];

    let (chunk_tx, mut chunk_rx) = mpsc::channel(10);
    tokio::spawn(produce_chunk_stream(files, chunk_size, chunk_tx));

    let mut chunks = Vec::new();
    while let Some(result) = chunk_rx.recv().await {
        chunks.push(result.expect("Should not error"));
    }

    // Calculate expected number of chunks
    let expected_chunks = actual_file_size.div_ceil(chunk_size);
    assert_eq!(chunks.len(), expected_chunks);

    // All chunks except the last should be full size
    for (i, chunk) in chunks.iter().enumerate().take(chunks.len() - 1) {
        assert_eq!(chunk.chunk_index, i as i32);
        assert_eq!(chunk.data.len(), chunk_size);
    }

    // Last chunk should have the remainder
    let last_chunk_index = chunks.len() - 1;
    assert_eq!(
        chunks[last_chunk_index].chunk_index,
        last_chunk_index as i32
    );
    let expected_last_size = actual_file_size - (last_chunk_index * chunk_size);
    assert_eq!(chunks[last_chunk_index].data.len(), expected_last_size);

    // Verify data integrity
    let mut reassembled = Vec::new();
    for chunk in &chunks {
        reassembled.extend_from_slice(&chunk.data);
    }
    assert_eq!(reassembled, file_data);
}

#[tokio::test]
async fn test_produce_chunk_stream_mixed_file_sizes() {
    // Test with files of various sizes to ensure robust chunking
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let temp_path = temp_dir.path();

    let chunk_size = 1000;

    // Create files with different sizes
    let test_cases = [
        (vec![1u8; 0], "empty.dat"),     // Empty file
        (vec![2u8; 500], "small.dat"),   // Smaller than chunk
        (vec![3u8; 1000], "exact.dat"),  // Exactly chunk size
        (vec![4u8; 1500], "medium.dat"), // 1.5 chunks
        (vec![5u8; 3000], "large.dat"),  // 3 chunks
    ];

    let mut files = Vec::new();
    for (data, filename) in test_cases.iter() {
        let file_path = temp_path.join(filename);
        fs::write(&file_path, data).expect("Failed to write test file");
        files.push(DiscoveredFile {
            path: file_path,
            size: data.len() as u64,
        });
    }

    let (chunk_tx, mut chunk_rx) = mpsc::channel(10);
    tokio::spawn(produce_chunk_stream(files, chunk_size, chunk_tx));

    let mut chunks = Vec::new();
    while let Some(result) = chunk_rx.recv().await {
        chunks.push(result.expect("Should not error"));
    }

    // Expected chunks:
    // Total data: 0 + 500 + 1000 + 1500 + 3000 = 6000 bytes
    // With 1000-byte chunks: 6 chunks of 1000 bytes each
    // Total: 6 chunks
    assert_eq!(chunks.len(), 6);

    // Verify chunk indices are sequential
    for (i, chunk) in chunks.iter().enumerate() {
        assert_eq!(chunk.chunk_index, i as i32);
    }

    // Verify data integrity by reassembling
    let mut reassembled = Vec::new();
    for chunk in &chunks {
        reassembled.extend_from_slice(&chunk.data);
    }

    // Expected reassembled data: 0 + 500 + 1000 + 1500 + 3000 = 6000 bytes
    assert_eq!(reassembled.len(), 6000);

    // Verify each section has the correct pattern
    let mut offset = 0;

    // Empty file: no data
    // Small file: 500 bytes of 2s
    assert!(reassembled[offset..offset + 500].iter().all(|&b| b == 2));
    offset += 500;

    // Exact file: 1000 bytes of 3s
    assert!(reassembled[offset..offset + 1000].iter().all(|&b| b == 3));
    offset += 1000;

    // Medium file: 1500 bytes of 4s
    assert!(reassembled[offset..offset + 1500].iter().all(|&b| b == 4));
    offset += 1500;

    // Large file: 3000 bytes of 5s
    assert!(reassembled[offset..offset + 3000].iter().all(|&b| b == 5));
}
