//! Ed25519 signing and verification for CRDT changes.
//!
//! Each Automerge change is wrapped in a signed envelope containing:
//! - The author's public key (EndpointId)
//! - The Ed25519 signature over the raw change bytes
//! - The raw change bytes
//!
//! On sync receive, the signature is verified against the project's ACL.

use iroh::{EndpointId, SecretKey};
use serde::{Deserialize, Serialize};

use crate::error::CryptoError;

/// A signed change envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedChange {
    /// The author's public key (hex-encoded EndpointId).
    pub author: String,
    /// Ed25519 signature over `data` (hex-encoded).
    pub signature: String,
    /// The raw Automerge change bytes (hex-encoded).
    pub data: String,
}

impl SignedChange {
    /// Sign raw change data with the device's secret key.
    pub fn sign(secret_key: &SecretKey, data: &[u8]) -> Self {
        let sig = secret_key.sign(data);
        Self {
            author: secret_key.public().to_string(),
            signature: hex_encode(sig.to_bytes().as_ref()),
            data: hex_encode(data),
        }
    }

    /// Verify the signature and return the raw change data.
    pub fn verify(&self) -> Result<Vec<u8>, CryptoError> {
        let author: EndpointId = self
            .author
            .parse()
            .map_err(|_| CryptoError::InvalidData("invalid author public key".into()))?;

        let sig_bytes = hex_decode(&self.signature)?;
        let sig = iroh::Signature::from_bytes(
            sig_bytes
                .as_slice()
                .try_into()
                .map_err(|_| CryptoError::InvalidData("signature wrong length".into()))?,
        );

        let data = hex_decode(&self.data)?;

        author
            .verify(&data, &sig)
            .map_err(|_| CryptoError::InvalidData("signature verification failed".into()))?;

        Ok(data)
    }

    /// Get the author's EndpointId.
    pub fn author_id(&self) -> Result<EndpointId, CryptoError> {
        self.author
            .parse()
            .map_err(|_| CryptoError::InvalidData("invalid author public key".into()))
    }

    /// Serialize to bytes (for transmission).
    pub fn to_bytes(&self) -> Result<Vec<u8>, CryptoError> {
        serde_json::to_vec(self).map_err(|e| CryptoError::InvalidData(e.to_string()))
    }

    /// Deserialize from bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self, CryptoError> {
        serde_json::from_slice(data).map_err(|e| CryptoError::InvalidData(e.to_string()))
    }
}

/// Verify that a signed change is from an authorized peer.
/// Returns the author's EndpointId and the raw change data.
pub fn verify_and_check_acl(
    signed: &SignedChange,
    allowed_peers: &[EndpointId],
) -> Result<(EndpointId, Vec<u8>), CryptoError> {
    let data = signed.verify()?;
    let author = signed.author_id()?;

    if !allowed_peers.contains(&author) {
        return Err(CryptoError::InvalidData(format!(
            "author {} is not in the project ACL",
            signed.author
        )));
    }

    Ok((author, data))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_keypair() -> SecretKey {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).unwrap();
        SecretKey::from_bytes(&bytes)
    }

    #[test]
    fn test_sign_and_verify() {
        let key = test_keypair();
        let data = b"test automerge change data";

        let signed = SignedChange::sign(&key, data);
        let recovered = signed.verify().unwrap();
        assert_eq!(recovered, data);
    }

    #[test]
    fn test_verify_wrong_data_fails() {
        let key = test_keypair();
        let mut signed = SignedChange::sign(&key, b"original data");
        // Tamper with the data
        signed.data = hex_encode(b"tampered data");
        assert!(signed.verify().is_err());
    }

    #[test]
    fn test_verify_and_check_acl_success() {
        let key = test_keypair();
        let signed = SignedChange::sign(&key, b"data");

        let allowed = vec![key.public()];
        let (author, data) = verify_and_check_acl(&signed, &allowed).unwrap();
        assert_eq!(author, key.public());
        assert_eq!(data, b"data");
    }

    #[test]
    fn test_verify_and_check_acl_unauthorized() {
        let key = test_keypair();
        let other_key = test_keypair();
        let signed = SignedChange::sign(&key, b"data");

        // Only other_key is allowed, not the signer
        let allowed = vec![other_key.public()];
        assert!(verify_and_check_acl(&signed, &allowed).is_err());
    }

    #[test]
    fn test_serde_roundtrip() {
        let key = test_keypair();
        let signed = SignedChange::sign(&key, b"test data");

        let bytes = signed.to_bytes().unwrap();
        let loaded = SignedChange::from_bytes(&bytes).unwrap();
        let recovered = loaded.verify().unwrap();
        assert_eq!(recovered, b"test data");
    }
}
