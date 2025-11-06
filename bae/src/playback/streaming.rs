use crate::db::DbTrackChunkCoords;
use crate::playback::ChunkBuffer;
use std::io::{Read, Result as IoResult, Seek, SeekFrom};
use std::sync::{Arc, Mutex};
use tracing::{debug, warn};

/// Streaming data source that reads from chunk buffer
///
/// Implements `Read` trait for use with Symphonia's `MediaSourceStream`.
/// Handles reading across chunk boundaries and extracting byte ranges for tracks.
pub struct StreamingChunkSource {
    chunk_buffer: Arc<ChunkBuffer>,
    coords: DbTrackChunkCoords,
    chunk_size_bytes: usize,
    /// Tokio runtime handle for accessing async chunk buffer from blocking contexts
    runtime_handle: tokio::runtime::Handle,
    /// Current position in the track (relative to start_byte_offset)
    current_position: Arc<Mutex<usize>>,
    /// FLAC headers to prepend (for CUE/FLAC tracks)
    headers: Option<Vec<u8>>,
    /// Whether we've read the headers yet
    headers_read: bool,
    /// Total bytes we've read from the stream (including headers)
    /// This is used to track header reading progress
    total_bytes_read: usize,
    /// Bytes we've read from the track data (excluding headers)
    /// This is used to calculate current_position
    track_bytes_read: usize,
}

impl StreamingChunkSource {
    /// Create a new streaming chunk source for a track
    pub fn new(
        chunk_buffer: Arc<ChunkBuffer>,
        coords: DbTrackChunkCoords,
        chunk_size_bytes: usize,
        headers: Option<Vec<u8>>,
        runtime_handle: tokio::runtime::Handle,
    ) -> Self {
        Self {
            chunk_buffer,
            coords,
            chunk_size_bytes,
            runtime_handle,
            current_position: Arc::new(Mutex::new(0)),
            headers,
            headers_read: false,
            total_bytes_read: 0,
            track_bytes_read: 0,
        }
    }
}

impl Read for StreamingChunkSource {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
        // First, read headers if present and not yet read completely
        if let Some(ref headers) = self.headers {
            if !self.headers_read {
                let headers_remaining = headers.len().saturating_sub(self.total_bytes_read);
                if headers_remaining > 0 {
                    let to_read = headers_remaining.min(buf.len());
                    let start_idx = self.total_bytes_read;
                    buf[..to_read].copy_from_slice(&headers[start_idx..start_idx + to_read]);
                    self.total_bytes_read += to_read;
                    if self.total_bytes_read >= headers.len() {
                        self.headers_read = true;
                    }
                    return Ok(to_read);
                }
            }
        }

        // Calculate byte position within the track
        // track_bytes_read is the number of bytes we've read from the track's audio data
        let current_pos = self.track_bytes_read;

        // start_byte_offset is the offset within start_chunk_index where the track begins
        // end_byte_offset is the offset within end_chunk_index where the track ends
        let track_start_offset = self.coords.start_byte_offset as usize;
        let track_end_offset = self.coords.end_byte_offset as usize;

        // Calculate the total byte position from the start of start_chunk_index
        // This is: offset within start_chunk + bytes read from track
        let total_bytes_from_chunk_start = track_start_offset + current_pos;

        // Determine which chunk contains this position
        // total_bytes_from_chunk_start tells us how many bytes into the chunk sequence we are
        let chunk_offset = total_bytes_from_chunk_start / self.chunk_size_bytes;
        let chunk_index = self.coords.start_chunk_index + chunk_offset as i32;

        // Calculate byte offset within that chunk
        let byte_offset_in_chunk = total_bytes_from_chunk_start % self.chunk_size_bytes;

        // Log chunk calculation for debugging (only for large seeks)
        if self.track_bytes_read > 10000000 {
            debug!(
                "📦 Read from track_bytes_read={}, calculated chunk_index={} (offset={}, start_chunk={}, total_from_start={})",
                self.track_bytes_read, chunk_index, byte_offset_in_chunk, self.coords.start_chunk_index, total_bytes_from_chunk_start
            );
        }

        // Check if we've exceeded the track bounds
        // We need to check if we've gone past end_byte_offset in the last chunk
        if chunk_index > self.coords.end_chunk_index {
            warn!(
                "EOF: chunk {} exceeds end chunk {}",
                chunk_index, self.coords.end_chunk_index
            );
            return Ok(0); // EOF - past end chunk
        }
        if chunk_index == self.coords.end_chunk_index && byte_offset_in_chunk >= track_end_offset {
            warn!(
                "EOF: at end chunk {} with offset {} >= end_offset {}",
                chunk_index, byte_offset_in_chunk, track_end_offset
            );
            return Ok(0); // EOF - past end offset in last chunk
        }

        // Try to get the chunk data (blocking read from async)
        // Use block_in_place to move blocking operation off async thread
        // Block indefinitely until chunk is available - chunks should always be loaded
        let chunk_data = tokio::task::block_in_place(|| {
            self.runtime_handle.block_on(async {
                loop {
                    if let Some(data) = self.chunk_buffer.get_chunk_data(chunk_index).await {
                        return Some(data);
                    }
                    // Wait a bit before retrying
                    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                }
            })
        });

        let chunk_data = match chunk_data {
            Some(data) => data,
            None => {
                // This should never happen since we loop forever above
                warn!("Chunk {} not available, returning EOF", chunk_index);
                return Ok(0); // EOF
            }
        };

        // Calculate how much we can read from this chunk
        let bytes_remaining_in_chunk = chunk_data.len().saturating_sub(byte_offset_in_chunk);

        // Calculate bytes remaining in track
        // If we're in the last chunk, limit by end_byte_offset
        let bytes_remaining_in_track = if chunk_index == self.coords.end_chunk_index {
            track_end_offset.saturating_sub(byte_offset_in_chunk)
        } else {
            // Not in last chunk, calculate based on remaining chunks
            let chunks_remaining = (self.coords.end_chunk_index - chunk_index) as usize;
            let bytes_in_current_chunk = self.chunk_size_bytes - byte_offset_in_chunk;
            bytes_in_current_chunk
                + (chunks_remaining.saturating_sub(1) * self.chunk_size_bytes)
                + track_end_offset
        };

        let bytes_to_read = buf
            .len()
            .min(bytes_remaining_in_chunk)
            .min(bytes_remaining_in_track);

        if bytes_to_read == 0 {
            warn!(
                "EOF: bytes_to_read=0, buf_len={}, remaining_in_chunk={}, remaining_in_track={}, chunk={}, offset={}",
                buf.len(), bytes_remaining_in_chunk, bytes_remaining_in_track, chunk_index, byte_offset_in_chunk
            );
            return Ok(0); // EOF
        }

        // Copy data from chunk to buffer
        buf[..bytes_to_read].copy_from_slice(
            &chunk_data[byte_offset_in_chunk..byte_offset_in_chunk + bytes_to_read],
        );

        // Update position
        self.track_bytes_read += bytes_to_read;
        {
            let mut pos = self.current_position.lock().unwrap();
            *pos = self.track_bytes_read;
        }
        self.total_bytes_read += bytes_to_read;

        // Only log every 100th read or large reads to avoid spam
        if bytes_to_read > 32768 || self.total_bytes_read % 3276800 == 0 {
            debug!(
                "Read {} bytes from chunk {} at offset {} (track position: {})",
                bytes_to_read, chunk_index, byte_offset_in_chunk, self.track_bytes_read
            );
        }

        Ok(bytes_to_read)
    }
}

impl Seek for StreamingChunkSource {
    fn seek(&mut self, pos: SeekFrom) -> IoResult<u64> {
        debug!("🔍 StreamingChunkSource::seek called with {:?}", pos);

        let headers_size = self.headers.as_ref().map(|h| h.len()).unwrap_or(0);

        // Calculate track size properly (accounting for chunks it spans)
        let track_start_offset = self.coords.start_byte_offset as usize;
        let track_end_offset = self.coords.end_byte_offset as usize;
        let chunks_span =
            (self.coords.end_chunk_index - self.coords.start_chunk_index).max(0) as usize;
        let track_size = if chunks_span == 0 {
            // Single chunk
            track_end_offset - track_start_offset
        } else {
            // Multiple chunks: start_chunk remainder + middle chunks + end_chunk portion
            let start_chunk_remainder = self.chunk_size_bytes - track_start_offset;
            let middle_chunks_bytes = chunks_span.saturating_sub(1) * self.chunk_size_bytes;
            start_chunk_remainder + middle_chunks_bytes + track_end_offset
        };

        debug!(
            "🔍 Seek context: headers_size={}, track_size={}, current track_bytes_read={}",
            headers_size, track_size, self.track_bytes_read
        );

        let new_pos = match pos {
            SeekFrom::Start(pos) => {
                // pos is the position in the stream (including headers)
                if pos < headers_size as u64 {
                    // Seeking within headers
                    self.total_bytes_read = pos as usize;
                    self.headers_read = pos >= headers_size as u64;
                    self.track_bytes_read = 0;
                    pos
                } else {
                    // Seeking within track data
                    let track_pos = (pos - headers_size as u64) as usize;
                    if track_pos > track_size {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            "Seek beyond end of track",
                        ));
                    }
                    self.total_bytes_read = pos as usize;
                    self.headers_read = true;
                    self.track_bytes_read = track_pos;
                    {
                        let mut current_pos = self.current_position.lock().unwrap();
                        *current_pos = track_pos;
                    }
                    pos
                }
            }
            SeekFrom::End(offset) => {
                let total_size = headers_size + track_size;
                let new_pos = if offset >= 0 {
                    total_size as i64 + offset
                } else {
                    (total_size as i64).saturating_add(offset)
                };
                if new_pos < 0 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Seek before start of stream",
                    ));
                }
                self.seek(SeekFrom::Start(new_pos as u64))?;
                new_pos as u64
            }
            SeekFrom::Current(offset) => {
                let current_pos = self.total_bytes_read as i64;
                let new_pos = current_pos + offset;
                if new_pos < 0 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Seek before start of stream",
                    ));
                }
                self.seek(SeekFrom::Start(new_pos as u64))?;
                new_pos as u64
            }
        };

        debug!(
            "🔍 Seek completed: new_pos={}, track_bytes_read={}",
            new_pos, self.track_bytes_read
        );

        Ok(new_pos)
    }
}
