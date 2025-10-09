use aws_config::{BehaviorVersion, Region};
use aws_credential_types::Credentials;
use aws_sdk_s3::{primitives::ByteStreamError, Client, Error as S3Error};
use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;
use tokio::fs;

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
    #[error("Download error: {0}")]
    Download(String),
}

/// S3 configuration for cloud storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3Config {
    pub bucket_name: String,
    pub region: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub endpoint_url: Option<String>, // For MinIO/S3-compatible services
}

impl S3Config {
    pub fn validate(&self) -> Result<(), CloudStorageError> {
        if self.bucket_name.trim().is_empty() {
            return Err(CloudStorageError::Config(
                "Bucket name cannot be empty".to_string(),
            ));
        }
        if self.region.trim().is_empty() {
            return Err(CloudStorageError::Config(
                "Region cannot be empty".to_string(),
            ));
        }
        if self.access_key_id.trim().is_empty() {
            return Err(CloudStorageError::Config(
                "Access key ID cannot be empty".to_string(),
            ));
        }
        if self.secret_access_key.trim().is_empty() {
            return Err(CloudStorageError::Config(
                "Secret access key cannot be empty".to_string(),
            ));
        }
        Ok(())
    }
}

/// Trait for cloud storage operations (allows mocking for tests)
#[async_trait::async_trait]
pub trait CloudStorage: Send + Sync {
    async fn upload_chunk(&self, chunk_id: &str, data: &[u8]) -> Result<String, CloudStorageError>;
    async fn download_chunk(&self, storage_location: &str) -> Result<Vec<u8>, CloudStorageError>;
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
            "bae-s3-config",
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

    /// Generate S3 key for a chunk using hash-based partitioning
    /// Example: chunk_abcd1234-5678-9abc-def0-123456789abc -> chunks/ab/cd/chunk_abcd1234-5678-9abc-def0-123456789abc.enc
    fn chunk_key(&self, chunk_id: &str) -> String {
        if chunk_id.len() < 4 {
            // Fallback for malformed chunk IDs
            return format!("chunks/misc/{}.enc", chunk_id);
        }

        let prefix = &chunk_id[..2]; // First 2 chars: "ab"
        let subprefix = &chunk_id[2..4]; // Next 2 chars: "cd"
        format!("chunks/{}/{}/{}.enc", prefix, subprefix, chunk_id)
    }
}

#[async_trait::async_trait]
impl CloudStorage for S3CloudStorage {
    async fn upload_chunk(&self, chunk_id: &str, data: &[u8]) -> Result<String, CloudStorageError> {
        let key = self.chunk_key(chunk_id);

        println!(
            "S3CloudStorage: Uploading chunk {} ({} bytes)",
            chunk_id,
            data.len()
        );

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
        println!(
            "S3CloudStorage: Successfully uploaded chunk to {}",
            storage_location
        );

        Ok(storage_location)
    }

    async fn download_chunk(&self, storage_location: &str) -> Result<Vec<u8>, CloudStorageError> {
        // Parse S3 location: s3://bucket/key
        let key = storage_location
            .strip_prefix(&format!("s3://{}/", self.bucket_name))
            .ok_or_else(|| {
                CloudStorageError::Download(format!("Invalid S3 location: {}", storage_location))
            })?;

        println!(
            "S3CloudStorage: Downloading chunk from {}",
            storage_location
        );

        let response = self
            .client
            .get_object()
            .bucket(&self.bucket_name)
            .key(key)
            .send()
            .await
            .map_err(|e| CloudStorageError::SdkError(format!("Get object failed: {}", e)))?;

        let data = response.body.collect().await?.into_bytes().to_vec();

        println!(
            "S3CloudStorage: Successfully downloaded {} bytes",
            data.len()
        );
        Ok(data)
    }
}

/// Cloud storage manager that handles chunk lifecycle
#[derive(Clone)]
pub struct CloudStorageManager {
    storage: std::sync::Arc<dyn CloudStorage>,
}

impl std::fmt::Debug for CloudStorageManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CloudStorageManager")
            .field("storage", &"<dyn CloudStorage>")
            .finish()
    }
}

impl CloudStorageManager {
    /// Create a new cloud storage manager with S3 configuration
    pub async fn new(config: S3Config) -> Result<Self, CloudStorageError> {
        let storage = S3CloudStorage::new(config).await?;
        Ok(CloudStorageManager {
            storage: std::sync::Arc::new(storage),
        })
    }

    /// Upload a chunk file to cloud storage
    pub async fn upload_chunk_file(
        &self,
        chunk_id: &str,
        file_path: &Path,
    ) -> Result<String, CloudStorageError> {
        let data = fs::read(file_path).await?;
        self.storage.upload_chunk(chunk_id, &data).await
    }

    /// Download chunk data from cloud storage
    pub async fn download_chunk(
        &self,
        storage_location: &str,
    ) -> Result<Vec<u8>, CloudStorageError> {
        self.storage.download_chunk(storage_location).await
    }
}
