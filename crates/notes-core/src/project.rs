use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

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
    /// Tracks whether a project's epoch keys were found during startup/runtime checks.
    epoch_key_presence: DashMap<String, bool>,
    /// Per-project cached X25519 identity bytes for this app session.
    x25519_identities: DashMap<String, Arc<ProjectX25519Identity>>,
    /// Tracks whether a project's X25519 identity exists in persisted storage.
    x25519_identity_presence: DashMap<String, bool>,
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

pub struct ProjectX25519Identity {
    #[allow(dead_code)]
    secret: Zeroizing<[u8; 32]>,
    public: [u8; 32],
}

impl ProjectX25519Identity {
    pub fn public_bytes(&self) -> [u8; 32] {
        self.public
    }
}

impl ProjectManager {
    fn manifest_doc_id_from_project_id(project_id: &str) -> DocId {
        uuid::Uuid::new_v5(
            &uuid::Uuid::NAMESPACE_URL,
            format!("p2p-notes/manifest/{project_id}").as_bytes(),
        )
    }

    pub async fn manifest_doc_id(&self, project_name: &str) -> Result<DocId, CoreError> {
        let manifest_arc = self.get_manifest(project_name)?;
        let manifest = manifest_arc.read().await;
        Ok(Self::manifest_doc_id_from_project_id(
            &manifest.project_id()?,
        ))
    }

    async fn sync_manifest_doc_from_bytes(
        &self,
        project_name: &str,
        manifest_data: &[u8],
    ) -> Result<DocId, CoreError> {
        let manifest = ProjectManifest::load(manifest_data)?;
        let manifest_doc_id = Self::manifest_doc_id_from_project_id(&manifest.project_id()?);

        if self.doc_store.contains(&manifest_doc_id) {
            self.doc_store
                .replace_doc(manifest_doc_id, manifest_data)
                .await?;
        } else {
            self.doc_store.load_doc(manifest_doc_id, manifest_data)?;
        }

        self.doc_projects
            .insert(manifest_doc_id, project_name.to_string());

        Ok(manifest_doc_id)
    }

    pub async fn ensure_manifest_doc_loaded(&self, project_name: &str) -> Result<DocId, CoreError> {
        let data = self.persistence.load_manifest(project_name).await?;
        self.sync_manifest_doc_from_bytes(project_name, &data).await
    }

    pub async fn ensure_local_actor_binding(
        &self,
        project_name: &str,
        local_peer_id: &str,
    ) -> Result<(), CoreError> {
        let Some(actor_id) = self.doc_store.device_actor_hex() else {
            return Ok(());
        };

        let manifest_arc = self.get_manifest(project_name)?;
        let mut manifest = manifest_arc.write().await;
        let owner = manifest.get_owner().unwrap_or_default();

        let mut changed = false;
        if owner == local_peer_id {
            if manifest.get_owner_actor_id()?.as_deref() != Some(actor_id.as_str()) {
                manifest.set_owner_actor_id(&actor_id)?;
                changed = true;
            }
        } else if manifest
            .list_peers()?
            .iter()
            .any(|peer| peer.peer_id == local_peer_id)
        {
            let aliases = manifest.get_actor_aliases()?;
            if !aliases.contains_key(&actor_id) {
                manifest.set_peer_actor_id(local_peer_id, &actor_id)?;
                changed = true;
            }
        }

        if !changed {
            return Ok(());
        }

        let data = manifest.save();
        drop(manifest);
        self.persistence.save_manifest(project_name, &data).await?;
        let _ = self
            .sync_manifest_doc_from_bytes(project_name, &data)
            .await?;
        Ok(())
    }

    async fn apply_manifest_doc_to_project_internal(
        &self,
        project_name: &str,
        manifest_doc_id: &DocId,
        create_missing_placeholders: bool,
    ) -> Result<Vec<DocId>, CoreError> {
        let manifest_data = self.doc_store.save_doc(manifest_doc_id).await?;
        self.persistence
            .save_manifest(project_name, &manifest_data)
            .await?;
        self.reload_manifest(project_name).await?;

        let manifest_arc = self.get_manifest(project_name)?;
        let files = {
            let manifest = manifest_arc.read().await;
            manifest.list_files().unwrap_or_default()
        };

        let mut registered = Vec::with_capacity(files.len());
        for file in files {
            self.doc_projects.insert(file.id, project_name.to_string());
            if !self.doc_store.contains(&file.id) {
                match self.persistence.load_doc(project_name, &file.id).await {
                    Ok(data) => {
                        let _ = self.doc_store.replace_doc(file.id, &data).await;
                    }
                    Err(_) if create_missing_placeholders => {
                        let _ = self.doc_store.create_doc_with_id(file.id);
                    }
                    Err(_) => {}
                }
            }
            registered.push(file.id);
        }

        Ok(registered)
    }

    pub async fn apply_manifest_doc_to_project(
        &self,
        project_name: &str,
        manifest_doc_id: &DocId,
    ) -> Result<Vec<DocId>, CoreError> {
        self.apply_manifest_doc_to_project_internal(project_name, manifest_doc_id, false)
            .await
    }

    pub async fn apply_remote_manifest_doc_to_project(
        &self,
        project_name: &str,
        manifest_doc_id: &DocId,
    ) -> Result<Vec<DocId>, CoreError> {
        self.apply_manifest_doc_to_project_internal(project_name, manifest_doc_id, true)
            .await
    }

    fn resolve_access_from_owner_and_peers(
        owner: &str,
        peers: &[PeerInfo],
        local_peer_id: &str,
    ) -> (Option<PeerRole>, ProjectAccessState) {
        if owner.is_empty() {
            return (Some(PeerRole::Owner), ProjectAccessState::LocalOwner);
        }
        if owner == local_peer_id {
            return (Some(PeerRole::Owner), ProjectAccessState::Owner);
        }
        if let Some(peer) = peers.iter().find(|peer| peer.peer_id == local_peer_id) {
            let access = match peer.role {
                PeerRole::Owner => ProjectAccessState::Owner,
                PeerRole::Editor => ProjectAccessState::Editor,
                PeerRole::Viewer => ProjectAccessState::Viewer,
            };
            return (Some(peer.role), access);
        }
        (None, ProjectAccessState::IdentityMismatch)
    }

    pub fn build_project_peer_roster(
        owner: &str,
        owner_alias: Option<String>,
        peers: &[PeerInfo],
        local_peer_id: &str,
        live_state: &HashMap<String, (bool, Option<String>)>,
    ) -> Vec<ProjectPeerSummary> {
        let mut roster = HashMap::<String, ProjectPeerSummary>::new();

        if !owner.trim().is_empty() {
            let (connected, active_doc) = live_state.get(owner).cloned().unwrap_or((false, None));
            roster.insert(
                owner.to_string(),
                ProjectPeerSummary {
                    peer_id: owner.to_string(),
                    connected,
                    alias: Some(
                        owner_alias
                            .filter(|alias| !alias.trim().is_empty())
                            .unwrap_or_else(|| "peer".into()),
                    ),
                    role: PeerRole::Owner,
                    active_doc,
                    is_self: owner == local_peer_id,
                },
            );
        }

        for peer in peers {
            let (connected, active_doc) = live_state
                .get(&peer.peer_id)
                .cloned()
                .unwrap_or((false, None));

            let normalized = ProjectPeerSummary {
                peer_id: peer.peer_id.clone(),
                connected,
                alias: Some(if peer.alias.trim().is_empty() {
                    "peer".into()
                } else {
                    peer.alias.clone()
                }),
                role: if peer.peer_id == owner {
                    PeerRole::Owner
                } else {
                    peer.role
                },
                active_doc,
                is_self: peer.peer_id == local_peer_id,
            };

            roster
                .entry(peer.peer_id.clone())
                .and_modify(|existing| {
                    if existing.role != PeerRole::Owner {
                        *existing = normalized.clone();
                    }
                })
                .or_insert(normalized);
        }

        let mut roster = roster.into_values().collect::<Vec<_>>();
        roster.sort_by(
            |a, b| match (a.role == PeerRole::Owner, b.role == PeerRole::Owner) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a
                    .alias
                    .as_deref()
                    .unwrap_or("peer")
                    .cmp(b.alias.as_deref().unwrap_or("peer"))
                    .then_with(|| a.peer_id.cmp(&b.peer_id)),
            },
        );
        roster
    }

    pub fn new(base_dir: PathBuf) -> Self {
        Self {
            persistence: Arc::new(Persistence::new(base_dir)),
            doc_store: Arc::new(DocStore::new()),
            manifests: DashMap::new(),
            epoch_keys: DashMap::new(),
            epoch_key_presence: DashMap::new(),
            x25519_identities: DashMap::new(),
            x25519_identity_presence: DashMap::new(),
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
            epoch_key_presence: DashMap::new(),
            x25519_identities: DashMap::new(),
            x25519_identity_presence: DashMap::new(),
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
            epoch_key_presence: DashMap::new(),
            x25519_identities: DashMap::new(),
            x25519_identity_presence: DashMap::new(),
            search_index: Some(search_index),
            project_search: DashMap::new(),
            save_tasks: Mutex::new(JoinSet::new()),
            save_tokens: DashMap::new(),
            doc_projects: DashMap::new(),
        }
    }

    /// Get the project name for a document (if known).
    pub fn get_project_for_doc(&self, doc_id: &DocId) -> Option<String> {
        self.doc_projects.get(doc_id).map(|e| e.value().clone())
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

    pub fn has_cached_project_x25519_identity(&self, project_name: &str) -> bool {
        self.x25519_identities.contains_key(project_name)
    }

    pub fn has_cached_epoch_keys(&self, project_name: &str) -> bool {
        self.epoch_keys.contains_key(project_name)
    }

    async fn project_x25519_key_name(&self, project_name: &str) -> Result<String, CoreError> {
        let project_id = if let Some(entry) = self.manifests.get(project_name) {
            let manifest_arc = Arc::clone(entry.value());
            drop(entry);
            let manifest = manifest_arc.read().await;
            manifest.project_id()?
        } else {
            let data = self.persistence.load_manifest(project_name).await?;
            ProjectManifest::load(&data)?.project_id()?
        };
        Ok(format!("x25519-identity-{project_id}"))
    }

    async fn project_epoch_key_name(&self, project_name: &str) -> Result<String, CoreError> {
        let project_id = if let Some(entry) = self.manifests.get(project_name) {
            let manifest_arc = Arc::clone(entry.value());
            drop(entry);
            let manifest = manifest_arc.read().await;
            manifest.project_id()?
        } else {
            let data = self.persistence.load_manifest(project_name).await?;
            ProjectManifest::load(&data)?.project_id()?
        };
        Ok(format!("epoch-keys-{project_id}"))
    }

    async fn project_uses_shared_crypto(&self, project_name: &str) -> Result<bool, CoreError> {
        let manifest_arc = self.get_manifest(project_name)?;
        let manifest = manifest_arc.read().await;
        Ok(!manifest.get_owner().unwrap_or_default().is_empty()
            || !manifest.list_peers()?.is_empty())
    }

    async fn require_epoch_keys_if_shared(&self, project_name: &str) -> Result<(), CoreError> {
        if self.project_uses_shared_crypto(project_name).await?
            && !self.epoch_keys.contains_key(project_name)
        {
            return Err(CoreError::InvalidData(format!(
                "shared project '{project_name}' is locked because epoch keys are unavailable"
            )));
        }
        Ok(())
    }

    async fn project_epoch_material(
        &self,
        project_name: &str,
    ) -> Result<Option<([u8; 32], u32)>, CoreError> {
        let shared = self.project_uses_shared_crypto(project_name).await?;
        let Some(epoch_mgr) = self.epoch_keys.get(project_name) else {
            if shared {
                return Err(CoreError::InvalidData(format!(
                    "shared project '{project_name}' is locked because epoch keys are unavailable"
                )));
            }
            return Ok(None);
        };

        let mgr = epoch_mgr.read().await;
        let key = mgr.current_key().map_err(|_| {
            CoreError::InvalidData(format!(
                "shared project '{project_name}' has unavailable epoch keys"
            ))
        })?;
        Ok(Some((*key.as_bytes(), mgr.current_epoch())))
    }

    async fn migrate_legacy_epoch_keys_after_rename(
        &self,
        old_name: &str,
        new_name: &str,
    ) -> Result<(), CoreError> {
        let old_dir = self
            .persistence
            .base_dir()
            .join(new_name)
            .join(".p2p")
            .join("keys");
        let keystore = notes_crypto::KeyStore::new(old_dir);
        let new_key_name = self.project_epoch_key_name(new_name).await?;
        if keystore.has_key(&new_key_name) {
            return Ok(());
        }
        let legacy_old_name = format!("epoch-keys-{old_name}");
        if let Ok(data) = keystore.load_key(&legacy_old_name) {
            keystore
                .store_key(&new_key_name, &data)
                .map_err(|e| CoreError::InvalidData(format!("epoch key store failed: {e}")))?;
            let _ = keystore.delete_key(&legacy_old_name);
        }
        Ok(())
    }

    async fn delete_persisted_project_secrets(&self, project_name: &str) -> Result<(), CoreError> {
        let keys_dir = self
            .persistence
            .base_dir()
            .join(project_name)
            .join(".p2p")
            .join("keys");
        let keystore = notes_crypto::KeyStore::new(keys_dir);
        let epoch_key_name = self.project_epoch_key_name(project_name).await?;
        let x25519_key_name = self.project_x25519_key_name(project_name).await?;
        let legacy_epoch_key_name = format!("epoch-keys-{project_name}");
        let _ = keystore.delete_key(&epoch_key_name);
        let _ = keystore.delete_key(&legacy_epoch_key_name);
        let _ = keystore.delete_key(&x25519_key_name);
        let _ = keystore.delete_key("owner-x25519-public");
        Ok(())
    }

    pub async fn get_or_create_project_x25519_identity(
        &self,
        project_name: &str,
    ) -> Result<Arc<ProjectX25519Identity>, CoreError> {
        if let Some(entry) = self.x25519_identities.get(project_name) {
            notes_crypto::debug_note_secret_cache_hit();
            return Ok(Arc::clone(entry.value()));
        }

        if !self
            .x25519_identity_presence
            .get(project_name)
            .map(|entry| *entry.value())
            .unwrap_or(true)
        {
            return self.create_project_x25519_identity(project_name).await;
        }

        if self
            .load_project_x25519_identity_if_present(project_name)
            .await?
            .is_some()
        {
            if let Some(entry) = self.x25519_identities.get(project_name) {
                return Ok(Arc::clone(entry.value()));
            }
        }

        self.create_project_x25519_identity(project_name).await
    }

    async fn create_project_x25519_identity(
        &self,
        project_name: &str,
    ) -> Result<Arc<ProjectX25519Identity>, CoreError> {
        let keys_dir = self
            .persistence
            .base_dir()
            .join(project_name)
            .join(".p2p")
            .join("keys");
        tokio::fs::create_dir_all(&keys_dir).await?;
        let keystore = notes_crypto::KeyStore::new(keys_dir);
        let key_name = self.project_x25519_key_name(project_name).await?;
        let (secret, public) = keystore
            .get_or_create_x25519(&key_name)
            .map_err(|e| CoreError::InvalidData(format!("X25519 key generation failed: {e}")))?;

        let identity = Arc::new(ProjectX25519Identity {
            secret: Zeroizing::new(secret.to_bytes()),
            public: public.to_bytes(),
        });
        self.x25519_identity_presence
            .insert(project_name.to_string(), true);
        self.x25519_identities
            .insert(project_name.to_string(), Arc::clone(&identity));
        Ok(identity)
    }

    pub async fn load_project_x25519_identity_if_present(
        &self,
        project_name: &str,
    ) -> Result<Option<Arc<ProjectX25519Identity>>, CoreError> {
        if let Some(entry) = self.x25519_identities.get(project_name) {
            notes_crypto::debug_note_secret_cache_hit();
            return Ok(Some(Arc::clone(entry.value())));
        }
        if let Some(entry) = self.x25519_identity_presence.get(project_name) {
            if !*entry.value() {
                notes_crypto::debug_note_secret_cache_hit();
                return Ok(None);
            }
        }
        notes_crypto::debug_note_secret_cache_miss();

        let keys_dir = self
            .persistence
            .base_dir()
            .join(project_name)
            .join(".p2p")
            .join("keys");
        let keystore = notes_crypto::KeyStore::new(keys_dir);
        let key_name = self.project_x25519_key_name(project_name).await?;
        let secret = match keystore.load_x25519_secret(&key_name) {
            Ok(secret) => secret,
            Err(notes_crypto::CryptoError::KeyNotFound(_)) => {
                self.x25519_identity_presence
                    .insert(project_name.to_string(), false);
                return Ok(None);
            }
            Err(err) => {
                return Err(CoreError::InvalidData(format!(
                    "X25519 key load failed: {err}"
                )))
            }
        };
        let public = x25519_dalek::PublicKey::from(&secret);

        let identity = Arc::new(ProjectX25519Identity {
            secret: Zeroizing::new(secret.to_bytes()),
            public: public.to_bytes(),
        });
        self.x25519_identity_presence
            .insert(project_name.to_string(), true);
        self.x25519_identities
            .insert(project_name.to_string(), Arc::clone(&identity));
        Ok(Some(identity))
    }

    pub async fn install_epoch_keys(
        &self,
        project_name: &str,
        mgr: notes_crypto::EpochKeyManager,
    ) -> Result<(), CoreError> {
        let data = mgr
            .serialize()
            .map_err(|e| CoreError::InvalidData(format!("epoch key serialize failed: {e}")))?;
        let keys_dir = self
            .persistence
            .base_dir()
            .join(project_name)
            .join(".p2p")
            .join("keys");
        tokio::fs::create_dir_all(&keys_dir).await?;
        let keystore = notes_crypto::KeyStore::new(keys_dir);
        let keychain_name = self.project_epoch_key_name(project_name).await?;
        keystore
            .store_key(&keychain_name, &data)
            .map_err(|e| CoreError::InvalidData(format!("epoch key store failed: {e}")))?;
        self.epoch_key_presence
            .insert(project_name.to_string(), true);
        self.epoch_keys
            .insert(project_name.to_string(), Arc::new(RwLock::new(mgr)));
        Ok(())
    }

    /// Initialize epoch keys for a project (called when sharing starts).
    pub async fn init_epoch_keys(&self, project_name: &str) -> Result<(), CoreError> {
        if self.epoch_keys.contains_key(project_name) {
            return Ok(()); // Already initialized
        }

        let mgr = notes_crypto::EpochKeyManager::new()
            .map_err(|e| CoreError::InvalidData(format!("epoch key init failed: {e}")))?;
        self.install_epoch_keys(project_name, mgr).await?;

        log::info!("Initialized epoch keys for project {project_name}");
        Ok(())
    }

    /// Load epoch keys for a project from disk (if they exist).
    pub async fn load_epoch_keys(&self, project_name: &str) -> Result<bool, CoreError> {
        if self.epoch_keys.contains_key(project_name) {
            notes_crypto::debug_note_secret_cache_hit();
            return Ok(true);
        }
        if let Some(entry) = self.epoch_key_presence.get(project_name) {
            if !*entry.value() {
                notes_crypto::debug_note_secret_cache_hit();
                return Ok(false);
            }
        }
        notes_crypto::debug_note_secret_cache_miss();

        let keys_dir = self
            .persistence
            .base_dir()
            .join(project_name)
            .join(".p2p")
            .join("keys");
        let keystore = notes_crypto::KeyStore::new(keys_dir.clone());
        let keychain_name = self.project_epoch_key_name(project_name).await?;
        let legacy_keychain_name = format!("epoch-keys-{project_name}");

        // Try loading from OS keychain (primary), fall back to legacy file
        let data = match keystore.load_key(&keychain_name) {
            Ok(data) => data,
            Err(_) => match keystore.load_key(&legacy_keychain_name) {
                Ok(data) => {
                    let _ = keystore.store_key(&keychain_name, &data);
                    let _ = keystore.delete_key(&legacy_keychain_name);
                    data
                }
                Err(_) => {
                    // Fall back to legacy plaintext file for migration
                    let legacy_path = keys_dir.join("epochs.json");
                    match tokio::fs::read(&legacy_path).await {
                        Ok(data) => {
                            notes_crypto::debug_record_secret_read(
                                &keychain_name,
                                notes_crypto::SecretReadBackend::LegacyFile,
                                notes_crypto::SecretReadOutcome::Hit,
                            );
                            // Migrate: store in keychain and delete plaintext file
                            if keystore.store_key(&keychain_name, &data).is_ok() {
                                let _ = tokio::fs::remove_file(&legacy_path).await;
                                log::info!(
                                    "Migrated epoch keys from file to keychain for {project_name}"
                                );
                            }
                            data
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                            self.epoch_key_presence
                                .insert(project_name.to_string(), false);
                            return Ok(false);
                        }
                        Err(e) => return Err(CoreError::Io(e)),
                    }
                }
            },
        };

        match notes_crypto::EpochKeyManager::deserialize(&data) {
            Ok(mgr) => {
                self.epoch_key_presence
                    .insert(project_name.to_string(), true);
                self.epoch_keys
                    .insert(project_name.to_string(), Arc::new(RwLock::new(mgr)));
                log::info!("Loaded epoch keys for project {project_name}");
                Ok(true)
            }
            Err(e) => Err(CoreError::InvalidData(format!(
                "epoch key load failed: {e}"
            ))),
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
            let epoch = mgr
                .ratchet()
                .map_err(|e| CoreError::InvalidData(format!("ratchet failed: {e}")))?;

            // Persist to OS keychain
            let data = mgr
                .serialize()
                .map_err(|e| CoreError::InvalidData(format!("serialize failed: {e}")))?;
            let keys_dir = self
                .persistence
                .base_dir()
                .join(project_name)
                .join(".p2p")
                .join("keys");
            let keystore = notes_crypto::KeyStore::new(keys_dir);
            let keychain_name = self.project_epoch_key_name(project_name).await?;
            keystore
                .store_key(&keychain_name, &data)
                .map_err(|e| CoreError::InvalidData(format!("epoch key store failed: {e}")))?;
            epoch
        };

        log::info!("Ratcheted epoch keys for {project_name} to epoch {new_epoch}");
        Ok(new_epoch)
    }

    pub async fn set_wrapped_epoch_key_for_peer(
        &self,
        project_name: &str,
        peer_id: &str,
        wrapped_key: &str,
    ) -> Result<(), CoreError> {
        let manifest_arc = self.get_manifest(project_name)?;
        let mut manifest = manifest_arc.write().await;
        manifest.set_wrapped_epoch_key(peer_id, wrapped_key)?;
        let data = manifest.save();
        drop(manifest);
        self.persistence.save_manifest(project_name, &data).await?;
        let _ = self
            .sync_manifest_doc_from_bytes(project_name, &data)
            .await?;
        Ok(())
    }

    pub async fn remove_wrapped_epoch_key_for_peer(
        &self,
        project_name: &str,
        peer_id: &str,
    ) -> Result<(), CoreError> {
        let manifest_arc = self.get_manifest(project_name)?;
        let mut manifest = manifest_arc.write().await;
        manifest.remove_wrapped_epoch_key(peer_id)?;
        let data = manifest.save();
        drop(manifest);
        self.persistence.save_manifest(project_name, &data).await?;
        let _ = self
            .sync_manifest_doc_from_bytes(project_name, &data)
            .await?;
        Ok(())
    }

    pub async fn wrapped_epoch_keys(
        &self,
        project_name: &str,
    ) -> Result<std::collections::BTreeMap<String, String>, CoreError> {
        let manifest_arc = self.get_manifest(project_name)?;
        let manifest = manifest_arc.read().await;
        manifest.list_wrapped_epoch_keys()
    }

    /// Get a reference to the Persistence layer.
    pub fn persistence(&self) -> &Persistence {
        &self.persistence
    }

    pub async fn preload_all_project_secrets(&self) -> Result<(usize, usize), CoreError> {
        let mut loaded_epoch_keys = 0usize;
        let mut loaded_x25519 = 0usize;

        for project_name in self.list_projects().await? {
            match self.load_epoch_keys(&project_name).await {
                Ok(true) => loaded_epoch_keys += 1,
                Ok(false) => {}
                Err(err) => {
                    log::warn!("Skipping epoch-key preload for project {project_name}: {err}");
                }
            }
            match self
                .load_project_x25519_identity_if_present(&project_name)
                .await
            {
                Ok(Some(_)) => loaded_x25519 += 1,
                Ok(None) => {}
                Err(err) => {
                    log::warn!("Skipping X25519 preload for project {project_name}: {err}");
                }
            }
        }

        Ok((loaded_epoch_keys, loaded_x25519))
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

    pub async fn get_project_id(&self, project_name: &str) -> Result<String, CoreError> {
        let manifest_arc = self.get_manifest(project_name)?;
        let manifest = manifest_arc.read().await;
        manifest.project_id()
    }

    pub async fn get_project_owner_alias(
        &self,
        project_name: &str,
    ) -> Result<Option<String>, CoreError> {
        let manifest_arc = self.get_manifest(project_name)?;
        let manifest = manifest_arc.read().await;
        manifest.get_owner_alias()
    }

    /// Get the peers list from the manifest.
    pub async fn get_project_peers(&self, project_name: &str) -> Result<Vec<PeerInfo>, CoreError> {
        let manifest_arc = self.get_manifest(project_name)?;
        let manifest = manifest_arc.read().await;
        manifest.list_peers()
    }

    pub async fn get_project_peer_roster(
        &self,
        project_name: &str,
        local_peer_id: &str,
    ) -> Result<Vec<ProjectPeerSummary>, CoreError> {
        let manifest_arc = self.get_manifest(project_name)?;
        let manifest = manifest_arc.read().await;
        let peers = manifest.list_peers()?;
        let owner = manifest.get_owner().unwrap_or_default();
        let owner_alias = manifest.get_owner_alias()?;
        Ok(Self::build_project_peer_roster(
            &owner,
            owner_alias,
            &peers,
            local_peer_id,
            &HashMap::new(),
        ))
    }

    /// Open per-project search and history databases.
    /// If the project has epoch keys, derives a SQLCipher encryption key.
    /// Otherwise, opens unencrypted (local-only projects).
    pub async fn open_project_databases(&self, project_name: &str) -> Result<(), CoreError> {
        // Skip if already opened
        if self.project_search.contains_key(project_name) {
            return Ok(());
        }

        self.require_epoch_keys_if_shared(project_name).await?;

        let project_dir = self.persistence.project_dir(project_name);
        let p2p_dir = project_dir.join(".p2p");
        tokio::fs::create_dir_all(&p2p_dir).await.ok();

        // Derive encryption key from epoch key if available
        let db_key: Option<[u8; 32]> =
            if let Some((project_key, _epoch)) = self.project_epoch_material(project_name).await? {
                use hkdf::Hkdf;
                use sha2::Sha256;
                let hk = Hkdf::<Sha256>::new(None, &project_key);
                let mut db_key = [0u8; 32];
                let info = format!("p2p-notes/v1/sqlite-encryption/{project_name}");
                hk.expand(info.as_bytes(), &mut db_key)
                    .map_err(|_| CoreError::InvalidData("HKDF expand failed".into()))?;
                Some(db_key)
            } else {
                None
            };

        let key_ref = db_key.as_ref();

        // Open per-project search index
        let search_path = p2p_dir.join("search.db");
        let search = SearchIndex::open_with_recovery(&search_path, key_ref)?;
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
        let _ = self.sync_manifest_doc_from_bytes(name, &data).await?;

        log::info!("Created project: {name}");
        Ok(())
    }

    /// Reload a project's manifest from disk, replacing the in-memory cache.
    /// Use this after externally writing a new manifest (e.g. after accepting an invite).
    pub async fn reload_manifest(&self, name: &str) -> Result<(), CoreError> {
        let data = self.persistence.load_manifest(name).await?;
        let manifest = ProjectManifest::load(&data)?;
        self.manifests
            .insert(name.to_string(), Arc::new(RwLock::new(manifest)));
        let _ = self.sync_manifest_doc_from_bytes(name, &data).await?;
        log::info!("Reloaded manifest for project: {name}");
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
        let _ = self.sync_manifest_doc_from_bytes(name, &data).await?;

        self.open_project_databases(name).await?;

        log::info!("Opened project: {name}");
        Ok(())
    }

    /// List all projects (directories in the base folder).
    pub async fn list_projects(&self) -> Result<Vec<String>, CoreError> {
        self.persistence.list_projects().await
    }

    /// Describe all projects for frontend bootstrapping.
    pub async fn resolve_local_access(
        &self,
        project_name: &str,
        local_peer_id: &str,
    ) -> Result<(Option<PeerRole>, ProjectAccessState), CoreError> {
        let owner = self.get_project_owner(project_name).await?;
        let peers = self.get_project_peers(project_name).await?;
        Ok(Self::resolve_access_from_owner_and_peers(
            &owner,
            &peers,
            local_peer_id,
        ))
    }

    pub async fn list_project_summaries(
        &self,
        local_peer_id: &str,
    ) -> Result<Vec<ProjectSummary>, CoreError> {
        let mut summaries = Vec::new();

        for name in self.list_projects().await? {
            let data = self.persistence.load_manifest(&name).await?;
            let manifest = ProjectManifest::load(&data)?;
            let peers = manifest.list_peers()?;
            let owner = manifest.get_owner().unwrap_or_default();
            let owner_alias = manifest.get_owner_alias()?;
            let shared = !owner.is_empty() || !peers.is_empty();
            let (role, access_state) =
                Self::resolve_access_from_owner_and_peers(&owner, &peers, local_peer_id);
            let roster = Self::build_project_peer_roster(
                &owner,
                owner_alias,
                &peers,
                local_peer_id,
                &HashMap::new(),
            );
            let visible_peer_count = roster.iter().filter(|peer| !peer.is_self).count();

            let file_count = manifest.list_files().map(|f| f.len()).unwrap_or(0);

            summaries.push(ProjectSummary {
                name: name.clone(),
                path: self.persistence.project_dir(&name).display().to_string(),
                shared,
                role,
                access_state,
                can_edit: matches!(
                    access_state,
                    ProjectAccessState::LocalOwner
                        | ProjectAccessState::Owner
                        | ProjectAccessState::Editor
                ),
                can_manage_peers: matches!(
                    access_state,
                    ProjectAccessState::LocalOwner | ProjectAccessState::Owner
                ),
                peer_count: visible_peer_count,
                file_count,
            });
        }

        summaries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(summaries)
    }

    // ── Project management ───────────────────────────────────────────

    /// Rename a project: renames the directory, updates the manifest, and re-keys all caches.
    pub async fn rename_project(&self, old_name: &str, new_name: &str) -> Result<(), CoreError> {
        validation::validate_project_name(old_name)?;
        validation::validate_project_name(new_name)?;

        if old_name == new_name {
            return Ok(());
        }

        let old_dir = self.persistence.project_dir(old_name);
        let new_dir = self.persistence.project_dir(new_name);

        if !old_dir.exists() {
            return Err(CoreError::ProjectNotFound(old_name.to_string()));
        }
        if new_dir.exists() {
            return Err(CoreError::ProjectAlreadyExists(new_name.to_string()));
        }

        // Close all open docs for this project first
        let doc_ids: Vec<DocId> = self
            .doc_projects
            .iter()
            .filter(|e| e.value() == old_name)
            .map(|e| *e.key())
            .collect();

        for doc_id in &doc_ids {
            self.close_doc(old_name, doc_id).await?;
        }

        // Rename the filesystem directory
        tokio::fs::rename(&old_dir, &new_dir).await?;

        // Update the manifest name
        if let Some(manifest_entry) = self.manifests.remove(old_name) {
            let (_, manifest_arc) = manifest_entry;
            {
                let mut manifest = manifest_arc.write().await;
                let _ = manifest.set_name(new_name);
                let data = manifest.save();
                self.persistence.save_manifest(new_name, &data).await?;
            }
            self.manifests.insert(new_name.to_string(), manifest_arc);
        }

        self.migrate_legacy_epoch_keys_after_rename(old_name, new_name)
            .await?;

        // Re-key epoch keys
        if let Some(entry) = self.epoch_keys.remove(old_name) {
            self.epoch_keys.insert(new_name.to_string(), entry.1);
        }
        if let Some(entry) = self.epoch_key_presence.remove(old_name) {
            self.epoch_key_presence
                .insert(new_name.to_string(), entry.1);
        }

        if let Some(entry) = self.x25519_identities.remove(old_name) {
            self.x25519_identities.insert(new_name.to_string(), entry.1);
        }
        if let Some(entry) = self.x25519_identity_presence.remove(old_name) {
            self.x25519_identity_presence
                .insert(new_name.to_string(), entry.1);
        }

        // Re-key project search indexes
        if let Some(entry) = self.project_search.remove(old_name) {
            self.project_search.insert(new_name.to_string(), entry.1);
        }

        // Update doc_projects mappings
        for doc_id in &doc_ids {
            self.doc_projects.insert(*doc_id, new_name.to_string());
        }

        // Update search index: collect data first (async), then lock and index (sync)
        if let Some(ref search) = self.search_index {
            let mut file_contents: Vec<(DocId, String, String)> = Vec::new();
            if let Ok(manifest_arc) = self.get_manifest(new_name) {
                let manifest = manifest_arc.read().await;
                if let Ok(files) = manifest.list_files() {
                    for file in files {
                        let md_path = new_dir.join(&file.path);
                        if let Ok(content) = tokio::fs::read_to_string(&md_path).await {
                            file_contents.push((file.id, file.path.clone(), content));
                        }
                    }
                }
            }
            // Now lock and index without holding across await
            if let Ok(index) = search.lock() {
                for (doc_id, path, content) in &file_contents {
                    let _ = index.index_document(doc_id, new_name, path, content);
                }
            }
        }

        log::info!("Renamed project: {old_name} -> {new_name}");
        Ok(())
    }

    /// Delete a project: removes all docs, the directory, and all associated state.
    pub async fn delete_project(&self, name: &str) -> Result<(), CoreError> {
        validation::validate_project_name(name)?;

        let project_dir = self.persistence.project_dir(name);
        if !project_dir.exists() {
            return Err(CoreError::ProjectNotFound(name.to_string()));
        }

        // Close all open docs
        let doc_ids: Vec<DocId> = self
            .doc_projects
            .iter()
            .filter(|e| e.value() == name)
            .map(|e| *e.key())
            .collect();

        for doc_id in &doc_ids {
            // Cancel save loops and remove from memory
            if let Some((_, token)) = self.save_tokens.remove(doc_id) {
                token.cancel();
            }
            self.doc_store.remove_doc(doc_id);
            self.doc_projects.remove(doc_id);
        }

        // Remove from manifests cache
        self.delete_persisted_project_secrets(name).await?;
        self.manifests.remove(name);

        // Remove epoch keys
        self.epoch_keys.remove(name);
        self.epoch_key_presence.remove(name);

        // Remove cached x25519 identity
        self.x25519_identities.remove(name);
        self.x25519_identity_presence.remove(name);

        // Remove project search index
        self.project_search.remove(name);

        // Remove from global search index
        if let Some(ref search) = self.search_index {
            if let Ok(index) = search.lock() {
                for doc_id in &doc_ids {
                    let _ = index.remove_document(doc_id);
                }
            }
        }

        // Delete the entire project directory tree
        if project_dir.exists() {
            tokio::fs::remove_dir_all(&project_dir).await?;
        }

        log::info!("Deleted project: {name}");
        Ok(())
    }

    /// Get a tree view of files in a project, grouped by subfolder.
    pub async fn list_project_tree(
        &self,
        project_name: &str,
    ) -> Result<std::collections::BTreeMap<String, Vec<DocInfo>>, CoreError> {
        let files = self.list_files(project_name).await?;
        let mut tree: std::collections::BTreeMap<String, Vec<DocInfo>> =
            std::collections::BTreeMap::new();

        for file in files {
            let folder = if let Some(pos) = file.path.rfind('/') {
                file.path[..pos].to_string()
            } else {
                String::new() // Root level
            };
            tree.entry(folder).or_default().push(file);
        }

        Ok(tree)
    }

    // ── Document operations ──────────────────────────────────────────

    /// Create a new note in a project. Returns the doc ID.
    pub async fn create_note(&self, project_name: &str, path: &str) -> Result<DocId, CoreError> {
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
        let save_result =
            if let Some((project_key, epoch)) = self.project_epoch_material(project_name).await? {
                self.persistence
                    .save_doc_encrypted(project_name, &doc_id, &doc_data, &project_key, epoch)
                    .await
            } else {
                self.persistence
                    .save_doc(project_name, &doc_id, &doc_data)
                    .await
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

        let _ = self
            .sync_manifest_doc_from_bytes(project_name, &manifest_data)
            .await?;

        // Export empty markdown file (best-effort)
        if let Err(e) = self
            .persistence
            .export_markdown(project_name, path, "")
            .await
        {
            log::warn!("Markdown export failed for new note {doc_id}: {e}");
        }

        // Track doc -> project mapping
        self.doc_projects.insert(doc_id, project_name.to_string());

        // Start background save loop
        self.start_save_loop(project_name.to_string(), doc_id, Duration::from_secs(5))
            .await;

        log::info!("Created note {doc_id} at {path} in {project_name}");
        Ok(doc_id)
    }

    /// Open a document: load from disk into DocStore if not already loaded.
    pub async fn open_doc(&self, project_name: &str, doc_id: &DocId) -> Result<(), CoreError> {
        validation::validate_project_name(project_name)?;

        if self.doc_store.contains(doc_id) {
            return Ok(());
        }

        let note_path = {
            let manifest_arc = self.get_manifest(project_name)?;
            let manifest = manifest_arc.read().await;
            manifest.get_file_path(doc_id).ok()
        };

        let load_result =
            if let Some((project_key, _epoch)) = self.project_epoch_material(project_name).await? {
                self.persistence
                    .load_doc_encrypted(project_name, doc_id, &project_key)
                    .await
            } else {
                self.persistence.load_doc(project_name, doc_id).await
            };
        let data = match load_result {
            Ok(data) => data,
            Err(CoreError::InvalidData(message))
                if message.contains("Primary and backup both corrupted") =>
            {
                if let Some(path) = note_path.as_deref() {
                    if self
                        .persistence
                        .markdown_export_exists(project_name, path)
                        .await
                        .unwrap_or(false)
                    {
                        return Err(CoreError::RecoverableDocCorruption {
                            doc_id: *doc_id,
                            note_path: path.to_string(),
                            suggested_path: path.to_string(),
                        });
                    }
                }
                return Err(CoreError::InvalidData(message));
            }
            Err(error) => return Err(error),
        };
        self.doc_store.load_doc(*doc_id, &data)?;

        // Track doc -> project mapping
        self.doc_projects.insert(*doc_id, project_name.to_string());

        // Start background save loop
        self.start_save_loop(project_name.to_string(), *doc_id, Duration::from_secs(5))
            .await;

        log::info!("Loaded document {doc_id} from {project_name}");
        Ok(())
    }

    pub async fn recover_note_from_markdown(
        &self,
        project_name: &str,
        doc_id: &DocId,
    ) -> Result<DocInfo, CoreError> {
        validation::validate_project_name(project_name)?;
        let note_path = {
            let manifest_arc = self.get_manifest(project_name)?;
            let manifest = manifest_arc.read().await;
            manifest.get_file_path(doc_id)?
        };
        let markdown = self
            .persistence
            .read_markdown_export(project_name, &note_path)
            .await?;
        self.persistence
            .quarantine_broken_doc_artifacts(project_name, doc_id)
            .await?;
        self.doc_store.remove_doc(doc_id);
        self.doc_projects.remove(doc_id);
        self.doc_store.create_doc_with_id(*doc_id)?;
        self.doc_store.replace_text(doc_id, &markdown).await?;
        self.save_doc(project_name, doc_id).await?;
        self.doc_projects.insert(*doc_id, project_name.to_string());
        self.start_save_loop(project_name.to_string(), *doc_id, Duration::from_secs(5))
            .await;

        Ok(DocInfo {
            id: *doc_id,
            path: note_path,
            file_type: FileType::Note,
            created: chrono::Utc::now(),
        })
    }

    /// Close a document: cancel save loop, save to disk, remove from memory.
    pub async fn close_doc(&self, project_name: &str, doc_id: &DocId) -> Result<(), CoreError> {
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
    pub async fn save_doc(&self, project_name: &str, doc_id: &DocId) -> Result<(), CoreError> {
        // 1. Read manifest first (consistent lock ordering: manifest -> doc)
        let path = {
            let manifest_arc = self.get_manifest(project_name)?;
            let manifest = manifest_arc.read().await;
            manifest.get_file_path(doc_id).ok()
        }; // manifest lock dropped

        // 2. Save Automerge binary (encrypted if epoch keys available)
        let data = self.doc_store.save_doc(doc_id).await?;
        if let Some((project_key, epoch)) = self.project_epoch_material(project_name).await? {
            self.persistence
                .save_doc_encrypted(project_name, doc_id, &data, &project_key, epoch)
                .await?;
        } else {
            self.persistence
                .save_doc(project_name, doc_id, &data)
                .await?;
        }

        // 3. Export markdown + update search index (no locks held)
        if let Some(ref path) = path {
            let text = self
                .doc_store
                .get_visible_text(doc_id)
                .await
                .unwrap_or_default();
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
    pub async fn delete_note(&self, project_name: &str, doc_id: &DocId) -> Result<(), CoreError> {
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
        let _ = self
            .sync_manifest_doc_from_bytes(project_name, &manifest_data)
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
        let _ = self
            .sync_manifest_doc_from_bytes(project_name, &manifest_data)
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
            let text = self
                .doc_store
                .get_visible_text(doc_id)
                .await
                .unwrap_or_default();
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
    pub async fn list_files(&self, project_name: &str) -> Result<Vec<DocInfo>, CoreError> {
        let manifest_arc = self.get_manifest(project_name)?;
        let manifest = manifest_arc.read().await;
        manifest.list_files()
    }

    /// Get the text content of a loaded document.
    pub async fn get_doc_text(&self, doc_id: &DocId) -> Result<String, CoreError> {
        self.doc_store.get_visible_text(doc_id).await
    }

    pub async fn doc_snapshot_exists(
        &self,
        project_name: &str,
        doc_id: &DocId,
    ) -> Result<bool, CoreError> {
        validation::validate_project_name(project_name)?;
        self.persistence.doc_exists(project_name, doc_id).await
    }

    /// Pure read snapshot for a document without mutating in-memory doc session state.
    pub async fn get_doc_read_snapshot(
        &self,
        project_name: &str,
        doc_id: &DocId,
    ) -> Result<DocReadSnapshot, CoreError> {
        validation::validate_project_name(project_name)?;
        if self.doc_store.contains(doc_id) {
            return self.doc_store.get_read_snapshot(doc_id).await;
        }

        let data =
            if let Some((project_key, _epoch)) = self.project_epoch_material(project_name).await? {
                self.persistence
                    .load_doc_encrypted(project_name, doc_id, &project_key)
                    .await?
            } else {
                self.persistence.load_doc(project_name, doc_id).await?
            };

        crate::doc_store::read_snapshot_from_bytes(&data)
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
    pub async fn compact_doc(&self, project_name: &str, doc_id: &DocId) -> Result<(), CoreError> {
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
                let md_path = self.persistence.base_dir().join(project).join(&file.path);
                let content = match self.get_doc_read_snapshot(project, &file.id).await {
                    Ok(snapshot) => snapshot.visible_text,
                    Err(_) => tokio::fs::read_to_string(&md_path)
                        .await
                        .unwrap_or_default(),
                };
                if let Ok(index) = search.lock() {
                    if let Err(e) = index.index_document(&file.id, project, &file.path, &content) {
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
    async fn start_save_loop(&self, project_name: String, doc_id: DocId, interval: Duration) {
        // Cancel any existing loop for this doc
        if let Some((_, old_token)) = self.save_tokens.remove(&doc_id) {
            old_token.cancel();
        }

        let token = CancellationToken::new();
        self.save_tokens.insert(doc_id, token.clone());

        let doc_store = Arc::clone(&self.doc_store);
        let persistence = Arc::clone(&self.persistence);
        let manifests_ref = &self.manifests;
        let shared_requires_crypto = self
            .project_uses_shared_crypto(&project_name)
            .await
            .unwrap_or(false);

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
                                let key = mgr.current_key().map_err(|_| {
                                    CoreError::InvalidData(format!(
                                        "shared project '{project_name}' has unavailable epoch keys"
                                    ))
                                })?;
                                persistence
                                    .save_doc_encrypted(
                                        &project_name,
                                        &doc_id,
                                        &data,
                                        key.as_bytes(),
                                        mgr.current_epoch(),
                                    )
                                    .await?;
                            } else if shared_requires_crypto {
                                return Err(CoreError::InvalidData(format!(
                                    "shared project '{project_name}' is locked because epoch keys are unavailable"
                                )));
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
                                    let text = doc_store
                                        .get_visible_text(&doc_id)
                                        .await
                                        .unwrap_or_default();
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

    fn get_manifest(&self, project_name: &str) -> Result<Arc<RwLock<ProjectManifest>>, CoreError> {
        self.manifests
            .get(project_name)
            .map(|entry| Arc::clone(entry.value()))
            .ok_or_else(|| CoreError::ProjectNotFound(project_name.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use automerge::transaction::Transactable;

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
    async fn test_open_legacy_doc_materializes_v2_read_model() {
        let dir = tempfile::tempdir().unwrap();
        let pm = ProjectManager::new(dir.path().to_path_buf());

        pm.create_project("test").await.unwrap();
        let id = pm.create_note("test", "legacy.md").await.unwrap();
        pm.close_doc("test", &id).await.unwrap();

        let mut legacy = automerge::AutoCommit::new();
        legacy.put(automerge::ROOT, "schemaVersion", 1_u64).unwrap();
        let text = legacy
            .put_object(automerge::ROOT, "text", automerge::ObjType::Text)
            .unwrap();
        legacy.splice_text(&text, 0, 0, "legacy body").unwrap();
        pm.persistence
            .save_doc("test", &id, &legacy.save())
            .await
            .unwrap();

        pm.open_doc("test", &id).await.unwrap();

        let editor_doc = pm.doc_store.get_editor_document(&id).await.unwrap();
        assert_eq!(editor_doc.schema_version, 2);
        assert_eq!(
            pm.doc_store.get_visible_text(&id).await.unwrap(),
            "legacy body"
        );
        assert_eq!(pm.get_doc_text(&id).await.unwrap(), "legacy body");
    }

    #[tokio::test]
    async fn test_get_doc_read_snapshot_is_pure_read_for_closed_legacy_doc() {
        let dir = tempfile::tempdir().unwrap();
        let pm = ProjectManager::new(dir.path().to_path_buf());

        pm.create_project("test").await.unwrap();
        let id = pm.create_note("test", "pure-read.md").await.unwrap();
        pm.close_doc("test", &id).await.unwrap();

        let mut legacy = automerge::AutoCommit::new();
        legacy.put(automerge::ROOT, "schemaVersion", 1_u64).unwrap();
        let text = legacy
            .put_object(automerge::ROOT, "text", automerge::ObjType::Text)
            .unwrap();
        legacy.splice_text(&text, 0, 0, "legacy snapshot").unwrap();
        pm.persistence
            .save_doc("test", &id, &legacy.save())
            .await
            .unwrap();

        let snapshot = pm.get_doc_read_snapshot("test", &id).await.unwrap();

        assert_eq!(snapshot.visible_text, "legacy snapshot");
        assert!(snapshot.needs_migration);
        assert!(!pm.doc_store.contains(&id));
    }

    #[tokio::test]
    async fn test_get_doc_read_snapshot_supports_encrypted_closed_docs() {
        let dir = tempfile::tempdir().unwrap();
        let pm = ProjectManager::new(dir.path().to_path_buf());

        pm.create_project("test").await.unwrap();
        pm.init_epoch_keys("test").await.unwrap();
        let id = pm.create_note("test", "encrypted.md").await.unwrap();
        pm.doc_store.replace_text(&id, "secret text").await.unwrap();
        pm.save_doc("test", &id).await.unwrap();
        pm.close_doc("test", &id).await.unwrap();

        let snapshot = pm.get_doc_read_snapshot("test", &id).await.unwrap();

        assert_eq!(snapshot.visible_text, "secret text");
        assert_eq!(snapshot.source_schema, DocumentSourceSchema::GraphV2);
        assert!(!snapshot.needs_migration);
        assert!(!pm.doc_store.contains(&id));
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

    #[tokio::test]
    async fn test_open_doc_returns_recoverable_corruption_when_markdown_exists() {
        let dir = tempfile::tempdir().unwrap();
        let pm = ProjectManager::new(dir.path().to_path_buf());
        pm.create_project("test").await.unwrap();
        pm.open_project("test").await.unwrap();

        let id = pm.create_note("test", "ideas.md").await.unwrap();
        pm.doc_store
            .replace_text(&id, "# Ideas\n\nRecovered")
            .await
            .unwrap();
        pm.save_doc("test", &id).await.unwrap();
        pm.close_doc("test", &id).await.unwrap();

        let primary = dir
            .path()
            .join("test")
            .join(".p2p")
            .join("automerge")
            .join(format!("{}.automerge", id));
        let backup = dir
            .path()
            .join("test")
            .join(".p2p")
            .join("automerge")
            .join(format!("{}.automerge.bak", id));
        tokio::fs::write(&primary, b"broken primary").await.unwrap();
        tokio::fs::write(&backup, b"broken backup").await.unwrap();

        let error = pm
            .open_doc("test", &id)
            .await
            .expect_err("expected recoverable corruption");
        match error {
            CoreError::RecoverableDocCorruption {
                doc_id,
                note_path,
                suggested_path,
            } => {
                assert_eq!(doc_id, id);
                assert_eq!(note_path, "ideas.md");
                assert_eq!(suggested_path, "ideas.md");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_recover_note_from_markdown_rebuilds_same_doc_id() {
        let dir = tempfile::tempdir().unwrap();
        let pm = ProjectManager::new(dir.path().to_path_buf());
        pm.create_project("test").await.unwrap();
        pm.open_project("test").await.unwrap();

        let id = pm.create_note("test", "ideas.md").await.unwrap();
        pm.doc_store
            .replace_text(&id, "# Ideas\n\nRecovered")
            .await
            .unwrap();
        pm.save_doc("test", &id).await.unwrap();
        pm.close_doc("test", &id).await.unwrap();

        let primary = dir
            .path()
            .join("test")
            .join(".p2p")
            .join("automerge")
            .join(format!("{}.automerge", id));
        let backup = dir
            .path()
            .join("test")
            .join(".p2p")
            .join("automerge")
            .join(format!("{}.automerge.bak", id));
        tokio::fs::write(&primary, b"broken primary").await.unwrap();
        tokio::fs::write(&backup, b"broken backup").await.unwrap();

        let recovered = pm.recover_note_from_markdown("test", &id).await.unwrap();
        assert_eq!(recovered.id, id);
        assert_eq!(recovered.path, "ideas.md");

        pm.open_doc("test", &recovered.id).await.unwrap();
        let text = pm.get_doc_text(&recovered.id).await.unwrap();
        assert_eq!(text, "# Ideas\n\nRecovered");
        let editor_doc = pm
            .doc_store
            .get_editor_document(&recovered.id)
            .await
            .unwrap();
        assert_eq!(
            pm.doc_store.get_visible_text(&recovered.id).await.unwrap(),
            "# Ideas\n\nRecovered"
        );
        assert_eq!(editor_doc.doc.content.len(), 2);

        let original_md = tokio::fs::read_to_string(dir.path().join("test").join("ideas.md"))
            .await
            .unwrap();
        assert_eq!(original_md, "# Ideas\n\nRecovered");
        let mut entries =
            tokio::fs::read_dir(dir.path().join("test").join(".p2p").join("automerge"))
                .await
                .unwrap();
        let mut quarantined = Vec::new();
        while let Some(entry) = entries.next_entry().await.unwrap() {
            quarantined.push(entry.file_name().to_string_lossy().into_owned());
        }
        assert!(quarantined
            .iter()
            .any(|name| name.starts_with(&format!("{}.automerge.corrupt-", id))));
        assert!(quarantined
            .iter()
            .any(|name| name.starts_with(&format!("{}.automerge.bak.corrupt-", id))));
    }

    #[tokio::test]
    async fn test_resolve_local_access_distinguishes_identity_mismatch_from_viewer() {
        let dir = tempfile::tempdir().unwrap();
        let pm = ProjectManager::new(dir.path().to_path_buf());
        pm.create_project("test").await.unwrap();
        pm.open_project("test").await.unwrap();

        {
            let manifest = pm.get_manifest("test").unwrap();
            let mut manifest = manifest.write().await;
            manifest.set_owner("owner-peer").unwrap();
            manifest
                .add_peer("viewer-peer", "viewer", "Viewer")
                .unwrap();
        }

        let (viewer_role, viewer_access) = pm
            .resolve_local_access("test", "viewer-peer")
            .await
            .unwrap();
        assert_eq!(viewer_role, Some(PeerRole::Viewer));
        assert_eq!(viewer_access, ProjectAccessState::Viewer);

        let (mismatch_role, mismatch_access) = pm
            .resolve_local_access("test", "someone-else")
            .await
            .unwrap();
        assert_eq!(mismatch_role, None);
        assert_eq!(mismatch_access, ProjectAccessState::IdentityMismatch);
    }

    #[tokio::test]
    async fn test_project_x25519_cache_moves_on_rename_and_clears_on_delete() {
        let dir = tempfile::tempdir().unwrap();
        let pm = ProjectManager::new(dir.path().to_path_buf());
        pm.create_project("alpha").await.unwrap();

        let identity = pm
            .get_or_create_project_x25519_identity("alpha")
            .await
            .unwrap();
        assert!(pm.has_cached_project_x25519_identity("alpha"));

        pm.rename_project("alpha", "beta").await.unwrap();
        assert!(!pm.has_cached_project_x25519_identity("alpha"));
        assert!(pm.has_cached_project_x25519_identity("beta"));
        assert_eq!(
            identity.public_bytes(),
            pm.get_or_create_project_x25519_identity("beta")
                .await
                .unwrap()
                .public_bytes()
        );

        pm.delete_project("beta").await.unwrap();
        assert!(!pm.has_cached_project_x25519_identity("beta"));
    }

    #[tokio::test]
    async fn test_project_x25519_identity_reloads_same_key_after_restart() {
        let dir = tempfile::tempdir().unwrap();
        let pm = ProjectManager::new(dir.path().to_path_buf());
        pm.create_project("alpha").await.unwrap();

        let initial = pm
            .get_or_create_project_x25519_identity("alpha")
            .await
            .unwrap()
            .public_bytes();

        let restarted = ProjectManager::new(dir.path().to_path_buf())
            .get_or_create_project_x25519_identity("alpha")
            .await
            .unwrap()
            .public_bytes();

        assert_eq!(initial, restarted);
    }

    #[tokio::test]
    async fn test_project_x25519_identity_is_scoped_per_project() {
        let dir = tempfile::tempdir().unwrap();
        let pm = ProjectManager::new(dir.path().to_path_buf());
        pm.create_project("alpha").await.unwrap();
        pm.create_project("beta").await.unwrap();

        let alpha = pm
            .get_or_create_project_x25519_identity("alpha")
            .await
            .unwrap()
            .public_bytes();
        let beta = pm
            .get_or_create_project_x25519_identity("beta")
            .await
            .unwrap()
            .public_bytes();

        assert_ne!(alpha, beta);
    }

    #[tokio::test]
    async fn test_preload_all_project_secrets_loads_existing_shared_crypto() {
        std::env::set_var("NOTES_KEYSTORE_MODE", "file-only");
        let dir = tempfile::tempdir().unwrap();
        let pm = ProjectManager::new(dir.path().to_path_buf());
        pm.create_project("shared").await.unwrap();
        pm.create_project("local").await.unwrap();
        pm.init_epoch_keys("shared").await.unwrap();
        let shared_public = pm
            .get_or_create_project_x25519_identity("shared")
            .await
            .unwrap()
            .public_bytes();

        let restarted = ProjectManager::new(dir.path().to_path_buf());
        let (epoch_count, x25519_count) = restarted.preload_all_project_secrets().await.unwrap();

        assert_eq!(epoch_count, 1);
        assert_eq!(x25519_count, 1);
        assert!(restarted.has_cached_epoch_keys("shared"));
        assert!(restarted.has_cached_project_x25519_identity("shared"));
        assert!(!restarted.has_cached_epoch_keys("local"));
        assert!(!restarted.has_cached_project_x25519_identity("local"));
        assert_eq!(
            shared_public,
            restarted
                .get_or_create_project_x25519_identity("shared")
                .await
                .unwrap()
                .public_bytes()
        );
    }

    #[tokio::test]
    async fn test_preload_all_project_secrets_skips_corrupt_projects() {
        std::env::set_var("NOTES_KEYSTORE_MODE", "file-only");
        let dir = tempfile::tempdir().unwrap();
        let pm = ProjectManager::new(dir.path().to_path_buf());
        pm.create_project("healthy").await.unwrap();
        pm.create_project("corrupt").await.unwrap();
        pm.init_epoch_keys("healthy").await.unwrap();
        let healthy_public = pm
            .get_or_create_project_x25519_identity("healthy")
            .await
            .unwrap()
            .public_bytes();

        let corrupt_keys_dir = dir.path().join("corrupt").join(".p2p").join("keys");
        tokio::fs::create_dir_all(&corrupt_keys_dir).await.unwrap();
        let corrupt_keystore = notes_crypto::KeyStore::new(corrupt_keys_dir);
        corrupt_keystore
            .store_key("epoch-keys-corrupt", b"not-valid-epoch-data")
            .unwrap();

        let restarted = ProjectManager::new(dir.path().to_path_buf());
        let (epoch_count, x25519_count) = restarted.preload_all_project_secrets().await.unwrap();

        assert_eq!(epoch_count, 1);
        assert_eq!(x25519_count, 1);
        assert!(restarted.has_cached_epoch_keys("healthy"));
        assert!(!restarted.has_cached_epoch_keys("corrupt"));
        assert_eq!(
            healthy_public,
            restarted
                .get_or_create_project_x25519_identity("healthy")
                .await
                .unwrap()
                .public_bytes()
        );
    }

    #[tokio::test]
    async fn test_rename_project_preserves_epoch_keys_across_restart() {
        std::env::set_var("NOTES_KEYSTORE_MODE", "file-only");
        let dir = tempfile::tempdir().unwrap();
        let pm = ProjectManager::new(dir.path().to_path_buf());
        pm.create_project("alpha").await.unwrap();
        {
            let manifest = pm.get_manifest("alpha").unwrap();
            let mut manifest = manifest.write().await;
            manifest.set_owner("owner-peer").unwrap();
            let data = manifest.save();
            pm.persistence.save_manifest("alpha", &data).await.unwrap();
        }
        pm.init_epoch_keys("alpha").await.unwrap();
        pm.rename_project("alpha", "beta").await.unwrap();

        let restarted = ProjectManager::new(dir.path().to_path_buf());
        restarted.preload_all_project_secrets().await.unwrap();
        restarted.open_project("beta").await.unwrap();
        assert!(restarted.has_cached_epoch_keys("beta"));
    }

    #[tokio::test]
    async fn test_delete_project_removes_persisted_crypto_secrets() {
        std::env::set_var("NOTES_KEYSTORE_MODE", "file-only");
        let dir = tempfile::tempdir().unwrap();
        let pm = ProjectManager::new(dir.path().to_path_buf());
        pm.create_project("shared").await.unwrap();
        let project_id = pm.get_project_id("shared").await.unwrap();
        pm.init_epoch_keys("shared").await.unwrap();
        pm.get_or_create_project_x25519_identity("shared")
            .await
            .unwrap();

        let keystore =
            notes_crypto::KeyStore::new(dir.path().join("shared").join(".p2p").join("keys"));
        assert!(keystore.has_key(&format!("epoch-keys-{project_id}")));
        assert!(keystore.has_key(&format!("x25519-identity-{project_id}")));

        pm.delete_project("shared").await.unwrap();

        assert!(!keystore.has_key(&format!("epoch-keys-{project_id}")));
        assert!(!keystore.has_key(&format!("x25519-identity-{project_id}")));
    }

    #[tokio::test]
    async fn test_open_shared_project_fails_when_epoch_keys_missing() {
        std::env::set_var("NOTES_KEYSTORE_MODE", "file-only");
        let dir = tempfile::tempdir().unwrap();
        let pm = ProjectManager::new(dir.path().to_path_buf());
        pm.create_project("shared").await.unwrap();
        let project_id = pm.get_project_id("shared").await.unwrap();
        {
            let manifest = pm.get_manifest("shared").unwrap();
            let mut manifest = manifest.write().await;
            manifest.set_owner("owner-peer").unwrap();
            let data = manifest.save();
            pm.persistence.save_manifest("shared", &data).await.unwrap();
        }
        pm.init_epoch_keys("shared").await.unwrap();
        let keystore =
            notes_crypto::KeyStore::new(dir.path().join("shared").join(".p2p").join("keys"));
        keystore
            .delete_key(&format!("epoch-keys-{project_id}"))
            .unwrap();

        let restarted = ProjectManager::new(dir.path().to_path_buf());
        let err = restarted.open_project("shared").await.unwrap_err();
        assert!(err.to_string().contains("epoch keys are unavailable"));
    }

    #[test]
    fn test_build_project_peer_roster_excludes_self_and_includes_owner() {
        let peers = vec![
            PeerInfo {
                peer_id: "editor-peer".into(),
                role: PeerRole::Editor,
                alias: "ed".into(),
                since: chrono::Utc::now(),
            },
            PeerInfo {
                peer_id: "viewer-peer".into(),
                role: PeerRole::Viewer,
                alias: "vi".into(),
                since: chrono::Utc::now(),
            },
        ];

        let roster = ProjectManager::build_project_peer_roster(
            "owner-peer",
            Some("olivia".into()),
            &peers,
            "editor-peer",
            &std::collections::HashMap::from([
                ("owner-peer".to_string(), (true, None)),
                ("editor-peer".to_string(), (true, Some("doc-1".to_string()))),
            ]),
        );

        assert_eq!(roster.len(), 3);
        assert_eq!(roster[0].peer_id, "owner-peer");
        assert_eq!(roster[0].role, PeerRole::Owner);
        assert_eq!(roster[0].alias.as_deref(), Some("olivia"));
        assert!(roster[0].connected);
        assert!(!roster[0].is_self);

        assert_eq!(roster[1].peer_id, "editor-peer");
        assert!(roster[1].is_self);
        assert_eq!(roster[1].role, PeerRole::Editor);
        assert!(roster[1].connected);
        assert_eq!(roster[1].active_doc.as_deref(), Some("doc-1"));

        assert_eq!(roster[2].peer_id, "viewer-peer");
        assert_eq!(roster[2].role, PeerRole::Viewer);
        assert!(!roster[2].connected);
        assert!(!roster[2].is_self);
    }

    #[test]
    fn test_build_project_peer_roster_dedupes_owner_and_keeps_owner_role() {
        let peers = vec![PeerInfo {
            peer_id: "owner-peer".into(),
            role: PeerRole::Editor,
            alias: "wrong".into(),
            since: chrono::Utc::now(),
        }];

        let roster = ProjectManager::build_project_peer_roster(
            "owner-peer",
            Some("owner".into()),
            &peers,
            "someone-else",
            &std::collections::HashMap::new(),
        );

        assert_eq!(roster.len(), 1);
        assert_eq!(roster[0].peer_id, "owner-peer");
        assert_eq!(roster[0].role, PeerRole::Owner);
        assert_eq!(roster[0].alias.as_deref(), Some("owner"));
        assert!(!roster[0].is_self);
    }

    #[test]
    fn test_build_project_peer_roster_handles_local_only_and_owner_only_projects() {
        let local_only = ProjectManager::build_project_peer_roster(
            "",
            None,
            &[],
            "local-peer",
            &std::collections::HashMap::new(),
        );
        assert!(local_only.is_empty());

        let owner_only = ProjectManager::build_project_peer_roster(
            "owner-peer",
            Some("owner".into()),
            &[],
            "owner-peer",
            &std::collections::HashMap::new(),
        );
        assert_eq!(owner_only.len(), 1);
        assert!(owner_only[0].is_self);
        assert_eq!(owner_only[0].role, PeerRole::Owner);
    }

    #[test]
    fn test_build_project_peer_roster_sorts_stably_and_keeps_unparseable_peers_offline() {
        let peers = vec![
            PeerInfo {
                peer_id: "zzz-peer".into(),
                role: PeerRole::Editor,
                alias: "".into(),
                since: chrono::Utc::now(),
            },
            PeerInfo {
                peer_id: "aaa-peer".into(),
                role: PeerRole::Viewer,
                alias: "amy".into(),
                since: chrono::Utc::now(),
            },
        ];

        let roster = ProjectManager::build_project_peer_roster(
            "owner-peer",
            None,
            &peers,
            "local-peer",
            &std::collections::HashMap::new(),
        );

        assert_eq!(
            roster
                .iter()
                .map(|peer| peer.peer_id.as_str())
                .collect::<Vec<_>>(),
            vec!["owner-peer", "aaa-peer", "zzz-peer",]
        );
        assert_eq!(roster[0].alias.as_deref(), Some("peer"));
        assert_eq!(roster[2].alias.as_deref(), Some("peer"));
        assert!(!roster[2].connected);
        assert_eq!(roster[2].active_doc, None);
    }
}
