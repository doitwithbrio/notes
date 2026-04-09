use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use iroh::endpoint::{Endpoint, RecvStream, SendStream};
use notes_core::invite_state::PersistedJoinSecret;
use notes_core::{
    manifest::ProjectManifest, CoreError, DocId, JoinSessionStore, OwnerInviteStateStore, PeerRole,
    PersistedJoinSession, PersistedJoinStage, PersistedOwnerInvitePhase,
    PersistedOwnerInviteRecord, ProjectManager,
};
use notes_sync::invite::{
    InviteAcceptanceContext, InviteLifecycleHandler, InvitePayload, InvitePersistenceHandler,
    InviteState, PendingInvite,
};
use notes_sync::peer_manager::PeerManager;
use notes_sync::peer_manager::ProjectSyncResolver;
use notes_sync::sync_engine::{SyncChangeHandler, SyncEngine};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptInviteResult {
    pub project_id: String,
    pub project_name: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingJoinResumeStatus {
    pub session_id: String,
    pub owner_peer_id: String,
    pub project_id: String,
    pub project_name: String,
    pub local_project_name: String,
    pub role: String,
    pub stage: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnerInviteStatus {
    pub invite_id: String,
    pub project_id: String,
    pub project_name: String,
    pub role: String,
    pub expires_at: String,
    pub phase: String,
    pub invitee_peer_id: Option<String>,
    pub session_id: Option<String>,
}

pub struct ProjectSyncObserver {
    project_manager: Arc<ProjectManager>,
    sync_engine: std::sync::Weak<SyncEngine>,
    peer_manager: std::sync::Weak<PeerManager>,
    local_peer_id: iroh::EndpointId,
}

pub struct ProjectSyncResolverImpl {
    project_manager: Arc<ProjectManager>,
}

impl ProjectSyncResolverImpl {
    pub fn new(project_manager: Arc<ProjectManager>) -> Self {
        Self { project_manager }
    }
}

impl ProjectSyncResolver for ProjectSyncResolverImpl {
    fn manifest_doc_for_project<'a>(
        &'a self,
        project: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<uuid::Uuid>> + Send + 'a>> {
        Box::pin(async move { self.project_manager.manifest_doc_id(project).await.ok() })
    }
}

impl ProjectSyncObserver {
    pub fn new(
        project_manager: Arc<ProjectManager>,
        sync_engine: std::sync::Weak<SyncEngine>,
        peer_manager: std::sync::Weak<PeerManager>,
        local_peer_id: iroh::EndpointId,
    ) -> Self {
        Self {
            project_manager,
            sync_engine,
            peer_manager,
            local_peer_id,
        }
    }

    async fn apply_doc_change(&self, doc_id: DocId) {
        let Some(project_name) = self.project_manager.get_project_for_doc(&doc_id) else {
            log::debug!("ProjectSyncObserver: no project mapping for doc {doc_id}");
            return;
        };

        let Some(sync_engine) = self.sync_engine.upgrade() else {
            log::debug!("ProjectSyncObserver: sync engine gone for doc {doc_id}");
            return;
        };
        let Some(peer_manager) = self.peer_manager.upgrade() else {
            log::debug!("ProjectSyncObserver: peer manager gone for doc {doc_id}");
            return;
        };

        let manifest_doc_id = match self.project_manager.manifest_doc_id(&project_name).await {
            Ok(id) => id,
            Err(_) => return,
        };

        if manifest_doc_id == doc_id {
            log::debug!(
                "ProjectSyncObserver: applying manifest doc {doc_id} for project {project_name}"
            );
            if let Ok(registered_docs) = self
                .project_manager
                .apply_remote_manifest_doc_to_project(&project_name, &doc_id)
                .await
            {
                log::debug!(
                    "ProjectSyncObserver: registered docs after manifest apply: {:?}",
                    registered_docs
                );
                for id in std::iter::once(manifest_doc_id).chain(registered_docs.iter().copied()) {
                    if let Ok(doc_arc) = self.project_manager.doc_store().get_doc(&id) {
                        sync_engine.register_doc(id, doc_arc);
                        populate_doc_acl_from_parts(
                            &self.project_manager,
                            &sync_engine,
                            &self.local_peer_id,
                            &project_name,
                            id,
                        )
                        .await;
                    }
                }
                let _ = hydrate_missing_doc_snapshots(
                    &self.project_manager,
                    &sync_engine,
                    &peer_manager,
                    &self.local_peer_id,
                    &project_name,
                    &registered_docs,
                )
                .await;
            }
        }
    }
}

impl SyncChangeHandler for ProjectSyncObserver {
    fn on_doc_changed<'a>(
        &'a self,
        doc_id: uuid::Uuid,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            self.apply_doc_change(doc_id).await;
        })
    }
}

pub fn list_pending_join_resumes(
    join_session_store: &JoinSessionStore,
) -> Result<Vec<PendingJoinResumeStatus>, CoreError> {
    Ok(join_session_store
        .load_all()?
        .into_iter()
        .map(|session| PendingJoinResumeStatus {
            session_id: session.session_id,
            owner_peer_id: session.owner_peer_id,
            project_id: session.project_id,
            project_name: session.project_name,
            local_project_name: session.local_project_name,
            role: session.role,
            stage: join_stage_label(&session.stage).to_string(),
            updated_at: session.updated_at.to_rfc3339(),
        })
        .collect())
}

pub fn list_owner_invites_from_store(
    owner_invite_store: &OwnerInviteStateStore,
    project_name: Option<&str>,
) -> Result<Vec<OwnerInviteStatus>, CoreError> {
    Ok(owner_invite_store
        .load_all()?
        .into_iter()
        .filter(|record| project_name.is_none_or(|name| name == record.project_name))
        .map(|record| {
            let (invitee_peer_id, session_id) = match &record.phase {
                PersistedOwnerInvitePhase::Reserved {
                    invitee_peer_id,
                    session_id,
                    ..
                }
                | PersistedOwnerInvitePhase::PreparedAckReceived {
                    invitee_peer_id,
                    session_id,
                    ..
                }
                | PersistedOwnerInvitePhase::CommittedPendingAck {
                    invitee_peer_id,
                    session_id,
                    ..
                }
                | PersistedOwnerInvitePhase::Consumed {
                    invitee_peer_id,
                    session_id,
                    ..
                } => (Some(invitee_peer_id.clone()), Some(session_id.clone())),
                PersistedOwnerInvitePhase::Open => (None, None),
            };

            OwnerInviteStatus {
                invite_id: record.invite_id,
                project_id: record.project_id,
                project_name: record.project_name,
                role: record.role,
                expires_at: record.expires_at.to_rfc3339(),
                phase: owner_phase_label(&record.phase).to_string(),
                invitee_peer_id,
                session_id,
            }
        })
        .collect())
}

pub struct InviteAcceptanceSession {
    payload: InvitePayload,
    peer_id: iroh::EndpointId,
    send: SendStream,
    recv: RecvStream,
}

pub struct StagedInviteInstall {
    local_project_name: String,
    payload: InvitePayload,
    peer_id: iroh::EndpointId,
    manifest_bytes: Vec<u8>,
}

#[derive(Default)]
pub struct SessionSecretCache {
    join_session_secrets: Mutex<HashMap<String, PersistedJoinSecret>>,
}

impl SessionSecretCache {
    pub fn cache_join_secret(&self, session_id: &str, secret: PersistedJoinSecret) {
        if let Ok(mut cache) = self.join_session_secrets.lock() {
            cache.insert(session_id.to_string(), secret);
        }
    }

    pub fn load_join_secret(
        &self,
        store: &JoinSessionStore,
        session_id: &str,
    ) -> Result<Option<PersistedJoinSecret>, CoreError> {
        if let Ok(cache) = self.join_session_secrets.lock() {
            if let Some(secret) = cache.get(session_id) {
                notes_crypto::debug_note_secret_cache_hit();
                return Ok(Some(secret.clone()));
            }
        }
        notes_crypto::debug_note_secret_cache_miss();

        let secret = store.load_secret_bundle(session_id)?;
        if let Some(secret_bundle) = &secret {
            self.cache_join_secret(session_id, secret_bundle.clone());
        }
        Ok(secret)
    }

    pub fn remove_join_secret(&self, session_id: &str) {
        if let Ok(mut cache) = self.join_session_secrets.lock() {
            cache.remove(session_id);
        }
    }

    pub fn has_join_passphrase(&self, session_id: &str) -> bool {
        self.join_session_secrets
            .lock()
            .map(|cache| cache.contains_key(session_id))
            .unwrap_or(false)
    }

    pub fn preload_join_secrets(&self, store: &JoinSessionStore) -> Result<usize, CoreError> {
        let mut loaded = 0usize;
        for session in store.load_all()? {
            if let Some(secret) = store.load_secret_bundle(&session.session_id)? {
                self.cache_join_secret(&session.session_id, secret);
                loaded += 1;
            }
        }
        Ok(loaded)
    }
}

pub struct OwnerInvitePersistence {
    base_dir: std::path::PathBuf,
    store: Arc<OwnerInviteStateStore>,
    keystore: notes_crypto::KeyStore,
    owner_peer_id: String,
    known_secret_ids: Mutex<HashSet<String>>,
}

impl OwnerInvitePersistence {
    pub fn new(base_dir: std::path::PathBuf, owner_peer_id: String) -> Self {
        let store = Arc::new(OwnerInviteStateStore::new(base_dir.clone()));
        let keystore = notes_crypto::KeyStore::new(base_dir.join(".p2p").join("invite-secrets"));
        Self {
            base_dir,
            store,
            keystore,
            owner_peer_id,
            known_secret_ids: Mutex::new(HashSet::new()),
        }
    }

    fn mark_secret_loaded(&self, invite_id: &str) {
        if let Ok(mut known) = self.known_secret_ids.lock() {
            known.insert(invite_id.to_string());
        }
    }

    fn forget_secret(&self, invite_id: &str) {
        if let Ok(mut known) = self.known_secret_ids.lock() {
            known.remove(invite_id);
        }
    }

    fn knows_secret(&self, invite_id: &str) -> bool {
        self.known_secret_ids
            .lock()
            .map(|known| known.contains(invite_id))
            .unwrap_or(false)
    }

    fn secret_key(invite_id: &str) -> String {
        format!("invite-passphrase-{invite_id}")
    }

    fn restore_reserved_state(
        session_id: &str,
        invitee_peer_id: &str,
        reserved_at: chrono::DateTime<Utc>,
        phase: &str,
    ) -> InviteState {
        let reserved_at_instant = restore_instant(reserved_at);
        InviteState::Reserved(notes_sync::invite::InviteReservation {
            session_id: session_id.to_string(),
            invitee_peer_id: invitee_peer_id.to_string(),
            reserved_at: reserved_at_instant,
            timeout_at: reserved_at_instant + std::time::Duration::from_secs(30),
            phase: match phase {
                "PayloadPrepared" => notes_sync::invite::InviteSessionPhase::PayloadPrepared,
                "PayloadSent" => notes_sync::invite::InviteSessionPhase::PayloadSent,
                "AwaitingPreparedAck" => {
                    notes_sync::invite::InviteSessionPhase::AwaitingPreparedAck
                }
                _ => notes_sync::invite::InviteSessionPhase::Reserved,
            },
        })
    }

    pub fn load_runtime_invites(&self) -> Result<Vec<(String, PendingInvite)>, CoreError> {
        self.load_runtime_invites_with(|_, _, _| None)
    }

    pub fn load_runtime_invites_with_manifest_reconcile(
        &self,
    ) -> Result<Vec<(String, PendingInvite)>, CoreError> {
        let mut out = Vec::new();
        for record in self.store.load_all()? {
            if matches!(record.phase, PersistedOwnerInvitePhase::Consumed { .. }) {
                let _ = self.delete_invite(&record.invite_id);
                continue;
            }

            let passphrase = match self.keystore.load_key(&Self::secret_key(&record.invite_id)) {
                Ok(bytes) => String::from_utf8(bytes).map_err(|_| {
                    CoreError::InvalidData("invalid invite passphrase bytes".into())
                })?,
                Err(notes_crypto::CryptoError::KeyNotFound(_)) => continue,
                Err(err) => {
                    log::warn!(
                        "Skipping persisted invite {} until secret storage recovers: {}",
                        record.invite_id,
                        err
                    );
                    continue;
                }
            };
            notes_crypto::debug_note_secret_cache_miss();
            self.mark_secret_loaded(&record.invite_id);

            let state = match &record.phase {
                PersistedOwnerInvitePhase::Open => InviteState::Open,
                PersistedOwnerInvitePhase::Reserved {
                    session_id,
                    invitee_peer_id,
                    reserved_at,
                    phase,
                } => Self::restore_reserved_state(session_id, invitee_peer_id, *reserved_at, phase),
                PersistedOwnerInvitePhase::PreparedAckReceived {
                    session_id,
                    invitee_peer_id,
                    prepared_at,
                } => {
                    let expected_role = match record.role.as_str() {
                        "owner" => PeerRole::Owner,
                        "viewer" => PeerRole::Viewer,
                        _ => PeerRole::Editor,
                    };
                    if self.manifest_contains_peer(
                        &record.project_name,
                        invitee_peer_id,
                        expected_role,
                    )? {
                        InviteState::CommittedPendingAck(
                            notes_sync::invite::InviteCommittedPendingAck {
                                session_id: session_id.clone(),
                                invitee_peer_id: invitee_peer_id.clone(),
                                committed_at: restore_instant(*prepared_at),
                            },
                        )
                    } else {
                        InviteState::Open
                    }
                }
                PersistedOwnerInvitePhase::CommittedPendingAck {
                    session_id,
                    invitee_peer_id,
                    committed_at,
                } => InviteState::CommittedPendingAck(
                    notes_sync::invite::InviteCommittedPendingAck {
                        session_id: session_id.clone(),
                        invitee_peer_id: invitee_peer_id.clone(),
                        committed_at: restore_instant(*committed_at),
                    },
                ),
                PersistedOwnerInvitePhase::Consumed { .. } => continue,
            };

            out.push((
                passphrase.clone(),
                PendingInvite {
                    invite_id: record.invite_id.clone(),
                    code: notes_sync::invite::InviteCode {
                        passphrase,
                        peer_id: self.owner_peer_id.clone(),
                        expires_at: record.expires_at,
                    },
                    created_at: std::time::Instant::now(),
                    attempts: record.attempts,
                    project_name: record.project_name.clone(),
                    project_id: record.project_id.clone(),
                    invite_role: record.role.clone(),
                    state,
                },
            ));
        }
        Ok(out)
    }

    fn load_runtime_invites_with<F>(
        &self,
        reconcile: F,
    ) -> Result<Vec<(String, PendingInvite)>, CoreError>
    where
        F: Fn(&str, &str, &str) -> Option<()>,
    {
        let mut out = Vec::new();
        for record in self.store.load_all()? {
            if matches!(record.phase, PersistedOwnerInvitePhase::Consumed { .. }) {
                let _ = self.delete_invite(&record.invite_id);
                continue;
            }

            let passphrase = match self.keystore.load_key(&Self::secret_key(&record.invite_id)) {
                Ok(bytes) => String::from_utf8(bytes).map_err(|_| {
                    CoreError::InvalidData("invalid invite passphrase bytes".into())
                })?,
                Err(_) => continue,
            };
            notes_crypto::debug_note_secret_cache_miss();
            self.mark_secret_loaded(&record.invite_id);

            let state = match &record.phase {
                PersistedOwnerInvitePhase::Open => InviteState::Open,
                PersistedOwnerInvitePhase::Reserved {
                    session_id,
                    invitee_peer_id,
                    reserved_at,
                    phase,
                } => Self::restore_reserved_state(session_id, invitee_peer_id, *reserved_at, phase),
                PersistedOwnerInvitePhase::PreparedAckReceived {
                    session_id,
                    invitee_peer_id,
                    prepared_at: _,
                } => {
                    if reconcile(&record.project_name, invitee_peer_id, &record.role).is_some() {
                        InviteState::CommittedPendingAck(
                            notes_sync::invite::InviteCommittedPendingAck {
                                session_id: session_id.clone(),
                                invitee_peer_id: invitee_peer_id.clone(),
                                committed_at: std::time::Instant::now(),
                            },
                        )
                    } else {
                        InviteState::Open
                    }
                }
                PersistedOwnerInvitePhase::CommittedPendingAck {
                    session_id,
                    invitee_peer_id,
                    committed_at,
                } => InviteState::CommittedPendingAck(
                    notes_sync::invite::InviteCommittedPendingAck {
                        session_id: session_id.clone(),
                        invitee_peer_id: invitee_peer_id.clone(),
                        committed_at: std::time::Instant::now()
                            .checked_sub((Utc::now() - *committed_at).to_std().unwrap_or_default())
                            .unwrap_or_else(std::time::Instant::now),
                    },
                ),
                PersistedOwnerInvitePhase::Consumed { .. } => continue,
            };

            out.push((
                passphrase.clone(),
                PendingInvite {
                    invite_id: record.invite_id.clone(),
                    code: notes_sync::invite::InviteCode {
                        passphrase,
                        peer_id: self.owner_peer_id.clone(),
                        expires_at: record.expires_at,
                    },
                    created_at: std::time::Instant::now(),
                    attempts: record.attempts,
                    project_name: record.project_name.clone(),
                    project_id: record.project_id.clone(),
                    invite_role: record.role.clone(),
                    state,
                },
            ));
        }
        Ok(out)
    }

    fn manifest_contains_peer(
        &self,
        project_name: &str,
        invitee_peer_id: &str,
        expected_role: PeerRole,
    ) -> Result<bool, CoreError> {
        let path = self
            .base_dir
            .join(project_name)
            .join(".p2p")
            .join("manifest.automerge");
        if !path.exists() {
            return Ok(false);
        }
        let raw = std::fs::read(path)?;
        let manifest = ProjectManifest::load(&raw)?;
        Ok(manifest
            .list_peers()?
            .into_iter()
            .any(|peer| peer.peer_id == invitee_peer_id && peer.role == expected_role))
    }

    pub fn list_owner_invites(
        &self,
        project_name: Option<&str>,
    ) -> Result<Vec<OwnerInviteStatus>, CoreError> {
        let mut out = Vec::new();
        for record in self.store.load_all()? {
            if project_name.is_some_and(|name| name != record.project_name) {
                continue;
            }
            let (invitee_peer_id, session_id) = match &record.phase {
                PersistedOwnerInvitePhase::Reserved {
                    invitee_peer_id,
                    session_id,
                    ..
                }
                | PersistedOwnerInvitePhase::PreparedAckReceived {
                    invitee_peer_id,
                    session_id,
                    ..
                }
                | PersistedOwnerInvitePhase::CommittedPendingAck {
                    invitee_peer_id,
                    session_id,
                    ..
                }
                | PersistedOwnerInvitePhase::Consumed {
                    invitee_peer_id,
                    session_id,
                    ..
                } => (Some(invitee_peer_id.clone()), Some(session_id.clone())),
                PersistedOwnerInvitePhase::Open => (None, None),
            };
            out.push(OwnerInviteStatus {
                invite_id: record.invite_id,
                project_id: record.project_id,
                project_name: record.project_name,
                role: record.role,
                expires_at: record.expires_at.to_rfc3339(),
                phase: owner_phase_label(&record.phase).to_string(),
                invitee_peer_id,
                session_id,
            });
        }
        Ok(out)
    }
}

fn restore_instant(timestamp: chrono::DateTime<Utc>) -> std::time::Instant {
    let elapsed = (Utc::now() - timestamp).to_std().unwrap_or_default();
    std::time::Instant::now()
        .checked_sub(elapsed)
        .unwrap_or_else(std::time::Instant::now)
}

fn emit_invite_accept_event(
    app_handle: Option<&AppHandle>,
    event: notes_sync::events::InviteAcceptEvent,
) {
    if let Some(handle) = app_handle {
        let _ = handle.emit(notes_sync::events::event_names::INVITE_ACCEPT_STATUS, event);
    }
}

fn join_stage_label(stage: &PersistedJoinStage) -> &'static str {
    match stage {
        PersistedJoinStage::PayloadStaged { .. } => "payload-staged",
        PersistedJoinStage::CommitConfirmed { .. } => "commit-confirmed",
        PersistedJoinStage::Finalized { .. } => "finalized",
    }
}

fn owner_phase_label(phase: &PersistedOwnerInvitePhase) -> &'static str {
    match phase {
        PersistedOwnerInvitePhase::Open => "open",
        PersistedOwnerInvitePhase::Reserved { .. } => "reserved",
        PersistedOwnerInvitePhase::PreparedAckReceived { .. } => "prepared-ack-received",
        PersistedOwnerInvitePhase::CommittedPendingAck { .. } => "committed-pending-ack",
        PersistedOwnerInvitePhase::Consumed { .. } => "consumed",
    }
}

impl InvitePersistenceHandler for OwnerInvitePersistence {
    fn sync_invite(
        &self,
        passphrase: &str,
        invite: &PendingInvite,
    ) -> Result<(), notes_sync::invite::InviteError> {
        let is_consumed = matches!(invite.state, InviteState::Consumed(_));
        let secret_name = Self::secret_key(&invite.invite_id);
        let should_store_secret = if is_consumed {
            false
        } else {
            let knows_secret = self.knows_secret(&invite.invite_id);
            if knows_secret {
                notes_crypto::debug_note_secret_cache_hit();
            } else {
                notes_crypto::debug_note_secret_cache_miss();
            }
            !knows_secret
        };

        let phase = match &invite.state {
            InviteState::Open => PersistedOwnerInvitePhase::Open,
            InviteState::Reserved(reservation) => match reservation.phase {
                notes_sync::invite::InviteSessionPhase::Reserved
                | notes_sync::invite::InviteSessionPhase::PayloadPrepared
                | notes_sync::invite::InviteSessionPhase::PayloadSent
                | notes_sync::invite::InviteSessionPhase::AwaitingPreparedAck => {
                    PersistedOwnerInvitePhase::Reserved {
                        session_id: reservation.session_id.clone(),
                        invitee_peer_id: reservation.invitee_peer_id.clone(),
                        reserved_at: Utc::now(),
                        phase: format!("{:?}", reservation.phase),
                    }
                }
                notes_sync::invite::InviteSessionPhase::PreparedAckReceived
                | notes_sync::invite::InviteSessionPhase::Committed => {
                    PersistedOwnerInvitePhase::PreparedAckReceived {
                        session_id: reservation.session_id.clone(),
                        invitee_peer_id: reservation.invitee_peer_id.clone(),
                        prepared_at: Utc::now(),
                    }
                }
            },
            InviteState::CommittedPendingAck(committed) => {
                PersistedOwnerInvitePhase::CommittedPendingAck {
                    session_id: committed.session_id.clone(),
                    invitee_peer_id: committed.invitee_peer_id.clone(),
                    committed_at: Utc::now(),
                }
            }
            InviteState::Consumed(consumed) => PersistedOwnerInvitePhase::Consumed {
                session_id: consumed.session_id.clone(),
                invitee_peer_id: consumed.invitee_peer_id.clone(),
                consumed_at: Utc::now(),
            },
        };

        if should_store_secret {
            self.keystore
                .store_key(&secret_name, passphrase.as_bytes())
                .map_err(|e| notes_sync::invite::InviteError::Lifecycle(e.to_string()))?;
            self.mark_secret_loaded(&invite.invite_id);
        }

        self.store
            .save(&PersistedOwnerInviteRecord {
                schema_version: 1,
                invite_id: invite.invite_id.clone(),
                project_name: invite.project_name.clone(),
                project_id: invite.project_id.clone(),
                owner_peer_id: self.owner_peer_id.clone(),
                role: invite.invite_role.clone(),
                created_at: Utc::now(),
                expires_at: invite.code.expires_at,
                attempts: invite.attempts,
                phase,
            })
            .map_err(|e| notes_sync::invite::InviteError::Lifecycle(e.to_string()))?;

        if is_consumed {
            self.forget_secret(&invite.invite_id);
            let _ = self.keystore.delete_key(&secret_name);
        }

        Ok(())
    }

    fn delete_invite(&self, invite_id: &str) -> Result<(), notes_sync::invite::InviteError> {
        self.store
            .delete(invite_id)
            .map_err(|e| notes_sync::invite::InviteError::Lifecycle(e.to_string()))?;
        self.forget_secret(invite_id);
        let _ = self.keystore.delete_key(&Self::secret_key(invite_id));
        Ok(())
    }
}

pub struct OwnerInviteCoordinator {
    project_manager: Arc<ProjectManager>,
    sync_engine: Arc<SyncEngine>,
    peer_manager: Arc<PeerManager>,
    local_peer_id: iroh::EndpointId,
}

impl OwnerInviteCoordinator {
    pub fn new(
        project_manager: Arc<ProjectManager>,
        sync_engine: Arc<SyncEngine>,
        peer_manager: Arc<PeerManager>,
        local_peer_id: iroh::EndpointId,
    ) -> Self {
        Self {
            project_manager,
            sync_engine,
            peer_manager,
            local_peer_id,
        }
    }

    fn encode_hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    async fn existing_peer_role(
        &self,
        project_name: &str,
        peer_id: &str,
    ) -> Result<Option<PeerRole>, CoreError> {
        let peers = self.project_manager.get_project_peers(project_name).await?;
        Ok(peers
            .into_iter()
            .find(|peer| peer.peer_id == peer_id)
            .map(|peer| peer.role))
    }

    pub async fn build_payload(
        &self,
        ctx: &InviteAcceptanceContext,
    ) -> Result<InvitePayload, CoreError> {
        self.project_manager
            .ensure_local_actor_binding(&ctx.project_name, &self.local_peer_id.to_string())
            .await?;
        let requested_role = match ctx.role.as_str() {
            "owner" => PeerRole::Owner,
            "editor" => PeerRole::Editor,
            "viewer" => PeerRole::Viewer,
            other => {
                return Err(CoreError::InvalidInput(format!(
                    "unsupported invite role: {other}"
                )))
            }
        };

        let already_member = if let Some(existing_role) = self
            .existing_peer_role(&ctx.project_name, &ctx.invitee_peer_id)
            .await?
        {
            if existing_role != requested_role {
                return Err(CoreError::InvalidInput(format!(
                    "peer {} is already a {:?} on project {}",
                    ctx.invitee_peer_id, existing_role, ctx.project_name
                )));
            }
            true
        } else {
            false
        };

        self.project_manager
            .init_epoch_keys(&ctx.project_name)
            .await?;

        let manifest_arc = self
            .project_manager
            .get_manifest_for_ui(&ctx.project_name)?;
        let manifest_data = {
            let mut manifest = manifest_arc.write().await;
            let current_owner = manifest.get_owner().unwrap_or_default();
            if current_owner.is_empty() {
                manifest.set_owner(&self.local_peer_id.to_string())?;
            }

            let saved = manifest.save();
            if already_member {
                saved
            } else {
                let mut temp_manifest = ProjectManifest::load(&saved)?;
                temp_manifest.add_peer(&ctx.invitee_peer_id, &ctx.role, "")?;
                temp_manifest.save()
            }
        };

        let owner_x25519_public = self
            .project_manager
            .get_or_create_project_x25519_identity(&ctx.project_name)
            .await?
            .public_bytes();

        let epoch_mgr_arc = self
            .project_manager
            .get_epoch_keys(&ctx.project_name)
            .map_err(|_| {
                CoreError::InvalidData(format!(
                    "shared project '{}' is missing epoch keys",
                    ctx.project_name
                ))
            })?;
        let mgr = epoch_mgr_arc.read().await;
        let current_epoch = mgr.current_epoch();
        let epoch_key_hex = Self::encode_hex(
            mgr.current_key()
                .map_err(|_| {
                    CoreError::InvalidData(format!(
                        "shared project '{}' has unavailable epoch keys",
                        ctx.project_name
                    ))
                })?
                .as_bytes(),
        );

        Ok(InvitePayload {
            invite_id: ctx.invite_id.clone(),
            session_id: ctx.session_id.clone(),
            project_id: ctx.project_id.clone(),
            project_name: ctx.project_name.clone(),
            role: ctx.role.clone(),
            manifest_hex: Self::encode_hex(&manifest_data),
            owner_x25519_public_hex: Self::encode_hex(&owner_x25519_public),
            epoch_key_hex,
            epoch: current_epoch,
        })
    }

    pub async fn apply_acceptance_commit(
        &self,
        ctx: &InviteAcceptanceContext,
    ) -> Result<(), CoreError> {
        let requested_role = match ctx.role.as_str() {
            "owner" => PeerRole::Owner,
            "editor" => PeerRole::Editor,
            "viewer" => PeerRole::Viewer,
            other => {
                return Err(CoreError::InvalidInput(format!(
                    "unsupported invite role: {other}"
                )))
            }
        };

        let already_member = if let Some(existing_role) = self
            .existing_peer_role(&ctx.project_name, &ctx.invitee_peer_id)
            .await?
        {
            if existing_role != requested_role {
                return Err(CoreError::InvalidInput(format!(
                    "peer {} is already a {:?} on project {}",
                    ctx.invitee_peer_id, existing_role, ctx.project_name
                )));
            }
            true
        } else {
            false
        };

        if !already_member {
            let manifest_arc = self
                .project_manager
                .get_manifest_for_ui(&ctx.project_name)?;
            let mut manifest = manifest_arc.write().await;
            manifest.add_peer(&ctx.invitee_peer_id, &ctx.role, "")?;
            let data = manifest.save();
            drop(manifest);
            self.project_manager
                .persistence()
                .save_manifest(&ctx.project_name, &data)
                .await?;
        }

        let manifest_doc_id = self
            .project_manager
            .ensure_manifest_doc_loaded(&ctx.project_name)
            .await?;
        let registered_docs = self
            .project_manager
            .apply_manifest_doc_to_project(&ctx.project_name, &manifest_doc_id)
            .await?;

        let peer_id: iroh::EndpointId = ctx
            .invitee_peer_id
            .parse()
            .map_err(|e| CoreError::InvalidInput(format!("invalid invitee peer ID: {e}")))?;

        self.peer_manager
            .add_peer_to_project(&ctx.project_name, peer_id);

        let manifest_arc = self
            .project_manager
            .get_manifest_for_ui(&ctx.project_name)?;
        let doc_ids = {
            let manifest = manifest_arc.read().await;
            manifest
                .list_files()
                .unwrap_or_default()
                .into_iter()
                .map(|f| f.id)
                .collect::<Vec<_>>()
        };
        let sync_role = match ctx.role.as_str() {
            "owner" => notes_sync::sync_engine::PeerRole::Owner,
            "viewer" => notes_sync::sync_engine::PeerRole::Viewer,
            _ => notes_sync::sync_engine::PeerRole::Editor,
        };
        for doc_id in std::iter::once(manifest_doc_id).chain(doc_ids.into_iter()) {
            self.sync_engine.set_peer_role(doc_id, peer_id, sync_role);
            if let Ok(doc_arc) = self.project_manager.doc_store().get_doc(&doc_id) {
                self.sync_engine.register_doc(doc_id, doc_arc);
            }
            populate_doc_acl_from_parts(
                &self.project_manager,
                &self.sync_engine,
                &self.local_peer_id,
                &ctx.project_name,
                doc_id,
            )
            .await;
        }
        for doc_id in registered_docs {
            if let Ok(doc_arc) = self.project_manager.doc_store().get_doc(&doc_id) {
                self.sync_engine.register_doc(doc_id, doc_arc);
                populate_doc_acl_from_parts(
                    &self.project_manager,
                    &self.sync_engine,
                    &self.local_peer_id,
                    &ctx.project_name,
                    doc_id,
                )
                .await;
            }
        }

        Ok(())
    }
}

impl InviteLifecycleHandler for OwnerInviteCoordinator {
    fn prepare_payload<'a>(
        &'a self,
        ctx: &'a InviteAcceptanceContext,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<InvitePayload, notes_sync::invite::InviteError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            self.build_payload(ctx)
                .await
                .map_err(|e| notes_sync::invite::InviteError::Lifecycle(e.to_string()))
        })
    }

    fn commit_acceptance<'a>(
        &'a self,
        ctx: &'a InviteAcceptanceContext,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<(), notes_sync::invite::InviteError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            self.apply_acceptance_commit(ctx)
                .await
                .map_err(|e| notes_sync::invite::InviteError::Lifecycle(e.to_string()))
        })
    }
}

pub fn persist_commit_confirmed_session(
    store: &JoinSessionStore,
    session: &InviteAcceptanceSession,
) -> Result<(), CoreError> {
    persist_commit_confirmed_payload(
        store,
        &session.payload,
        &session.peer_id.to_string(),
        &session.payload.project_name,
    )
}

pub fn persist_payload_staged_session(
    store: &JoinSessionStore,
    secret_cache: &SessionSecretCache,
    payload: &InvitePayload,
    owner_peer_id: &str,
    local_project_name: &str,
    passphrase: &str,
) -> Result<(), CoreError> {
    let persisted_payload = persisted_payload(payload);
    store.save(&PersistedJoinSession {
        schema_version: 1,
        session_id: payload.session_id.clone(),
        owner_peer_id: owner_peer_id.to_string(),
        project_id: payload.project_id.clone(),
        project_name: payload.project_name.clone(),
        local_project_name: local_project_name.to_string(),
        role: payload.role.clone(),
        payload: serde_json::to_string(&persisted_payload)?,
        stage: PersistedJoinStage::PayloadStaged {
            staged_at: Utc::now(),
        },
        updated_at: Utc::now(),
    })?;
    let secret = PersistedJoinSecret {
        passphrase: passphrase.to_string(),
        epoch_key_hex: (!payload.epoch_key_hex.is_empty()).then(|| payload.epoch_key_hex.clone()),
    };
    store.save_secret_bundle(&payload.session_id, &secret)?;
    secret_cache.cache_join_secret(&payload.session_id, secret);
    Ok(())
}

pub fn persist_commit_confirmed_payload(
    store: &JoinSessionStore,
    payload: &InvitePayload,
    owner_peer_id: &str,
    local_project_name: &str,
) -> Result<(), CoreError> {
    let persisted_payload = persisted_payload(payload);
    store.save(&PersistedJoinSession {
        schema_version: 1,
        session_id: payload.session_id.clone(),
        owner_peer_id: owner_peer_id.to_string(),
        project_id: payload.project_id.clone(),
        project_name: payload.project_name.clone(),
        local_project_name: local_project_name.to_string(),
        role: payload.role.clone(),
        payload: serde_json::to_string(&persisted_payload)?,
        stage: PersistedJoinStage::CommitConfirmed {
            confirmed_at: Utc::now(),
        },
        updated_at: Utc::now(),
    })
}

pub fn persist_finalized_session(
    store: &JoinSessionStore,
    session_id: &str,
    payload: &InvitePayload,
    owner_peer_id: &str,
    local_project_name: &str,
) -> Result<(), CoreError> {
    let persisted_payload = persisted_payload(payload);
    store.save(&PersistedJoinSession {
        schema_version: 1,
        session_id: session_id.to_string(),
        owner_peer_id: owner_peer_id.to_string(),
        project_id: payload.project_id.clone(),
        project_name: payload.project_name.clone(),
        local_project_name: local_project_name.to_string(),
        role: payload.role.clone(),
        payload: serde_json::to_string(&persisted_payload)?,
        stage: PersistedJoinStage::Finalized {
            finalized_at: Utc::now(),
        },
        updated_at: Utc::now(),
    })
}

pub fn delete_join_session(
    store: &JoinSessionStore,
    secret_cache: &SessionSecretCache,
    session_id: &str,
) {
    secret_cache.remove_join_secret(session_id);
    let _ = store.delete(session_id);
}

fn persisted_payload(payload: &InvitePayload) -> InvitePayload {
    let mut payload = payload.clone();
    payload.epoch_key_hex.clear();
    payload
}

fn rehydrate_payload(secret: &PersistedJoinSecret, payload: &mut InvitePayload) {
    if payload.epoch_key_hex.is_empty() {
        payload.epoch_key_hex = secret.epoch_key_hex.clone().unwrap_or_default();
    }
}

pub async fn resume_owner_commit_status(
    endpoint: Endpoint,
    passphrase: String,
    owner_peer_id: String,
) -> Result<InviteAcceptanceSession, CoreError> {
    use notes_sync::invite;

    let peer_id: iroh::EndpointId = owner_peer_id
        .parse()
        .map_err(|e| CoreError::InvalidInput(format!("invalid owner peer ID: {e}")))?;

    let (mut send, mut recv, connection) = tokio::time::timeout(Duration::from_secs(30), async {
        let connection = endpoint
            .connect(peer_id, invite::INVITE_ALPN)
            .await
            .map_err(|e| CoreError::InvalidInput(format!("connection failed: {e}")))?;

        let (mut send, mut recv) = connection
            .open_bi()
            .await
            .map_err(|e| CoreError::InvalidData(format!("stream open failed: {e}")))?;

        let (invitee_state, invitee_msg) = invite::start_invitee_handshake(&passphrase);
        let len = (invitee_msg.len() as u32).to_be_bytes();
        send.write_all(&len)
            .await
            .map_err(|e| CoreError::InvalidData(format!("send spake2 len failed: {e}")))?;
        send.write_all(&invitee_msg)
            .await
            .map_err(|e| CoreError::InvalidData(format!("send spake2 msg failed: {e}")))?;

        let mut owner_msg_len_buf = [0u8; 4];
        recv.read_exact(&mut owner_msg_len_buf)
            .await
            .map_err(|e| CoreError::InvalidData(format!("read spake2 len failed: {e}")))?;
        let owner_msg_len = u32::from_be_bytes(owner_msg_len_buf) as usize;
        let mut owner_msg = vec![0u8; owner_msg_len];
        recv.read_exact(&mut owner_msg)
            .await
            .map_err(|e| CoreError::InvalidData(format!("read spake2 msg failed: {e}")))?;

        let _shared_key = invite::finish_handshake(invitee_state, &owner_msg)
            .map_err(|_| CoreError::InvalidData("SPAKE2 handshake failed — wrong code".into()))?;

        Ok::<_, CoreError>((send, recv, connection))
    })
    .await
    .map_err(|_| CoreError::InvalidData("invite resume timed out after 30s".into()))??;

    let mut final_status = [0u8; 1];
    recv.read_exact(&mut final_status)
        .await
        .map_err(|e| CoreError::InvalidData(format!("read final invite status failed: {e}")))?;
    if final_status[0] != 1 {
        let _ = send.finish();
        return Err(CoreError::InvalidData(
            "owner did not confirm committed invite acceptance".into(),
        ));
    }

    Ok(InviteAcceptanceSession {
        payload: InvitePayload {
            invite_id: String::new(),
            session_id: String::new(),
            project_id: String::new(),
            project_name: String::new(),
            role: String::new(),
            manifest_hex: String::new(),
            owner_x25519_public_hex: String::new(),
            epoch_key_hex: String::new(),
            epoch: 0,
        },
        peer_id: connection.remote_id(),
        send,
        recv,
    })
}

pub async fn resume_join_sessions(
    join_session_store: Arc<JoinSessionStore>,
    secret_cache: Arc<SessionSecretCache>,
    project_manager: Arc<ProjectManager>,
    sync_engine: Arc<SyncEngine>,
    peer_manager: Arc<PeerManager>,
    endpoint: Endpoint,
    app_handle: Option<AppHandle>,
) {
    let sessions = match join_session_store.load_all() {
        Ok(sessions) => sessions,
        Err(err) => {
            log::warn!("Failed to load persisted join sessions: {err}");
            return;
        }
    };

    for session in sessions {
        match session.stage {
            PersistedJoinStage::PayloadStaged { .. } => {
                emit_invite_accept_event(
                    app_handle.as_ref(),
                    notes_sync::events::InviteAcceptEvent {
                        stage: notes_sync::events::InviteAcceptStage::Resuming,
                        source: notes_sync::events::InviteAcceptSource::Resume,
                        session_id: session.session_id.clone(),
                        owner_peer_id: session.owner_peer_id.clone(),
                        project_id: session.project_id.clone(),
                        project_name: session.project_name.clone(),
                        local_project_name: Some(session.local_project_name.clone()),
                        role: session.role.clone(),
                        error: None,
                    },
                );
                let Some(secret) = secret_cache
                    .load_join_secret(&join_session_store, &session.session_id)
                    .ok()
                    .flatten()
                else {
                    log::warn!(
                        "Missing secret for staged join session {}; dropping",
                        session.session_id
                    );
                    delete_join_session(&join_session_store, &secret_cache, &session.session_id);
                    continue;
                };
                let mut payload: InvitePayload = match serde_json::from_str(&session.payload) {
                    Ok(payload) => payload,
                    Err(err) => {
                        log::warn!("Failed to decode staged join payload: {err}");
                        delete_join_session(
                            &join_session_store,
                            &secret_cache,
                            &session.session_id,
                        );
                        continue;
                    }
                };
                rehydrate_payload(&secret, &mut payload);
                let resume_session = match resume_owner_commit_status(
                    endpoint.clone(),
                    secret.passphrase.clone(),
                    session.owner_peer_id.clone(),
                )
                .await
                {
                    Ok(resume_session) => resume_session,
                    Err(err) => {
                        log::warn!("Failed to resume staged join session: {err}");
                        emit_invite_accept_event(
                            app_handle.as_ref(),
                            notes_sync::events::InviteAcceptEvent {
                                stage: notes_sync::events::InviteAcceptStage::Failed,
                                source: notes_sync::events::InviteAcceptSource::Resume,
                                session_id: session.session_id.clone(),
                                owner_peer_id: session.owner_peer_id.clone(),
                                project_id: session.project_id.clone(),
                                project_name: session.project_name.clone(),
                                local_project_name: Some(session.local_project_name.clone()),
                                role: session.role.clone(),
                                error: Some(err.to_string()),
                            },
                        );
                        continue;
                    }
                };
                if persist_commit_confirmed_payload(
                    &join_session_store,
                    &payload,
                    &session.owner_peer_id,
                    &session.local_project_name,
                )
                .is_err()
                {
                    continue;
                }
                let peer_id = match session.owner_peer_id.parse() {
                    Ok(peer_id) => peer_id,
                    Err(err) => {
                        log::warn!("Failed to parse persisted owner peer id: {err}");
                        continue;
                    }
                };
                let staged = StagedInviteInstall {
                    local_project_name: session.local_project_name.clone(),
                    payload: payload.clone(),
                    peer_id,
                    manifest_bytes: match stage_accepted_invite(
                        Arc::clone(&project_manager),
                        payload.clone(),
                        peer_id,
                    )
                    .await
                    {
                        Ok(staged) => staged.manifest_bytes,
                        Err(err) => {
                            log::warn!("Failed to stage resumed join session: {err}");
                            continue;
                        }
                    },
                };
                if let Ok((result, doc_ids)) = finalize_accepted_invite(
                    Arc::clone(&project_manager),
                    Arc::clone(&sync_engine),
                    Arc::clone(&peer_manager),
                    endpoint.clone(),
                    staged,
                )
                .await
                {
                    emit_invite_accept_event(
                        app_handle.as_ref(),
                        notes_sync::events::InviteAcceptEvent {
                            stage: notes_sync::events::InviteAcceptStage::Finalized,
                            source: notes_sync::events::InviteAcceptSource::Resume,
                            session_id: session.session_id.clone(),
                            owner_peer_id: session.owner_peer_id.clone(),
                            project_id: session.project_id.clone(),
                            project_name: session.project_name.clone(),
                            local_project_name: Some(session.local_project_name.clone()),
                            role: session.role.clone(),
                            error: None,
                        },
                    );
                    let _ = persist_finalized_session(
                        &join_session_store,
                        &payload.session_id,
                        &payload,
                        &session.owner_peer_id,
                        &session.local_project_name,
                    );
                    if send_applied_ack(resume_session).await.is_ok() {
                        delete_join_session(
                            &join_session_store,
                            &secret_cache,
                            &session.session_id,
                        );
                        emit_invite_accept_event(
                            app_handle.as_ref(),
                            notes_sync::events::InviteAcceptEvent {
                                stage: notes_sync::events::InviteAcceptStage::Completed,
                                source: notes_sync::events::InviteAcceptSource::Resume,
                                session_id: session.session_id.clone(),
                                owner_peer_id: session.owner_peer_id.clone(),
                                project_id: session.project_id.clone(),
                                project_name: session.project_name.clone(),
                                local_project_name: Some(session.local_project_name.clone()),
                                role: session.role.clone(),
                                error: None,
                            },
                        );
                    }
                    spawn_initial_invite_sync(
                        Arc::clone(&project_manager),
                        Arc::clone(&sync_engine),
                        Arc::clone(&peer_manager),
                        endpoint.id(),
                        result.project_name,
                        doc_ids,
                    );
                }
            }
            PersistedJoinStage::CommitConfirmed { .. } => {
                emit_invite_accept_event(
                    app_handle.as_ref(),
                    notes_sync::events::InviteAcceptEvent {
                        stage: notes_sync::events::InviteAcceptStage::Resuming,
                        source: notes_sync::events::InviteAcceptSource::Resume,
                        session_id: session.session_id.clone(),
                        owner_peer_id: session.owner_peer_id.clone(),
                        project_id: session.project_id.clone(),
                        project_name: session.project_name.clone(),
                        local_project_name: Some(session.local_project_name.clone()),
                        role: session.role.clone(),
                        error: None,
                    },
                );
                let Some(secret) = secret_cache
                    .load_join_secret(&join_session_store, &session.session_id)
                    .ok()
                    .flatten()
                else {
                    log::warn!(
                        "Missing secret for commit-confirmed join session {}; dropping",
                        session.session_id
                    );
                    delete_join_session(&join_session_store, &secret_cache, &session.session_id);
                    continue;
                };
                let mut payload: InvitePayload = match serde_json::from_str(&session.payload) {
                    Ok(payload) => payload,
                    Err(err) => {
                        log::warn!("Failed to decode persisted join payload: {err}");
                        delete_join_session(
                            &join_session_store,
                            &secret_cache,
                            &session.session_id,
                        );
                        continue;
                    }
                };
                rehydrate_payload(&secret, &mut payload);
                let peer_id = match session.owner_peer_id.parse() {
                    Ok(peer_id) => peer_id,
                    Err(err) => {
                        log::warn!("Failed to parse persisted owner peer id: {err}");
                        delete_join_session(
                            &join_session_store,
                            &secret_cache,
                            &session.session_id,
                        );
                        continue;
                    }
                };
                let staged = StagedInviteInstall {
                    local_project_name: session.local_project_name.clone(),
                    payload: payload.clone(),
                    peer_id,
                    manifest_bytes: match stage_accepted_invite(
                        Arc::clone(&project_manager),
                        payload,
                        peer_id,
                    )
                    .await
                    {
                        Ok(staged) => staged.manifest_bytes,
                        Err(err) => {
                            log::warn!("Failed to stage persisted join session: {err}");
                            delete_join_session(
                                &join_session_store,
                                &secret_cache,
                                &session.session_id,
                            );
                            continue;
                        }
                    },
                };
                if finalize_accepted_invite(
                    Arc::clone(&project_manager),
                    Arc::clone(&sync_engine),
                    Arc::clone(&peer_manager),
                    endpoint.clone(),
                    staged,
                )
                .await
                .map(|(result, doc_ids)| {
                    spawn_initial_invite_sync(
                        Arc::clone(&project_manager),
                        Arc::clone(&sync_engine),
                        Arc::clone(&peer_manager),
                        endpoint.id(),
                        result.project_name,
                        doc_ids,
                    );
                })
                .is_ok()
                {
                    delete_join_session(&join_session_store, &secret_cache, &session.session_id);
                    emit_invite_accept_event(
                        app_handle.as_ref(),
                        notes_sync::events::InviteAcceptEvent {
                            stage: notes_sync::events::InviteAcceptStage::Completed,
                            source: notes_sync::events::InviteAcceptSource::Resume,
                            session_id: session.session_id.clone(),
                            owner_peer_id: session.owner_peer_id.clone(),
                            project_id: session.project_id.clone(),
                            project_name: session.project_name.clone(),
                            local_project_name: Some(session.local_project_name.clone()),
                            role: session.role.clone(),
                            error: None,
                        },
                    );
                }
            }
            PersistedJoinStage::Finalized { .. } => {
                emit_invite_accept_event(
                    app_handle.as_ref(),
                    notes_sync::events::InviteAcceptEvent {
                        stage: notes_sync::events::InviteAcceptStage::Resuming,
                        source: notes_sync::events::InviteAcceptSource::Resume,
                        session_id: session.session_id.clone(),
                        owner_peer_id: session.owner_peer_id.clone(),
                        project_id: session.project_id.clone(),
                        project_name: session.project_name.clone(),
                        local_project_name: Some(session.local_project_name.clone()),
                        role: session.role.clone(),
                        error: None,
                    },
                );
                if let Ok(Some(secret)) =
                    secret_cache.load_join_secret(&join_session_store, &session.session_id)
                {
                    if let Ok(resume_session) = resume_owner_commit_status(
                        endpoint.clone(),
                        secret.passphrase,
                        session.owner_peer_id.clone(),
                    )
                    .await
                    {
                        if send_applied_ack(resume_session).await.is_ok() {
                            delete_join_session(
                                &join_session_store,
                                &secret_cache,
                                &session.session_id,
                            );
                            emit_invite_accept_event(
                                app_handle.as_ref(),
                                notes_sync::events::InviteAcceptEvent {
                                    stage: notes_sync::events::InviteAcceptStage::Completed,
                                    source: notes_sync::events::InviteAcceptSource::Resume,
                                    session_id: session.session_id.clone(),
                                    owner_peer_id: session.owner_peer_id.clone(),
                                    project_id: session.project_id.clone(),
                                    project_name: session.project_name.clone(),
                                    local_project_name: Some(session.local_project_name.clone()),
                                    role: session.role.clone(),
                                    error: None,
                                },
                            );
                        }
                    }
                }
                if let Ok(files) = project_manager
                    .list_files(&session.local_project_name)
                    .await
                {
                    let doc_ids = files.into_iter().map(|file| file.id).collect::<Vec<_>>();
                    spawn_initial_invite_sync(
                        Arc::clone(&project_manager),
                        Arc::clone(&sync_engine),
                        Arc::clone(&peer_manager),
                        endpoint.id(),
                        session.local_project_name.clone(),
                        doc_ids,
                    );
                }
            }
        }
    }
}

pub async fn receive_invite_payload_session(
    endpoint: Endpoint,
    passphrase: String,
    owner_peer_id: String,
) -> Result<InviteAcceptanceSession, CoreError> {
    use notes_sync::invite;

    let peer_id: iroh::EndpointId = owner_peer_id
        .parse()
        .map_err(|e| CoreError::InvalidInput(format!("invalid owner peer ID: {e}")))?;

    let (send, recv, connection, payload) = tokio::time::timeout(Duration::from_secs(30), async {
        let connection = endpoint
            .connect(peer_id, invite::INVITE_ALPN)
            .await
            .map_err(|e| CoreError::InvalidInput(format!("connection failed: {e}")))?;

        let (mut send, mut recv) = connection
            .open_bi()
            .await
            .map_err(|e| CoreError::InvalidData(format!("stream open failed: {e}")))?;

        let (invitee_state, invitee_msg) = invite::start_invitee_handshake(&passphrase);
        let len = (invitee_msg.len() as u32).to_be_bytes();
        send.write_all(&len)
            .await
            .map_err(|e| CoreError::InvalidData(format!("send spake2 len failed: {e}")))?;
        send.write_all(&invitee_msg)
            .await
            .map_err(|e| CoreError::InvalidData(format!("send spake2 msg failed: {e}")))?;

        let mut owner_msg_len_buf = [0u8; 4];
        recv.read_exact(&mut owner_msg_len_buf)
            .await
            .map_err(|e| CoreError::InvalidData(format!("read spake2 len failed: {e}")))?;
        let owner_msg_len = u32::from_be_bytes(owner_msg_len_buf) as usize;
        if owner_msg_len > 256 {
            return Err(CoreError::InvalidData("SPAKE2 message too large".into()));
        }
        let mut owner_msg = vec![0u8; owner_msg_len];
        recv.read_exact(&mut owner_msg)
            .await
            .map_err(|e| CoreError::InvalidData(format!("read spake2 msg failed: {e}")))?;

        let shared_key = invite::finish_handshake(invitee_state, &owner_msg)
            .map_err(|_| CoreError::InvalidData("SPAKE2 handshake failed — wrong code".into()))?;

        let mut len_buf = [0u8; 4];
        recv.read_exact(&mut len_buf)
            .await
            .map_err(|e| CoreError::InvalidData(format!("read length failed: {e}")))?;
        let payload_len = u32::from_be_bytes(len_buf) as usize;
        if payload_len > 16 * 1024 * 1024 {
            return Err(CoreError::InvalidData("invite payload too large".into()));
        }

        let mut encrypted = vec![0u8; payload_len];
        recv.read_exact(&mut encrypted)
            .await
            .map_err(|e| CoreError::InvalidData(format!("read payload failed: {e}")))?;

        let plaintext = invite::decrypt_payload(&shared_key, &encrypted)
            .map_err(|e| CoreError::InvalidData(format!("decrypt failed — wrong code: {e}")))?;

        {
            use zeroize::Zeroize;
            let mut key_to_zeroize = shared_key;
            key_to_zeroize.zeroize();
        }

        let payload: InvitePayload = serde_json::from_slice(&plaintext)
            .map_err(|e| CoreError::InvalidData(format!("invalid payload: {e}")))?;

        Ok::<_, CoreError>((send, recv, connection, payload))
    })
    .await
    .map_err(|_| CoreError::InvalidData("invite timed out after 30s".into()))??;

    Ok(InviteAcceptanceSession {
        payload,
        peer_id: connection.remote_id(),
        send,
        recv,
    })
}

pub async fn await_owner_commit_result(
    mut session: InviteAcceptanceSession,
    join_session_store: &JoinSessionStore,
    secret_cache: &SessionSecretCache,
    local_project_name: &str,
) -> Result<InviteAcceptanceSession, CoreError> {
    use tokio::io::AsyncWriteExt;

    session
        .send
        .write_all(&[1])
        .await
        .map_err(|e| CoreError::InvalidData(format!("send prepared ack failed: {e}")))?;
    session
        .send
        .flush()
        .await
        .map_err(|e| CoreError::InvalidData(format!("flush prepared ack failed: {e}")))?;

    let mut final_status = [0u8; 1];
    tokio::time::timeout(
        Duration::from_secs(30),
        session.recv.read_exact(&mut final_status),
    )
    .await
    .map_err(|_| CoreError::InvalidData("owner finalize timed out after 30s".into()))?
    .map_err(|e| CoreError::InvalidData(format!("read final invite status failed: {e}")))?;
    if final_status[0] != 1 {
        let _ = session.send.finish();
        return Err(CoreError::InvalidData(
            "owner failed to finalize invite acceptance".into(),
        ));
    }

    persist_commit_confirmed_payload(
        join_session_store,
        &session.payload,
        &session.peer_id.to_string(),
        local_project_name,
    )?;
    if let Some(secret) =
        secret_cache.load_join_secret(join_session_store, &session.payload.session_id)?
    {
        secret_cache.cache_join_secret(&session.payload.session_id, secret);
    }

    Ok(session)
}

pub async fn send_applied_ack(mut session: InviteAcceptanceSession) -> Result<(), CoreError> {
    use tokio::io::AsyncWriteExt;

    session
        .send
        .write_all(&[1])
        .await
        .map_err(|e| CoreError::InvalidData(format!("send applied ack failed: {e}")))?;
    session
        .send
        .flush()
        .await
        .map_err(|e| CoreError::InvalidData(format!("flush applied ack failed: {e}")))?;
    let _ = session.send.finish();
    Ok(())
}

pub async fn stage_accepted_invite(
    project_manager: Arc<ProjectManager>,
    payload: InvitePayload,
    peer_id: iroh::EndpointId,
) -> Result<StagedInviteInstall, CoreError> {
    if payload.manifest_hex.len() % 2 != 0 {
        return Err(CoreError::InvalidData("manifest hex has odd length".into()));
    }
    let manifest_bytes: Vec<u8> = (0..payload.manifest_hex.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&payload.manifest_hex[i..i + 2], 16)
                .map_err(|_| CoreError::InvalidData("manifest hex is invalid".into()))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let local_project_name =
        allocate_local_project_name(&project_manager, &payload.project_name).await?;

    Ok(StagedInviteInstall {
        local_project_name,
        payload,
        peer_id,
        manifest_bytes,
    })
}

pub async fn finalize_accepted_invite(
    project_manager: Arc<ProjectManager>,
    sync_engine: Arc<SyncEngine>,
    peer_manager: Arc<PeerManager>,
    endpoint: Endpoint,
    staged: StagedInviteInstall,
) -> Result<(AcceptInviteResult, Vec<DocId>), CoreError> {
    let StagedInviteInstall {
        local_project_name,
        payload,
        peer_id,
        manifest_bytes,
    } = staged;
    let pm = Arc::clone(&project_manager);
    let owner_pk_bytes = if payload.owner_x25519_public_hex.is_empty() {
        None
    } else {
        Some(
            hex_decode_32(&payload.owner_x25519_public_hex).map_err(|_| {
                CoreError::InvalidData("invalid owner public key in invite payload".into())
            })?,
        )
    };
    let epoch_key_bytes =
        if payload.epoch_key_hex.is_empty() {
            None
        } else {
            Some(hex_decode_32(&payload.epoch_key_hex).map_err(|_| {
                CoreError::InvalidData("invalid epoch key in invite payload".into())
            })?)
        };
    let shared_payload = if manifest_bytes.is_empty() {
        false
    } else {
        let manifest = ProjectManifest::load(&manifest_bytes)?;
        !manifest.get_owner().unwrap_or_default().is_empty() || !manifest.list_peers()?.is_empty()
    };
    if shared_payload && (owner_pk_bytes.is_none() || epoch_key_bytes.is_none()) {
        return Err(CoreError::InvalidData(
            "shared invite payload is missing required crypto material".into(),
        ));
    }

    pm.create_project(&local_project_name).await.or_else(|e| {
        if matches!(e, CoreError::ProjectAlreadyExists(_)) {
            Ok(())
        } else {
            Err(e)
        }
    })?;

    if !manifest_bytes.is_empty() {
        pm.persistence()
            .save_manifest(&local_project_name, &manifest_bytes)
            .await?;
        pm.reload_manifest(&local_project_name).await?;
    }

    peer_manager.add_peer_to_project(&local_project_name, peer_id);

    {
        let keys_dir = pm
            .persistence()
            .project_dir(&local_project_name)
            .join(".p2p")
            .join("keys");
        std::fs::create_dir_all(&keys_dir).ok();
        let keystore = notes_crypto::KeyStore::new(keys_dir);

        if let Some(owner_pk_bytes) = owner_pk_bytes {
            keystore
                .store_key("owner-x25519-public", &owner_pk_bytes)
                .ok();
        }

        let _ = pm
            .get_or_create_project_x25519_identity(&local_project_name)
            .await?;

        if let Some(epoch_key_bytes) = epoch_key_bytes {
            let mgr = notes_crypto::EpochKeyManager::from_key(payload.epoch, &epoch_key_bytes);
            pm.install_epoch_keys(&local_project_name, mgr).await?;
        }
    }

    let doc_ids = register_remote_project_sync_objects(
        &project_manager,
        &sync_engine,
        &endpoint.id(),
        &local_project_name,
    )
    .await?;

    Ok((
        AcceptInviteResult {
            project_id: payload.project_id,
            project_name: local_project_name,
            role: payload.role,
        },
        doc_ids,
    ))
}

async fn allocate_local_project_name(
    project_manager: &ProjectManager,
    base_name: &str,
) -> Result<String, CoreError> {
    if !project_manager
        .persistence()
        .is_initialized(base_name)
        .await
    {
        return Ok(base_name.to_string());
    }

    for suffix in 1..=999 {
        let candidate = format!("{base_name}-{suffix}");
        if !project_manager
            .persistence()
            .is_initialized(&candidate)
            .await
        {
            return Ok(candidate);
        }
    }

    Err(CoreError::ProjectAlreadyExists(base_name.to_string()))
}

pub fn spawn_initial_invite_sync(
    project_manager: Arc<ProjectManager>,
    sync_engine: Arc<SyncEngine>,
    peer_manager: Arc<PeerManager>,
    local_peer_id: iroh::EndpointId,
    project_name: String,
    doc_ids: Vec<DocId>,
) {
    tauri::async_runtime::spawn(async move {
        let _ = hydrate_missing_doc_snapshots(
            &project_manager,
            &sync_engine,
            &peer_manager,
            &local_peer_id,
            &project_name,
            &doc_ids,
        )
        .await;

        for doc_id in doc_ids {
            let mut ok = 0;
            for attempt in 0..10 {
                let results = peer_manager
                    .sync_doc_with_project_peers(&project_name, doc_id)
                    .await;
                ok = results.iter().filter(|(_, r)| r.is_ok()).count();
                if ok > 0 {
                    break;
                }
                let delay_ms = 200 + (attempt * 100);
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
            if ok > 0 {
                let _ = project_manager.save_doc(&project_name, &doc_id).await;
            }
        }
    });
}

pub async fn perform_initial_invite_sync(
    project_manager: Arc<ProjectManager>,
    sync_engine: Arc<SyncEngine>,
    peer_manager: Arc<PeerManager>,
    local_peer_id: iroh::EndpointId,
    project_name: &str,
    doc_ids: &[DocId],
) {
    let _ = hydrate_missing_doc_snapshots(
        &project_manager,
        &sync_engine,
        &peer_manager,
        &local_peer_id,
        project_name,
        doc_ids,
    )
    .await;

    for doc_id in doc_ids {
        let mut ok = 0;
        for attempt in 0..10 {
            let results = peer_manager
                .sync_doc_with_project_peers(project_name, *doc_id)
                .await;
            ok = results.iter().filter(|(_, r)| r.is_ok()).count();
            if ok > 0 {
                break;
            }
            let delay_ms = 200 + (attempt * 100);
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }
        if ok > 0 {
            let _ = project_manager.save_doc(project_name, doc_id).await;
        }
    }
}

pub async fn accept_invite_impl(
    project_manager: Arc<ProjectManager>,
    sync_engine: Arc<SyncEngine>,
    peer_manager: Arc<PeerManager>,
    join_session_store: Arc<JoinSessionStore>,
    secret_cache: Arc<SessionSecretCache>,
    endpoint: Endpoint,
    app_handle: Option<AppHandle>,
    passphrase: String,
    owner_peer_id: String,
) -> Result<AcceptInviteResult, CoreError> {
    let started_at = std::time::Instant::now();
    log::info!("accept_invite_impl: start");
    #[cfg(test)]
    eprintln!("accept_invite_impl: start");
    let session =
        receive_invite_payload_session(endpoint.clone(), passphrase.clone(), owner_peer_id).await?;
    log::info!(
        "accept_invite_impl: payload received after {}ms",
        started_at.elapsed().as_millis()
    );
    #[cfg(test)]
    eprintln!(
        "accept_invite_impl: payload received after {}ms",
        started_at.elapsed().as_millis()
    );
    let session_id = session.payload.session_id.clone();
    let staged = stage_accepted_invite(
        Arc::clone(&project_manager),
        session.payload.clone(),
        session.peer_id,
    )
    .await?;
    log::info!(
        "accept_invite_impl: payload staged after {}ms",
        started_at.elapsed().as_millis()
    );
    #[cfg(test)]
    eprintln!(
        "accept_invite_impl: payload staged after {}ms",
        started_at.elapsed().as_millis()
    );
    let local_project_name = staged.local_project_name.clone();
    persist_payload_staged_session(
        &join_session_store,
        &secret_cache,
        &session.payload,
        &session.peer_id.to_string(),
        &local_project_name,
        &passphrase,
    )?;
    emit_invite_accept_event(
        app_handle.as_ref(),
        notes_sync::events::InviteAcceptEvent {
            stage: notes_sync::events::InviteAcceptStage::PayloadStaged,
            source: notes_sync::events::InviteAcceptSource::Interactive,
            session_id: session.payload.session_id.clone(),
            owner_peer_id: session.peer_id.to_string(),
            project_id: session.payload.project_id.clone(),
            project_name: session.payload.project_name.clone(),
            local_project_name: Some(local_project_name.clone()),
            role: session.payload.role.clone(),
            error: None,
        },
    );
    let session = await_owner_commit_result(
        session,
        &join_session_store,
        &secret_cache,
        &local_project_name,
    )
    .await?;
    log::info!(
        "accept_invite_impl: owner commit confirmed after {}ms",
        started_at.elapsed().as_millis()
    );
    #[cfg(test)]
    eprintln!(
        "accept_invite_impl: owner commit confirmed after {}ms",
        started_at.elapsed().as_millis()
    );
    let local_peer_id = endpoint.id();
    let (result, doc_ids) = finalize_accepted_invite(
        Arc::clone(&project_manager),
        Arc::clone(&sync_engine),
        Arc::clone(&peer_manager),
        endpoint,
        staged,
    )
    .await?;
    log::info!(
        "accept_invite_impl: finalize complete after {}ms ({} docs)",
        started_at.elapsed().as_millis(),
        doc_ids.len()
    );
    #[cfg(test)]
    eprintln!(
        "accept_invite_impl: finalize complete after {}ms ({} docs)",
        started_at.elapsed().as_millis(),
        doc_ids.len()
    );
    emit_invite_accept_event(
        app_handle.as_ref(),
        notes_sync::events::InviteAcceptEvent {
            stage: notes_sync::events::InviteAcceptStage::Finalized,
            source: notes_sync::events::InviteAcceptSource::Interactive,
            session_id: session.payload.session_id.clone(),
            owner_peer_id: session.peer_id.to_string(),
            project_id: session.payload.project_id.clone(),
            project_name: session.payload.project_name.clone(),
            local_project_name: Some(result.project_name.clone()),
            role: result.role.clone(),
            error: None,
        },
    );
    persist_finalized_session(
        &join_session_store,
        &session.payload.session_id,
        &session.payload,
        &session.peer_id.to_string(),
        &local_project_name,
    )?;
    let owner_peer_id_str = session.peer_id.to_string();
    if let Err(err) = send_applied_ack(session).await {
        log::warn!(
            "Invite finalize succeeded for project {} but applied ack failed: {err}",
            result.project_name
        );
        emit_invite_accept_event(
            app_handle.as_ref(),
            notes_sync::events::InviteAcceptEvent {
                stage: notes_sync::events::InviteAcceptStage::Failed,
                source: notes_sync::events::InviteAcceptSource::Interactive,
                session_id,
                owner_peer_id: owner_peer_id_str.clone(),
                project_id: result.project_id.clone(),
                project_name: result.project_name.clone(),
                local_project_name: Some(result.project_name.clone()),
                role: result.role.clone(),
                error: Some(err.to_string()),
            },
        );
    } else {
        delete_join_session(&join_session_store, &secret_cache, &session_id);
        emit_invite_accept_event(
            app_handle.as_ref(),
            notes_sync::events::InviteAcceptEvent {
                stage: notes_sync::events::InviteAcceptStage::Completed,
                source: notes_sync::events::InviteAcceptSource::Interactive,
                session_id,
                owner_peer_id: owner_peer_id_str,
                project_id: result.project_id.clone(),
                project_name: result.project_name.clone(),
                local_project_name: Some(result.project_name.clone()),
                role: result.role.clone(),
                error: None,
            },
        );
    }
    log::info!(
        "accept_invite_impl: returning success after {}ms",
        started_at.elapsed().as_millis()
    );
    #[cfg(test)]
    eprintln!(
        "accept_invite_impl: returning success after {}ms",
        started_at.elapsed().as_millis()
    );
    spawn_initial_invite_sync(
        project_manager,
        sync_engine,
        peer_manager,
        local_peer_id,
        result.project_name.clone(),
        doc_ids,
    );
    Ok(result)
}

pub fn hex_decode_32(hex: &str) -> Result<[u8; 32], CoreError> {
    if hex.len() != 64 {
        return Err(CoreError::InvalidData(format!(
            "expected 64 hex chars, got {}",
            hex.len()
        )));
    }
    let bytes: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&hex[i..i + 2], 16)
                .map_err(|_| CoreError::InvalidData("bad hex".into()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Ok(arr)
}

pub async fn populate_doc_acl_from_parts(
    project_manager: &ProjectManager,
    sync_engine: &SyncEngine,
    local_peer_id: &iroh::EndpointId,
    project: &str,
    doc_id: DocId,
) {
    use notes_sync::sync_engine::PeerRole as SyncPeerRole;

    sync_engine.set_peer_role(doc_id, *local_peer_id, SyncPeerRole::Owner);

    if let Ok(peers) = project_manager.get_project_peers(project).await {
        for peer in peers {
            if let Ok(peer_id) = peer.peer_id.parse::<iroh::EndpointId>() {
                let sync_role = match peer.role {
                    PeerRole::Owner => SyncPeerRole::Owner,
                    PeerRole::Editor => SyncPeerRole::Editor,
                    PeerRole::Viewer => SyncPeerRole::Viewer,
                };
                sync_engine.set_peer_role(doc_id, peer_id, sync_role);
            }
        }
    }

    if let Ok(manifest_arc) = project_manager.get_manifest_for_ui(project) {
        let manifest = manifest_arc.read().await;
        if let Ok(aliases) = manifest.get_actor_aliases() {
            let mut known_actors: std::collections::HashSet<String> =
                aliases.keys().cloned().collect();
            if let Some(actor) = project_manager.doc_store().device_actor_hex() {
                known_actors.insert(actor);
            }
            if !known_actors.is_empty() {
                sync_engine.set_known_actors(doc_id, known_actors);
            }
        }
    }
}

pub async fn register_project_sync_objects(
    project_manager: &ProjectManager,
    sync_engine: &SyncEngine,
    local_peer_id: &iroh::EndpointId,
    project: &str,
) -> Result<Vec<DocId>, CoreError> {
    project_manager
        .ensure_local_actor_binding(project, &local_peer_id.to_string())
        .await?;
    let manifest_doc_id = project_manager.ensure_manifest_doc_loaded(project).await?;
    let registered_docs = project_manager
        .apply_manifest_doc_to_project(project, &manifest_doc_id)
        .await?;

    let mut all_doc_ids = Vec::with_capacity(registered_docs.len() + 1);
    all_doc_ids.push(manifest_doc_id);
    all_doc_ids.extend(registered_docs);

    for doc_id in &all_doc_ids {
        if let Ok(doc_arc) = project_manager.doc_store().get_doc(doc_id) {
            sync_engine.register_doc(*doc_id, doc_arc);
            populate_doc_acl_from_parts(
                project_manager,
                sync_engine,
                local_peer_id,
                project,
                *doc_id,
            )
            .await;
        }
    }

    Ok(all_doc_ids)
}

async fn register_remote_project_sync_objects(
    project_manager: &ProjectManager,
    sync_engine: &SyncEngine,
    local_peer_id: &iroh::EndpointId,
    project: &str,
) -> Result<Vec<DocId>, CoreError> {
    project_manager
        .ensure_local_actor_binding(project, &local_peer_id.to_string())
        .await?;
    let manifest_doc_id = project_manager.ensure_manifest_doc_loaded(project).await?;
    let registered_docs = project_manager
        .apply_remote_manifest_doc_to_project(project, &manifest_doc_id)
        .await?;

    let mut all_doc_ids = Vec::with_capacity(registered_docs.len() + 1);
    all_doc_ids.push(manifest_doc_id);
    all_doc_ids.extend(registered_docs);

    for doc_id in &all_doc_ids {
        if let Ok(doc_arc) = project_manager.doc_store().get_doc(doc_id) {
            sync_engine.register_doc(*doc_id, doc_arc);
            populate_doc_acl_from_parts(
                project_manager,
                sync_engine,
                local_peer_id,
                project,
                *doc_id,
            )
            .await;
        }
    }

    Ok(all_doc_ids)
}

pub async fn hydrate_missing_doc_snapshots(
    project_manager: &ProjectManager,
    sync_engine: &SyncEngine,
    peer_manager: &PeerManager,
    local_peer_id: &iroh::EndpointId,
    project: &str,
    doc_ids: &[DocId],
) -> Result<(), CoreError> {
    for doc_id in doc_ids {
        if project_manager.doc_snapshot_exists(project, doc_id).await? {
            continue;
        }

        let results = peer_manager
            .fetch_doc_snapshot_from_project_peers(project, *doc_id)
            .await;
        let snapshot = results.into_iter().find_map(|(_, result)| result.ok());
        if let Some(snapshot) = snapshot {
            project_manager
                .doc_store()
                .replace_doc(*doc_id, &snapshot)
                .await?;
            project_manager.save_doc(project, doc_id).await?;
            if let Ok(doc_arc) = project_manager.doc_store().get_doc(doc_id) {
                sync_engine.register_doc(*doc_id, doc_arc);
                populate_doc_acl_from_parts(
                    project_manager,
                    sync_engine,
                    local_peer_id,
                    project,
                    *doc_id,
                )
                .await;
            }
        } else {
            log::warn!(
                "No peer snapshot available for project {project} doc {doc_id}; keeping placeholder"
            );
        }
    }
    Ok(())
}
