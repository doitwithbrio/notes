//! # notes-crypto
//!
//! Encryption, signing, and key management for the notes app.
//!
//! - `encryption` — XChaCha20-Poly1305 at-rest encryption with HKDF key derivation
//! - `epochs` — Epoch-based key ratcheting for forward secrecy on peer removal
//! - `key_wrapping` — Per-peer epoch key wrapping via HKDF + XChaCha20-Poly1305
//! - `keystore` — OS keychain integration (macOS Keychain, with file fallback)
//! - `signing` — Ed25519 change signing and ACL verification
//! - `error` — Crypto error types

pub mod encryption;
pub mod epochs;
pub mod error;
pub mod key_wrapping;
pub mod keystore;
pub mod signing;

pub use encryption::{decrypt_document, decrypt_snapshot, encrypt_document, encrypt_snapshot};
pub use epochs::{EpochKeyManager, EpochKeys};
pub use error::CryptoError;
pub use key_wrapping::{unwrap_epoch_key, wrap_epoch_key};
pub use keystore::{
    debug_enable_secret_read_tracking, debug_get_secret_read_stats, debug_note_secret_cache_hit,
    debug_note_secret_cache_miss, debug_record_secret_read, debug_reset_secret_read_tracking,
    debug_set_secret_read_phase, KeyStore, SecretReadBackend, SecretReadClass, SecretReadEvent,
    SecretReadOutcome, SecretReadPhase, SecretReadStats,
};
pub use signing::SignedChange;
