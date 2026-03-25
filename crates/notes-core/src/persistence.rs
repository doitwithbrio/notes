use std::path::{Path, PathBuf};

use tokio::fs;

use crate::error::CoreError;
use crate::types::DocId;
use crate::validation;

/// Handles atomic file I/O for Automerge documents and markdown exports.
pub struct Persistence {
    base_dir: PathBuf,
}

impl Persistence {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
        }
    }

    /// Get the path to the .p2p/automerge directory for a project.
    fn automerge_dir(&self, project_name: &str) -> PathBuf {
        self.base_dir
            .join(project_name)
            .join(".p2p")
            .join("automerge")
    }

    /// Get the path to a specific document file.
    fn doc_path(&self, project_name: &str, doc_id: &DocId) -> PathBuf {
        self.automerge_dir(project_name)
            .join(format!("{}.automerge", doc_id))
    }

    /// Get the path to the backup file for a document.
    fn backup_path(&self, project_name: &str, doc_id: &DocId) -> PathBuf {
        self.automerge_dir(project_name)
            .join(format!("{}.automerge.bak", doc_id))
    }

    /// Get the path to the project manifest.
    fn manifest_path(&self, project_name: &str) -> PathBuf {
        self.base_dir
            .join(project_name)
            .join(".p2p")
            .join("manifest.automerge")
    }

    /// Ensure the .p2p directory structure exists for a project.
    pub async fn ensure_project_dirs(&self, project_name: &str) -> Result<(), CoreError> {
        validation::validate_project_name(project_name)?;

        let p2p_dir = self.base_dir.join(project_name).join(".p2p");
        fs::create_dir_all(p2p_dir.join("automerge")).await?;
        fs::create_dir_all(p2p_dir.join("keys")).await?;

        // Set restrictive permissions on the .p2p directory
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o700);
            let _ = fs::set_permissions(&p2p_dir, perms).await;
        }

        Ok(())
    }

    /// Atomically save document data to disk.
    /// Writes to a .tmp file first, then renames (atomic on most filesystems).
    /// Also keeps a .bak backup of the previous version.
    pub async fn save_doc(
        &self,
        project_name: &str,
        doc_id: &DocId,
        data: &[u8],
    ) -> Result<(), CoreError> {
        let path = self.doc_path(project_name, doc_id);
        let backup = self.backup_path(project_name, doc_id);

        // Attempt backup — ignore NotFound (no prior version)
        match fs::copy(&path, &backup).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log::warn!("Failed to create backup for {doc_id}: {e}"),
        }

        atomic_write(&path, data).await?;

        log::debug!("Saved document {doc_id}: {} bytes", data.len());
        Ok(())
    }

    /// Atomically save an encrypted document to disk.
    /// Encrypts the Automerge binary with the epoch key before writing.
    pub async fn save_doc_encrypted(
        &self,
        project_name: &str,
        doc_id: &DocId,
        data: &[u8],
        epoch_key: &[u8; 32],
        epoch: u32,
    ) -> Result<(), CoreError> {
        let doc_id_bytes = doc_id_to_bytes(doc_id);
        let encrypted = notes_crypto::encrypt_document(epoch_key, &doc_id_bytes, epoch, data)
            .map_err(|e| CoreError::InvalidData(format!("encryption failed: {e}")))?;

        self.save_doc(project_name, doc_id, &encrypted).await
    }

    /// Load and decrypt a document from disk.
    /// Detects whether the file is encrypted (has epoch header) or plaintext.
    /// Plaintext files are returned as-is (for migration — they'll be encrypted on next save).
    pub async fn load_doc_encrypted(
        &self,
        project_name: &str,
        doc_id: &DocId,
        epoch_key: &[u8; 32],
    ) -> Result<Vec<u8>, CoreError> {
        let path = self.doc_path(project_name, doc_id);

        let raw = match fs::read(&path).await {
            Ok(data) => data,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(CoreError::DocNotFound(*doc_id));
            }
            Err(e) => {
                log::warn!("Failed to read {doc_id}, trying backup: {e}");
                return self
                    .try_recover_from_backup(project_name, doc_id, &path)
                    .await;
            }
        };

        // Try to load as plaintext Automerge first (handles migration)
        if automerge::AutoCommit::load(&raw).is_ok() {
            log::debug!("Loaded plaintext doc {doc_id} (will encrypt on next save)");
            return Ok(raw);
        }

        // Try to decrypt
        let doc_id_bytes = doc_id_to_bytes(doc_id);
        match notes_crypto::decrypt_document(epoch_key, &doc_id_bytes, &raw) {
            Ok((_epoch, plaintext)) => {
                // Validate the decrypted data
                if automerge::AutoCommit::load(&plaintext).is_ok() {
                    Ok(plaintext)
                } else {
                    log::warn!("Decrypted data for {doc_id} is not valid Automerge, trying backup");
                    self.try_recover_from_backup(project_name, doc_id, &path)
                        .await
                }
            }
            Err(e) => {
                log::warn!("Decryption failed for {doc_id}: {e}, trying backup");
                self.try_recover_from_backup(project_name, doc_id, &path)
                    .await
            }
        }
    }

    /// Load a document from disk.
    /// Falls back to the backup file if the primary is corrupted or unreadable.
    pub async fn load_doc(
        &self,
        project_name: &str,
        doc_id: &DocId,
    ) -> Result<Vec<u8>, CoreError> {
        let path = self.doc_path(project_name, doc_id);

        match fs::read(&path).await {
            Ok(data) => {
                // Validate the data is a well-formed Automerge document
                if automerge::AutoCommit::load(&data).is_ok() {
                    Ok(data)
                } else {
                    log::warn!("Primary file for {doc_id} is corrupted, trying backup");
                    self.try_recover_from_backup(project_name, doc_id, &path)
                        .await
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(CoreError::DocNotFound(*doc_id))
            }
            Err(e) => {
                log::warn!("Failed to read {doc_id}, trying backup: {e}");
                self.try_recover_from_backup(project_name, doc_id, &path)
                    .await
            }
        }
    }

    /// Attempt to recover a document from its backup file.
    async fn try_recover_from_backup(
        &self,
        project_name: &str,
        doc_id: &DocId,
        primary_path: &Path,
    ) -> Result<Vec<u8>, CoreError> {
        let backup = self.backup_path(project_name, doc_id);
        let data = fs::read(&backup).await.map_err(|_| {
            CoreError::InvalidData(format!(
                "Primary and backup both unreadable for {doc_id}"
            ))
        })?;

        // Validate the backup too
        if automerge::AutoCommit::load(&data).is_err() {
            return Err(CoreError::InvalidData(format!(
                "Primary and backup both corrupted for {doc_id}"
            )));
        }

        log::info!("Recovered {doc_id} from backup");
        // Restore the backup as the primary
        if let Err(e) = fs::copy(&backup, primary_path).await {
            log::warn!("Failed to restore backup to primary: {e}");
        }
        Ok(data)
    }

    /// Delete a document file and its backup from disk.
    pub async fn delete_doc(
        &self,
        project_name: &str,
        doc_id: &DocId,
    ) -> Result<(), CoreError> {
        let path = self.doc_path(project_name, doc_id);
        let backup = self.backup_path(project_name, doc_id);

        // Remove files, ignoring NotFound
        match fs::remove_file(&path).await {
            Ok(()) | Err(_) => {} // best-effort
        }
        match fs::remove_file(&backup).await {
            Ok(()) | Err(_) => {} // best-effort
        }

        Ok(())
    }

    /// Save the project manifest to disk (atomic write).
    pub async fn save_manifest(
        &self,
        project_name: &str,
        data: &[u8],
    ) -> Result<(), CoreError> {
        let path = self.manifest_path(project_name);
        atomic_write(&path, data).await
    }

    /// Load the project manifest from disk.
    pub async fn load_manifest(&self, project_name: &str) -> Result<Vec<u8>, CoreError> {
        let path = self.manifest_path(project_name);
        Ok(fs::read(&path).await?)
    }

    /// Export a document's text content as a read-only .md file.
    pub async fn export_markdown(
        &self,
        project_name: &str,
        relative_path: &str,
        content: &str,
    ) -> Result<(), CoreError> {
        // Validate the path to prevent traversal
        validation::validate_relative_path(relative_path)?;

        let path = self.base_dir.join(project_name).join(relative_path);

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }

        atomic_write(&path, content.as_bytes()).await?;

        // Set file to read-only (best-effort)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o444);
            let _ = fs::set_permissions(&path, perms).await;
        }

        log::debug!("Exported markdown: {relative_path}");
        Ok(())
    }

    /// List all automerge document files in a project.
    pub async fn list_doc_files(
        &self,
        project_name: &str,
    ) -> Result<Vec<(DocId, PathBuf)>, CoreError> {
        let dir = self.automerge_dir(project_name);
        if !fs::try_exists(&dir).await.unwrap_or(false) {
            return Ok(vec![]);
        }

        let mut entries = fs::read_dir(&dir).await?;
        let mut docs = Vec::new();

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if let Some(ext) = path.extension() {
                if ext == "automerge" {
                    if let Some(stem) = path.file_stem() {
                        if let Ok(id) = stem.to_string_lossy().parse::<DocId>() {
                            docs.push((id, path));
                        }
                    }
                }
            }
        }

        Ok(docs)
    }

    /// List all project directories (folders in the base directory).
    pub async fn list_projects(&self) -> Result<Vec<String>, CoreError> {
        let mut entries = fs::read_dir(&self.base_dir).await?;
        let mut projects = Vec::new();

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_dir() {
                let name = entry.file_name().to_string_lossy().into_owned();
                // Skip hidden directories
                if !name.starts_with('.') {
                    projects.push(name);
                }
            }
        }

        projects.sort();
        Ok(projects)
    }

    /// Check if a project has been initialized with .p2p directory.
    pub async fn is_initialized(&self, project_name: &str) -> bool {
        fs::try_exists(
            self.base_dir
                .join(project_name)
                .join(".p2p")
                .join("automerge"),
        )
        .await
        .unwrap_or(false)
    }

    /// Get the base directory.
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Get the full path for a project.
    pub fn project_dir(&self, project_name: &str) -> PathBuf {
        self.base_dir.join(project_name)
    }
}

/// Convert a UUID to a 16-byte array for crypto operations.
fn doc_id_to_bytes(doc_id: &DocId) -> [u8; 16] {
    *doc_id.as_bytes()
}

/// Write data to a file atomically via tmp + rename.
/// Appends `.tmp` to the full filename (e.g., `foo.automerge.tmp`).
async fn atomic_write(path: &Path, data: &[u8]) -> Result<(), CoreError> {
    let mut tmp_name = path.as_os_str().to_owned();
    tmp_name.push(".tmp");
    let tmp_path = PathBuf::from(tmp_name);

    // Ensure parent dir exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }

    fs::write(&tmp_path, data).await?;

    // Set restrictive permissions before rename
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o600)).await;
    }

    fs::rename(&tmp_path, path).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use automerge::transaction::Transactable;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_atomic_write_and_read() {
        let dir = tempfile::tempdir().unwrap();
        let persistence = Persistence::new(dir.path());

        let project = "test-project";
        persistence.ensure_project_dirs(project).await.unwrap();

        let doc_id = Uuid::new_v4();
        let data = b"test automerge data";

        persistence.save_doc(project, &doc_id, data).await.unwrap();
        // Note: save_doc doesn't validate Automerge format, load_doc does.
        // For this test, read raw bytes to verify write correctness.
        let path = persistence.doc_path(project, &doc_id);
        let loaded = fs::read(&path).await.unwrap();
        assert_eq!(loaded, data);
    }

    #[tokio::test]
    async fn test_backup_recovery_on_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let persistence = Persistence::new(dir.path());

        let project = "test-project";
        persistence.ensure_project_dirs(project).await.unwrap();

        let doc_id = Uuid::new_v4();

        // Create a real Automerge document for valid data
        let mut doc = automerge::AutoCommit::new();
        doc.put(automerge::ROOT, "test", "hello").unwrap();
        let valid_data = doc.save();

        // Save valid data twice (second save creates backup of first)
        persistence
            .save_doc(project, &doc_id, &valid_data)
            .await
            .unwrap();
        persistence
            .save_doc(project, &doc_id, &valid_data)
            .await
            .unwrap();

        // Now corrupt the primary file
        let path = persistence.doc_path(project, &doc_id);
        fs::write(&path, b"corrupted garbage data").await.unwrap();

        // load_doc should recover from backup
        let loaded = persistence.load_doc(project, &doc_id).await.unwrap();
        assert_eq!(loaded, valid_data);
    }

    #[tokio::test]
    async fn test_load_nonexistent_doc() {
        let dir = tempfile::tempdir().unwrap();
        let persistence = Persistence::new(dir.path());
        persistence
            .ensure_project_dirs("test")
            .await
            .unwrap();

        let result = persistence.load_doc("test", &Uuid::new_v4()).await;
        assert!(matches!(result, Err(CoreError::DocNotFound(_))));
    }

    #[tokio::test]
    async fn test_markdown_export() {
        let dir = tempfile::tempdir().unwrap();
        let persistence = Persistence::new(dir.path());

        let project = "test-project";
        fs::create_dir_all(dir.path().join(project))
            .await
            .unwrap();

        persistence
            .export_markdown(project, "notes/hello.md", "# Hello\n\nWorld")
            .await
            .unwrap();

        let content = fs::read_to_string(dir.path().join(project).join("notes/hello.md"))
            .await
            .unwrap();
        assert_eq!(content, "# Hello\n\nWorld");
    }

    #[tokio::test]
    async fn test_markdown_export_path_traversal_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let persistence = Persistence::new(dir.path());

        let result = persistence
            .export_markdown("project", "../evil.md", "pwned")
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_projects() {
        let dir = tempfile::tempdir().unwrap();
        let persistence = Persistence::new(dir.path());

        fs::create_dir_all(dir.path().join("project-a"))
            .await
            .unwrap();
        fs::create_dir_all(dir.path().join("project-b"))
            .await
            .unwrap();
        fs::create_dir_all(dir.path().join(".hidden"))
            .await
            .unwrap();

        let projects = persistence.list_projects().await.unwrap();
        assert_eq!(projects, vec!["project-a", "project-b"]);
    }

    #[tokio::test]
    async fn test_ensure_project_dirs_rejects_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let persistence = Persistence::new(dir.path());

        let result = persistence.ensure_project_dirs("../../evil").await;
        assert!(result.is_err());
    }
}
