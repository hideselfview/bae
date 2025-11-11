// Torrent Chunk Producer
//
// Reads torrent pieces as they complete and produces chunks for the import pipeline.
// Maps torrent pieces to bae chunks using TorrentPieceMapper.

use crate::import::pipeline::ChunkData;
use crate::torrent::{TorrentHandle, TorrentPieceMapper};
use std::collections::HashMap;
use tokio::sync::mpsc;
use tracing::debug;
use uuid::Uuid;

/// Read torrent pieces as they complete and stream chunks.
///
/// This producer waits for torrent pieces to complete downloading, then maps them
/// to bae chunks and sends them to the encryption pipeline. Pieces and chunks have
/// independent boundaries, so a piece may span multiple chunks or a chunk may span
/// multiple pieces.
///
/// Returns a map of piece_index -> (chunk_ids, start_byte, end_byte) for persistence.
pub async fn produce_chunk_stream_from_torrent(
    torrent_handle: &TorrentHandle,
    piece_mapper: TorrentPieceMapper,
    chunk_size: usize,
    chunk_tx: mpsc::Sender<Result<ChunkData, String>>,
) -> HashMap<usize, (Vec<String>, i64, i64)> {
    let total_pieces = match torrent_handle.num_pieces().await {
        Ok(n) => n as usize,
        Err(e) => {
            let _ = chunk_tx
                .send(Err(format!("Failed to get torrent piece count: {}", e)))
                .await;
            return HashMap::new();
        }
    };

    // Track which chunks we've started and their current data
    let mut chunk_buffers: HashMap<usize, Vec<u8>> = HashMap::new();

    // Track piece-to-chunk mappings for database persistence
    // Maps piece_index -> (chunk_ids, start_byte_in_first_chunk, end_byte_in_last_chunk)
    let mut piece_mappings: HashMap<usize, (Vec<String>, i64, i64)> = HashMap::new();

    // Track chunk_index -> chunk_id mapping as chunks are sent
    // This allows us to update piece mappings even if chunks complete out of order
    let mut chunk_id_map: HashMap<usize, String> = HashMap::new();

    // Process pieces as they complete
    for piece_index in 0..total_pieces {
        // Wait for piece to be available
        debug!("Waiting for piece {} to complete", piece_index);

        // Poll until piece is available
        loop {
            match torrent_handle.is_piece_ready(piece_index).await {
                Ok(true) => break,
                Ok(false) => {
                    // Piece not ready yet, wait a bit and check progress
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    continue;
                }
                Err(e) => {
                    let _ = chunk_tx
                        .send(Err(format!(
                            "Failed to check piece {} availability: {}",
                            piece_index, e
                        )))
                        .await;
                    return piece_mappings;
                }
            }
        }

        // Map piece to chunks
        let chunk_mappings = piece_mapper.map_piece_to_chunks(piece_index);

        // Read piece data from torrent
        let piece_data = match torrent_handle.read_piece(piece_index).await {
            Ok(data) => data,
            Err(e) => {
                let _ = chunk_tx
                    .send(Err(format!("Failed to read piece {}: {}", piece_index, e)))
                    .await;
                return piece_mappings;
            }
        };

        debug!(
            "Read piece {} ({} bytes), mapping to {} chunks",
            piece_index,
            piece_data.len(),
            chunk_mappings.len()
        );

        // Calculate piece boundaries for byte extraction
        let piece_length = piece_mapper.piece_length();
        let total_size = piece_mapper.total_size();
        let piece_start_byte = piece_index * piece_length;
        let piece_end_byte = ((piece_index + 1) * piece_length).min(total_size);

        // Initialize piece mapping for this piece
        // We'll collect chunk IDs as chunks are sent, and track start/end bytes
        let first_chunk_mapping = chunk_mappings.first();
        let last_chunk_mapping = chunk_mappings.last();
        let start_byte_in_first_chunk = first_chunk_mapping
            .map(|m| m.start_byte as i64)
            .unwrap_or(0);
        let end_byte_in_last_chunk = last_chunk_mapping.map(|m| m.end_byte as i64).unwrap_or(0);

        // Track which chunk indices this piece contributes to (for later ID lookup)
        let mut piece_chunk_indices: Vec<usize> =
            chunk_mappings.iter().map(|m| m.chunk_index).collect();
        piece_chunk_indices.sort();
        piece_chunk_indices.dedup();

        // For each chunk mapping, extract the relevant bytes and add to chunk buffer
        for chunk_mapping in chunk_mappings {
            let chunk_index = chunk_mapping.chunk_index;

            // Calculate chunk boundaries
            let chunk_start_byte = chunk_index * chunk_size;
            let chunk_end_byte = ((chunk_index + 1) * chunk_size).min(total_size);

            // Calculate overlap between piece and chunk
            let overlap_start = piece_start_byte.max(chunk_start_byte);
            let overlap_end = piece_end_byte.min(chunk_end_byte);

            // Calculate byte range within the piece data
            let piece_data_start = overlap_start - piece_start_byte;
            let piece_data_end = overlap_end - piece_start_byte;

            // Ensure chunk buffer exists
            if !chunk_buffers.contains_key(&chunk_index) {
                chunk_buffers.insert(chunk_index, Vec::with_capacity(chunk_size));
            }

            // Extract bytes from piece data for this chunk
            if piece_data_end > piece_data_start && piece_data_end <= piece_data.len() {
                let chunk_data = &piece_data[piece_data_start..piece_data_end];
                chunk_buffers
                    .get_mut(&chunk_index)
                    .unwrap()
                    .extend_from_slice(chunk_data);
            }

            // Check if chunk is complete
            let chunk_buffer = chunk_buffers.get(&chunk_index).unwrap();
            if chunk_buffer.len() >= chunk_size {
                // Chunk is complete, send it
                let complete_chunk = chunk_buffers.remove(&chunk_index).unwrap();
                let chunk_id = Uuid::new_v4().to_string();
                let chunk = ChunkData {
                    chunk_id: chunk_id.clone(),
                    chunk_index: chunk_index as i32,
                    data: complete_chunk,
                };

                if chunk_tx.send(Ok(chunk)).await.is_err() {
                    // Receiver dropped, stop reading
                    return piece_mappings;
                }

                // Track chunk ID for this chunk index
                chunk_id_map.insert(chunk_index, chunk_id.clone());

                // Update piece mapping for this piece if it contributes to this chunk
                // Check all pieces that might contribute to this chunk
                let piece_len = piece_mapper.piece_length();
                let total_sz = piece_mapper.total_size();
                for (pi, piece_mapping) in piece_mappings.iter_mut() {
                    // Check if this piece contributes to this chunk
                    let piece_start = *pi * piece_len;
                    let piece_end = ((*pi + 1) * piece_len).min(total_sz);
                    let chunk_start = chunk_index * chunk_size;
                    let chunk_end = ((chunk_index + 1) * chunk_size).min(total_sz);

                    if piece_start < chunk_end && piece_end > chunk_start {
                        // This piece contributes to this chunk
                        if !piece_mapping.0.contains(&chunk_id) {
                            piece_mapping.0.push(chunk_id.clone());
                        }
                    }
                }

                debug!(
                    "Sent complete chunk {} (piece {})",
                    chunk_index, piece_index
                );
            }
        }

        // Initialize piece mapping for this piece (if not already created)
        // We'll update chunk IDs as chunks complete
        let piece_mapping = piece_mappings.entry(piece_index).or_insert_with(|| {
            (
                Vec::new(),
                start_byte_in_first_chunk,
                end_byte_in_last_chunk,
            )
        });

        // Update end byte for piece mapping
        piece_mapping.2 = end_byte_in_last_chunk;

        // Add any chunk IDs we already have for chunks this piece contributes to
        for chunk_index in &piece_chunk_indices {
            if let Some(chunk_id) = chunk_id_map.get(chunk_index) {
                if !piece_mapping.0.contains(chunk_id) {
                    piece_mapping.0.push(chunk_id.clone());
                }
            }
        }
    }

    // Send any remaining partial chunks
    for (chunk_index, data) in chunk_buffers {
        if !data.is_empty() {
            let chunk_id = Uuid::new_v4().to_string();
            let chunk = ChunkData {
                chunk_id: chunk_id.clone(),
                chunk_index: chunk_index as i32,
                data,
            };
            if chunk_tx.send(Ok(chunk)).await.is_err() {
                return piece_mappings;
            }

            // Track chunk ID
            chunk_id_map.insert(chunk_index, chunk_id.clone());

            // Update piece mappings for pieces that contribute to this chunk
            let piece_len = piece_mapper.piece_length();
            let total_sz = piece_mapper.total_size();
            for (pi, piece_mapping) in piece_mappings.iter_mut() {
                let piece_start = *pi * piece_len;
                let piece_end = ((*pi + 1) * piece_len).min(total_sz);
                let chunk_start = chunk_index * chunk_size;
                let chunk_end = ((chunk_index + 1) * chunk_size).min(total_sz);

                if piece_start < chunk_end && piece_end > chunk_start {
                    if !piece_mapping.0.contains(&chunk_id) {
                        piece_mapping.0.push(chunk_id.clone());
                    }
                }
            }
        }
    }

    piece_mappings
}
