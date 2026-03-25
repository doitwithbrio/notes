//! XChaCha20-Poly1305 at-rest encryption for Automerge documents.
//!
//! Each document is encrypted with a per-document key derived from the
//! project's epoch key via HKDF-SHA256. The HKDF context includes the
//! document ID and epoch number to ensure unique keys.

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::XChaCha20Poly1305;
use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::Zeroize;

use crate::error::CryptoError;

/// HKDF context for deriving per-document encryption keys.
const DOC_ENCRYPTION_CONTEXT: &[u8] = b"p2p-notes/v1/document-encryption";

/// Derive a per-document encryption key from the epoch key.
///
/// Uses HKDF-SHA256 with context: `"p2p-notes/v1/document-encryption" || doc_id || epoch`.
pub fn derive_document_key(epoch_key: &[u8; 32], doc_id: &[u8; 16], epoch: u32) -> [u8; 32] {
    // Build the info string: context || doc_id || epoch
    let mut info = Vec::with_capacity(DOC_ENCRYPTION_CONTEXT.len() + 16 + 4);
    info.extend_from_slice(DOC_ENCRYPTION_CONTEXT);
    info.extend_from_slice(doc_id);
    info.extend_from_slice(&epoch.to_be_bytes());

    let hk = Hkdf::<Sha256>::new(None, epoch_key);
    let mut key = [0u8; 32];
    hk.expand(&info, &mut key)
        .expect("HKDF expand should not fail with 32-byte output");
    key
}

/// Encrypt an Automerge document for at-rest storage.
///
/// Format: [4 bytes: epoch (big-endian)][24 bytes: nonce][N bytes: ciphertext + tag]
pub fn encrypt_document(
    epoch_key: &[u8; 32],
    doc_id: &[u8; 16],
    epoch: u32,
    plaintext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let mut doc_key = derive_document_key(epoch_key, doc_id, epoch);

    let cipher_key = chacha20poly1305::Key::from_slice(&doc_key);
    let cipher = XChaCha20Poly1305::new(cipher_key);

    // Generate random 24-byte nonce
    let mut nonce_bytes = [0u8; 24];
    getrandom::fill(&mut nonce_bytes).map_err(|_| CryptoError::RandomFailed)?;
    let nonce = chacha20poly1305::XNonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|_| CryptoError::EncryptionFailed)?;

    // Zeroize the derived key
    doc_key.zeroize();

    // Build output: epoch || nonce || ciphertext
    let mut output = Vec::with_capacity(4 + 24 + ciphertext.len());
    output.extend_from_slice(&epoch.to_be_bytes());
    output.extend_from_slice(&nonce_bytes);
    output.extend_from_slice(&ciphertext);

    Ok(output)
}

/// Decrypt an at-rest encrypted Automerge document.
///
/// Reads the epoch from the header, derives the document key, and decrypts.
/// Returns `(epoch, plaintext)`.
pub fn decrypt_document(
    epoch_key: &[u8; 32],
    doc_id: &[u8; 16],
    encrypted: &[u8],
) -> Result<(u32, Vec<u8>), CryptoError> {
    if encrypted.len() < 4 + 24 {
        return Err(CryptoError::InvalidData(
            "encrypted document too short".into(),
        ));
    }

    // Read epoch
    let epoch = u32::from_be_bytes(
        encrypted[..4]
            .try_into()
            .map_err(|_| CryptoError::InvalidData("bad epoch header".into()))?,
    );

    // Read nonce
    let nonce = chacha20poly1305::XNonce::from_slice(&encrypted[4..28]);

    // Read ciphertext
    let ciphertext = &encrypted[28..];

    let mut doc_key = derive_document_key(epoch_key, doc_id, epoch);
    let cipher_key = chacha20poly1305::Key::from_slice(&doc_key);
    let cipher = XChaCha20Poly1305::new(cipher_key);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| CryptoError::DecryptionFailed)?;

    doc_key.zeroize();

    Ok((epoch, plaintext))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_document_key_deterministic() {
        let epoch_key = [0xAA; 32];
        let doc_id = [0xBB; 16];
        let k1 = derive_document_key(&epoch_key, &doc_id, 1);
        let k2 = derive_document_key(&epoch_key, &doc_id, 1);
        assert_eq!(k1, k2);
    }

    #[test]
    fn test_derive_document_key_unique_per_doc() {
        let epoch_key = [0xAA; 32];
        let doc1 = [0x01; 16];
        let doc2 = [0x02; 16];
        let k1 = derive_document_key(&epoch_key, &doc1, 1);
        let k2 = derive_document_key(&epoch_key, &doc2, 1);
        assert_ne!(k1, k2);
    }

    #[test]
    fn test_derive_document_key_unique_per_epoch() {
        let epoch_key = [0xAA; 32];
        let doc_id = [0xBB; 16];
        let k1 = derive_document_key(&epoch_key, &doc_id, 1);
        let k2 = derive_document_key(&epoch_key, &doc_id, 2);
        assert_ne!(k1, k2);
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let epoch_key = [0x42; 32];
        let doc_id = [0x01; 16];
        let epoch = 1;
        let plaintext = b"This is secret Automerge document data";

        let encrypted = encrypt_document(&epoch_key, &doc_id, epoch, plaintext).unwrap();
        assert_ne!(encrypted, plaintext);
        assert!(encrypted.len() > plaintext.len()); // Overhead: epoch + nonce + tag

        let (dec_epoch, decrypted) = decrypt_document(&epoch_key, &doc_id, &encrypted).unwrap();
        assert_eq!(dec_epoch, epoch);
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_decrypt_wrong_key_fails() {
        let epoch_key = [0x42; 32];
        let wrong_key = [0x43; 32];
        let doc_id = [0x01; 16];

        let encrypted = encrypt_document(&epoch_key, &doc_id, 1, b"secret").unwrap();

        let result = decrypt_document(&wrong_key, &doc_id, &encrypted);
        assert!(matches!(result, Err(CryptoError::DecryptionFailed)));
    }

    #[test]
    fn test_decrypt_wrong_doc_id_fails() {
        let epoch_key = [0x42; 32];
        let doc1 = [0x01; 16];
        let doc2 = [0x02; 16];

        let encrypted = encrypt_document(&epoch_key, &doc1, 1, b"secret").unwrap();

        let result = decrypt_document(&epoch_key, &doc2, &encrypted);
        assert!(matches!(result, Err(CryptoError::DecryptionFailed)));
    }

    #[test]
    fn test_decrypt_truncated_data() {
        let epoch_key = [0x42; 32];
        let doc_id = [0x01; 16];

        let result = decrypt_document(&epoch_key, &doc_id, &[0; 10]);
        assert!(result.is_err());
    }

    #[test]
    fn test_decrypt_tampered_ciphertext() {
        let epoch_key = [0x42; 32];
        let doc_id = [0x01; 16];

        let mut encrypted = encrypt_document(&epoch_key, &doc_id, 1, b"secret data").unwrap();
        // Flip a bit in the ciphertext
        let last = encrypted.len() - 1;
        encrypted[last] ^= 0x01;

        let result = decrypt_document(&epoch_key, &doc_id, &encrypted);
        assert!(matches!(result, Err(CryptoError::DecryptionFailed)));
    }

    #[test]
    fn test_encrypt_empty_document() {
        let epoch_key = [0x42; 32];
        let doc_id = [0x01; 16];

        let encrypted = encrypt_document(&epoch_key, &doc_id, 0, b"").unwrap();
        let (epoch, decrypted) = decrypt_document(&epoch_key, &doc_id, &encrypted).unwrap();
        assert_eq!(epoch, 0);
        assert_eq!(decrypted, b"");
    }
}
