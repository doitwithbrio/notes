//! Per-peer epoch key wrapping using X25519 ECDH.
//!
//! When distributing a new epoch key to remaining peers after a removal,
//! each peer gets the key wrapped with a shared secret derived from
//! X25519 Diffie-Hellman key agreement between the owner and the peer.
//!
//! Security:
//! - X25519 ECDH produces a shared secret known only to the two parties
//! - HKDF-SHA256 derives a wrapping key from the shared secret + context
//! - XChaCha20-Poly1305 provides authenticated encryption of the epoch key
//! - All intermediate key material is zeroized after use

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::XChaCha20Poly1305;
use hkdf::Hkdf;
use sha2::Sha256;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroize;

use crate::error::CryptoError;

/// HKDF context for key wrapping.
const WRAP_CONTEXT: &[u8] = b"p2p-notes/v1/key-wrapping";

/// Derive a wrapping key from an X25519 ECDH shared secret.
///
/// The shared secret is used as IKM for HKDF-SHA256. The info string
/// includes the context, epoch, and both public keys for binding
/// (prevents key confusion attacks where wrapped keys are swapped
/// between peers).
fn derive_wrapping_key(
    shared_secret: &[u8; 32],
    owner_pk: &[u8; 32],
    peer_pk: &[u8; 32],
    epoch: u32,
) -> [u8; 32] {
    // Info: context || epoch || owner_pk || peer_pk
    let mut info = Vec::with_capacity(WRAP_CONTEXT.len() + 4 + 32 + 32);
    info.extend_from_slice(WRAP_CONTEXT);
    info.extend_from_slice(&epoch.to_be_bytes());
    info.extend_from_slice(owner_pk);
    info.extend_from_slice(peer_pk);

    let hk = Hkdf::<Sha256>::new(None, shared_secret);
    let mut key = [0u8; 32];
    hk.expand(&info, &mut key)
        .expect("HKDF expand should not fail");
    key
}

/// Wrap an epoch key for a specific peer using X25519 ECDH.
///
/// The owner's X25519 secret key and the peer's X25519 public key are used
/// to derive a shared secret via Diffie-Hellman. This shared secret is then
/// used to derive a wrapping key via HKDF, which encrypts the epoch key.
///
/// Only the owner and the target peer can unwrap the result.
pub fn wrap_epoch_key(
    epoch_key: &[u8; 32],
    owner_secret: &StaticSecret,
    peer_public: &PublicKey,
    epoch: u32,
) -> Result<Vec<u8>, CryptoError> {
    // X25519 ECDH
    let shared_secret = owner_secret.diffie_hellman(peer_public);
    let owner_pk = PublicKey::from(owner_secret);

    let mut wrapping_key = derive_wrapping_key(
        shared_secret.as_bytes(),
        owner_pk.as_bytes(),
        peer_public.as_bytes(),
        epoch,
    );

    // Zeroize a copy of the shared secret
    let mut ss_copy = *shared_secret.as_bytes();
    ss_copy.zeroize();

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

/// Unwrap an epoch key using X25519 ECDH.
///
/// The peer's X25519 secret key and the owner's X25519 public key are used
/// to derive the same shared secret as the owner.
pub fn unwrap_epoch_key(
    wrapped: &[u8],
    peer_secret: &StaticSecret,
    owner_public: &PublicKey,
    epoch: u32,
) -> Result<[u8; 32], CryptoError> {
    if wrapped.len() < 24 {
        return Err(CryptoError::InvalidData("wrapped key too short".into()));
    }

    // X25519 ECDH (same shared secret, opposite roles)
    let shared_secret = peer_secret.diffie_hellman(owner_public);
    let peer_pk = PublicKey::from(peer_secret);

    let mut wrapping_key = derive_wrapping_key(
        shared_secret.as_bytes(),
        owner_public.as_bytes(),
        peer_pk.as_bytes(),
        epoch,
    );

    let mut ss_copy = *shared_secret.as_bytes();
    ss_copy.zeroize();

    let cipher_key = chacha20poly1305::Key::from_slice(&wrapping_key);
    let cipher = XChaCha20Poly1305::new(cipher_key);

    let nonce = chacha20poly1305::XNonce::from_slice(&wrapped[..24]);
    let ciphertext = &wrapped[24..];

    let mut plaintext = cipher.decrypt(nonce, ciphertext).map_err(|_| {
        wrapping_key.zeroize();
        CryptoError::DecryptionFailed
    })?;

    wrapping_key.zeroize();

    if plaintext.len() != 32 {
        plaintext.zeroize();
        return Err(CryptoError::InvalidData(
            "unwrapped key wrong length".into(),
        ));
    }

    let mut key = [0u8; 32];
    key.copy_from_slice(&plaintext);
    plaintext.zeroize();
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_keypair() -> (StaticSecret, PublicKey) {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).unwrap();
        let secret = StaticSecret::from(bytes);
        let public = PublicKey::from(&secret);
        (secret, public)
    }

    #[test]
    fn test_wrap_unwrap_roundtrip() {
        let epoch_key = [0x42u8; 32];
        let (owner_secret, owner_public) = test_keypair();
        let (peer_secret, peer_public) = test_keypair();
        let epoch = 1;

        let wrapped = wrap_epoch_key(&epoch_key, &owner_secret, &peer_public, epoch).unwrap();
        let unwrapped = unwrap_epoch_key(&wrapped, &peer_secret, &owner_public, epoch).unwrap();
        assert_eq!(unwrapped, epoch_key);
    }

    #[test]
    fn test_wrong_peer_fails() {
        let epoch_key = [0x42u8; 32];
        let (owner_secret, owner_public) = test_keypair();
        let (_peer_a_secret, peer_a_public) = test_keypair();
        let (peer_b_secret, _peer_b_public) = test_keypair();

        // Wrap for peer A
        let wrapped = wrap_epoch_key(&epoch_key, &owner_secret, &peer_a_public, 1).unwrap();
        // Try to unwrap as peer B — should fail
        let result = unwrap_epoch_key(&wrapped, &peer_b_secret, &owner_public, 1);
        assert!(matches!(result, Err(CryptoError::DecryptionFailed)));
    }

    #[test]
    fn test_wrong_epoch_fails() {
        let epoch_key = [0x42u8; 32];
        let (owner_secret, owner_public) = test_keypair();
        let (peer_secret, peer_public) = test_keypair();

        let wrapped = wrap_epoch_key(&epoch_key, &owner_secret, &peer_public, 1).unwrap();
        let result = unwrap_epoch_key(&wrapped, &peer_secret, &owner_public, 2);
        assert!(matches!(result, Err(CryptoError::DecryptionFailed)));
    }

    #[test]
    fn test_different_peers_get_different_wrapped_keys() {
        let epoch_key = [0x42u8; 32];
        let (owner_secret, _owner_public) = test_keypair();
        let (_peer_a_secret, peer_a_public) = test_keypair();
        let (_peer_b_secret, peer_b_public) = test_keypair();

        let wrapped_a = wrap_epoch_key(&epoch_key, &owner_secret, &peer_a_public, 1).unwrap();
        let wrapped_b = wrap_epoch_key(&epoch_key, &owner_secret, &peer_b_public, 1).unwrap();
        assert_ne!(wrapped_a, wrapped_b);
    }

    #[test]
    fn test_unwrap_truncated_data() {
        let (peer_secret, _) = test_keypair();
        let (_, owner_public) = test_keypair();
        let result = unwrap_epoch_key(&[0u8; 10], &peer_secret, &owner_public, 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_ecdh_shared_secret_is_symmetric() {
        // Verify that ECDH produces the same shared secret from both sides
        let (alice_secret, alice_public) = test_keypair();
        let (bob_secret, bob_public) = test_keypair();

        let alice_shared = alice_secret.diffie_hellman(&bob_public);
        let bob_shared = bob_secret.diffie_hellman(&alice_public);

        assert_eq!(alice_shared.as_bytes(), bob_shared.as_bytes());
    }

    #[test]
    fn test_third_party_cannot_derive_wrapping_key() {
        // A third party who knows both public keys but no secret keys
        // cannot derive the wrapping key (they can't compute the ECDH shared secret)
        let epoch_key = [0x42u8; 32];
        let (owner_secret, owner_public) = test_keypair();
        let (peer_secret, peer_public) = test_keypair();
        let (attacker_secret, _attacker_public) = test_keypair();

        let wrapped = wrap_epoch_key(&epoch_key, &owner_secret, &peer_public, 1).unwrap();

        // Attacker tries with their own secret + owner's public (wrong shared secret)
        let result = unwrap_epoch_key(&wrapped, &attacker_secret, &owner_public, 1);
        assert!(matches!(result, Err(CryptoError::DecryptionFailed)));

        // Legitimate peer can unwrap
        let unwrapped = unwrap_epoch_key(&wrapped, &peer_secret, &owner_public, 1).unwrap();
        assert_eq!(unwrapped, epoch_key);
    }
}
