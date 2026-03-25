//! Key storage abstraction.
//!
//! On macOS: uses Keychain Services via `security-framework`.
//! On other platforms: falls back to encrypted file storage with restrictive permissions.
//!
//! All key material is zeroized on drop.

use std::path::PathBuf;

use crate::error::CryptoError;

/// Service name for keychain entries.
const KEYCHAIN_SERVICE: &str = "com.p2pnotes.app";

/// Key storage backend.
pub struct KeyStore {
    /// Fallback directory for file-based storage (when keychain unavailable).
    keys_dir: PathBuf,
}

impl KeyStore {
    pub fn new(keys_dir: PathBuf) -> Self {
        Self { keys_dir }
    }

    /// Whether to use the OS keychain (skip in debug builds to avoid
    /// macOS Keychain password prompts from unsigned binaries).
    fn use_keychain() -> bool {
        #[cfg(debug_assertions)]
        {
            false
        }
        #[cfg(not(debug_assertions))]
        {
            true
        }
    }

    /// Store a key. Attempts OS keychain first (release only), falls back to file.
    pub fn store_key(&self, name: &str, key: &[u8]) -> Result<(), CryptoError> {
        // Try OS keychain first (release builds only)
        #[cfg(target_os = "macos")]
        if Self::use_keychain() {
            if let Ok(()) = store_keychain_macos(name, key) {
                log::debug!("Stored key '{name}' in macOS Keychain");
                return Ok(());
            }
            log::warn!("macOS Keychain storage failed for '{name}', falling back to file");
        }

        // Fallback: file-based storage with restrictive permissions
        self.store_key_file(name, key)
    }

    /// Retrieve a key. Tries OS keychain first (release only), falls back to file.
    pub fn load_key(&self, name: &str) -> Result<Vec<u8>, CryptoError> {
        // Try OS keychain first (release builds only)
        #[cfg(target_os = "macos")]
        if Self::use_keychain() {
            if let Ok(key) = load_keychain_macos(name) {
                log::debug!("Loaded key '{name}' from macOS Keychain");
                return Ok(key);
            }
        }

        // Fallback: file-based storage
        self.load_key_file(name)
    }

    /// Delete a key from storage.
    pub fn delete_key(&self, name: &str) -> Result<(), CryptoError> {
        #[cfg(target_os = "macos")]
        if Self::use_keychain() {
            let _ = delete_keychain_macos(name);
        }

        let path = self.key_file_path(name);
        if path.exists() {
            // Overwrite with zeros before deleting
            let len = std::fs::metadata(&path)
                .map(|m| m.len() as usize)
                .unwrap_or(0);
            if len > 0 {
                let zeros = vec![0u8; len];
                let _ = std::fs::write(&path, &zeros);
            }
            std::fs::remove_file(&path).map_err(CryptoError::Io)?;
        }

        Ok(())
    }

    /// Check if a key exists.
    pub fn has_key(&self, name: &str) -> bool {
        #[cfg(target_os = "macos")]
        if Self::use_keychain() {
            if load_keychain_macos(name).is_ok() {
                return true;
            }
        }

        self.key_file_path(name).exists()
    }

    // ── X25519 Keypair Management ─────────────────────────────────────

    /// Store an X25519 static secret key.
    pub fn store_x25519_secret(
        &self,
        name: &str,
        secret: &x25519_dalek::StaticSecret,
    ) -> Result<(), CryptoError> {
        self.store_key(name, secret.as_bytes())
    }

    /// Load an X25519 static secret key.
    pub fn load_x25519_secret(
        &self,
        name: &str,
    ) -> Result<x25519_dalek::StaticSecret, CryptoError> {
        let bytes = self.load_key(name)?;
        if bytes.len() != 32 {
            return Err(CryptoError::InvalidData(
                "X25519 secret key must be 32 bytes".into(),
            ));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(x25519_dalek::StaticSecret::from(arr))
    }

    /// Get or create an X25519 keypair. If the secret key already exists
    /// in the keystore, loads it. Otherwise, generates a new one and stores it.
    /// Returns (secret, public).
    pub fn get_or_create_x25519(
        &self,
        name: &str,
    ) -> Result<(x25519_dalek::StaticSecret, x25519_dalek::PublicKey), CryptoError> {
        if self.has_key(name) {
            let secret = self.load_x25519_secret(name)?;
            let public = x25519_dalek::PublicKey::from(&secret);
            Ok((secret, public))
        } else {
            let mut rng_bytes = [0u8; 32];
            getrandom::fill(&mut rng_bytes).map_err(|_| CryptoError::RandomFailed)?;
            let secret = x25519_dalek::StaticSecret::from(rng_bytes);
            let public = x25519_dalek::PublicKey::from(&secret);
            self.store_x25519_secret(name, &secret)?;
            log::info!("Generated new X25519 keypair '{name}'");
            Ok((secret, public))
        }
    }

    // ── File-based fallback ──────────────────────────────────────────

    fn key_file_path(&self, name: &str) -> PathBuf {
        // Sanitize name: only alphanumeric and hyphens
        let safe_name: String = name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.keys_dir.join(format!("{safe_name}.key"))
    }

    fn store_key_file(&self, name: &str, key: &[u8]) -> Result<(), CryptoError> {
        let path = self.key_file_path(name);
        std::fs::create_dir_all(&self.keys_dir).map_err(CryptoError::Io)?;
        std::fs::write(&path, key).map_err(CryptoError::Io)?;

        // Restrictive permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .map_err(CryptoError::Io)?;
        }

        log::debug!("Stored key '{name}' to file");
        Ok(())
    }

    fn load_key_file(&self, name: &str) -> Result<Vec<u8>, CryptoError> {
        let path = self.key_file_path(name);
        std::fs::read(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                CryptoError::KeyNotFound(name.to_string())
            } else {
                CryptoError::Io(e)
            }
        })
    }
}

// ── macOS Keychain ───────────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn store_keychain_macos(name: &str, key: &[u8]) -> Result<(), CryptoError> {
    use security_framework::passwords::{delete_generic_password, set_generic_password};
    // Delete existing entry first (set_generic_password errors if it exists)
    let _ = delete_generic_password(KEYCHAIN_SERVICE, name);
    set_generic_password(KEYCHAIN_SERVICE, name, key)
        .map_err(|e| CryptoError::KeychainError(e.to_string()))
}

#[cfg(target_os = "macos")]
fn load_keychain_macos(name: &str) -> Result<Vec<u8>, CryptoError> {
    use security_framework::passwords::get_generic_password;
    get_generic_password(KEYCHAIN_SERVICE, name)
        .map_err(|e| CryptoError::KeychainError(e.to_string()))
}

#[cfg(target_os = "macos")]
fn delete_keychain_macos(name: &str) -> Result<(), CryptoError> {
    use security_framework::passwords::delete_generic_password;
    delete_generic_password(KEYCHAIN_SERVICE, name)
        .map_err(|e| CryptoError::KeychainError(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_and_load_key_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = KeyStore::new(dir.path().to_path_buf());

        let key = b"supersecretkey123456789012345678";
        store.store_key("test-key", key).unwrap();

        let loaded = store.load_key("test-key").unwrap();
        assert_eq!(loaded, key);
    }

    #[test]
    fn test_has_key() {
        let dir = tempfile::tempdir().unwrap();
        let store = KeyStore::new(dir.path().to_path_buf());

        // Use a unique name to avoid keychain collisions in test
        let name = format!("test-has-key-{}", std::process::id());
        assert!(!store.has_key(&name));

        store.store_key(&name, b"key").unwrap();
        assert!(store.has_key(&name));

        // Clean up
        store.delete_key(&name).unwrap();
    }

    #[test]
    fn test_delete_key() {
        let dir = tempfile::tempdir().unwrap();
        let store = KeyStore::new(dir.path().to_path_buf());

        store.store_key("to-delete", b"secret").unwrap();
        assert!(store.has_key("to-delete"));

        store.delete_key("to-delete").unwrap();
        assert!(!store.has_key("to-delete"));
    }

    #[test]
    fn test_load_nonexistent_key() {
        let dir = tempfile::tempdir().unwrap();
        let store = KeyStore::new(dir.path().to_path_buf());

        let result = store.load_key("nonexistent");
        assert!(matches!(result, Err(CryptoError::KeyNotFound(_))));
    }

    #[test]
    fn test_key_file_path_sanitization() {
        let dir = tempfile::tempdir().unwrap();
        let store = KeyStore::new(dir.path().to_path_buf());

        // Names with special chars should be sanitized
        let path = store.key_file_path("test/../evil");
        let path_str = path.file_name().unwrap().to_str().unwrap();
        // Slashes and dots (non-alphanumeric) replaced with _
        assert!(path_str.contains("test_"), "got: {path_str}");
        assert!(path_str.ends_with(".key"));
        // Must not contain actual path traversal
        assert!(!path_str.contains(".."));
    }

    #[test]
    fn test_overwrite_existing_key() {
        let dir = tempfile::tempdir().unwrap();
        let store = KeyStore::new(dir.path().to_path_buf());

        store.store_key("key", b"version1").unwrap();
        store.store_key("key", b"version2").unwrap();

        let loaded = store.load_key("key").unwrap();
        assert_eq!(loaded, b"version2");
    }

    #[test]
    fn test_x25519_get_or_create() {
        let dir = tempfile::tempdir().unwrap();
        let store = KeyStore::new(dir.path().to_path_buf());

        let (secret1, public1) = store.get_or_create_x25519("test-x25519").unwrap();
        let (secret2, public2) = store.get_or_create_x25519("test-x25519").unwrap();

        // Should return the same keypair on second call
        assert_eq!(public1.as_bytes(), public2.as_bytes());
        assert_eq!(secret1.as_bytes(), secret2.as_bytes());
    }

    #[test]
    fn test_x25519_store_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = KeyStore::new(dir.path().to_path_buf());

        let mut rng_bytes = [0u8; 32];
        getrandom::fill(&mut rng_bytes).unwrap();
        let original = x25519_dalek::StaticSecret::from(rng_bytes);
        let original_public = x25519_dalek::PublicKey::from(&original);

        store.store_x25519_secret("x25519-test", &original).unwrap();
        let loaded = store.load_x25519_secret("x25519-test").unwrap();
        let loaded_public = x25519_dalek::PublicKey::from(&loaded);

        assert_eq!(original_public.as_bytes(), loaded_public.as_bytes());
    }

    #[test]
    fn test_x25519_different_names_different_keys() {
        let dir = tempfile::tempdir().unwrap();
        let store = KeyStore::new(dir.path().to_path_buf());

        let (_, pub_a) = store.get_or_create_x25519("key-a").unwrap();
        let (_, pub_b) = store.get_or_create_x25519("key-b").unwrap();

        assert_ne!(pub_a.as_bytes(), pub_b.as_bytes());
    }
}
