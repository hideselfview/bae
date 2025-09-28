use aws_config::{BehaviorVersion, Region};
use aws_sdk_s3::{Client, Error as S3Error, primitives::ByteStreamError};
use aws_credential_types::Credentials;
use std::path::Path;
use thiserror::Error;
use tokio::fs;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Error, Debug)]
pub enum CloudStorageError {
    #[error("S3 error: {0}")]
    S3(#[from] S3Error),
    #[error("S3 SDK error: {0}")]
    SdkError(String),
    #[error("ByteStream error: {0}")]
    ByteStream(#[from] ByteStreamError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("Upload error: {0}")]
    Upload(String),
    #[error("Download error: {0}")]
    Download(String),
}

/// Configuration for S3 cloud storage
#[derive(Debug, Clone)]
pub struct S3Config {
    pub bucket_name: String,
    pub region: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub endpoint_url: Option<String>, // For S3-compatible services like MinIO
}

impl S3Config {
    /// Create config from environment variables
    pub fn from_env() -> Result<Self, CloudStorageError> {
        let bucket_name = std::env::var("BAE_S3_BUCKET")
            .map_err(|_| CloudStorageError::Config("BAE_S3_BUCKET not set".to_string()))?;
        
        let region = std::env::var("BAE_S3_REGION")
            .map_err(|_| CloudStorageError::Config("BAE_S3_REGION not set".to_string()))?;
        
        let access_key_id = std::env::var("BAE_S3_ACCESS_KEY_ID")
            .map_err(|_| CloudStorageError::Config("BAE_S3_ACCESS_KEY_ID not set".to_string()))?;
        
        let secret_access_key = std::env::var("BAE_S3_SECRET_ACCESS_KEY")
            .map_err(|_| CloudStorageError::Config("BAE_S3_SECRET_ACCESS_KEY not set".to_string()))?;
        
        let endpoint_url = std::env::var("BAE_S3_ENDPOINT_URL").ok();
        
        Ok(S3Config {
            bucket_name,
            region,
            access_key_id,
            secret_access_key,
            endpoint_url,
        })
    }

    /// Create config for testing (uses in-memory mock)
    pub fn for_testing() -> Self {
        S3Config {
            bucket_name: "test-bucket".to_string(),
            region: "us-east-1".to_string(),
            access_key_id: "test-key".to_string(),
            secret_access_key: "test-secret".to_string(),
            endpoint_url: None,
        }
    }
}

/// Trait for cloud storage operations (allows mocking for tests)
#[async_trait::async_trait]
pub trait CloudStorage: Send + Sync {
    async fn upload_chunk(&self, chunk_id: &str, data: &[u8]) -> Result<String, CloudStorageError>;
    async fn download_chunk(&self, storage_location: &str) -> Result<Vec<u8>, CloudStorageError>;
    async fn delete_chunk(&self, storage_location: &str) -> Result<(), CloudStorageError>;
    async fn chunk_exists(&self, storage_location: &str) -> Result<bool, CloudStorageError>;
}

/// Production S3 cloud storage implementation
pub struct S3CloudStorage {
    client: Client,
    bucket_name: String,
}

impl S3CloudStorage {
    /// Create a new S3 cloud storage client
    pub async fn new(config: S3Config) -> Result<Self, CloudStorageError> {
        // Create AWS credentials
        let credentials = Credentials::new(
            config.access_key_id,
            config.secret_access_key,
            None, // session_token
            None, // expiration
            "bae-s3-config"
        );

        // Build AWS config
        let mut aws_config_builder = aws_config::defaults(BehaviorVersion::latest())
            .region(Region::new(config.region))
            .credentials_provider(credentials);

        // Set custom endpoint if provided (for S3-compatible services)
        if let Some(endpoint) = config.endpoint_url {
            aws_config_builder = aws_config_builder.endpoint_url(endpoint);
        }

        let aws_config = aws_config_builder.load().await;
        let client = Client::new(&aws_config);

        Ok(S3CloudStorage {
            client,
            bucket_name: config.bucket_name,
        })
    }

    /// Generate S3 key for a chunk
    fn chunk_key(&self, chunk_id: &str) -> String {
        format!("chunks/{}", chunk_id)
    }
}

#[async_trait::async_trait]
impl CloudStorage for S3CloudStorage {
    async fn upload_chunk(&self, chunk_id: &str, data: &[u8]) -> Result<String, CloudStorageError> {
        let key = self.chunk_key(chunk_id);
        
        println!("S3CloudStorage: Uploading chunk {} ({} bytes)", chunk_id, data.len());
        
        self.client
            .put_object()
            .bucket(&self.bucket_name)
            .key(&key)
            .body(data.to_vec().into())
            .content_type("application/octet-stream")
            .send()
            .await
            .map_err(|e| CloudStorageError::SdkError(format!("Put object failed: {}", e)))?;
        
        let storage_location = format!("s3://{}/{}", self.bucket_name, key);
        println!("S3CloudStorage: Successfully uploaded chunk to {}", storage_location);
        
        Ok(storage_location)
    }

    async fn download_chunk(&self, storage_location: &str) -> Result<Vec<u8>, CloudStorageError> {
        // Parse S3 location: s3://bucket/key
        let key = storage_location
            .strip_prefix(&format!("s3://{}/", self.bucket_name))
            .ok_or_else(|| CloudStorageError::Download(
                format!("Invalid S3 location: {}", storage_location)
            ))?;
        
        println!("S3CloudStorage: Downloading chunk from {}", storage_location);
        
        let response = self.client
            .get_object()
            .bucket(&self.bucket_name)
            .key(key)
            .send()
            .await
            .map_err(|e| CloudStorageError::SdkError(format!("Get object failed: {}", e)))?;
        
        let data = response.body.collect().await?.into_bytes().to_vec();
        
        println!("S3CloudStorage: Successfully downloaded {} bytes", data.len());
        Ok(data)
    }

    async fn delete_chunk(&self, storage_location: &str) -> Result<(), CloudStorageError> {
        let key = storage_location
            .strip_prefix(&format!("s3://{}/", self.bucket_name))
            .ok_or_else(|| CloudStorageError::Download(
                format!("Invalid S3 location: {}", storage_location)
            ))?;
        
        println!("S3CloudStorage: Deleting chunk at {}", storage_location);
        
        self.client
            .delete_object()
            .bucket(&self.bucket_name)
            .key(key)
            .send()
            .await
            .map_err(|e| CloudStorageError::SdkError(format!("Delete object failed: {}", e)))?;
        
        println!("S3CloudStorage: Successfully deleted chunk");
        Ok(())
    }

    async fn chunk_exists(&self, storage_location: &str) -> Result<bool, CloudStorageError> {
        let key = storage_location
            .strip_prefix(&format!("s3://{}/", self.bucket_name))
            .ok_or_else(|| CloudStorageError::Download(
                format!("Invalid S3 location: {}", storage_location)
            ))?;
        
        match self.client
            .head_object()
            .bucket(&self.bucket_name)
            .key(key)
            .send()
            .await
        {
            Ok(_) => Ok(true),
            Err(e) => {
                // Check if it's a "not found" error
                let error_str = format!("{}", e);
                if error_str.contains("NoSuchKey") || error_str.contains("NotFound") {
                    Ok(false)
                } else {
                    Err(CloudStorageError::SdkError(format!("Head object failed: {}", e)))
                }
            }
        }
    }
}

/// In-memory cloud storage for testing
#[derive(Clone)]
pub struct MockCloudStorage {
    data: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl MockCloudStorage {
    pub fn new() -> Self {
        MockCloudStorage {
            data: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait::async_trait]
impl CloudStorage for MockCloudStorage {
    async fn upload_chunk(&self, chunk_id: &str, data: &[u8]) -> Result<String, CloudStorageError> {
        let storage_location = format!("mock://{}", chunk_id);
        let mut storage = self.data.lock().unwrap();
        storage.insert(storage_location.clone(), data.to_vec());
        Ok(storage_location)
    }

    async fn download_chunk(&self, storage_location: &str) -> Result<Vec<u8>, CloudStorageError> {
        let storage = self.data.lock().unwrap();
        storage.get(storage_location)
            .cloned()
            .ok_or_else(|| CloudStorageError::Download("Chunk not found".to_string()))
    }

    async fn delete_chunk(&self, storage_location: &str) -> Result<(), CloudStorageError> {
        let mut storage = self.data.lock().unwrap();
        storage.remove(storage_location);
        Ok(())
    }

    async fn chunk_exists(&self, storage_location: &str) -> Result<bool, CloudStorageError> {
        let storage = self.data.lock().unwrap();
        Ok(storage.contains_key(storage_location))
    }
}

/// Cloud storage manager that handles chunk lifecycle
pub struct CloudStorageManager {
    storage: Box<dyn CloudStorage>,
}

impl CloudStorageManager {
    /// Create a new cloud storage manager with S3
    pub async fn new_s3(config: S3Config) -> Result<Self, CloudStorageError> {
        let storage = S3CloudStorage::new(config).await?;
        Ok(CloudStorageManager {
            storage: Box::new(storage),
        })
    }

    /// Create a new cloud storage manager for testing
    pub fn new_mock() -> Self {
        CloudStorageManager {
            storage: Box::new(MockCloudStorage::new()),
        }
    }

    /// Upload a chunk file to cloud storage
    pub async fn upload_chunk_file(&self, chunk_id: &str, file_path: &Path) -> Result<String, CloudStorageError> {
        let data = fs::read(file_path).await?;
        self.storage.upload_chunk(chunk_id, &data).await
    }

    /// Download a chunk from cloud storage to local file
    pub async fn download_chunk_to_file(&self, storage_location: &str, file_path: &Path) -> Result<(), CloudStorageError> {
        let data = self.storage.download_chunk(storage_location).await?;
        fs::write(file_path, data).await?;
        Ok(())
    }

    /// Check if a chunk exists in cloud storage
    pub async fn chunk_exists(&self, storage_location: &str) -> Result<bool, CloudStorageError> {
        self.storage.chunk_exists(storage_location).await
    }

    /// Delete a chunk from cloud storage
    pub async fn delete_chunk(&self, storage_location: &str) -> Result<(), CloudStorageError> {
        self.storage.delete_chunk(storage_location).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;
    use std::io::Write;

    #[tokio::test]
    async fn test_mock_cloud_storage() {
        let storage_manager = CloudStorageManager::new_mock();
        
        // Create test data
        let test_data = b"Hello, cloud storage!";
        let chunk_id = "test_chunk_123";
        
        // Create temporary file
        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(test_data).unwrap();
        temp_file.flush().unwrap();
        
        // Upload chunk
        let storage_location = storage_manager
            .upload_chunk_file(chunk_id, temp_file.path())
            .await
            .unwrap();
        
        assert!(storage_location.starts_with("mock://"));
        
        // Check if chunk exists
        assert!(storage_manager.chunk_exists(&storage_location).await.unwrap());
        
        // Download chunk
        let download_file = NamedTempFile::new().unwrap();
        storage_manager
            .download_chunk_to_file(&storage_location, download_file.path())
            .await
            .unwrap();
        
        // Verify downloaded data
        let downloaded_data = std::fs::read(download_file.path()).unwrap();
        assert_eq!(downloaded_data, test_data);
        
        // Delete chunk
        storage_manager.delete_chunk(&storage_location).await.unwrap();
        
        // Verify chunk is deleted
        assert!(!storage_manager.chunk_exists(&storage_location).await.unwrap());
    }

    #[tokio::test]
    async fn test_s3_config_for_testing() {
        let config = S3Config::for_testing();
        assert_eq!(config.bucket_name, "test-bucket");
        assert_eq!(config.region, "us-east-1");
    }
}
