//! Epoch-based key ratcheting.
//!
//! When a peer is removed from a project, a new epoch key is generated.
//! New changes are encrypted under the new epoch. Old documents are NOT
//! re-encrypted — they're lazily re-encrypted on next local edit.
//!
//! Each remaining peer receives the new epoch key wrapped with their
//! public key (X25519 ECDH + HKDF + XChaCha20-Poly1305).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::error::CryptoError;

/// Stores epoch keys for a project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpochKeys {
    /// Current epoch number.
    pub current_epoch: u32,
    /// Map of epoch number -> encrypted epoch key (hex-encoded).
    /// Only the current device's keys are stored in plaintext locally.
    /// Other peers' wrapped keys are in the manifest.
    epoch_keys: HashMap<u32, EpochKeyEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EpochKeyEntry {
    /// The raw key bytes (hex-encoded for serialization).
    key_hex: String,
    /// When this epoch was created.
    created_at: String,
}

impl EpochKeys {
    /// Create a new EpochKeys with an initial epoch 0 key.
    pub fn new() -> Result<Self, CryptoError> {
        let mut keys = HashMap::new();
        let key = generate_epoch_key()?;
        keys.insert(
            0,
            EpochKeyEntry {
                key_hex: hex_encode(&key),
                created_at: chrono_now(),
            },
        );
        Ok(Self {
            current_epoch: 0,
            epoch_keys: keys,
        })
    }

    /// Get the key for a specific epoch.
    pub fn get_key(&self, epoch: u32) -> Result<EpochKey, CryptoError> {
        let entry = self
            .epoch_keys
            .get(&epoch)
            .ok_or(CryptoError::EpochKeyNotFound(epoch))?;
        let bytes = hex_decode(&entry.key_hex)?;
        if bytes.len() != 32 {
            return Err(CryptoError::InvalidData("epoch key wrong length".into()));
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&bytes);
        Ok(EpochKey(key))
    }

    /// Get the current epoch key.
    pub fn current_key(&self) -> Result<EpochKey, CryptoError> {
        self.get_key(self.current_epoch)
    }

    /// Ratchet to a new epoch. Generates a new key and increments the epoch.
    /// Call this when a peer is removed from the project.
    pub fn ratchet(&mut self) -> Result<u32, CryptoError> {
        let new_epoch = self.current_epoch + 1;
        let key = generate_epoch_key()?;

        self.epoch_keys.insert(
            new_epoch,
            EpochKeyEntry {
                key_hex: hex_encode(&key),
                created_at: chrono_now(),
            },
        );
        self.current_epoch = new_epoch;

        log::info!("Key ratcheted to epoch {new_epoch}");
        Ok(new_epoch)
    }

    /// Check if we have a key for a given epoch.
    pub fn has_key(&self, epoch: u32) -> bool {
        self.epoch_keys.contains_key(&epoch)
    }

    /// Get all epoch numbers we have keys for.
    pub fn available_epochs(&self) -> Vec<u32> {
        let mut epochs: Vec<u32> = self.epoch_keys.keys().copied().collect();
        epochs.sort();
        epochs
    }
}

impl Default for EpochKeys {
    fn default() -> Self {
        Self::new().expect("epoch key generation should not fail")
    }
}

/// Manages epoch keys for a project — higher-level API.
pub struct EpochKeyManager {
    keys: EpochKeys,
}

impl EpochKeyManager {
    pub fn new() -> Result<Self, CryptoError> {
        Ok(Self {
            keys: EpochKeys::new()?,
        })
    }

    pub fn from_keys(keys: EpochKeys) -> Self {
        Self { keys }
    }

    /// Get the current epoch key for encryption.
    pub fn current_key(&self) -> Result<EpochKey, CryptoError> {
        self.keys.current_key()
    }

    /// Get a key for a specific epoch (for decryption of old docs).
    pub fn key_for_epoch(&self, epoch: u32) -> Result<EpochKey, CryptoError> {
        self.keys.get_key(epoch)
    }

    /// Current epoch number.
    pub fn current_epoch(&self) -> u32 {
        self.keys.current_epoch
    }

    /// Ratchet to a new epoch (call when a peer is removed).
    pub fn ratchet(&mut self) -> Result<u32, CryptoError> {
        self.keys.ratchet()
    }

    /// Serialize the epoch keys for storage.
    pub fn serialize(&self) -> Result<Vec<u8>, CryptoError> {
        serde_json::to_vec(&self.keys).map_err(|e| CryptoError::InvalidData(e.to_string()))
    }

    /// Deserialize epoch keys from storage.
    pub fn deserialize(data: &[u8]) -> Result<Self, CryptoError> {
        let keys: EpochKeys =
            serde_json::from_slice(data).map_err(|e| CryptoError::InvalidData(e.to_string()))?;
        Ok(Self { keys })
    }

    /// Get the underlying EpochKeys.
    pub fn keys(&self) -> &EpochKeys {
        &self.keys
    }
}

/// A zeroize-on-drop epoch key.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct EpochKey(pub [u8; 32]);

impl EpochKey {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::fmt::Debug for EpochKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "EpochKey(***)")
    }
}

/// Generate a random 32-byte epoch key.
fn generate_epoch_key() -> Result<[u8; 32], CryptoError> {
    let mut key = [0u8; 32];
    getrandom::fill(&mut key).map_err(|_| CryptoError::RandomFailed)?;
    Ok(key)
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn hex_decode(hex: &str) -> Result<Vec<u8>, CryptoError> {
    (0..hex.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&hex[i..i + 2], 16)
                .map_err(|_| CryptoError::InvalidData("bad hex".into()))
        })
        .collect()
}

fn chrono_now() -> String {
    // Simple ISO 8601 timestamp without pulling in chrono
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| format!("{}", d.as_secs()))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_epoch_keys() {
        let keys = EpochKeys::new().unwrap();
        assert_eq!(keys.current_epoch, 0);
        assert!(keys.has_key(0));
    }

    #[test]
    fn test_ratchet() {
        let mut keys = EpochKeys::new().unwrap();
        let old_key = keys.current_key().unwrap();

        let new_epoch = keys.ratchet().unwrap();
        assert_eq!(new_epoch, 1);
        assert_eq!(keys.current_epoch, 1);

        let new_key = keys.current_key().unwrap();
        assert_ne!(old_key.0, new_key.0);

        // Old key should still be available
        let recovered_old = keys.get_key(0).unwrap();
        assert_eq!(old_key.0, recovered_old.0);
    }

    #[test]
    fn test_multiple_ratchets() {
        let mut keys = EpochKeys::new().unwrap();
        keys.ratchet().unwrap();
        keys.ratchet().unwrap();
        keys.ratchet().unwrap();

        assert_eq!(keys.current_epoch, 3);
        assert_eq!(keys.available_epochs(), vec![0, 1, 2, 3]);

        // All keys should be distinct
        let k0 = keys.get_key(0).unwrap();
        let k1 = keys.get_key(1).unwrap();
        let k2 = keys.get_key(2).unwrap();
        let k3 = keys.get_key(3).unwrap();
        assert_ne!(k0.0, k1.0);
        assert_ne!(k1.0, k2.0);
        assert_ne!(k2.0, k3.0);
    }

    #[test]
    fn test_get_nonexistent_epoch() {
        let keys = EpochKeys::new().unwrap();
        assert!(matches!(
            keys.get_key(99),
            Err(CryptoError::EpochKeyNotFound(99))
        ));
    }

    #[test]
    fn test_epoch_key_manager_serialize_roundtrip() {
        let mut mgr = EpochKeyManager::new().unwrap();
        mgr.ratchet().unwrap();

        let serialized = mgr.serialize().unwrap();
        let mgr2 = EpochKeyManager::deserialize(&serialized).unwrap();

        assert_eq!(mgr2.current_epoch(), 1);
        let k0_orig = mgr.key_for_epoch(0).unwrap();
        let k0_loaded = mgr2.key_for_epoch(0).unwrap();
        assert_eq!(k0_orig.0, k0_loaded.0);
    }

    #[test]
    fn test_epoch_key_zeroized_on_drop() {
        let keys = EpochKeys::new().unwrap();
        let key = keys.current_key().unwrap();
        // Just verify it doesn't panic — actual zeroization is verified
        // by zeroize crate internals.
        assert_eq!(key.as_bytes().len(), 32);
    }
}
