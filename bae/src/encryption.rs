use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum EncryptionError {
    #[error("Encryption failed: {0}")]
    Encryption(String),
    #[error("Decryption failed: {0}")]
    Decryption(String),
    #[error("Key management error: {0}")]
    KeyManagement(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Manages encryption keys and provides AES-256-GCM encryption/decryption
///
/// This implements the security model described in the README:
/// - Files are split into chunks and each chunk is encrypted separately
/// - Uses AES-256-GCM for authenticated encryption
/// - Each chunk gets a unique nonce for security
#[derive(Clone)]
pub struct EncryptionService {
    cipher: Aes256Gcm,
}

impl std::fmt::Debug for EncryptionService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EncryptionService")
            .field("cipher", &"<initialized>")
            .finish()
    }
}

impl EncryptionService {
    /// Create a new encryption service, loading the key from config
    pub fn new(config: &crate::config::Config) -> Result<Self, EncryptionError> {
        println!("EncryptionService: Loading master key...");

        // Decode hex key
        let key_bytes = hex::decode(&config.encryption_key)
            .map_err(|e| EncryptionError::KeyManagement(format!("Invalid key format: {}", e)))?;

        if key_bytes.len() != 32 {
            return Err(EncryptionError::KeyManagement(
                "Invalid key length, expected 32 bytes".to_string(),
            ));
        }

        let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
        let cipher = Aes256Gcm::new(key);

        Ok(EncryptionService { cipher })
    }

    /// Encrypt data with AES-256-GCM
    /// Returns (encrypted_data, nonce) - both needed for decryption
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>), EncryptionError> {
        // Generate a unique nonce for this encryption
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);

        // Encrypt the data
        let ciphertext = self.cipher.encrypt(&nonce, plaintext).map_err(|e| {
            EncryptionError::Encryption(format!("AES-GCM encryption failed: {}", e))
        })?;

        Ok((ciphertext, nonce.to_vec()))
    }

    /// Decrypt data with AES-256-GCM
    /// Requires both the encrypted data and the nonce used during encryption
    pub fn decrypt(&self, ciphertext: &[u8], nonce: &[u8]) -> Result<Vec<u8>, EncryptionError> {
        // Convert nonce bytes back to Nonce
        if nonce.len() != 12 {
            return Err(EncryptionError::Decryption(
                "Invalid nonce length, expected 12 bytes".to_string(),
            ));
        }

        let nonce = Nonce::from_slice(nonce);

        // Decrypt the data
        let plaintext = self.cipher.decrypt(nonce, ciphertext).map_err(|e| {
            EncryptionError::Decryption(format!("AES-GCM decryption failed: {}", e))
        })?;

        Ok(plaintext)
    }

    /// Decrypt a chunk from its serialized format
    /// This reads the chunk file and decrypts it back to original data
    pub fn decrypt_chunk(&self, chunk_bytes: &[u8]) -> Result<Vec<u8>, EncryptionError> {
        // Parse the encrypted chunk from bytes
        let encrypted_chunk = EncryptedChunk::from_bytes(chunk_bytes)?;

        // Note: We don't verify key_id anymore since we only have one master key per app
        // The encrypted_chunk still stores it for backward compatibility

        // Decrypt using the nonce and encrypted data
        self.decrypt(&encrypted_chunk.encrypted_data, &encrypted_chunk.nonce)
    }
}

/// Encrypted chunk format that includes all data needed for decryption
#[derive(Debug, Clone)]
pub struct EncryptedChunk {
    pub encrypted_data: Vec<u8>,
    pub nonce: Vec<u8>,
    pub key_id: String,
}

impl EncryptedChunk {
    /// Create a new encrypted chunk
    pub fn new(encrypted_data: Vec<u8>, nonce: Vec<u8>, key_id: String) -> Self {
        EncryptedChunk {
            encrypted_data,
            nonce,
            key_id,
        }
    }

    /// Serialize the encrypted chunk to bytes for storage
    /// Format: [nonce_len(4)][nonce][key_id_len(4)][key_id][encrypted_data]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();

        // Write nonce length and nonce
        bytes.extend_from_slice(&(self.nonce.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&self.nonce);

        // Write key ID length and key ID
        let key_id_bytes = self.key_id.as_bytes();
        bytes.extend_from_slice(&(key_id_bytes.len() as u32).to_le_bytes());
        bytes.extend_from_slice(key_id_bytes);

        // Write encrypted data
        bytes.extend_from_slice(&self.encrypted_data);

        bytes
    }

    /// Deserialize encrypted chunk from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, EncryptionError> {
        if bytes.len() < 8 {
            return Err(EncryptionError::Decryption(
                "Invalid chunk format".to_string(),
            ));
        }

        let mut offset = 0;

        // Read nonce length and nonce
        let nonce_len = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        offset += 4;

        if offset + nonce_len > bytes.len() {
            return Err(EncryptionError::Decryption(
                "Invalid nonce length".to_string(),
            ));
        }

        let nonce = bytes[offset..offset + nonce_len].to_vec();
        offset += nonce_len;

        // Read key ID length and key ID
        if offset + 4 > bytes.len() {
            return Err(EncryptionError::Decryption(
                "Invalid key ID format".to_string(),
            ));
        }

        let key_id_len = u32::from_le_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ]) as usize;
        offset += 4;

        if offset + key_id_len > bytes.len() {
            return Err(EncryptionError::Decryption(
                "Invalid key ID length".to_string(),
            ));
        }

        let key_id = String::from_utf8(bytes[offset..offset + key_id_len].to_vec())
            .map_err(|e| EncryptionError::Decryption(format!("Invalid key ID: {}", e)))?;
        offset += key_id_len;

        // Read encrypted data
        let encrypted_data = bytes[offset..].to_vec();

        Ok(EncryptedChunk {
            encrypted_data,
            nonce,
            key_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a test encryption service with a pre-populated test key (avoids keyring)
    fn create_test_encryption_service() -> EncryptionService {
        // Generate a test encryption key
        let test_key = Aes256Gcm::generate_key(OsRng);
        let test_key_hex = hex::encode(test_key.as_slice());

        // Create a test config with the generated key
        let test_config = crate::config::Config {
            library_id: "test-library".to_string(),
            discogs_api_key: "test-key".to_string(),
            s3_config: crate::cloud_storage::S3Config {
                bucket_name: "test-bucket".to_string(),
                region: "us-east-1".to_string(),
                access_key_id: "test-access".to_string(),
                secret_access_key: "test-secret".to_string(),
                endpoint_url: None,
            },
            encryption_key: test_key_hex,
        };

        EncryptionService::new(&test_config).expect("Failed to create test encryption service")
    }

    #[test]
    fn test_encryption_roundtrip() {
        let encryption_service = create_test_encryption_service();
        let plaintext = b"Hello, world! This is a test message for encryption.";

        // Encrypt
        let (ciphertext, nonce) = encryption_service.encrypt(plaintext).unwrap();

        // Verify ciphertext is different from plaintext
        assert_ne!(ciphertext, plaintext);
        assert_eq!(nonce.len(), 12); // AES-GCM nonce is 12 bytes

        // Decrypt
        let decrypted = encryption_service.decrypt(&ciphertext, &nonce).unwrap();

        // Verify decryption matches original
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_encrypted_chunk_serialization() {
        let chunk = EncryptedChunk::new(
            vec![1, 2, 3, 4, 5],
            vec![6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17], // 12 bytes
            "test_key_id".to_string(),
        );

        // Serialize
        let bytes = chunk.to_bytes();

        // Deserialize
        let deserialized = EncryptedChunk::from_bytes(&bytes).unwrap();

        // Verify
        assert_eq!(deserialized.encrypted_data, chunk.encrypted_data);
        assert_eq!(deserialized.nonce, chunk.nonce);
        assert_eq!(deserialized.key_id, chunk.key_id);
    }

    #[test]
    fn test_different_nonces() {
        let encryption_service = create_test_encryption_service();
        let plaintext = b"Same message";

        // Encrypt twice
        let (ciphertext1, nonce1) = encryption_service.encrypt(plaintext).unwrap();
        let (ciphertext2, nonce2) = encryption_service.encrypt(plaintext).unwrap();

        // Nonces should be different
        assert_ne!(nonce1, nonce2);
        // Ciphertexts should be different (due to different nonces)
        assert_ne!(ciphertext1, ciphertext2);

        // Both should decrypt to the same plaintext
        let decrypted1 = encryption_service.decrypt(&ciphertext1, &nonce1).unwrap();
        let decrypted2 = encryption_service.decrypt(&ciphertext2, &nonce2).unwrap();

        assert_eq!(decrypted1, plaintext);
        assert_eq!(decrypted2, plaintext);
    }
}
