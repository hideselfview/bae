// Test support utilities for both unit and integration tests

use crate::cloud_storage::{CloudStorage, CloudStorageError};
use std::collections::HashMap;
use std::sync::Mutex;

/// Mock cloud storage for testing
///
/// Stores chunks in memory instead of uploading to S3.
/// Useful for testing without external dependencies.
pub struct MockCloudStorage {
    chunks: Mutex<HashMap<String, Vec<u8>>>,
}

impl Default for MockCloudStorage {
    fn default() -> Self {
        MockCloudStorage {
            chunks: Mutex::new(HashMap::new()),
        }
    }
}

impl MockCloudStorage {
    /// Create a new mock cloud storage instance
    #[allow(unused)] // Used in tests
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait::async_trait]
impl CloudStorage for MockCloudStorage {
    async fn upload_chunk(&self, chunk_id: &str, data: &[u8]) -> Result<String, CloudStorageError> {
        let location = format!(
            "s3://test-bucket/chunks/{}/{}/{}.enc",
            &chunk_id[0..2],
            &chunk_id[2..4],
            chunk_id
        );

        self.chunks
            .lock()
            .unwrap()
            .insert(location.clone(), data.to_vec());

        Ok(location)
    }

    async fn download_chunk(&self, storage_location: &str) -> Result<Vec<u8>, CloudStorageError> {
        self.chunks
            .lock()
            .unwrap()
            .get(storage_location)
            .cloned()
            .ok_or_else(|| {
                CloudStorageError::Download(format!("Chunk not found: {}", storage_location))
            })
    }
}
