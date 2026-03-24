use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinSet;

use crate::doc_store::DocStore;
use crate::error::CoreError;
use crate::manifest::ProjectManifest;
use crate::persistence::Persistence;
use crate::types::*;

/// Manages all projects, their documents, manifests, and persistence.
///
/// This is the top-level API that Tauri commands interact with.
pub struct ProjectManager {
    persistence: Arc<Persistence>,
    doc_store: Arc<DocStore>,
    /// Per-project manifest, protected by RwLock.
    manifests: Arc<DashMap<String, Arc<RwLock<ProjectManifest>>>>,
    /// Background save tasks.
    save_tasks: Mutex<JoinSet<()>>,
}

use dashmap::DashMap;

impl ProjectManager {
    pub fn new(base_dir: PathBuf) -> Self {
        Self {
            persistence: Arc::new(Persistence::new(base_dir)),
            doc_store: Arc::new(DocStore::new()),
            manifests: Arc::new(DashMap::new()),
            save_tasks: Mutex::new(JoinSet::new()),
        }
    }

    /// Get a reference to the DocStore.
    pub fn doc_store(&self) -> &Arc<DocStore> {
        &self.doc_store
    }

    /// Get a reference to the Persistence layer.
    pub fn persistence(&self) -> &Arc<Persistence> {
        &self.persistence
    }

    // ── Project operations ───────────────────────────────────────────

    /// Create a new project with an initialized .p2p directory and manifest.
    pub async fn create_project(&self, name: &str) -> Result<(), CoreError> {
        if self.persistence.is_initialized(name).await {
            return Err(CoreError::ProjectAlreadyExists(name.to_string()));
        }

        // Create directory structure
        self.persistence.ensure_project_dirs(name).await?;

        // Create and save manifest
        let mut manifest = ProjectManifest::new(name);
        let data = manifest.save();
        self.persistence.save_manifest(name, &data).await?;

        // Store in memory
        self.manifests
            .insert(name.to_string(), Arc::new(RwLock::new(manifest)));

        log::info!("Created project: {name}");
        Ok(())
    }

    /// Open an existing project: load manifest and make it ready for use.
    pub async fn open_project(&self, name: &str) -> Result<(), CoreError> {
        // Skip if already loaded
        if self.manifests.contains_key(name) {
            return Ok(());
        }

        if !self.persistence.is_initialized(name).await {
            return Err(CoreError::ProjectNotFound(name.to_string()));
        }

        // Load manifest
        let data = self.persistence.load_manifest(name).await?;
        let manifest = ProjectManifest::load(&data)?;

        self.manifests
            .insert(name.to_string(), Arc::new(RwLock::new(manifest)));

        log::info!("Opened project: {name}");
        Ok(())
    }

    /// List all projects (directories in the base folder).
    pub async fn list_projects(&self) -> Result<Vec<String>, CoreError> {
        self.persistence.list_projects().await
    }

    // ── Document operations ──────────────────────────────────────────

    /// Create a new note in a project. Returns the doc ID.
    pub async fn create_note(
        &self,
        project_name: &str,
        path: &str,
    ) -> Result<DocId, CoreError> {
        let manifest_arc = self.get_manifest(project_name)?;
        let mut manifest = manifest_arc.write().await;

        // Register in manifest
        let doc_id = manifest.add_file(path, FileType::Note)?;

        // Create Automerge document in the store
        let created_id = self.doc_store.create_doc();
        // We want a specific ID, so remove the auto-generated one and re-insert with our ID
        // Actually, DocStore.create_doc() generates its own ID. Let's load it differently.
        let doc_data = self.doc_store.save_doc(&created_id).await?;
        self.doc_store.remove_doc(&created_id);
        self.doc_store.load_doc(doc_id, &doc_data)?;

        // Save manifest
        let manifest_data = manifest.save();
        self.persistence
            .save_manifest(project_name, &manifest_data)
            .await?;

        // Save the new document
        let doc_data = self.doc_store.save_doc(&doc_id).await?;
        self.persistence
            .save_doc(project_name, &doc_id, &doc_data)
            .await?;

        // Export empty markdown file
        self.persistence
            .export_markdown(project_name, path, "")
            .await?;

        log::info!("Created note {doc_id} at {path} in {project_name}");
        Ok(doc_id)
    }

    /// Open a document: load from disk into DocStore if not already loaded.
    pub async fn open_doc(
        &self,
        project_name: &str,
        doc_id: &DocId,
    ) -> Result<(), CoreError> {
        if self.doc_store.contains(doc_id) {
            return Ok(());
        }

        let data = self.persistence.load_doc(project_name, doc_id).await?;
        self.doc_store.load_doc(*doc_id, &data)?;

        log::info!("Loaded document {doc_id} from {project_name}");
        Ok(())
    }

    /// Close a document: save to disk and remove from memory.
    pub async fn close_doc(
        &self,
        project_name: &str,
        doc_id: &DocId,
    ) -> Result<(), CoreError> {
        if !self.doc_store.contains(doc_id) {
            return Ok(());
        }

        // Save before closing
        self.save_doc(project_name, doc_id).await?;

        // Remove from memory
        self.doc_store.remove_doc(doc_id);
        log::info!("Closed document {doc_id}");
        Ok(())
    }

    /// Save a document to disk and export its markdown.
    pub async fn save_doc(
        &self,
        project_name: &str,
        doc_id: &DocId,
    ) -> Result<(), CoreError> {
        // Save Automerge binary
        let data = self.doc_store.save_doc(doc_id).await?;
        self.persistence
            .save_doc(project_name, doc_id, &data)
            .await?;

        // Export markdown
        let manifest_arc = self.get_manifest(project_name)?;
        let manifest = manifest_arc.read().await;
        if let Ok(path) = manifest.get_file_path(doc_id) {
            let text = self.doc_store.get_text(doc_id).await.unwrap_or_default();
            self.persistence
                .export_markdown(project_name, &path, &text)
                .await?;
        }

        Ok(())
    }

    /// Delete a note from a project.
    pub async fn delete_note(
        &self,
        project_name: &str,
        doc_id: &DocId,
    ) -> Result<(), CoreError> {
        let manifest_arc = self.get_manifest(project_name)?;
        let mut manifest = manifest_arc.write().await;

        // Get the path before removing from manifest
        let path = manifest.get_file_path(doc_id).ok();

        // Remove from manifest
        manifest.remove_file(doc_id)?;

        // Save manifest
        let manifest_data = manifest.save();
        self.persistence
            .save_manifest(project_name, &manifest_data)
            .await?;

        // Remove from DocStore
        self.doc_store.remove_doc(doc_id);

        // Delete from disk
        self.persistence.delete_doc(project_name, doc_id).await?;

        // Delete the .md export if it exists
        if let Some(path) = path {
            let md_path = self.persistence.base_dir().join(project_name).join(&path);
            if md_path.exists() {
                // Need to make writable before deleting on unix
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let perms = std::fs::Permissions::from_mode(0o644);
                    let _ = std::fs::set_permissions(&md_path, perms);
                }
                let _ = tokio::fs::remove_file(&md_path).await;
            }
        }

        log::info!("Deleted note {doc_id} from {project_name}");
        Ok(())
    }

    /// Rename a note (changes path in manifest, re-exports markdown).
    pub async fn rename_note(
        &self,
        project_name: &str,
        doc_id: &DocId,
        new_path: &str,
    ) -> Result<(), CoreError> {
        let manifest_arc = self.get_manifest(project_name)?;
        let mut manifest = manifest_arc.write().await;

        let old_path = manifest.get_file_path(doc_id)?;
        manifest.rename_file(doc_id, new_path)?;

        // Save manifest
        let manifest_data = manifest.save();
        self.persistence
            .save_manifest(project_name, &manifest_data)
            .await?;

        // Delete old .md export
        let old_md = self.persistence.base_dir().join(project_name).join(&old_path);
        if old_md.exists() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let perms = std::fs::Permissions::from_mode(0o644);
                let _ = std::fs::set_permissions(&old_md, perms);
            }
            let _ = tokio::fs::remove_file(&old_md).await;
        }

        // Re-export markdown at new path
        if self.doc_store.contains(doc_id) {
            let text = self.doc_store.get_text(doc_id).await.unwrap_or_default();
            self.persistence
                .export_markdown(project_name, new_path, &text)
                .await?;
        }

        log::info!("Renamed note {doc_id}: {old_path} -> {new_path}");
        Ok(())
    }

    /// List all files in a project.
    pub async fn list_files(
        &self,
        project_name: &str,
    ) -> Result<Vec<DocInfo>, CoreError> {
        let manifest_arc = self.get_manifest(project_name)?;
        let manifest = manifest_arc.read().await;
        manifest.list_files()
    }

    /// Get the text content of a loaded document.
    pub async fn get_doc_text(&self, doc_id: &DocId) -> Result<String, CoreError> {
        self.doc_store.get_text(doc_id).await
    }

    /// Apply incremental changes from the frontend Automerge WASM instance.
    pub async fn apply_changes(
        &self,
        project_name: &str,
        doc_id: &DocId,
        data: &[u8],
    ) -> Result<(), CoreError> {
        self.doc_store.apply_incremental(doc_id, data).await?;

        // Debounced save will handle persistence, but we can trigger an immediate
        // save for now (a proper debounced save loop comes in Phase 2).
        self.save_doc(project_name, doc_id).await?;

        Ok(())
    }

    /// Get a full save of a document (for the frontend to initialize its WASM Automerge).
    pub async fn get_doc_binary(&self, doc_id: &DocId) -> Result<Vec<u8>, CoreError> {
        self.doc_store.save_doc(doc_id).await
    }

    /// Compact a document to reduce memory/disk usage.
    pub async fn compact_doc(
        &self,
        project_name: &str,
        doc_id: &DocId,
    ) -> Result<(), CoreError> {
        self.doc_store.compact(doc_id).await?;
        self.save_doc(project_name, doc_id).await?;
        Ok(())
    }

    // ── Helpers ──────────────────────────────────────────────────────

    fn get_manifest(
        &self,
        project_name: &str,
    ) -> Result<Arc<RwLock<ProjectManifest>>, CoreError> {
        self.manifests
            .get(project_name)
            .map(|entry| Arc::clone(entry.value()))
            .ok_or_else(|| CoreError::ProjectNotFound(project_name.to_string()))
    }

    /// Start a background save loop for a document.
    /// Saves every `interval` until the document is closed.
    pub async fn start_save_loop(
        &self,
        project_name: String,
        doc_id: DocId,
        interval: Duration,
    ) {
        let doc_store = Arc::clone(&self.doc_store);
        let persistence = Arc::clone(&self.persistence);
        let manifests = Arc::clone(&self.manifests);

        let mut tasks = self.save_tasks.lock().await;
        tasks.spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                ticker.tick().await;

                // If document is no longer loaded, stop the save loop
                if !doc_store.contains(&doc_id) {
                    log::debug!("Save loop for {doc_id} stopping: doc unloaded");
                    break;
                }

                // Save Automerge binary
                match doc_store.save_doc(&doc_id).await {
                    Ok(data) => {
                        if let Err(e) = persistence
                            .save_doc(&project_name, &doc_id, &data)
                            .await
                        {
                            log::error!("Background save failed for {doc_id}: {e}");
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to serialize {doc_id} for save: {e}");
                    }
                }

                // Export markdown
                if let Some(manifest_entry) = manifests.get(&project_name) {
                    let manifest = manifest_entry.value().read().await;
                    if let Ok(path) = manifest.get_file_path(&doc_id) {
                        let text = doc_store.get_text(&doc_id).await.unwrap_or_default();
                        if let Err(e) = persistence
                            .export_markdown(&project_name, &path, &text)
                            .await
                        {
                            log::error!("Background markdown export failed for {doc_id}: {e}");
                        }
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_and_open_project() {
        let dir = tempfile::tempdir().unwrap();
        let pm = ProjectManager::new(dir.path().to_path_buf());

        pm.create_project("my-project").await.unwrap();

        let projects = pm.list_projects().await.unwrap();
        assert!(projects.contains(&"my-project".to_string()));
    }

    #[tokio::test]
    async fn test_create_note_and_list() {
        let dir = tempfile::tempdir().unwrap();
        let pm = ProjectManager::new(dir.path().to_path_buf());

        pm.create_project("test").await.unwrap();

        let id = pm.create_note("test", "hello.md").await.unwrap();
        let files = pm.list_files("test").await.unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].id, id);
        assert_eq!(files[0].path, "hello.md");
    }

    #[tokio::test]
    async fn test_open_and_read_doc() {
        let dir = tempfile::tempdir().unwrap();
        let pm = ProjectManager::new(dir.path().to_path_buf());

        pm.create_project("test").await.unwrap();
        let id = pm.create_note("test", "hello.md").await.unwrap();

        // Document should be loaded after creation
        let text = pm.get_doc_text(&id).await.unwrap();
        assert_eq!(text, "");
    }

    #[tokio::test]
    async fn test_delete_note() {
        let dir = tempfile::tempdir().unwrap();
        let pm = ProjectManager::new(dir.path().to_path_buf());

        pm.create_project("test").await.unwrap();
        let id = pm.create_note("test", "to-delete.md").await.unwrap();
        assert_eq!(pm.list_files("test").await.unwrap().len(), 1);

        pm.delete_note("test", &id).await.unwrap();
        assert_eq!(pm.list_files("test").await.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_rename_note() {
        let dir = tempfile::tempdir().unwrap();
        let pm = ProjectManager::new(dir.path().to_path_buf());

        pm.create_project("test").await.unwrap();
        let id = pm.create_note("test", "old.md").await.unwrap();

        pm.rename_note("test", &id, "new.md").await.unwrap();

        let files = pm.list_files("test").await.unwrap();
        assert_eq!(files[0].path, "new.md");
    }

    #[tokio::test]
    async fn test_persistence_across_reloads() {
        let dir = tempfile::tempdir().unwrap();

        let id;
        // Create project and note
        {
            let pm = ProjectManager::new(dir.path().to_path_buf());
            pm.create_project("test").await.unwrap();
            id = pm.create_note("test", "persist.md").await.unwrap();
        }

        // Reload from disk
        {
            let pm = ProjectManager::new(dir.path().to_path_buf());
            pm.open_project("test").await.unwrap();
            pm.open_doc("test", &id).await.unwrap();

            let text = pm.get_doc_text(&id).await.unwrap();
            assert_eq!(text, "");
        }
    }
}
