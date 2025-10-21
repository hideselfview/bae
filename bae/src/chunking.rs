/// Maps each file to its chunk range and byte offsets within those chunks.
/// Used by the chunk producer to efficiently stream files in sequence.
/// A file can represent either a single track or a complete disc image containing multiple tracks.
#[derive(Debug, Clone)]
pub struct FileChunkMapping {
    pub file_path: std::path::PathBuf,
    pub start_chunk_index: i32,
    pub end_chunk_index: i32,
    pub start_byte_offset: i64,
    pub end_byte_offset: i64,
}
