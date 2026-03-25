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
    /// Global search index (fallback for projects without per-project DBs).
    search_index: Option<Arc<std::sync::Mutex<SearchIndex>>>,
    /// Per-project search indexes (encrypted with project epoch key).
    project_search: DashMap<String, Arc<std::sync::Mutex<SearchIndex>>>,
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
            project_search: DashMap::new(),
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
            project_search: DashMap::new(),
            save_tasks: Mutex::new(JoinSet::new()),
            save_tokens: DashMap::new(),
            doc_projects: DashMap::new(),
        }
    }

    /// Create a ProjectManager with search and a stable device actor ID.
    pub fn with_full_config(
        base_dir: PathBuf,
        search_index: Arc<std::sync::Mutex<SearchIndex>>,
        device_actor_id: automerge::ActorId,
    ) -> Self {
        Self {
            persistence: Arc::new(Persistence::new(base_dir)),
            doc_store: Arc::new(DocStore::with_actor_id(device_actor_id)),
            manifests: DashMap::new(),
            epoch_keys: DashMap::new(),
            search_index: Some(search_index),
            project_search: DashMap::new(),
            save_tasks: Mutex::new(JoinSet::new()),
            save_tokens: DashMap::new(),
            doc_projects: DashMap::new(),
        }
    }

    /// Get the epoch key manager for a project (if loaded).
    pub fn get_epoch_keys(
        &self,
        project_name: &str,
    ) -> Result<Arc<RwLock<notes_crypto::EpochKeyManager>>, CoreError> {
        self.epoch_keys
            .get(project_name)
            .map(|entry| Arc::clone(entry.value()))
            .ok_or_else(|| CoreError::InvalidData(format!("no epoch keys for {project_name}")))
    }

    /// Get a reference to the DocStore.
    pub fn doc_store(&self) -> &DocStore {
        &self.doc_store
    }

    /// Initialize epoch keys for a project (called when sharing starts).
    pub async fn init_epoch_keys(&self, project_name: &str) -> Result<(), CoreError> {
        if self.epoch_keys.contains_key(project_name) {
            return Ok(()); // Already initialized
        }

        let mgr = notes_crypto::EpochKeyManager::new()
            .map_err(|e| CoreError::InvalidData(format!("epoch key init failed: {e}")))?;

        // Persist to OS keychain (primary) and encrypted file (fallback)
        let data = mgr.serialize()
            .map_err(|e| CoreError::InvalidData(format!("epoch key serialize failed: {e}")))?;
        let keys_dir = self
            .persistence
            .base_dir()
            .join(project_name)
            .join(".p2p")
            .join("keys");
        tokio::fs::create_dir_all(&keys_dir).await?;
        let keystore = notes_crypto::KeyStore::new(keys_dir);
        let keychain_name = format!("epoch-keys-{project_name}");
        keystore
            .store_key(&keychain_name, &data)
            .map_err(|e| CoreError::InvalidData(format!("epoch key store failed: {e}")))?;

        self.epoch_keys.insert(
            project_name.to_string(),
            Arc::new(RwLock::new(mgr)),
        );

        log::info!("Initialized epoch keys for project {project_name}");
        Ok(())
    }

    /// Load epoch keys for a project from disk (if they exist).
    pub async fn load_epoch_keys(&self, project_name: &str) -> Result<bool, CoreError> {
        if self.epoch_keys.contains_key(project_name) {
            return Ok(true);
        }

        let keys_dir = self
            .persistence
            .base_dir()
            .join(project_name)
            .join(".p2p")
            .join("keys");
        let keystore = notes_crypto::KeyStore::new(keys_dir.clone());
        let keychain_name = format!("epoch-keys-{project_name}");

        // Try loading from OS keychain (primary), fall back to legacy file
        let data = match keystore.load_key(&keychain_name) {
            Ok(data) => data,
            Err(_) => {
                // Fall back to legacy plaintext file for migration
                let legacy_path = keys_dir.join("epochs.json");
                match tokio::fs::read(&legacy_path).await {
                    Ok(data) => {
                        // Migrate: store in keychain and delete plaintext file
                        if keystore.store_key(&keychain_name, &data).is_ok() {
                            let _ = tokio::fs::remove_file(&legacy_path).await;
                            log::info!("Migrated epoch keys from file to keychain for {project_name}");
                        }
                        data
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
                    Err(e) => return Err(CoreError::Io(e)),
                }
            }
        };

        match notes_crypto::EpochKeyManager::deserialize(&data) {
            Ok(mgr) => {
                self.epoch_keys.insert(
                    project_name.to_string(),
                    Arc::new(RwLock::new(mgr)),
                );
                log::info!("Loaded epoch keys for project {project_name}");
                Ok(true)
            }
            Err(e) => Err(CoreError::InvalidData(format!("epoch key load failed: {e}"))),
        }
    }

    /// Ratchet epoch keys for a project (called when a peer is removed).
    pub async fn ratchet_epoch_keys(&self, project_name: &str) -> Result<u32, CoreError> {
        let mgr_arc = self
            .epoch_keys
            .get(project_name)
            .map(|e| Arc::clone(e.value()))
            .ok_or_else(|| CoreError::InvalidData("no epoch keys for project".into()))?;

        let new_epoch = {
            let mut mgr = mgr_arc.write().await;
            let epoch = mgr.ratchet()
                .map_err(|e| CoreError::InvalidData(format!("ratchet failed: {e}")))?;

            // Persist to OS keychain
            let data = mgr.serialize()
                .map_err(|e| CoreError::InvalidData(format!("serialize failed: {e}")))?;
            let keys_dir = self
                .persistence
                .base_dir()
                .join(project_name)
                .join(".p2p")
                .join("keys");
            let keystore = notes_crypto::KeyStore::new(keys_dir);
            let keychain_name = format!("epoch-keys-{project_name}");
            keystore
                .store_key(&keychain_name, &data)
                .map_err(|e| CoreError::InvalidData(format!("epoch key store failed: {e}")))?;
            epoch
        };

        log::info!("Ratcheted epoch keys for {project_name} to epoch {new_epoch}");
        Ok(new_epoch)
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

    /// Open per-project search and history databases.
    /// If the project has epoch keys, derives a SQLCipher encryption key.
    /// Otherwise, opens unencrypted (local-only projects).
    pub async fn open_project_databases(&self, project_name: &str) -> Result<(), CoreError> {
        // Skip if already opened
        if self.project_search.contains_key(project_name) {
            return Ok(());
        }

        let project_dir = self.persistence.project_dir(project_name);
        let p2p_dir = project_dir.join(".p2p");
        tokio::fs::create_dir_all(&p2p_dir).await.ok();

        // Derive encryption key from epoch key if available
        let db_key: Option<[u8; 32]> = if let Some(epoch_mgr) = self.epoch_keys.get(project_name) {
            let mgr = epoch_mgr.read().await;
            if let Ok(key) = mgr.current_key() {
                // Derive a SQLCipher key using HKDF with a db-specific context
                use hkdf::Hkdf;
                use sha2::Sha256;
                let hk = Hkdf::<Sha256>::new(None, key.as_bytes());
                let mut db_key = [0u8; 32];
                let info = format!("p2p-notes/v1/sqlite-encryption/{project_name}");
                hk.expand(info.as_bytes(), &mut db_key)
                    .map_err(|_| CoreError::InvalidData("HKDF expand failed".into()))?;
                Some(db_key)
            } else {
                None
            }
        } else {
            None
        };

        let key_ref = db_key.as_ref();

        // Open per-project search index
        let search_path = p2p_dir.join("search.db");
        let search = SearchIndex::open(&search_path, key_ref)?;
        self.project_search.insert(
            project_name.to_string(),
            Arc::new(std::sync::Mutex::new(search)),
        );

        log::info!(
            "Opened per-project databases for {project_name} (encrypted: {})",
            db_key.is_some()
        );
        Ok(())
    }

    /// Get the search index for a project (per-project if available, global fallback).
    pub fn search_index_for_project(
        &self,
        project_name: &str,
    ) -> Option<Arc<std::sync::Mutex<SearchIndex>>> {
        // Prefer per-project store
        if let Some(index) = self.project_search.get(project_name) {
            return Some(Arc::clone(index.value()));
        }
        // Fall back to global index
        self.search_index.as_ref().map(Arc::clone)
    }

    /// Validate manifest integrity after receiving remote changes.
    ///
    /// Call this after a manifest sync completes. Checks that `_ownerControlled`
    /// fields were only modified by the project owner. If unauthorized changes
    /// are detected, reverts the manifest to `before_heads` and returns an error.
    ///
    /// `before_heads` should be captured before the sync starts.
    /// `owner_actor_hex` is the Automerge actor ID of the owner (from the manifest).
    pub async fn validate_manifest_after_sync(
        &self,
        project_name: &str,
        before_heads: &[automerge::ChangeHash],
        owner_actor_hex: &str,
    ) -> Result<(), CoreError> {
        let manifest_arc = self.get_manifest(project_name)?;
        let mut manifest = manifest_arc.write().await;

        if let Err(e) = manifest.validate_owner_controlled_changes(before_heads, owner_actor_hex) {
            log::error!(
                "Manifest validation failed for {project_name}: {e}. \
                 Reverting to pre-sync state."
            );
            // Revert: reload manifest from the last persisted save
            drop(manifest);
            let data = self.persistence.load_manifest(project_name).await?;
            let reverted = crate::manifest::ProjectManifest::load(&data)?;
            self.manifests
                .insert(project_name.to_string(), Arc::new(RwLock::new(reverted)));
            return Err(e);
        }

        Ok(())
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

        // Open per-project encrypted databases (search + history)
        if let Err(e) = self.open_project_databases(name).await {
            log::warn!("Failed to open per-project databases for {name}: {e}");
            // Non-fatal: will fall back to global databases
        }

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
        // Use encrypted save path when epoch keys are available
        let doc_data = self.doc_store.save_doc(&doc_id).await?;
        let save_result = if let Some(epoch_mgr) = self.epoch_keys.get(project_name) {
            let mgr = epoch_mgr.read().await;
            if let Ok(key) = mgr.current_key() {
                self.persistence
                    .save_doc_encrypted(project_name, &doc_id, &doc_data, key.as_bytes(), mgr.current_epoch())
                    .await
            } else {
                self.persistence.save_doc(project_name, &doc_id, &doc_data).await
            }
        } else {
            self.persistence.save_doc(project_name, &doc_id, &doc_data).await
        };
        if let Err(e) = save_result {
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
    /// Version history is preserved in the VersionStore (SQLite) — compaction
    /// only affects the in-memory/on-disk Automerge document.
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

    /// Reindex all documents in the search index.
    /// Call on startup to ensure the FTS5 index is up-to-date.
    pub async fn reindex_search(&self) -> usize {
        let search = match &self.search_index {
            Some(s) => s,
            None => return 0,
        };

        let projects = self.persistence.list_projects().await.unwrap_or_default();
        let mut indexed = 0;

        for project in &projects {
            if let Err(_) = self.open_project(project).await {
                continue;
            }
            let files = self.list_files(project).await.unwrap_or_default();
            for file in files {
                // Try to read the .md export (faster than loading automerge)
                let md_path = self
                    .persistence
                    .base_dir()
                    .join(project)
                    .join(&file.path);
                let content = tokio::fs::read_to_string(&md_path)
                    .await
                    .unwrap_or_default();
                if let Ok(index) = search.lock() {
                    if let Err(e) =
                        index.index_document(&file.id, project, &file.path, &content)
                    {
                        log::warn!("Reindex failed for {}: {e}", file.path);
                    } else {
                        indexed += 1;
                    }
                }
            }
        }

        log::info!("Reindexed {indexed} documents in search index");
        indexed
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

        // Clone epoch key manager for encrypted saves (None for local-only projects)
        let epoch_mgr = self
            .epoch_keys
            .get(&project_name)
            .map(|entry| Arc::clone(entry.value()));

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

                        // Save Automerge binary (encrypted if epoch keys available)
                        let save_result = async {
                            let data = doc_store.save_doc(&doc_id).await?;
                            if let Some(ref mgr_arc) = epoch_mgr {
                                let mgr = mgr_arc.read().await;
                                if let Ok(key) = mgr.current_key() {
                                    persistence
                                        .save_doc_encrypted(
                                            &project_name,
                                            &doc_id,
                                            &data,
                                            key.as_bytes(),
                                            mgr.current_epoch(),
                                        )
                                        .await?;
                                } else {
                                    persistence
                                        .save_doc(&project_name, &doc_id, &data)
                                        .await?;
                                }
                            } else {
                                persistence
                                    .save_doc(&project_name, &doc_id, &data)
                                    .await?;
                            }
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
