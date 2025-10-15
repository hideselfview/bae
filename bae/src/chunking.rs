/// Represents the mapping of a file to chunks within an album
#[derive(Debug, Clone)]
pub struct FileChunkMapping {
    pub file_path: std::path::PathBuf,
    pub start_chunk_index: i32,
    pub end_chunk_index: i32,
    pub start_byte_offset: i64,
    pub end_byte_offset: i64,
}
