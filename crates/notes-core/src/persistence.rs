use std::path::{Path, PathBuf};

use tokio::fs;

use crate::error::CoreError;
use crate::types::DocId;

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
        let p2p_dir = self.base_dir.join(project_name).join(".p2p");
        fs::create_dir_all(p2p_dir.join("automerge")).await?;
        fs::create_dir_all(p2p_dir.join("keys")).await?;
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

        // If the file already exists, create a backup first
        if path.exists() {
            // Copy current to backup (not rename, since we still need current until tmp is ready)
            if let Err(e) = fs::copy(&path, &backup).await {
                log::warn!("Failed to create backup for {doc_id}: {e}");
            }
        }

        // Atomic write: tmp -> rename
        atomic_write(&path, data).await?;

        log::debug!("Saved document {doc_id}: {} bytes", data.len());
        Ok(())
    }

    /// Load a document from disk.
    /// Falls back to the backup file if the primary is corrupted.
    pub async fn load_doc(
        &self,
        project_name: &str,
        doc_id: &DocId,
    ) -> Result<Vec<u8>, CoreError> {
        let path = self.doc_path(project_name, doc_id);

        match fs::read(&path).await {
            Ok(data) => Ok(data),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(CoreError::DocNotFound(*doc_id))
            }
            Err(e) => {
                log::warn!("Failed to read {doc_id}, trying backup: {e}");
                let backup = self.backup_path(project_name, doc_id);
                match fs::read(&backup).await {
                    Ok(data) => {
                        log::info!("Recovered {doc_id} from backup");
                        // Restore the backup as the primary
                        if let Err(re) = fs::copy(&backup, &path).await {
                            log::warn!("Failed to restore backup to primary: {re}");
                        }
                        Ok(data)
                    }
                    Err(_) => Err(CoreError::Io(e)),
                }
            }
        }
    }

    /// Delete a document file and its backup from disk.
    pub async fn delete_doc(
        &self,
        project_name: &str,
        doc_id: &DocId,
    ) -> Result<(), CoreError> {
        let path = self.doc_path(project_name, doc_id);
        let backup = self.backup_path(project_name, doc_id);

        if path.exists() {
            fs::remove_file(&path).await?;
        }
        if backup.exists() {
            fs::remove_file(&backup).await?;
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
            if let Err(e) = fs::set_permissions(&path, perms).await {
                log::warn!("Failed to set read-only permissions on {relative_path}: {e}");
            }
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
        if !dir.exists() {
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

    /// List all project directories (folders that contain a .p2p subdirectory,
    /// or plain folders that could become projects).
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
        self.base_dir
            .join(project_name)
            .join(".p2p")
            .join("automerge")
            .exists()
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

/// Write data to a file atomically via tmp + rename.
async fn atomic_write(path: &Path, data: &[u8]) -> Result<(), CoreError> {
    let tmp_path = path.with_extension("tmp");

    // Ensure parent dir exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }

    fs::write(&tmp_path, data).await?;
    fs::rename(&tmp_path, path).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let loaded = persistence.load_doc(project, &doc_id).await.unwrap();
        assert_eq!(loaded, data);
    }

    #[tokio::test]
    async fn test_backup_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let persistence = Persistence::new(dir.path());

        let project = "test-project";
        persistence.ensure_project_dirs(project).await.unwrap();

        let doc_id = Uuid::new_v4();

        // Save initial version
        persistence
            .save_doc(project, &doc_id, b"version 1")
            .await
            .unwrap();

        // Save second version (creates backup of version 1)
        persistence
            .save_doc(project, &doc_id, b"version 2")
            .await
            .unwrap();

        // Verify backup exists
        let backup_path = persistence.backup_path(project, &doc_id);
        assert!(backup_path.exists());
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
}
