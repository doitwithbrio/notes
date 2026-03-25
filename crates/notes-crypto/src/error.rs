use thiserror::Error;

#[derive(Error, Debug)]
pub enum CryptoError {
    #[error("Encryption failed")]
    EncryptionFailed,

    #[error("Decryption failed — wrong key or tampered data")]
    DecryptionFailed,

    #[error("Random number generation failed")]
    RandomFailed,

    #[error("Epoch key not found: epoch {0}")]
    EpochKeyNotFound(u32),

    #[error("Key not found: {0}")]
    KeyNotFound(String),

    #[error("Keychain error: {0}")]
    KeychainError(String),

    #[error("Invalid data: {0}")]
    InvalidData(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
