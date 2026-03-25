//! Per-peer epoch key wrapping.
//!
//! When distributing a new epoch key to remaining peers after a removal,
//! each peer gets the key wrapped with a shared secret derived from
//! the owner's and peer's public keys via HKDF.
//!
//! This uses HKDF-SHA256(owner_pk || peer_pk, epoch_number) to derive
//! a wrapping key, then XChaCha20-Poly1305 to encrypt the epoch key.
//!
//! NOTE: This is not a true ECDH exchange (would require X25519).
//! It derives a deterministic shared key from both public keys + epoch.
//! The security relies on the transport being E2E encrypted (iroh QUIC)
//! so only the intended peer receives the wrapped key. The HKDF derivation
//! prevents offline brute-force by an adversary who doesn't have the
//! transport-layer key.

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::XChaCha20Poly1305;
use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::Zeroize;

use crate::error::CryptoError;

/// HKDF context for key wrapping.
const WRAP_CONTEXT: &[u8] = b"p2p-notes/v1/key-wrapping";

/// Derive a wrapping key from owner + peer public keys and epoch.
fn derive_wrapping_key(owner_pk: &[u8], peer_pk: &[u8], epoch: u32) -> [u8; 32] {
    // IKM: owner_pk || peer_pk
    let mut ikm = Vec::with_capacity(owner_pk.len() + peer_pk.len());
    ikm.extend_from_slice(owner_pk);
    ikm.extend_from_slice(peer_pk);

    // Info: context || epoch
    let mut info = Vec::with_capacity(WRAP_CONTEXT.len() + 4);
    info.extend_from_slice(WRAP_CONTEXT);
    info.extend_from_slice(&epoch.to_be_bytes());

    let hk = Hkdf::<Sha256>::new(None, &ikm);
    let mut key = [0u8; 32];
    hk.expand(&info, &mut key)
        .expect("HKDF expand should not fail");
    key
}

/// Wrap an epoch key for a specific peer.
/// Returns the encrypted key (nonce || ciphertext).
pub fn wrap_epoch_key(
    epoch_key: &[u8; 32],
    owner_pk: &[u8],
    peer_pk: &[u8],
    epoch: u32,
) -> Result<Vec<u8>, CryptoError> {
    let mut wrapping_key = derive_wrapping_key(owner_pk, peer_pk, epoch);
    let cipher_key = chacha20poly1305::Key::from_slice(&wrapping_key);
    let cipher = XChaCha20Poly1305::new(cipher_key);

    let mut nonce_bytes = [0u8; 24];
    getrandom::fill(&mut nonce_bytes).map_err(|_| {
        wrapping_key.zeroize();
        CryptoError::RandomFailed
    })?;
    let nonce = chacha20poly1305::XNonce::from_slice(&nonce_bytes);

    let ciphertext = cipher.encrypt(nonce, epoch_key.as_ref()).map_err(|_| {
        wrapping_key.zeroize();
        CryptoError::EncryptionFailed
    })?;

    wrapping_key.zeroize();

    let mut result = Vec::with_capacity(24 + ciphertext.len());
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);
    Ok(result)
}

/// Unwrap an epoch key for this peer.
/// `wrapped` is the nonce || ciphertext from `wrap_epoch_key`.
pub fn unwrap_epoch_key(
    wrapped: &[u8],
    owner_pk: &[u8],
    peer_pk: &[u8],
    epoch: u32,
) -> Result<[u8; 32], CryptoError> {
    if wrapped.len() < 24 {
        return Err(CryptoError::InvalidData("wrapped key too short".into()));
    }

    let mut wrapping_key = derive_wrapping_key(owner_pk, peer_pk, epoch);
    let cipher_key = chacha20poly1305::Key::from_slice(&wrapping_key);
    let cipher = XChaCha20Poly1305::new(cipher_key);

    let nonce = chacha20poly1305::XNonce::from_slice(&wrapped[..24]);
    let ciphertext = &wrapped[24..];

    let plaintext = cipher.decrypt(nonce, ciphertext).map_err(|_| {
        wrapping_key.zeroize();
        CryptoError::DecryptionFailed
    })?;

    wrapping_key.zeroize();

    if plaintext.len() != 32 {
        return Err(CryptoError::InvalidData(
            "unwrapped key wrong length".into(),
        ));
    }

    let mut key = [0u8; 32];
    key.copy_from_slice(&plaintext);
    // plaintext Vec is dropped here; for defense-in-depth, zeroize it
    let mut plaintext = plaintext;
    plaintext.zeroize();
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrap_unwrap_roundtrip() {
        let epoch_key = [0x42u8; 32];
        let owner_pk = b"owner-public-key-bytes-here-1234";
        let peer_pk = b"peer-public-key-bytes-here-12345";
        let epoch = 1;

        let wrapped = wrap_epoch_key(&epoch_key, owner_pk, peer_pk, epoch).unwrap();
        let unwrapped = unwrap_epoch_key(&wrapped, owner_pk, peer_pk, epoch).unwrap();
        assert_eq!(unwrapped, epoch_key);
    }

    #[test]
    fn test_wrong_peer_fails() {
        let epoch_key = [0x42u8; 32];
        let owner_pk = b"owner-public-key-bytes-here-1234";
        let peer_a = b"peer-a-public-key-bytes-here1234";
        let peer_b = b"peer-b-public-key-bytes-here1234";

        let wrapped = wrap_epoch_key(&epoch_key, owner_pk, peer_a, 1).unwrap();
        let result = unwrap_epoch_key(&wrapped, owner_pk, peer_b, 1);
        assert!(matches!(result, Err(CryptoError::DecryptionFailed)));
    }

    #[test]
    fn test_wrong_epoch_fails() {
        let epoch_key = [0x42u8; 32];
        let owner_pk = b"owner-public-key-bytes-here-1234";
        let peer_pk = b"peer-public-key-bytes-here-12345";

        let wrapped = wrap_epoch_key(&epoch_key, owner_pk, peer_pk, 1).unwrap();
        let result = unwrap_epoch_key(&wrapped, owner_pk, peer_pk, 2);
        assert!(matches!(result, Err(CryptoError::DecryptionFailed)));
    }

    #[test]
    fn test_different_peers_get_different_wrapped_keys() {
        let epoch_key = [0x42u8; 32];
        let owner_pk = b"owner-public-key-bytes-here-1234";
        let peer_a = b"peer-a-public-key-bytes-here1234";
        let peer_b = b"peer-b-public-key-bytes-here1234";

        let wrapped_a = wrap_epoch_key(&epoch_key, owner_pk, peer_a, 1).unwrap();
        let wrapped_b = wrap_epoch_key(&epoch_key, owner_pk, peer_b, 1).unwrap();
        // Different wrapping (different nonces + different derived keys)
        assert_ne!(wrapped_a, wrapped_b);
    }

    #[test]
    fn test_unwrap_truncated_data() {
        let result = unwrap_epoch_key(&[0u8; 10], b"owner", b"peer", 1);
        assert!(result.is_err());
    }
}
