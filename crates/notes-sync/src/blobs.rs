//! Image/asset sync via iroh-blobs.
//!
//! Images are stored as content-addressed blobs. When a user pastes an image:
//! 1. The image bytes are added to the local blob store
//! 2. The blake3 hash is stored in the Automerge document as a reference
//! 3. When syncing, missing blobs are fetched from peers on-demand

pub use iroh_blobs::Hash;

/// Re-export the ALPN for blob protocol registration.
pub fn blob_alpn() -> &'static [u8] {
    iroh_blobs::ALPN
}

/// Convert a Hash to a hex string for storage in Automerge documents.
pub fn hash_to_hex(hash: &Hash) -> String {
    hash.to_hex()
}

/// Parse a 32-byte array back to a Hash.
pub fn hash_from_bytes(bytes: &[u8; 32]) -> Hash {
    Hash::from(*bytes)
}

/// Compute a blob hash from raw data (blake3).
pub fn hash_data(data: &[u8]) -> Hash {
    Hash::new(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_data() {
        let h1 = hash_data(b"test image data");
        let h2 = hash_data(b"test image data");
        assert_eq!(h1, h2);

        let h3 = hash_data(b"different data");
        assert_ne!(h1, h3);
    }

    #[test]
    fn test_hash_hex() {
        let hash = hash_data(b"test");
        let hex = hash_to_hex(&hash);
        assert_eq!(hex.len(), 64); // 32 bytes = 64 hex chars
    }

    #[test]
    fn test_hash_from_bytes() {
        let hash = hash_data(b"test");
        let bytes = *hash.as_bytes();
        let recovered = hash_from_bytes(&bytes);
        assert_eq!(hash, recovered);
    }
}
