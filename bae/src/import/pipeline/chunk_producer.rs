// Chunk Producer Module
//
// Handles reading files and producing chunks for the import pipeline.
// This module is responsible for the first stage of the pipeline: reading
// files sequentially and streaming chunks as they're produced.

use crate::import::pipeline::ChunkData;
use crate::import::types::DiscoveredFile;
use tokio::io::{AsyncReadExt, BufReader};
use tokio::sync::mpsc;
use uuid::Uuid;

/// Read files sequentially and stream chunks as they're produced.
///
/// Treats all files as a concatenated byte stream, dividing it into fixed-size chunks.
/// Chunks are sent to the channel as soon as they're full, allowing downstream
/// encryption and upload to start immediately without buffering the entire album.
///
/// Files don't align to chunk boundaries - a chunk may contain data from multiple files.
pub async fn produce_chunk_stream(
    files: Vec<DiscoveredFile>,
    chunk_size: usize,
    chunk_tx: mpsc::Sender<Result<ChunkData, String>>,
) {
    let mut current_chunk_buffer = Vec::with_capacity(chunk_size);
    let mut current_chunk_index = 0i32;

    for file in files {
        let file_handle = match tokio::fs::File::open(&file.path).await {
            Ok(f) => f,
            Err(e) => {
                let _ = chunk_tx
                    .send(Err(format!("Failed to open file {:?}: {}", file.path, e)))
                    .await;
                return;
            }
        };

        let mut reader = BufReader::new(file_handle);

        loop {
            let space_remaining = chunk_size - current_chunk_buffer.len();
            let mut temp_buffer = vec![0u8; space_remaining];

            let bytes_read = match reader.read(&mut temp_buffer).await {
                Ok(n) => n,
                Err(e) => {
                    let _ = chunk_tx
                        .send(Err(format!("Failed to read from file: {}", e)))
                        .await;
                    return;
                }
            };

            if bytes_read == 0 {
                // EOF - move to next file
                break;
            }

            // Add the bytes we read to current chunk
            current_chunk_buffer.extend_from_slice(&temp_buffer[..bytes_read]);

            // If chunk is full, send it and start a new one
            if current_chunk_buffer.len() == chunk_size {
                let chunk = finalize_chunk(current_chunk_index, current_chunk_buffer);
                if chunk_tx.send(Ok(chunk)).await.is_err() {
                    // Receiver dropped, stop reading
                    return;
                }
                current_chunk_index += 1;
                current_chunk_buffer = Vec::with_capacity(chunk_size);
            }
        }
    }

    // Send final partial chunk if any data remains
    if !current_chunk_buffer.is_empty() {
        let chunk = finalize_chunk(current_chunk_index, current_chunk_buffer);
        let _ = chunk_tx.send(Ok(chunk)).await;
    }
}

/// Finalize a chunk by creating ChunkData with a unique ID.
pub fn finalize_chunk(chunk_index: i32, data: Vec<u8>) -> ChunkData {
    ChunkData {
        chunk_id: Uuid::new_v4().to_string(),
        chunk_index,
        data,
    }
}
