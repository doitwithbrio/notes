//! Image/asset storage and sync via content-addressed blobs.
//!
//! Images are stored as blake3-hashed files in a flat directory.
//! The hash is used as the filename, ensuring deduplication.
//! References in Automerge documents use the hash string.
//!
//! For P2P sync, iroh-blobs handles the transfer protocol.
//! Missing blobs are fetched on-demand from connected peers.

use std::path::{Path, PathBuf};

pub use iroh_blobs::Hash;

/// Re-export the ALPN for blob protocol registration.
pub fn blob_alpn() -> &'static [u8] {
    iroh_blobs::ALPN
}

/// Manages content-addressed blob storage on the filesystem.
pub struct BlobStore {
    /// Root directory for blob storage (e.g., `~/Notes/.p2p/blobs/`).
    store_dir: PathBuf,
}

impl BlobStore {
    /// Create a new BlobStore at the given directory.
    pub async fn new(store_dir: PathBuf) -> Result<Self, BlobError> {
        tokio::fs::create_dir_all(&store_dir)
            .await
            .map_err(|e| BlobError::Io(format!("failed to create blob dir: {e}")))?;
        Ok(Self { store_dir })
    }

    /// Import raw bytes as a blob. Returns the blake3 hash.
    /// Also writes a copy to the project's `assets/` directory for human readability.
    pub async fn import(
        &self,
        data: &[u8],
        project_assets_dir: Option<&Path>,
        original_filename: Option<&str>,
    ) -> Result<BlobMeta, BlobError> {
        let hash = Hash::new(data);
        let hash_hex = hash.to_hex();

        // Determine file extension from original filename
        let ext = original_filename
            .and_then(|f| f.rsplit('.').next())
            .unwrap_or("bin");

        // Write to blob store: <store_dir>/<hash_hex>.<ext>
        let blob_filename = format!("{hash_hex}.{ext}");
        let blob_path = self.store_dir.join(&blob_filename);

        if !blob_path.exists() {
            tokio::fs::write(&blob_path, data)
                .await
                .map_err(|e| BlobError::Io(format!("write blob failed: {e}")))?;
        }

        // Also copy to project assets directory (human-readable)
        if let Some(assets_dir) = project_assets_dir {
            tokio::fs::create_dir_all(assets_dir)
                .await
                .map_err(|e| BlobError::Io(format!("create assets dir failed: {e}")))?;

            let readable_name = original_filename.unwrap_or(&blob_filename);
            let asset_path = assets_dir.join(readable_name);

            // Avoid overwriting if a different file exists with the same name
            if !asset_path.exists() {
                let _ = tokio::fs::write(&asset_path, data).await;
            }
        }

        log::info!("Imported blob: {hash_hex} ({} bytes, ext: {ext})", data.len());

        Ok(BlobMeta {
            hash: hash_hex.to_string(),
            size: data.len() as u64,
            filename: blob_filename,
            mime_type: mime_from_ext(ext),
        })
    }

    /// Read a blob by its hex hash. Returns the raw bytes.
    pub async fn read(&self, hash_hex: &str) -> Result<Vec<u8>, BlobError> {
        // Find the file — we need to check with any extension
        let mut entries = tokio::fs::read_dir(&self.store_dir)
            .await
            .map_err(|e| BlobError::Io(format!("read blob dir failed: {e}")))?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| BlobError::Io(format!("read entry failed: {e}")))?
        {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(hash_hex) {
                let data = tokio::fs::read(entry.path())
                    .await
                    .map_err(|e| BlobError::Io(format!("read blob failed: {e}")))?;
                return Ok(data);
            }
        }

        Err(BlobError::NotFound(hash_hex.to_string()))
    }

    /// Check if a blob exists locally.
    pub async fn has(&self, hash_hex: &str) -> bool {
        if let Ok(mut entries) = tokio::fs::read_dir(&self.store_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                if entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(hash_hex)
                {
                    return true;
                }
            }
        }
        false
    }

    /// List all blobs in the store.
    pub async fn list(&self) -> Result<Vec<String>, BlobError> {
        let mut hashes = Vec::new();
        let mut entries = tokio::fs::read_dir(&self.store_dir)
            .await
            .map_err(|e| BlobError::Io(format!("read blob dir failed: {e}")))?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| BlobError::Io(format!("read entry failed: {e}")))?
        {
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(hash) = name.split('.').next() {
                if hash.len() == 64 {
                    // blake3 hashes are 64 hex chars
                    hashes.push(hash.to_string());
                }
            }
        }

        Ok(hashes)
    }

    /// Get the filesystem path for a blob (for Tauri asset protocol).
    pub async fn get_path(&self, hash_hex: &str) -> Result<PathBuf, BlobError> {
        let mut entries = tokio::fs::read_dir(&self.store_dir)
            .await
            .map_err(|e| BlobError::Io(format!("read blob dir failed: {e}")))?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| BlobError::Io(format!("read entry failed: {e}")))?
        {
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with(hash_hex)
            {
                return Ok(entry.path());
            }
        }

        Err(BlobError::NotFound(hash_hex.to_string()))
    }

    /// Get the store directory path.
    pub fn store_dir(&self) -> &Path {
        &self.store_dir
    }
}

/// Metadata returned after importing a blob.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobMeta {
    /// Blake3 hash as hex string (64 chars).
    pub hash: String,
    /// Size in bytes.
    pub size: u64,
    /// Filename in the blob store (hash.ext).
    pub filename: String,
    /// MIME type inferred from extension.
    pub mime_type: String,
}

/// Errors from blob operations.
#[derive(Debug, thiserror::Error)]
pub enum BlobError {
    #[error("Blob not found: {0}")]
    NotFound(String),

    #[error("IO error: {0}")]
    Io(String),
}

/// Compute a blob hash from raw data.
pub fn hash_data(data: &[u8]) -> Hash {
    Hash::new(data)
}

/// Convert a hash to hex string.
pub fn hash_to_hex(hash: &Hash) -> String {
    hash.to_hex()
}

/// Infer MIME type from file extension.
fn mime_from_ext(ext: &str) -> String {
    match ext.to_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "mp4" => "video/mp4",
        "mp3" => "audio/mpeg",
        _ => "application/octet-stream",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_import_and_read() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::new(dir.path().to_path_buf()).await.unwrap();

        let data = b"test image data here";
        let meta = store.import(data, None, Some("photo.png")).await.unwrap();

        assert_eq!(meta.size, data.len() as u64);
        assert_eq!(meta.mime_type, "image/png");
        assert_eq!(meta.hash.len(), 64);

        let read_back = store.read(&meta.hash).await.unwrap();
        assert_eq!(read_back, data);
    }

    #[tokio::test]
    async fn test_has_blob() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::new(dir.path().to_path_buf()).await.unwrap();

        assert!(!store.has("nonexistent").await);

        let meta = store.import(b"data", None, Some("test.bin")).await.unwrap();
        assert!(store.has(&meta.hash).await);
    }

    #[tokio::test]
    async fn test_deduplication() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::new(dir.path().to_path_buf()).await.unwrap();

        let data = b"same data twice";
        let meta1 = store.import(data, None, Some("a.bin")).await.unwrap();
        let meta2 = store.import(data, None, Some("b.bin")).await.unwrap();

        // Same data → same hash
        assert_eq!(meta1.hash, meta2.hash);

        // Only one file on disk
        let list = store.list().await.unwrap();
        assert_eq!(list.len(), 1);
    }

    #[tokio::test]
    async fn test_project_assets_copy() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::new(dir.path().join("blobs")).await.unwrap();
        let assets_dir = dir.path().join("assets");

        let data = b"image bytes";
        store
            .import(data, Some(&assets_dir), Some("photo.jpg"))
            .await
            .unwrap();

        // Both blob store and assets dir should have the file
        assert!(assets_dir.join("photo.jpg").exists());
    }

    #[tokio::test]
    async fn test_get_path() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::new(dir.path().to_path_buf()).await.unwrap();

        let meta = store.import(b"data", None, Some("test.png")).await.unwrap();
        let path = store.get_path(&meta.hash).await.unwrap();
        assert!(path.exists());
    }

    #[test]
    fn test_mime_types() {
        assert_eq!(mime_from_ext("png"), "image/png");
        assert_eq!(mime_from_ext("jpg"), "image/jpeg");
        assert_eq!(mime_from_ext("JPEG"), "image/jpeg");
        assert_eq!(mime_from_ext("gif"), "image/gif");
        assert_eq!(mime_from_ext("xyz"), "application/octet-stream");
    }

    #[test]
    fn test_hash_data() {
        let h1 = hash_data(b"test");
        let h2 = hash_data(b"test");
        assert_eq!(h1, h2);

        let h3 = hash_data(b"different");
        assert_ne!(h1, h3);
    }
}
