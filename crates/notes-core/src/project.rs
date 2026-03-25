use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::doc_store::DocStore;
use crate::error::CoreError;
use crate::manifest::ProjectManifest;
use crate::persistence::Persistence;
use crate::search::SearchIndex;
use crate::types::*;
use crate::validation;

/// Manages all projects, their documents, manifests, and persistence.
///
/// This is the top-level API that Tauri commands interact with.
pub struct ProjectManager {
    persistence: Arc<Persistence>,
    doc_store: Arc<DocStore>,
    /// Per-project manifest, protected by RwLock.
    manifests: DashMap<String, Arc<RwLock<ProjectManifest>>>,
    /// Per-project epoch key managers (for encrypted projects).
    epoch_keys: DashMap<String, Arc<RwLock<notes_crypto::EpochKeyManager>>>,
    /// Optional search index (shared across all projects).
    search_index: Option<Arc<std::sync::Mutex<SearchIndex>>>,
    /// Background save tasks.
    save_tasks: Mutex<JoinSet<()>>,
    /// Cancellation tokens for active save loops, keyed by DocId.
    save_tokens: DashMap<DocId, CancellationToken>,
    /// Mapping from DocId to project name (for shutdown saves).
    doc_projects: DashMap<DocId, String>,
}

impl ProjectManager {
    pub fn new(base_dir: PathBuf) -> Self {
        Self {
            persistence: Arc::new(Persistence::new(base_dir)),
            doc_store: Arc::new(DocStore::new()),
            manifests: DashMap::new(),
            epoch_keys: DashMap::new(),
            search_index: None,
            save_tasks: Mutex::new(JoinSet::new()),
            save_tokens: DashMap::new(),
            doc_projects: DashMap::new(),
        }
    }

    /// Create a ProjectManager with a search index.
    pub fn with_search_index(
        base_dir: PathBuf,
        search_index: Arc<std::sync::Mutex<SearchIndex>>,
    ) -> Self {
        Self {
            persistence: Arc::new(Persistence::new(base_dir)),
            doc_store: Arc::new(DocStore::new()),
            manifests: DashMap::new(),
            epoch_keys: DashMap::new(),
            search_index: Some(search_index),
            save_tasks: Mutex::new(JoinSet::new()),
            save_tokens: DashMap::new(),
            doc_projects: DashMap::new(),
        }
    }

    /// Get a reference to the DocStore.
    pub fn doc_store(&self) -> &DocStore {
        &self.doc_store
    }

    /// Get a reference to the Persistence layer.
    pub fn persistence(&self) -> &Persistence {
        &self.persistence
    }

    /// Expose a loaded manifest for read-only UI metadata queries.
    pub fn get_manifest_for_ui(
        &self,
        project_name: &str,
    ) -> Result<Arc<RwLock<ProjectManifest>>, CoreError> {
        self.get_manifest(project_name)
    }

    /// Get the project owner's peer ID.
    pub async fn get_project_owner(&self, project_name: &str) -> Result<String, CoreError> {
        let manifest_arc = self.get_manifest(project_name)?;
        let manifest = manifest_arc.read().await;
        manifest.get_owner()
    }

    /// Get the peers list from the manifest.
    pub async fn get_project_peers(
        &self,
        project_name: &str,
    ) -> Result<Vec<PeerInfo>, CoreError> {
        let manifest_arc = self.get_manifest(project_name)?;
        let manifest = manifest_arc.read().await;
        manifest.list_peers()
    }

    // ── Project operations ───────────────────────────────────────────

    /// Create a new project with an initialized .p2p directory and manifest.
    pub async fn create_project(&self, name: &str) -> Result<(), CoreError> {
        validation::validate_project_name(name)?;

        if self.persistence.is_initialized(name).await {
            return Err(CoreError::ProjectAlreadyExists(name.to_string()));
        }

        // Create directory structure
        self.persistence.ensure_project_dirs(name).await?;

        // Create and save manifest
        let mut manifest = ProjectManifest::new(name)?;
        let data = manifest.save();
        self.persistence.save_manifest(name, &data).await?;

        // Store in memory (atomic insert-if-absent)
        self.manifests
            .entry(name.to_string())
            .or_insert_with(|| Arc::new(RwLock::new(manifest)));

        log::info!("Created project: {name}");
        Ok(())
    }

    /// Open an existing project: load manifest and make it ready for use.
    pub async fn open_project(&self, name: &str) -> Result<(), CoreError> {
        validation::validate_project_name(name)?;

        // Fast path: already loaded
        if self.manifests.contains_key(name) {
            return Ok(());
        }

        if !self.persistence.is_initialized(name).await {
            return Err(CoreError::ProjectNotFound(name.to_string()));
        }

        // Load manifest
        let data = self.persistence.load_manifest(name).await?;
        let manifest = ProjectManifest::load(&data)?;

        // Atomic insert-if-absent (handles concurrent open_project calls)
        self.manifests
            .entry(name.to_string())
            .or_insert_with(|| Arc::new(RwLock::new(manifest)));

        log::info!("Opened project: {name}");
        Ok(())
    }

    /// List all projects (directories in the base folder).
    pub async fn list_projects(&self) -> Result<Vec<String>, CoreError> {
        self.persistence.list_projects().await
    }

    /// Describe all projects for frontend bootstrapping.
    pub async fn list_project_summaries(
        &self,
        local_peer_id: &str,
    ) -> Result<Vec<ProjectSummary>, CoreError> {
        let mut summaries = Vec::new();

        for name in self.list_projects().await? {
            self.open_project(&name).await?;
            let manifest_arc = self.get_manifest(&name)?;
            let manifest = manifest_arc.read().await;
            let peers = manifest.list_peers()?;
            let owner = manifest.get_owner().unwrap_or_default();
            let shared = !owner.is_empty() || !peers.is_empty();
            let role = if owner.is_empty() || owner == local_peer_id {
                PeerRole::Owner
            } else {
                peers.iter()
                    .find(|peer| peer.peer_id == local_peer_id)
                    .map(|peer| peer.role)
                    .unwrap_or(PeerRole::Viewer)
            };

            let file_count = self
                .list_files(&name)
                .await
                .map(|f| f.len())
                .unwrap_or(0);

            summaries.push(ProjectSummary {
                name: name.clone(),
                path: self.persistence.project_dir(&name).display().to_string(),
                shared,
                role,
                peer_count: peers.len(),
                file_count,
            });
        }

        summaries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(summaries)
    }

    // ── Document operations ──────────────────────────────────────────

    /// Create a new note in a project. Returns the doc ID.
    pub async fn create_note(
        &self,
        project_name: &str,
        path: &str,
    ) -> Result<DocId, CoreError> {
        validation::validate_project_name(project_name)?;
        validation::validate_note_path(path)?;

        let manifest_arc = self.get_manifest(project_name)?;

        // Short critical section: mutate manifest, serialize, release
        let (doc_id, manifest_data) = {
            let mut manifest = manifest_arc.write().await;
            let doc_id = manifest.add_file(path, FileType::Note)?;
            let manifest_data = manifest.save();
            (doc_id, manifest_data)
        }; // write lock dropped here

        // Create Automerge document with the manifest-assigned ID
        self.doc_store.create_doc_with_id(doc_id)?;

        // Save the new document first (before manifest, so manifest never points to missing doc)
        let doc_data = self.doc_store.save_doc(&doc_id).await?;
        if let Err(e) = self
            .persistence
            .save_doc(project_name, &doc_id, &doc_data)
            .await
        {
            // Compensate: undo in-memory changes
            self.doc_store.remove_doc(&doc_id);
            let mut manifest = manifest_arc.write().await;
            let _ = manifest.remove_file(&doc_id);
            return Err(e);
        }

        // Save manifest
        if let Err(e) = self
            .persistence
            .save_manifest(project_name, &manifest_data)
            .await
        {
            // Compensate: remove the doc we just saved
            let _ = self.persistence.delete_doc(project_name, &doc_id).await;
            self.doc_store.remove_doc(&doc_id);
            let mut manifest = manifest_arc.write().await;
            let _ = manifest.remove_file(&doc_id);
            return Err(e);
        }

        // Export empty markdown file (best-effort)
        if let Err(e) = self
            .persistence
            .export_markdown(project_name, path, "")
            .await
        {
            log::warn!("Markdown export failed for new note {doc_id}: {e}");
        }

        // Track doc -> project mapping
        self.doc_projects
            .insert(doc_id, project_name.to_string());

        // Start background save loop
        self.start_save_loop(project_name.to_string(), doc_id, Duration::from_secs(5))
            .await;

        log::info!("Created note {doc_id} at {path} in {project_name}");
        Ok(doc_id)
    }

    /// Open a document: load from disk into DocStore if not already loaded.
    pub async fn open_doc(
        &self,
        project_name: &str,
        doc_id: &DocId,
    ) -> Result<(), CoreError> {
        validation::validate_project_name(project_name)?;

        if self.doc_store.contains(doc_id) {
            return Ok(());
        }

        // Load doc — use encrypted loading if epoch keys exist for this project
        let data = if let Some(epoch_mgr) = self.epoch_keys.get(project_name) {
            let mgr = epoch_mgr.read().await;
            if let Ok(key) = mgr.current_key() {
                self.persistence
                    .load_doc_encrypted(project_name, doc_id, key.as_bytes())
                    .await?
            } else {
                self.persistence.load_doc(project_name, doc_id).await?
            }
        } else {
            self.persistence.load_doc(project_name, doc_id).await?
        };
        self.doc_store.load_doc(*doc_id, &data)?;

        // Track doc -> project mapping
        self.doc_projects
            .insert(*doc_id, project_name.to_string());

        // Start background save loop
        self.start_save_loop(project_name.to_string(), *doc_id, Duration::from_secs(5))
            .await;

        log::info!("Loaded document {doc_id} from {project_name}");
        Ok(())
    }

    /// Close a document: cancel save loop, save to disk, remove from memory.
    pub async fn close_doc(
        &self,
        project_name: &str,
        doc_id: &DocId,
    ) -> Result<(), CoreError> {
        if !self.doc_store.contains(doc_id) {
            return Ok(());
        }

        // Cancel the save loop first
        if let Some((_, token)) = self.save_tokens.remove(doc_id) {
            token.cancel();
        }

        // Final save before unloading
        self.save_doc(project_name, doc_id).await?;

        // Remove from memory
        self.doc_store.remove_doc(doc_id);
        self.doc_projects.remove(doc_id);
        log::info!("Closed document {doc_id}");
        Ok(())
    }

    /// Save a document to disk and export its markdown.
    /// Lock ordering: manifest(read) first, then doc_store.
    pub async fn save_doc(
        &self,
        project_name: &str,
        doc_id: &DocId,
    ) -> Result<(), CoreError> {
        // 1. Read manifest first (consistent lock ordering: manifest -> doc)
        let path = {
            let manifest_arc = self.get_manifest(project_name)?;
            let manifest = manifest_arc.read().await;
            manifest.get_file_path(doc_id).ok()
        }; // manifest lock dropped

        // 2. Save Automerge binary (encrypted if epoch keys available)
        let data = self.doc_store.save_doc(doc_id).await?;
        if let Some(epoch_mgr) = self.epoch_keys.get(project_name) {
            let mgr = epoch_mgr.read().await;
            if let Ok(key) = mgr.current_key() {
                self.persistence
                    .save_doc_encrypted(
                        project_name,
                        doc_id,
                        &data,
                        key.as_bytes(),
                        mgr.current_epoch(),
                    )
                    .await?;
            } else {
                self.persistence
                    .save_doc(project_name, doc_id, &data)
                    .await?;
            }
        } else {
            self.persistence
                .save_doc(project_name, doc_id, &data)
                .await?;
        }

        // 3. Export markdown + update search index (no locks held)
        if let Some(ref path) = path {
            let text = self.doc_store.get_text(doc_id).await.unwrap_or_default();
            if let Err(e) = self
                .persistence
                .export_markdown(project_name, path, &text)
                .await
            {
                log::warn!("Markdown export failed for {doc_id}: {e}");
            }

            // 4. Update search index
            if let Some(ref search) = self.search_index {
                if let Ok(index) = search.lock() {
                    if let Err(e) = index.index_document(doc_id, project_name, path, &text) {
                        log::warn!("Search index update failed for {doc_id}: {e}");
                    }
                }
            }
        }

        Ok(())
    }

    /// Delete a note from a project.
    pub async fn delete_note(
        &self,
        project_name: &str,
        doc_id: &DocId,
    ) -> Result<(), CoreError> {
        validation::validate_project_name(project_name)?;

        // Cancel save loop
        if let Some((_, token)) = self.save_tokens.remove(doc_id) {
            token.cancel();
        }

        let manifest_arc = self.get_manifest(project_name)?;

        // Short critical section: get path and remove from manifest
        let (old_path, manifest_data) = {
            let mut manifest = manifest_arc.write().await;
            let path = manifest.get_file_path(doc_id).ok();
            manifest.remove_file(doc_id)?;
            let data = manifest.save();
            (path, data)
        }; // write lock dropped

        // All I/O outside the lock
        self.persistence
            .save_manifest(project_name, &manifest_data)
            .await?;

        self.doc_store.remove_doc(doc_id);
        self.doc_projects.remove(doc_id);
        self.persistence.delete_doc(project_name, doc_id).await?;

        // Delete the .md export if it exists
        if let Some(path) = old_path {
            let md_path = self.persistence.base_dir().join(project_name).join(&path);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let perms = std::fs::Permissions::from_mode(0o644);
                let _ = std::fs::set_permissions(&md_path, perms);
            }
            let _ = tokio::fs::remove_file(&md_path).await;
        }

        // Remove from search index
        if let Some(ref search) = self.search_index {
            if let Ok(index) = search.lock() {
                let _ = index.remove_document(doc_id);
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
        validation::validate_project_name(project_name)?;
        validation::validate_note_path(new_path)?;

        let manifest_arc = self.get_manifest(project_name)?;

        // Short critical section
        let (old_path, manifest_data) = {
            let mut manifest = manifest_arc.write().await;
            let old_path = manifest.get_file_path(doc_id)?;
            manifest.rename_file(doc_id, new_path)?;
            let data = manifest.save();
            (old_path, data)
        }; // write lock dropped

        // All I/O outside the lock
        self.persistence
            .save_manifest(project_name, &manifest_data)
            .await?;

        // Delete old .md export
        let old_md = self
            .persistence
            .base_dir()
            .join(project_name)
            .join(&old_path);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o644);
            let _ = std::fs::set_permissions(&old_md, perms);
        }
        let _ = tokio::fs::remove_file(&old_md).await;

        // Re-export markdown at new path
        if self.doc_store.contains(doc_id) {
            let text = self.doc_store.get_text(doc_id).await.unwrap_or_default();
            if let Err(e) = self
                .persistence
                .export_markdown(project_name, new_path, &text)
                .await
            {
                log::warn!("Markdown export failed for renamed {doc_id}: {e}");
            }
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
    /// Does NOT save to disk — the background save loop handles persistence.
    pub async fn apply_changes(
        &self,
        _project_name: &str,
        doc_id: &DocId,
        data: &[u8],
    ) -> Result<(), CoreError> {
        self.doc_store.apply_incremental(doc_id, data).await
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

    /// Graceful shutdown: save all loaded documents.
    pub async fn shutdown(&self) {
        // Cancel all save loops
        self.save_tokens.iter().for_each(|entry| {
            entry.value().cancel();
        });

        // Wait for save tasks to finish
        {
            let mut tasks = self.save_tasks.lock().await;
            tasks.shutdown().await;
        }

        // Save all dirty documents
        let doc_ids = self.doc_store.loaded_doc_ids();
        for doc_id in &doc_ids {
            if let Some(entry) = self.doc_projects.get(doc_id) {
                let project_name = entry.value().clone();
                if let Err(e) = self.save_doc(&project_name, doc_id).await {
                    log::error!("Shutdown save failed for {doc_id}: {e}");
                }
            }
        }

        log::info!("Shutdown: saved {} documents", doc_ids.len());
    }

    // ── Background Save Loop ─────────────────────────────────────────

    /// Start a background save loop for a document.
    /// Only saves when the document has unsaved changes (dirty flag).
    async fn start_save_loop(
        &self,
        project_name: String,
        doc_id: DocId,
        interval: Duration,
    ) {
        // Cancel any existing loop for this doc
        if let Some((_, old_token)) = self.save_tokens.remove(&doc_id) {
            old_token.cancel();
        }

        let token = CancellationToken::new();
        self.save_tokens.insert(doc_id, token.clone());

        let doc_store = Arc::clone(&self.doc_store);
        let persistence = Arc::clone(&self.persistence);
        let manifests_ref = &self.manifests;

        // Clone the manifest Arc out of DashMap BEFORE spawning (avoid holding DashMap ref across await)
        let manifest_arc = manifests_ref
            .get(&project_name)
            .map(|entry| Arc::clone(entry.value()));

        let mut tasks = self.save_tasks.lock().await;
        tasks.spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            let mut consecutive_failures: u32 = 0;

            loop {
                tokio::select! {
                    _ = token.cancelled() => {
                        log::debug!("Save loop for {doc_id} cancelled");
                        break;
                    }
                    _ = ticker.tick() => {
                        // Skip if document is no longer loaded
                        if !doc_store.contains(&doc_id) {
                            log::debug!("Save loop for {doc_id} stopping: doc unloaded");
                            break;
                        }

                        // Skip if document has no unsaved changes
                        if !doc_store.take_dirty(&doc_id) {
                            continue;
                        }

                        // Save Automerge binary
                        let save_result = async {
                            let data = doc_store.save_doc(&doc_id).await?;
                            persistence
                                .save_doc(&project_name, &doc_id, &data)
                                .await?;
                            Ok::<(), CoreError>(())
                        }
                        .await;

                        match save_result {
                            Ok(()) => {
                                if consecutive_failures > 0 {
                                    log::info!("Save loop for {doc_id} recovered after {consecutive_failures} failures");
                                }
                                consecutive_failures = 0;
                            }
                            Err(e) => {
                                consecutive_failures += 1;
                                log::error!(
                                    "Background save failed for {doc_id} ({consecutive_failures}x): {e}"
                                );
                                // Re-mark as dirty so we retry next tick
                                doc_store.mark_dirty(&doc_id);
                            }
                        }

                        // Export markdown (best-effort, lower frequency)
                        if consecutive_failures == 0 {
                            if let Some(ref manifest_arc) = manifest_arc {
                                let manifest = manifest_arc.read().await;
                                if let Ok(path) = manifest.get_file_path(&doc_id) {
                                    let text =
                                        doc_store.get_text(&doc_id).await.unwrap_or_default();
                                    if let Err(e) = persistence
                                        .export_markdown(&project_name, &path, &text)
                                        .await
                                    {
                                        log::warn!(
                                            "Background markdown export failed for {doc_id}: {e}"
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });
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
    async fn test_create_project_validates_name() {
        let dir = tempfile::tempdir().unwrap();
        let pm = ProjectManager::new(dir.path().to_path_buf());

        assert!(pm.create_project("../../evil").await.is_err());
        assert!(pm.create_project("").await.is_err());
        assert!(pm.create_project(".hidden").await.is_err());
        assert!(pm.create_project("CON").await.is_err());
    }

    #[tokio::test]
    async fn test_create_duplicate_project() {
        let dir = tempfile::tempdir().unwrap();
        let pm = ProjectManager::new(dir.path().to_path_buf());

        pm.create_project("test").await.unwrap();
        assert!(matches!(
            pm.create_project("test").await,
            Err(CoreError::ProjectAlreadyExists(_))
        ));
    }

    #[tokio::test]
    async fn test_open_nonexistent_project() {
        let dir = tempfile::tempdir().unwrap();
        let pm = ProjectManager::new(dir.path().to_path_buf());

        assert!(matches!(
            pm.open_project("ghost").await,
            Err(CoreError::ProjectNotFound(_))
        ));
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
    async fn test_create_note_validates_path() {
        let dir = tempfile::tempdir().unwrap();
        let pm = ProjectManager::new(dir.path().to_path_buf());
        pm.create_project("test").await.unwrap();

        assert!(pm.create_note("test", "../../evil.md").await.is_err());
        assert!(pm.create_note("test", "hello.txt").await.is_err());
        assert!(pm.create_note("test", "").await.is_err());
    }

    #[tokio::test]
    async fn test_open_and_read_doc() {
        let dir = tempfile::tempdir().unwrap();
        let pm = ProjectManager::new(dir.path().to_path_buf());

        pm.create_project("test").await.unwrap();
        let id = pm.create_note("test", "hello.md").await.unwrap();

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
        {
            let pm = ProjectManager::new(dir.path().to_path_buf());
            pm.create_project("test").await.unwrap();
            id = pm.create_note("test", "persist.md").await.unwrap();
            pm.shutdown().await;
        }

        {
            let pm = ProjectManager::new(dir.path().to_path_buf());
            pm.open_project("test").await.unwrap();
            pm.open_doc("test", &id).await.unwrap();

            let text = pm.get_doc_text(&id).await.unwrap();
            assert_eq!(text, "");
        }
    }
}
