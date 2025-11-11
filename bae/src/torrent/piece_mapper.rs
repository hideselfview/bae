use std::collections::HashMap;

/// Maps torrent pieces to bae chunks
pub struct TorrentPieceMapper {
    piece_length: usize,
    chunk_size: usize,
    total_pieces: usize,
    total_size: usize,
}

/// Mapping from a torrent piece to bae chunks
#[derive(Debug, Clone)]
pub struct ChunkMapping {
    pub chunk_index: usize,
    pub start_byte: usize,
    pub end_byte: usize,
}

/// Mapping from a bae chunk to torrent pieces
#[derive(Debug, Clone)]
pub struct PieceMapping {
    pub piece_index: usize,
    pub start_byte_in_piece: usize,
    pub end_byte_in_piece: usize,
}

impl TorrentPieceMapper {
    /// Create a new piece mapper
    pub fn new(piece_length: usize, chunk_size: usize, total_pieces: usize, total_size: usize) -> Self {
        TorrentPieceMapper {
            piece_length,
            chunk_size,
            total_pieces,
            total_size,
        }
    }

    /// Map a torrent piece to the bae chunks it spans
    pub fn map_piece_to_chunks(&self, piece_index: usize) -> Vec<ChunkMapping> {
        if piece_index >= self.total_pieces {
            return Vec::new();
        }

        let piece_start_byte = piece_index * self.piece_length;
        let piece_end_byte = ((piece_index + 1) * self.piece_length).min(self.total_size);

        let start_chunk = piece_start_byte / self.chunk_size;
        let end_chunk = (piece_end_byte - 1) / self.chunk_size;

        let mut mappings = Vec::new();
        for chunk_index in start_chunk..=end_chunk {
            let chunk_start_byte = chunk_index * self.chunk_size;
            let chunk_end_byte = ((chunk_index + 1) * self.chunk_size).min(self.total_size);

            let overlap_start = piece_start_byte.max(chunk_start_byte);
            let overlap_end = piece_end_byte.min(chunk_end_byte);

            if overlap_start < overlap_end {
                mappings.push(ChunkMapping {
                    chunk_index,
                    start_byte: overlap_start - chunk_start_byte,
                    end_byte: overlap_end - chunk_start_byte,
                });
            }
        }

        mappings
    }

    /// Map a bae chunk to the torrent pieces it spans
    pub fn map_chunk_to_pieces(&self, chunk_index: usize) -> Vec<PieceMapping> {
        let chunk_start_byte = chunk_index * self.chunk_size;
        let chunk_end_byte = ((chunk_index + 1) * self.chunk_size).min(self.total_size);

        let start_piece = chunk_start_byte / self.piece_length;
        let end_piece = (chunk_end_byte - 1) / self.piece_length;

        let mut mappings = Vec::new();
        for piece_index in start_piece..=end_piece {
            let piece_start_byte = piece_index * self.piece_length;
            let piece_end_byte = ((piece_index + 1) * self.piece_length).min(self.total_size);

            let overlap_start = chunk_start_byte.max(piece_start_byte);
            let overlap_end = chunk_end_byte.min(piece_end_byte);

            if overlap_start < overlap_end {
                mappings.push(PieceMapping {
                    piece_index,
                    start_byte_in_piece: overlap_start - piece_start_byte,
                    end_byte_in_piece: overlap_end - piece_start_byte,
                });
            }
        }

        mappings
    }

    /// Get all chunk indices that a piece spans
    pub fn get_chunk_indices_for_piece(&self, piece_index: usize) -> Vec<usize> {
        self.map_piece_to_chunks(piece_index)
            .into_iter()
            .map(|m| m.chunk_index)
            .collect()
    }

    /// Get all piece indices that a chunk spans
    pub fn get_piece_indices_for_chunk(&self, chunk_index: usize) -> Vec<usize> {
        self.map_chunk_to_pieces(chunk_index)
            .into_iter()
            .map(|m| m.piece_index)
            .collect()
    }
}

