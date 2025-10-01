use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Nonce, Key
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

/// Trait for key storage backends
trait KeyStorage: Send + Sync {
    fn store_key(&self, key_id: &str, key: &Key<Aes256Gcm>) -> Result<(), EncryptionError>;
    fn load_key(&self, key_id: &str) -> Result<Key<Aes256Gcm>, EncryptionError>;
}

/// Production key storage using system keyring
struct KeyringStorage;

impl KeyStorage for KeyringStorage {
    fn store_key(&self, key_id: &str, key: &Key<Aes256Gcm>) -> Result<(), EncryptionError> {
        use keyring::Entry;
        
        let entry = Entry::new("bae", key_id)
            .map_err(|e| EncryptionError::KeyManagement(format!("Failed to access keyring: {}", e)))?;
        
        // Store key as hex string
        let key_hex = hex::encode(key.as_slice());
        
        // Try to store the key, but ignore "already exists" errors
        match entry.set_password(&key_hex) {
            Ok(_) => {
                println!("EncryptionService: Master key stored securely in system keyring");
                Ok(())
            }
            Err(e) => {
                let error_msg = format!("{}", e);
                if error_msg.contains("already exists") {
                    // Key already exists, which is fine
                    Ok(())
                } else {
                    Err(EncryptionError::KeyManagement(format!("Failed to store key: {}", e)))
                }
            }
        }
    }

    fn load_key(&self, key_id: &str) -> Result<Key<Aes256Gcm>, EncryptionError> {
        use keyring::Entry;
        
        let entry = Entry::new("bae", key_id)
            .map_err(|e| EncryptionError::KeyManagement(format!("Failed to access keyring: {}", e)))?;
        
        let key_bytes = entry.get_password()
            .map_err(|e| EncryptionError::KeyManagement(format!("Failed to load key: {}", e)))?;
        
        // Decode hex string back to bytes
        let key_bytes = hex::decode(key_bytes)
            .map_err(|e| EncryptionError::KeyManagement(format!("Invalid key format: {}", e)))?;
        
        if key_bytes.len() != 32 {
            return Err(EncryptionError::KeyManagement(
                "Invalid key length, expected 32 bytes".to_string()
            ));
        }
        
        Ok(*Key::<Aes256Gcm>::from_slice(&key_bytes))
    }
}

#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::sync::{Arc, Mutex};

/// In-memory key storage for testing
#[cfg(test)]
#[derive(Clone)]
struct InMemoryStorage {
    keys: Arc<Mutex<HashMap<String, String>>>,
}

#[cfg(test)]
impl InMemoryStorage {
    fn new() -> Self {
        InMemoryStorage {
            keys: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[cfg(test)]
impl KeyStorage for InMemoryStorage {
    fn store_key(&self, key_id: &str, key: &Key<Aes256Gcm>) -> Result<(), EncryptionError> {
        let key_hex = hex::encode(key.as_slice());
        let mut keys = self.keys.lock().unwrap();
        keys.insert(key_id.to_string(), key_hex);
        Ok(())
    }

    fn load_key(&self, key_id: &str) -> Result<Key<Aes256Gcm>, EncryptionError> {
        let keys = self.keys.lock().unwrap();
        let key_hex = keys.get(key_id)
            .ok_or_else(|| EncryptionError::KeyManagement("Key not found".to_string()))?;
        
        let key_bytes = hex::decode(key_hex)
            .map_err(|e| EncryptionError::KeyManagement(format!("Invalid key format: {}", e)))?;
        
        if key_bytes.len() != 32 {
            return Err(EncryptionError::KeyManagement(
                "Invalid key length, expected 32 bytes".to_string()
            ));
        }
        
        Ok(*Key::<Aes256Gcm>::from_slice(&key_bytes))
    }
}

/// Manages encryption keys and provides AES-256-GCM encryption/decryption
/// 
/// This implements the security model described in the README:
/// - Files are split into chunks and each chunk is encrypted separately
/// - Uses AES-256-GCM for authenticated encryption
/// - Master key is stored securely in system keyring
/// - Each chunk gets a unique nonce for security
pub struct EncryptionService {
    cipher: Aes256Gcm,
    key_id: String,
}

impl EncryptionService {
    /// Create a new encryption service with a master key
    pub fn new() -> Result<Self, EncryptionError> {
        Self::new_with_storage("bae_master_encryption_key".to_string(), Box::new(KeyringStorage))
    }

    /// Create a new encryption service with in-memory storage (for testing)
    #[cfg(test)]
    fn new_for_testing(key_id: String) -> Result<Self, EncryptionError> {
        Self::new_with_storage(key_id, Box::new(InMemoryStorage::new()))
    }

    /// Create a new encryption service with custom storage
    fn new_with_storage(key_id: String, storage: Box<dyn KeyStorage>) -> Result<Self, EncryptionError> {
        // Try to load existing key, or generate a new one
        let key = match storage.load_key(&key_id) {
            Ok(key) => key,
            Err(_) => {
                // Generate new master key
                let key = Self::generate_master_key()?;
                storage.store_key(&key_id, &key)?;
                key
            }
        };
        
        let cipher = Aes256Gcm::new(&key);
        
        Ok(EncryptionService { cipher, key_id })
    }

    /// Encrypt data with AES-256-GCM
    /// Returns (encrypted_data, nonce) - both needed for decryption
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>), EncryptionError> {
        // Generate a unique nonce for this encryption
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        
        // Encrypt the data
        let ciphertext = self.cipher
            .encrypt(&nonce, plaintext)
            .map_err(|e| EncryptionError::Encryption(format!("AES-GCM encryption failed: {}", e)))?;
        
        Ok((ciphertext, nonce.to_vec()))
    }

    /// Decrypt data with AES-256-GCM
    /// Requires both the encrypted data and the nonce used during encryption
    pub fn decrypt(&self, ciphertext: &[u8], nonce: &[u8]) -> Result<Vec<u8>, EncryptionError> {
        // Convert nonce bytes back to Nonce
        if nonce.len() != 12 {
            return Err(EncryptionError::Decryption(
                "Invalid nonce length, expected 12 bytes".to_string()
            ));
        }
        
        let nonce = Nonce::from_slice(nonce);
        
        // Decrypt the data
        let plaintext = self.cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| EncryptionError::Decryption(format!("AES-GCM decryption failed: {}", e)))?;
        
        Ok(plaintext)
    }

    /// Decrypt a chunk from its serialized format
    /// This reads the chunk file and decrypts it back to original data
    pub fn decrypt_chunk(&self, chunk_bytes: &[u8]) -> Result<Vec<u8>, EncryptionError> {
        // Parse the encrypted chunk from bytes
        let encrypted_chunk = EncryptedChunk::from_bytes(chunk_bytes)?;
        
        // Verify the key ID matches (for security)
        if encrypted_chunk.key_id != self.key_id {
            return Err(EncryptionError::Decryption(
                format!("Key ID mismatch: expected {}, got {}", self.key_id, encrypted_chunk.key_id)
            ));
        }
        
        // Decrypt using the nonce and encrypted data
        self.decrypt(&encrypted_chunk.encrypted_data, &encrypted_chunk.nonce)
    }

    /// Generate a new 256-bit master key
    fn generate_master_key() -> Result<Key<Aes256Gcm>, EncryptionError> {
        Ok(Aes256Gcm::generate_key(OsRng))
    }

    /// Get the key ID for this encryption service
    pub fn key_id(&self) -> &str {
        &self.key_id
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
            return Err(EncryptionError::Decryption("Invalid chunk format".to_string()));
        }
        
        let mut offset = 0;
        
        // Read nonce length and nonce
        let nonce_len = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        offset += 4;
        
        if offset + nonce_len > bytes.len() {
            return Err(EncryptionError::Decryption("Invalid nonce length".to_string()));
        }
        
        let nonce = bytes[offset..offset + nonce_len].to_vec();
        offset += nonce_len;
        
        // Read key ID length and key ID
        if offset + 4 > bytes.len() {
            return Err(EncryptionError::Decryption("Invalid key ID format".to_string()));
        }
        
        let key_id_len = u32::from_le_bytes([
            bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]
        ]) as usize;
        offset += 4;
        
        if offset + key_id_len > bytes.len() {
            return Err(EncryptionError::Decryption("Invalid key ID length".to_string()));
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
    use uuid::Uuid;

    #[test]
    fn test_encryption_roundtrip() {
        // Use in-memory storage for testing
        let test_key_id = format!("test_key_{}", Uuid::new_v4());
        let encryption_service = EncryptionService::new_for_testing(test_key_id).unwrap();
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
        // Use in-memory storage for testing
        let test_key_id = format!("test_key_nonce_{}", Uuid::new_v4());
        let encryption_service = EncryptionService::new_for_testing(test_key_id).unwrap();
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
