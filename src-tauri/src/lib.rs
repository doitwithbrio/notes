use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

pub mod invite_accept;

use iroh::endpoint::Endpoint;
use iroh::protocol::Router;
use notes_core::{
    CoreError, DocId, DocInfo, JoinSessionStore, OwnerInviteStateStore, PeerRole, ProjectManager,
    ProjectPeerSummary, ProjectSummary,
};
use notes_sync::events;
use notes_sync::invite::{InviteHandler, INVITE_ALPN};
use notes_sync::peer_manager::PeerManager;
use notes_sync::presence::{
    ApplyOutcome, PresenceUpdate, PRESENCE_PROTOCOL_VERSION, PRESENCE_TTL_MS,
};
use notes_sync::sync_engine::{SyncEngine, NOTES_SYNC_ALPN};
use notes_sync::{PresenceManager, SyncStateStore, GOSSIP_ALPN};
use serde::{Deserialize, Serialize};
use tauri::ipc::{Channel, InvokeResponseBody, Response};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State};
use tauri_plugin_updater::UpdaterExt;

use crate::invite_accept::{
    accept_invite_impl, list_owner_invites_from_store, list_pending_join_resumes,
    populate_doc_acl_from_parts, register_project_sync_objects, resume_join_sessions,
    AcceptInviteResult, OwnerInviteCoordinator, OwnerInvitePersistence, OwnerInviteStatus,
    PendingJoinResumeStatus, ProjectSyncObserver, ProjectSyncResolverImpl, SessionSecretCache,
};

/// Shared app state accessible from all Tauri commands.
struct AppState {
    project_manager: Arc<ProjectManager>,
    sync_engine: Arc<SyncEngine>,
    peer_manager: Arc<PeerManager>,
    invite_handler: Arc<InviteHandler>,
    #[allow(dead_code)]
    owner_invite_store: Arc<OwnerInviteStateStore>,
    join_session_store: Arc<JoinSessionStore>,
    session_secret_cache: Arc<SessionSecretCache>,
    #[allow(dead_code)] // Used by sync sessions — wired via SyncEngine in Phase 2+
    sync_state_store: Arc<SyncStateStore>,
    search_index: Arc<std::sync::Mutex<notes_core::SearchIndex>>,
    version_store: Arc<std::sync::Mutex<notes_core::VersionStore>>,
    blob_store: Arc<notes_sync::blobs::BlobStore>,
    presence_manager: Arc<PresenceManager>,
    presence_session_id: String,
    presence_session_started_at: u64,
    presence_seq: Arc<Mutex<HashMap<String, u64>>>,
    /// Stable device actor ID (hex string) for the frontend to use.
    device_actor_hex: String,
    /// Stable local peer ID available before router startup completes.
    local_peer_id: String,
    /// Persistent local secret key used for signing and endpoint binding.
    secret_key: iroh::SecretKey,
    /// Channel to trigger auto-sync when documents change.
    sync_trigger: tokio::sync::mpsc::Sender<(String, DocId)>,
    /// Receiver consumed once during deferred networking startup.
    sync_receiver: Mutex<Option<tokio::sync::mpsc::Receiver<(String, DocId)>>>,
    /// Local-vs-synced change counters per doc for truthful unsent tracking.
    unsent_changes: Arc<Mutex<HashMap<DocId, UnsentChangesState>>>,
    endpoint: Endpoint,
    router: Mutex<Option<Router>>,
    network_status: RwLock<NetworkStatus>,
    app_handle: tauri::AppHandle,
}

#[derive(Serialize)]
struct DebugSecretReadEvent {
    phase: &'static str,
    class: &'static str,
    backend: &'static str,
    outcome: &'static str,
}

#[derive(Serialize)]
struct DebugSecretReadStats {
    enabled: bool,
    phase: &'static str,
    startup_reads: usize,
    runtime_reads: usize,
    cache_hits: usize,
    cache_misses: usize,
    events: Vec<DebugSecretReadEvent>,
}

fn secret_read_debug_enabled() -> bool {
    std::env::var("NOTES_DEBUG_SECRET_READS")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn secret_read_phase_label(phase: notes_crypto::SecretReadPhase) -> &'static str {
    match phase {
        notes_crypto::SecretReadPhase::Startup => "startup",
        notes_crypto::SecretReadPhase::Runtime => "runtime",
    }
}

fn secret_read_class_label(class: notes_crypto::SecretReadClass) -> &'static str {
    match class {
        notes_crypto::SecretReadClass::Unknown => "unknown",
        notes_crypto::SecretReadClass::PeerIdentity => "peer_identity",
        notes_crypto::SecretReadClass::ProjectEpochKeys => "project_epoch_keys",
        notes_crypto::SecretReadClass::ProjectX25519Identity => "project_x25519_identity",
        notes_crypto::SecretReadClass::OwnerInvitePassphrase => "owner_invite_passphrase",
        notes_crypto::SecretReadClass::JoinSessionSecret => "join_session_secret",
    }
}

fn secret_read_backend_label(backend: notes_crypto::SecretReadBackend) -> &'static str {
    match backend {
        notes_crypto::SecretReadBackend::Keychain => "keychain",
        notes_crypto::SecretReadBackend::File => "file",
        notes_crypto::SecretReadBackend::LegacyKeychain => "legacy_keychain",
        notes_crypto::SecretReadBackend::LegacyFile => "legacy_file",
    }
}

fn secret_read_outcome_label(outcome: notes_crypto::SecretReadOutcome) -> &'static str {
    match outcome {
        notes_crypto::SecretReadOutcome::Hit => "hit",
        notes_crypto::SecretReadOutcome::Miss => "miss",
        notes_crypto::SecretReadOutcome::Error => "error",
    }
}

fn snapshot_secret_read_stats() -> DebugSecretReadStats {
    let stats = notes_crypto::debug_get_secret_read_stats();
    DebugSecretReadStats {
        enabled: stats.enabled,
        phase: secret_read_phase_label(stats.phase),
        startup_reads: stats.startup_reads,
        runtime_reads: stats.runtime_reads,
        cache_hits: stats.cache_hits,
        cache_misses: stats.cache_misses,
        events: stats
            .events
            .into_iter()
            .map(|event| DebugSecretReadEvent {
                phase: secret_read_phase_label(event.phase),
                class: secret_read_class_label(event.class),
                backend: secret_read_backend_label(event.backend),
                outcome: secret_read_outcome_label(event.outcome),
            })
            .collect(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NetworkStatus {
    NotStarted,
    Starting,
    Ready,
    Failed(String),
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn project_eviction_dir(base_dir: &std::path::Path) -> std::path::PathBuf {
    base_dir.join(".p2p").join("project-evictions")
}

fn normalize_project_eviction_id(project_id: &str) -> Result<String, CoreError> {
    Ok(uuid::Uuid::parse_str(project_id)
        .map_err(|_| CoreError::InvalidInput("invalid project eviction id".into()))?
        .to_string())
}

fn project_eviction_path(
    base_dir: &std::path::Path,
    project_id: &str,
) -> Result<std::path::PathBuf, CoreError> {
    Ok(project_eviction_dir(base_dir).join(format!(
        "{}.json",
        normalize_project_eviction_id(project_id)?
    )))
}

async fn save_project_eviction_notice(
    base_dir: &std::path::Path,
    notice: &ProjectEvictionNotice,
) -> Result<(), CoreError> {
    let dir = project_eviction_dir(base_dir);
    tokio::fs::create_dir_all(&dir).await?;
    let path = project_eviction_path(base_dir, &notice.project_id)?;
    let tmp = path.with_extension("json.tmp");
    tokio::fs::write(&tmp, serde_json::to_vec_pretty(notice)?).await?;
    tokio::fs::rename(tmp, path).await?;
    Ok(())
}

async fn list_project_eviction_notices_from_disk(
    base_dir: &std::path::Path,
) -> Result<Vec<ProjectEvictionNotice>, CoreError> {
    let dir = project_eviction_dir(base_dir);
    let mut notices = Vec::new();
    let mut entries = match tokio::fs::read_dir(&dir).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(notices),
        Err(err) => return Err(CoreError::Io(err)),
    };
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let raw = tokio::fs::read(&path).await?;
        if let Ok(notice) = serde_json::from_slice::<ProjectEvictionNotice>(&raw) {
            notices.push(notice);
        }
    }
    notices.sort_by(|a, b| a.project_name.cmp(&b.project_name));
    Ok(notices)
}

async fn dismiss_project_eviction_notice_on_disk(
    base_dir: &std::path::Path,
    project_id: &str,
) -> Result<(), CoreError> {
    match tokio::fs::remove_file(project_eviction_path(base_dir, project_id)?).await {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(CoreError::Io(err)),
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct UnsentChangesState {
    local_changes: u32,
    synced_changes: u32,
}

fn require_version_store(
    state: &AppState,
) -> Result<Arc<std::sync::Mutex<notes_core::VersionStore>>, CoreError> {
    Ok(Arc::clone(&state.version_store))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GenerateInviteResult {
    invite_id: String,
    passphrase: String,
    peer_id: String,
    expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectEvictionNotice {
    project_id: String,
    project_name: String,
    reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReconnectQueueStats {
    attempted: usize,
    queued: usize,
    dropped: usize,
}

fn reconnect_peer_id(status_event: &events::PeerStatusEvent) -> Option<iroh::EndpointId> {
    if !matches!(
        status_event.state,
        notes_sync::events::PeerConnectionState::Connected
    ) {
        return None;
    }
    status_event.peer_id.parse::<iroh::EndpointId>().ok()
}

fn try_queue_reconnect_syncs(
    sync_tx: &tokio::sync::mpsc::Sender<(String, DocId)>,
    project_name: &str,
    files: &[DocInfo],
) -> ReconnectQueueStats {
    let mut stats = ReconnectQueueStats {
        attempted: files.len(),
        queued: 0,
        dropped: 0,
    };

    for file in files {
        if sync_tx
            .try_send((project_name.to_string(), file.id))
            .is_ok()
        {
            stats.queued += 1;
        } else {
            stats.dropped += 1;
        }
    }

    stats
}

fn pending_unsent_changes(state: &AppState, doc_id: DocId) -> u32 {
    state
        .unsent_changes
        .lock()
        .ok()
        .and_then(|map| {
            map.get(&doc_id)
                .map(|entry| entry.local_changes.saturating_sub(entry.synced_changes))
        })
        .unwrap_or(0)
}

fn emit_presence_event(app_handle: &AppHandle, update: &PresenceUpdate) {
    let _ = app_handle.emit(
        events::event_names::PRESENCE_UPDATE,
        events::PresenceEvent {
            project_id: update.project_id.clone(),
            peer_id: update.peer_id.clone(),
            session_id: update.session_id.clone(),
            session_started_at: update.session_started_at,
            seq: update.seq,
            alias: update.alias.clone(),
            active_doc: update.active_doc,
            cursor_pos: update.cursor_pos,
            selection: update.selection,
        },
    );
}

fn emit_project_evicted_event(app_handle: &AppHandle, notice: &ProjectEvictionNotice) {
    let _ = app_handle.emit(
        events::event_names::PROJECT_EVICTED,
        events::ProjectEvictedEvent {
            project_id: notice.project_id.clone(),
            project_name: notice.project_name.clone(),
            reason: notice.reason.clone(),
        },
    );
}

async fn ensure_project_presence_subscription(
    state: &AppState,
    project_name: &str,
) -> Result<String, CoreError> {
    let project_id = state.project_manager.get_project_id(project_name).await?;
    if project_id.trim().is_empty() {
        return Err(CoreError::InvalidData("manifest missing project id".into()));
    }

    let roster = state
        .project_manager
        .get_project_peer_roster(project_name, &state.local_peer_id)
        .await?;
    let bootstrap_peers = roster
        .into_iter()
        .filter(|peer| !peer.is_self)
        .filter_map(|peer| peer.peer_id.parse::<iroh::EndpointId>().ok())
        .collect::<Vec<_>>();
    state
        .presence_manager
        .ensure_joined(&project_id, bootstrap_peers)
        .await
        .map_err(|err| CoreError::InvalidData(err.to_string()))?;
    Ok(project_id)
}

async fn active_presence_overlay(
    state: &AppState,
    project_name: &str,
) -> Result<HashMap<String, (bool, Option<String>)>, CoreError> {
    let project_id = state.project_manager.get_project_id(project_name).await?;
    let live_presence = state.presence_manager.cached_presence(&project_id);
    Ok(live_presence
        .into_iter()
        .map(|(peer_id, cached)| {
            (
                peer_id,
                (
                    true,
                    cached.update.active_doc.map(|doc_id| doc_id.to_string()),
                ),
            )
        })
        .collect())
}

async fn purge_project_local_state(
    state: &AppState,
    project_name: &str,
    reason: &str,
) -> Result<ProjectEvictionNotice, CoreError> {
    let project_id = state.project_manager.get_project_id(project_name).await?;
    let docs = state
        .project_manager
        .list_files(project_name)
        .await
        .unwrap_or_default();
    let mut doc_ids = docs.iter().map(|doc| doc.id).collect::<Vec<_>>();
    if let Ok(manifest_doc_id) = state.project_manager.manifest_doc_id(project_name).await {
        doc_ids.push(manifest_doc_id);
    }

    for doc_id in &doc_ids {
        state.sync_engine.unregister_doc(doc_id);
        state.unsent_changes.lock().unwrap().remove(doc_id);
    }
    state
        .sync_state_store
        .delete_all_for_docs(&doc_ids)
        .await
        .map_err(CoreError::Io)?;

    let peers = state.peer_manager.get_project_peers(project_name);
    for peer_id in &peers {
        state
            .peer_manager
            .remove_peer_from_project(project_name, peer_id);
    }

    state.presence_manager.leave_project(&project_id).await;
    state.presence_seq.lock().unwrap().remove(&project_id);

    state
        .search_index
        .lock()
        .map_err(|_| CoreError::InvalidData("search index lock poisoned".into()))?
        .remove_project(project_name)?;
    state
        .version_store
        .lock()
        .map_err(|_| CoreError::InvalidData("version store lock poisoned".into()))?
        .delete_project(project_name)?;

    state.owner_invite_store.delete_project(&project_id)?;
    state.join_session_store.delete_project(&project_id)?;

    state.project_manager.delete_project(project_name).await?;

    let notice = ProjectEvictionNotice {
        project_id,
        project_name: project_name.to_string(),
        reason: reason.to_string(),
    };
    save_project_eviction_notice(state.project_manager.persistence().base_dir(), &notice).await?;
    Ok(notice)
}

fn add_unsent_changes(state: &AppState, doc_id: DocId, delta: u32) -> u32 {
    if delta == 0 {
        return pending_unsent_changes(state, doc_id);
    }

    let mut map = state
        .unsent_changes
        .lock()
        .expect("unsent changes mutex poisoned");
    let entry = map.entry(doc_id).or_default();
    entry.local_changes = entry.local_changes.saturating_add(delta);
    entry.local_changes.saturating_sub(entry.synced_changes)
}

fn synced_checkpoint(state: &AppState, doc_id: DocId) -> u32 {
    state
        .unsent_changes
        .lock()
        .ok()
        .and_then(|map| map.get(&doc_id).map(|entry| entry.local_changes))
        .unwrap_or(0)
}

fn mark_unsent_changes_synced(state: &AppState, doc_id: DocId, synced_through: u32) -> u32 {
    let mut map = state
        .unsent_changes
        .lock()
        .expect("unsent changes mutex poisoned");
    let entry = map.entry(doc_id).or_default();
    entry.synced_changes = entry
        .synced_changes
        .max(synced_through)
        .min(entry.local_changes);
    let pending = entry.local_changes.saturating_sub(entry.synced_changes);
    if pending == 0 {
        map.remove(&doc_id);
    }
    pending
}

fn sync_state_for_project(peer_manager: &PeerManager, project: &str) -> events::SyncState {
    let peer_count = peer_manager.get_project_peers(project).len();
    let connected_count = peer_manager
        .get_project_peers(project)
        .into_iter()
        .filter(|peer_id| peer_manager.is_peer_connected(peer_id))
        .count();

    if peer_count == 0 {
        events::SyncState::LocalOnly
    } else if connected_count > 0 {
        events::SyncState::Synced
    } else {
        events::SyncState::LocalOnly
    }
}

fn emit_sync_status(
    app_handle: &AppHandle,
    doc_id: DocId,
    state: events::SyncState,
    unsent_changes: u32,
) {
    let _ = app_handle.emit(
        events::event_names::SYNC_STATUS,
        events::SyncStatusEvent {
            doc_id,
            state,
            unsent_changes,
        },
    );
}

fn network_status_message(status: &NetworkStatus) -> &'static str {
    match status {
        NetworkStatus::NotStarted | NetworkStatus::Starting => "networking is still starting",
        NetworkStatus::Ready => "networking is ready",
        NetworkStatus::Failed(_) => "networking failed to initialize",
    }
}

async fn load_seen_state_or_default(project_dir: &std::path::Path) -> notes_core::ProjectSeenState {
    match notes_core::SeenStateManager::load(project_dir).await {
        Ok(state) => state,
        Err(error) => {
            log::warn!(
                "Failed to load seen state for {}: {error}. Continuing with empty seen state.",
                project_dir.display()
            );
            notes_core::ProjectSeenState::default()
        }
    }
}

async fn save_seen_state_best_effort(
    project_dir: &std::path::Path,
    state: &notes_core::ProjectSeenState,
) {
    if let Err(error) = notes_core::SeenStateManager::save(project_dir, state).await {
        log::warn!(
            "Failed to save seen state for {}: {error}. Continuing without persisting seen state.",
            project_dir.display()
        );
    }
}

async fn mark_seen_heads_best_effort(
    project_manager: &ProjectManager,
    project: &str,
    doc_id: DocId,
    heads: Vec<String>,
) {
    let project_dir = project_manager.persistence().project_dir(project);
    let mut seen_state = load_seen_state_or_default(&project_dir).await;
    seen_state.mark_seen_heads(&doc_id, heads);
    save_seen_state_best_effort(&project_dir, &seen_state).await;
}

async fn get_unseen_docs_for_project(
    project_manager: &ProjectManager,
    project: &str,
) -> Result<Vec<notes_core::UnseenDocInfo>, CoreError> {
    notes_core::validate_project_name(project)?;
    let project_dir = project_manager.persistence().project_dir(project);
    let seen_state = load_seen_state_or_default(&project_dir).await;
    let files = project_manager.list_files(project).await?;

    let mut results = Vec::new();
    for file in files {
        if let Err(_) = project_manager.open_doc(project, &file.id).await {
            results.push(notes_core::UnseenDocInfo {
                doc_id: file.id,
                path: file.path,
                has_unseen_changes: false,
                last_seen_at: seen_state.last_seen_at(&file.id),
            });
            continue;
        }

        let doc_arc = project_manager.doc_store().get_doc(&file.id)?;
        let mut doc = doc_arc.write().await;
        let has_unseen = seen_state.has_unseen_changes(&file.id, &mut doc);

        results.push(notes_core::UnseenDocInfo {
            doc_id: file.id,
            path: file.path,
            has_unseen_changes: has_unseen,
            last_seen_at: seen_state.last_seen_at(&file.id),
        });
    }

    Ok(results)
}

async fn mark_doc_seen_for_project(
    project_manager: &ProjectManager,
    project: &str,
    doc_id: DocId,
) -> Result<(), CoreError> {
    project_manager.open_doc(project, &doc_id).await?;

    let current_heads = {
        let doc_arc = project_manager.doc_store().get_doc(&doc_id)?;
        let mut doc = doc_arc.write().await;
        doc.get_heads()
            .iter()
            .map(|head| head.to_string())
            .collect::<Vec<_>>()
    };

    mark_seen_heads_best_effort(project_manager, project, doc_id, current_heads).await;

    Ok(())
}

fn require_network_ready_status(status: &NetworkStatus) -> Result<(), CoreError> {
    match status {
        NetworkStatus::Ready => Ok(()),
        NetworkStatus::Failed(message) => Err(CoreError::InvalidData(format!(
            "networking failed to initialize: {message}"
        ))),
        other => Err(CoreError::InvalidData(network_status_message(other).into())),
    }
}

fn begin_network_startup(status: &RwLock<NetworkStatus>) -> bool {
    let mut current = status.write().expect("network status lock poisoned");
    if !matches!(*current, NetworkStatus::NotStarted) {
        return false;
    }
    *current = NetworkStatus::Starting;
    true
}

fn require_network_ready(state: &AppState) -> Result<(), CoreError> {
    let status = state
        .network_status
        .read()
        .expect("network status lock poisoned");
    require_network_ready_status(&status)
}

fn set_network_status(state: &AppState, status: NetworkStatus) {
    if let Ok(mut current) = state.network_status.write() {
        *current = status;
    }
}

fn is_network_ready(state: &AppState) -> bool {
    state
        .network_status
        .read()
        .map(|status| matches!(*status, NetworkStatus::Ready))
        .unwrap_or(false)
}

// ── Authorization Helpers ────────────────────────────────────────────

/// Minimum role required for an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MinRole {
    /// Read access for project members, including viewers.
    Viewer,
    /// At least Editor (rejects Viewers).
    Editor,
    /// Must be Owner.
    Owner,
}

fn is_authorized_for_min_role(
    role: Option<PeerRole>,
    access_state: notes_core::ProjectAccessState,
    min_role: MinRole,
) -> Result<(), CoreError> {
    if access_state == notes_core::ProjectAccessState::LocalOwner {
        return Ok(());
    }

    match min_role {
        MinRole::Viewer => match role {
            Some(PeerRole::Owner) | Some(PeerRole::Editor) | Some(PeerRole::Viewer) => Ok(()),
            None => Err(CoreError::ProjectIdentityMismatch),
        },
        MinRole::Owner => match access_state {
            notes_core::ProjectAccessState::Owner => Ok(()),
            notes_core::ProjectAccessState::IdentityMismatch => {
                Err(CoreError::ProjectIdentityMismatch)
            }
            _ => match role {
                Some(PeerRole::Owner) | Some(PeerRole::Editor) | Some(PeerRole::Viewer) => {
                    Err(CoreError::InvalidInput(
                        "only the project owner can perform this action".into(),
                    ))
                }
                None => Err(CoreError::ProjectIdentityMismatch),
            },
        },
        MinRole::Editor => match role {
            Some(PeerRole::Owner) | Some(PeerRole::Editor) => Ok(()),
            Some(PeerRole::Viewer) => Err(CoreError::InvalidInput(
                "viewers cannot modify documents".into(),
            )),
            None => Err(CoreError::ProjectIdentityMismatch),
        },
    }
}

/// Check the local device's role in a project. Returns Ok(()) if authorized,
/// Err if the role is insufficient. For local-only projects (no owner set),
/// all operations are allowed.
async fn check_role(state: &AppState, project: &str, min_role: MinRole) -> Result<(), CoreError> {
    let my_peer_id = state.local_peer_id.clone();
    let (my_role, access_state) = state
        .project_manager
        .resolve_local_access(project, &my_peer_id)
        .await?;

    is_authorized_for_min_role(my_role, access_state, min_role)
}

// ── Project Commands ─────────────────────────────────────────────────

#[tauri::command]
async fn list_projects(state: State<'_, AppState>) -> Result<Vec<String>, CoreError> {
    state.project_manager.list_projects().await
}

#[tauri::command]
async fn create_project(state: State<'_, AppState>, name: String) -> Result<(), CoreError> {
    state.project_manager.create_project(&name).await?;

    let my_peer_id = state.local_peer_id.clone();
    let settings =
        notes_core::AppSettings::load(state.project_manager.persistence().base_dir()).await;
    let manifest_arc = state.project_manager.get_manifest_for_ui(&name)?;
    let manifest_data = {
        let mut manifest = manifest_arc.write().await;
        manifest.set_owner(&my_peer_id)?;
        manifest.set_owner_alias(&settings.display_name)?;
        manifest.save()
    };

    state
        .project_manager
        .persistence()
        .save_manifest(&name, &manifest_data)
        .await?;

    Ok(())
}

#[tauri::command]
async fn list_project_summaries(
    state: State<'_, AppState>,
) -> Result<Vec<ProjectSummary>, CoreError> {
    let my_peer_id = state.local_peer_id.clone();
    state
        .project_manager
        .list_project_summaries(&my_peer_id)
        .await
}

#[tauri::command]
async fn open_project(
    state: State<'_, AppState>,
    name: String,
    connect_peers: Option<bool>,
) -> Result<(), CoreError> {
    state.project_manager.open_project(&name).await?;
    let _ = register_project_sync_objects(
        &state.project_manager,
        &state.sync_engine,
        &state.endpoint.id(),
        &name,
    )
    .await?;

    if !connect_peers.unwrap_or(false) {
        return Ok(());
    }

    // Restore peers from manifest into PeerManager and connect immediately.
    if let Ok(mut roster) = state
        .project_manager
        .get_project_peer_roster(&name, &state.local_peer_id)
        .await
    {
        roster.retain(|peer| !peer.is_self);
        let peer_ids: Vec<(iroh::EndpointId, String)> = roster
            .iter()
            .filter_map(|peer| {
                peer.peer_id
                    .parse::<iroh::EndpointId>()
                    .ok()
                    .map(|id| (id, peer.peer_id.clone()))
            })
            .collect();

        if !is_network_ready(&state) {
            log::debug!(
                "Skipping peer restore for project {name} because networking is not ready yet"
            );
            return Ok(());
        }

        ensure_project_presence_subscription(&state, &name)
            .await
            .ok();

        for (peer_id, _) in &peer_ids {
            state.peer_manager.add_peer_to_project(&name, *peer_id);
        }

        // Eagerly connect to all peers (fire-and-forget, non-blocking)
        if !peer_ids.is_empty() {
            let peer_mgr = Arc::clone(&state.peer_manager);
            let app_handle = state.app_handle.clone();
            let peer_ids_owned: Vec<(iroh::EndpointId, String)> = peer_ids;
            tauri::async_runtime::spawn(async move {
                for (peer_id, peer_id_str) in peer_ids_owned {
                    match peer_mgr.get_or_connect(peer_id).await {
                        Ok(_) => {
                            log::info!("Eager connect succeeded for peer {peer_id}");
                            let _ = app_handle.emit(
                                events::event_names::PEER_STATUS,
                                events::PeerStatusEvent {
                                    peer_id: peer_id_str,
                                    state: events::PeerConnectionState::Connected,
                                    alias: None,
                                },
                            );
                        }
                        Err(e) => {
                            log::debug!("Eager connect failed for peer {peer_id}: {e}");
                        }
                    }
                }
            });
        }
    }

    Ok(())
}

#[tauri::command]
async fn rename_project(
    state: State<'_, AppState>,
    old_name: String,
    new_name: String,
) -> Result<(), CoreError> {
    let old_project_id = state.project_manager.get_project_id(&old_name).await.ok();
    // Unregister all docs from sync engine
    if let Ok(files) = state.project_manager.list_files(&old_name).await {
        for file in &files {
            state.sync_engine.unregister_doc(&file.id);
        }
    }
    state
        .project_manager
        .rename_project(&old_name, &new_name)
        .await?;
    if let Some(project_id) = old_project_id {
        state.presence_manager.leave_project(&project_id).await;
    }
    Ok(())
}

#[tauri::command]
async fn delete_project(state: State<'_, AppState>, name: String) -> Result<(), CoreError> {
    let notice = purge_project_local_state(&state, &name, "deleted").await?;
    dismiss_project_eviction_notice_on_disk(
        state.project_manager.persistence().base_dir(),
        &notice.project_id,
    )
    .await?;
    Ok(())
}

#[tauri::command]
async fn purge_project_local_data(
    state: State<'_, AppState>,
    project: String,
    reason: String,
) -> Result<(), CoreError> {
    let notice = purge_project_local_state(&state, &project, &reason).await?;
    emit_project_evicted_event(&state.app_handle, &notice);
    Ok(())
}

#[tauri::command]
async fn list_project_eviction_notices(
    state: State<'_, AppState>,
) -> Result<Vec<ProjectEvictionNotice>, CoreError> {
    list_project_eviction_notices_from_disk(state.project_manager.persistence().base_dir()).await
}

#[tauri::command]
async fn dismiss_project_eviction_notice(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<(), CoreError> {
    dismiss_project_eviction_notice_on_disk(
        state.project_manager.persistence().base_dir(),
        &project_id,
    )
    .await
}

#[tauri::command]
async fn get_project_metadata(
    state: State<'_, AppState>,
    project: String,
) -> Result<notes_core::ProjectMetadata, CoreError> {
    state.project_manager.open_project(&project).await?;
    let manifest_arc = state.project_manager.get_manifest_for_ui(&project)?;
    let manifest = manifest_arc.read().await;

    let files = state
        .project_manager
        .list_files(&project)
        .await
        .unwrap_or_default();
    let peers = state
        .project_manager
        .get_project_peers(&project)
        .await
        .unwrap_or_default();

    Ok(notes_core::ProjectMetadata {
        name: manifest.name().unwrap_or_default(),
        project_id: manifest.project_id().unwrap_or_default(),
        emoji: manifest.emoji(),
        description: manifest.description(),
        color: manifest.color(),
        archived: manifest.is_archived(),
        created: manifest.created(),
        owner: manifest.get_owner().ok(),
        peer_count: peers.len(),
        file_count: files.len(),
    })
}

#[tauri::command]
async fn update_project_metadata(
    state: State<'_, AppState>,
    project: String,
    emoji: Option<String>,
    description: Option<String>,
    color: Option<String>,
) -> Result<(), CoreError> {
    let manifest_arc = state.project_manager.get_manifest_for_ui(&project)?;
    let data = {
        let mut manifest = manifest_arc.write().await;
        if let Some(ref e) = emoji {
            manifest.set_emoji(e)?;
        }
        if let Some(ref d) = description {
            manifest.set_description(d)?;
        }
        if let Some(ref c) = color {
            manifest.set_color(c)?;
        }
        manifest.save()
    };
    state
        .project_manager
        .persistence()
        .save_manifest(&project, &data)
        .await
}

#[tauri::command]
async fn archive_project(
    state: State<'_, AppState>,
    project: String,
    archived: bool,
) -> Result<(), CoreError> {
    let manifest_arc = state.project_manager.get_manifest_for_ui(&project)?;
    let data = {
        let mut manifest = manifest_arc.write().await;
        manifest.set_archived(archived)?;
        manifest.save()
    };
    state
        .project_manager
        .persistence()
        .save_manifest(&project, &data)
        .await
}

#[tauri::command]
async fn list_project_tree(
    state: State<'_, AppState>,
    project: String,
) -> Result<std::collections::BTreeMap<String, Vec<DocInfo>>, CoreError> {
    state.project_manager.list_project_tree(&project).await
}

// ── Todo Commands ────────────────────────────────────────────────────

#[tauri::command]
async fn add_project_todo(
    state: State<'_, AppState>,
    project: String,
    text: String,
    linked_doc_id: Option<String>,
) -> Result<String, CoreError> {
    check_role(&state, &project, MinRole::Editor).await?;
    execute_add_project_todo(
        &state.project_manager,
        &state.sync_engine,
        &state.sync_trigger,
        &state.local_peer_id,
        &project,
        &text,
        linked_doc_id.as_deref(),
    )
    .await
}

#[tauri::command]
async fn toggle_project_todo(
    state: State<'_, AppState>,
    project: String,
    todo_id: String,
) -> Result<bool, CoreError> {
    check_role(&state, &project, MinRole::Editor).await?;
    execute_toggle_project_todo(
        &state.project_manager,
        &state.sync_engine,
        &state.sync_trigger,
        &state.local_peer_id,
        &project,
        &todo_id,
    )
    .await
}

#[tauri::command]
async fn remove_project_todo(
    state: State<'_, AppState>,
    project: String,
    todo_id: String,
) -> Result<(), CoreError> {
    check_role(&state, &project, MinRole::Editor).await?;
    execute_remove_project_todo(
        &state.project_manager,
        &state.sync_engine,
        &state.sync_trigger,
        &state.local_peer_id,
        &project,
        &todo_id,
    )
    .await
}

#[tauri::command]
async fn update_project_todo(
    state: State<'_, AppState>,
    project: String,
    todo_id: String,
    text: String,
) -> Result<(), CoreError> {
    check_role(&state, &project, MinRole::Editor).await?;
    execute_update_project_todo(
        &state.project_manager,
        &state.sync_engine,
        &state.sync_trigger,
        &state.local_peer_id,
        &project,
        &todo_id,
        &text,
    )
    .await
}

#[tauri::command]
async fn list_project_todos(
    state: State<'_, AppState>,
    project: String,
) -> Result<Vec<notes_core::TodoItem>, CoreError> {
    check_role(&state, &project, MinRole::Viewer).await?;
    let manifest_arc = state.project_manager.get_manifest_for_ui(&project)?;
    let manifest = manifest_arc.read().await;
    manifest.list_todos()
}

// ── Image Commands ───────────────────────────────────────────────────

/// Import an image into the blob store. Returns the blob metadata (hash, size, mime).
/// The frontend should use the hash to construct the image URL for TipTap.
#[tauri::command]
async fn import_image(
    state: State<'_, AppState>,
    project: String,
    data: Vec<u8>,
    filename: String,
) -> Result<notes_sync::blobs::BlobMeta, CoreError> {
    notes_core::validate_project_name(&project)?;
    check_role(&state, &project, MinRole::Editor).await?;
    let assets_dir = state
        .project_manager
        .persistence()
        .project_dir(&project)
        .join("assets");

    state
        .blob_store
        .import(&data, Some(&assets_dir), Some(&filename))
        .await
        .map_err(|e| CoreError::InvalidData(format!("image import failed: {e}")))
}

/// Get the raw bytes of a blob by its hash.
/// Used by the frontend to display images via object URLs.
#[tauri::command]
async fn get_image(state: State<'_, AppState>, hash: String) -> Result<Response, CoreError> {
    let data = state
        .blob_store
        .read(&hash)
        .await
        .map_err(|e| CoreError::InvalidData(format!("image read failed: {e}")))?;
    Ok(Response::new(InvokeResponseBody::Raw(data)))
}

/// Check if a blob exists locally.
#[tauri::command]
async fn has_image(state: State<'_, AppState>, hash: String) -> Result<bool, CoreError> {
    Ok(state.blob_store.has(&hash).await)
}

// ── Document Commands ────────────────────────────────────────────────

#[tauri::command]
async fn list_files(
    state: State<'_, AppState>,
    project: String,
) -> Result<Vec<DocInfo>, CoreError> {
    state.project_manager.list_files(&project).await
}

#[tauri::command]
async fn create_note(
    state: State<'_, AppState>,
    project: String,
    path: String,
) -> Result<DocId, CoreError> {
    check_role(&state, &project, MinRole::Editor).await?;
    let doc_id = state.project_manager.create_note(&project, &path).await?;
    let manifest_doc_id = state.project_manager.manifest_doc_id(&project).await?;
    let doc_arc = state.project_manager.doc_store().get_doc(&doc_id)?;
    state.sync_engine.register_doc(doc_id, doc_arc);
    // Populate ACL for this doc from the project's peer list
    populate_doc_acl(&state, &project, doc_id).await;
    if let Ok(manifest_doc) = state.project_manager.doc_store().get_doc(&manifest_doc_id) {
        state
            .sync_engine
            .register_doc(manifest_doc_id, manifest_doc);
        populate_doc_acl(&state, &project, manifest_doc_id).await;
    }
    let _ = state
        .sync_trigger
        .send((project.clone(), manifest_doc_id))
        .await;
    Ok(doc_id)
}

#[tauri::command]
async fn open_doc(
    state: State<'_, AppState>,
    project: String,
    doc_id: DocId,
) -> Result<(), CoreError> {
    state.project_manager.open_doc(&project, &doc_id).await?;
    let doc_arc = state.project_manager.doc_store().get_doc(&doc_id)?;
    state.sync_engine.register_doc(doc_id, doc_arc);
    // Populate ACL for this doc
    populate_doc_acl(&state, &project, doc_id).await;

    emit_sync_status(
        &state.app_handle,
        doc_id,
        sync_state_for_project(&state.peer_manager, &project),
        pending_unsent_changes(&state, doc_id),
    );

    Ok(())
}

#[tauri::command]
async fn close_doc(
    state: State<'_, AppState>,
    project: String,
    doc_id: DocId,
) -> Result<(), CoreError> {
    notes_core::validate_project_name(&project)?;
    state.sync_engine.unregister_doc(&doc_id);
    state.project_manager.close_doc(&project, &doc_id).await
}

#[tauri::command]
async fn delete_note(
    state: State<'_, AppState>,
    project: String,
    doc_id: DocId,
) -> Result<(), CoreError> {
    check_role(&state, &project, MinRole::Editor).await?;
    let manifest_doc_id = state.project_manager.manifest_doc_id(&project).await?;
    state.sync_engine.unregister_doc(&doc_id);
    state.project_manager.delete_note(&project, &doc_id).await?;
    let _ = state.sync_trigger.send((project, manifest_doc_id)).await;
    Ok(())
}

#[tauri::command]
async fn rename_note(
    state: State<'_, AppState>,
    project: String,
    doc_id: DocId,
    new_path: String,
) -> Result<(), CoreError> {
    check_role(&state, &project, MinRole::Editor).await?;
    let manifest_doc_id = state.project_manager.manifest_doc_id(&project).await?;
    state
        .project_manager
        .rename_note(&project, &doc_id, &new_path)
        .await?;
    let _ = state.sync_trigger.send((project, manifest_doc_id)).await;
    Ok(())
}

#[tauri::command]
async fn recover_doc_from_markdown_cmd(
    state: State<'_, AppState>,
    project: String,
    doc_id: DocId,
) -> Result<DocInfo, CoreError> {
    check_role(&state, &project, MinRole::Editor).await?;
    let doc = state
        .project_manager
        .recover_note_from_markdown(&project, &doc_id)
        .await?;
    Ok(doc)
}

#[tauri::command]
async fn get_doc_binary(
    state: State<'_, AppState>,
    project: String,
    doc_id: DocId,
) -> Result<Response, CoreError> {
    state.project_manager.open_doc(&project, &doc_id).await?;
    let bytes = state.project_manager.get_doc_binary(&doc_id).await?;
    Ok(Response::new(InvokeResponseBody::Raw(bytes)))
}

#[tauri::command]
async fn get_doc_text(
    state: State<'_, AppState>,
    project: String,
    doc_id: DocId,
) -> Result<String, CoreError> {
    state.project_manager.open_doc(&project, &doc_id).await?;
    state.project_manager.get_doc_text(&doc_id).await
}

#[tauri::command]
async fn apply_changes(
    state: State<'_, AppState>,
    project: String,
    doc_id: DocId,
    data: Vec<u8>,
) -> Result<(), CoreError> {
    notes_core::validate_project_name(&project)?;
    check_role(&state, &project, MinRole::Editor).await?;
    let applied = state
        .project_manager
        .doc_store()
        .apply_incremental_and_collect(&doc_id, &data)
        .await?;

    // Sign locally-created changes with the device's Ed25519 key.
    // These signatures are stored in the SyncEngine and transmitted
    // as sidecar SignatureBatch messages during sync.
    {
        let secret_key = state.secret_key.clone();
        for (hash, raw_bytes) in &applied.new_changes {
            let signed = notes_crypto::SignedChange::sign(&secret_key, raw_bytes);
            let sig = notes_sync::protocol::ChangeSignature {
                change_hash: hash.clone(),
                author: signed.author,
                signature: signed.signature,
            };
            state.sync_engine.store_signature(doc_id, hash.clone(), sig);
        }
    }

    // Mark doc as seen after local edits, but keep disk I/O outside the doc lock.
    mark_seen_heads_best_effort(
        &state.project_manager,
        &project,
        doc_id,
        applied.current_heads,
    )
    .await;

    let unsent_delta = applied.new_changes.len() as u32;
    let should_track_unsent = !state.peer_manager.get_project_peers(&project).is_empty();
    let unsent_changes = if should_track_unsent {
        add_unsent_changes(&state, doc_id, unsent_delta)
    } else {
        0
    };

    emit_sync_status(
        &state.app_handle,
        doc_id,
        sync_state_for_project(&state.peer_manager, &project),
        unsent_changes,
    );

    // Trigger auto-sync with peers (debounced by the receiver)
    let _ = state.sync_trigger.send((project.clone(), doc_id)).await;

    Ok(())
}

#[tauri::command]
async fn save_doc(
    state: State<'_, AppState>,
    project: String,
    doc_id: DocId,
) -> Result<(), CoreError> {
    notes_core::validate_project_name(&project)?;
    state.project_manager.save_doc(&project, &doc_id).await
}

#[tauri::command]
async fn compact_doc(
    state: State<'_, AppState>,
    project: String,
    doc_id: DocId,
) -> Result<(), CoreError> {
    notes_core::validate_project_name(&project)?;
    state.project_manager.compact_doc(&project, &doc_id).await?;
    // Invalidate persisted sync states and signatures for this doc (compaction changes internal state)
    state
        .sync_state_store
        .delete_all_for_doc(&doc_id)
        .await
        .map_err(CoreError::Io)?;
    state.sync_engine.evict_signatures(doc_id);
    Ok(())
}

/// Get an incremental save of a document (for frontend WASM Automerge sync).
/// More efficient than `get_doc_binary` for ongoing edits — only returns
/// changes since the last `save_incremental` call.
#[tauri::command]
async fn get_doc_incremental(
    state: State<'_, AppState>,
    project: String,
    doc_id: DocId,
) -> Result<Response, CoreError> {
    state.project_manager.open_doc(&project, &doc_id).await?;
    let doc_arc = state.project_manager.doc_store().get_doc(&doc_id)?;
    let mut doc = doc_arc.write().await;
    let bytes = doc.save_incremental();
    Ok(Response::new(InvokeResponseBody::Raw(bytes)))
}

#[tauri::command]
async fn get_viewer_doc_snapshot(
    state: State<'_, AppState>,
    project: String,
    doc_id: DocId,
) -> Result<Response, CoreError> {
    notes_core::validate_project_name(&project)?;
    check_role(&state, &project, MinRole::Viewer).await?;
    state.project_manager.open_doc(&project, &doc_id).await?;
    let doc_arc = state.project_manager.doc_store().get_doc(&doc_id)?;
    let mut doc = doc_arc.write().await;
    let bytes = doc.save();
    Ok(Response::new(InvokeResponseBody::Raw(bytes)))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnsureBlobAvailableResult {
    available: bool,
    fetched: bool,
}

#[tauri::command]
async fn ensure_blob_available(
    state: State<'_, AppState>,
    project: String,
    hash: String,
) -> Result<EnsureBlobAvailableResult, CoreError> {
    notes_core::validate_project_name(&project)?;
    check_role(&state, &project, MinRole::Viewer).await?;

    if state.blob_store.has(&hash).await {
        return Ok(EnsureBlobAvailableResult {
            available: true,
            fetched: false,
        });
    }

    let fetched = state
        .peer_manager
        .fetch_blob_from_project_peers(&project, &state.blob_store, &hash)
        .await
        .is_some();

    Ok(EnsureBlobAvailableResult {
        available: fetched,
        fetched,
    })
}

// ── Unseen Changes Commands ──────────────────────────────────────────

/// Get a list of documents in a project with unseen-change indicators.
/// Returns `[{ docId, path, hasUnseenChanges, lastSeenAt }]`.
#[tauri::command]
async fn get_unseen_docs(
    state: State<'_, AppState>,
    project: String,
) -> Result<Vec<notes_core::UnseenDocInfo>, CoreError> {
    get_unseen_docs_for_project(&state.project_manager, &project).await
}

/// Mark a document as "seen" (user has opened and viewed it).
/// Call this when the frontend opens a document.
#[tauri::command]
async fn mark_doc_seen(
    state: State<'_, AppState>,
    project: String,
    doc_id: DocId,
) -> Result<(), CoreError> {
    mark_doc_seen_for_project(&state.project_manager, &project, doc_id).await
}

// ── Blame Commands ──────────────────────────────────────────────────

/// Get per-character blame attribution for a document.
/// Returns coalesced spans of contiguous characters by the same author.
#[tauri::command]
async fn get_doc_blame(
    state: State<'_, AppState>,
    project: String,
    doc_id: DocId,
) -> Result<notes_core::DocBlame, CoreError> {
    state.project_manager.open_doc(&project, &doc_id).await?;
    let doc_arc = state.project_manager.doc_store().get_doc(&doc_id)?;
    let mut doc = doc_arc.write().await;

    // Build actor → alias map from manifest + local actor map
    let mut aliases = notes_core::blame::get_actor_map(&mut doc);

    // Overlay manifest peer aliases (actor_id → display name)
    if let Ok(manifest_arc) = state.project_manager.get_manifest_for_ui(&project) {
        let manifest = manifest_arc.read().await;
        if let Ok(manifest_aliases) = manifest.get_actor_aliases() {
            for (actor_id, alias) in manifest_aliases {
                aliases.insert(actor_id, alias);
            }
        }
    }

    notes_core::blame::get_document_blame(&mut doc, &aliases)
}

/// Get actor hex -> display alias mapping for a project.
#[tauri::command]
async fn get_actor_aliases(
    state: State<'_, AppState>,
    project: String,
) -> Result<std::collections::HashMap<String, String>, CoreError> {
    state.project_manager.open_project(&project).await?;
    let manifest_arc = state.project_manager.get_manifest_for_ui(&project)?;
    let manifest = manifest_arc.read().await;
    manifest.get_actor_aliases()
}

// ── Version Commands (new system) ────────────────────────────────────

/// Get the stable device actor ID (hex string) for the frontend to use.
#[tauri::command]
async fn get_device_actor_id(state: State<'_, AppState>) -> Result<String, CoreError> {
    Ok(state.device_actor_hex.clone())
}

/// Get all versions for a document.
#[tauri::command]
async fn get_doc_versions(
    state: State<'_, AppState>,
    doc_id: DocId,
) -> Result<Vec<notes_core::Version>, CoreError> {
    let project = state
        .project_manager
        .get_project_for_doc(&doc_id)
        .ok_or_else(|| CoreError::InvalidInput("document does not belong to a project".into()))?;
    check_role(&state, &project, MinRole::Viewer).await?;
    let store = require_version_store(&state)?;
    let store = store
        .lock()
        .map_err(|_| CoreError::InvalidData("version store lock poisoned".into()))?;
    store.get_versions(&doc_id)
}

/// Create a new version (auto or named).
#[tauri::command]
async fn create_version(
    state: State<'_, AppState>,
    project: String,
    doc_id: DocId,
    label: Option<String>,
) -> Result<notes_core::Version, CoreError> {
    check_role(&state, &project, MinRole::Editor).await?;
    state.project_manager.open_doc(&project, &doc_id).await?;

    let doc_arc = state.project_manager.doc_store().get_doc(&doc_id)?;
    let mut doc = doc_arc.write().await;

    let current_heads = notes_core::version::get_current_heads(&mut *doc);
    let heads_strings = notes_core::version::heads_to_strings(&current_heads);

    // Get the previous version's heads for significance scoring
    let (prev_heads, used_names, seq) = {
        let store = require_version_store(&state)?;
        let store = store
            .lock()
            .map_err(|_| CoreError::InvalidData("version store lock poisoned".into()))?;

        let prev_heads = store
            .get_latest_version(&doc_id)?
            .map(|v| notes_core::version::strings_to_heads(&v.heads))
            .unwrap_or_default();
        let used_names = store.get_used_names(&doc_id)?;
        let seq = store.next_seq(&doc_id)?;
        (prev_heads, used_names, seq)
    };

    let is_named = label.is_some();

    // Compute significance
    let (significance, chars_added, chars_removed, blocks_changed) = if is_named {
        (notes_core::version::VersionSignificance::Named, 0, 0, 0)
    } else {
        notes_core::version::compute_significance(&mut doc, &prev_heads, &current_heads)
    };

    // Skip trivial auto-versions
    if !is_named && significance == notes_core::version::VersionSignificance::Skip {
        return Err(CoreError::InvalidInput(
            "no significant changes to version".into(),
        ));
    }

    let change_count = notes_core::version::count_changes_since(&mut doc, &prev_heads);

    // Generate unique creature name
    let version_id = uuid::Uuid::new_v4().to_string();
    let name = notes_core::version::unique_creature_name(&version_id, &used_names);

    let actor = state.device_actor_hex.clone();

    let version = notes_core::Version {
        id: version_id,
        doc_id: doc_id.to_string(),
        project: project.clone(),
        version_type: if is_named {
            notes_core::version::VersionType::Named
        } else {
            notes_core::version::VersionType::Auto
        },
        name,
        label,
        heads: heads_strings,
        actor,
        created_at: notes_core::version::now_secs(),
        change_count,
        chars_added,
        chars_removed,
        blocks_changed,
        significance,
        seq,
    };

    // Save an Automerge snapshot for rich text restore (encrypted when epoch keys available)
    let snapshot_raw = {
        let mut snapshot_doc = doc.clone();
        snapshot_doc.save()
    };

    let snapshot_to_store =
        if let Ok(epoch_mgr_arc) = state.project_manager.get_epoch_keys(&project) {
            let mgr = epoch_mgr_arc.read().await;
            if let Ok(key) = mgr.current_key() {
                let doc_id_bytes = *doc_id.as_bytes();
                match notes_crypto::encrypt_snapshot(
                    key.as_bytes(),
                    &doc_id_bytes,
                    mgr.current_epoch(),
                    &snapshot_raw,
                ) {
                    Ok(encrypted) => encrypted,
                    Err(e) => {
                        log::warn!("Snapshot encryption failed, storing plaintext: {e}");
                        snapshot_raw
                    }
                }
            } else {
                snapshot_raw
            }
        } else {
            snapshot_raw
        };

    {
        let store = require_version_store(&state)?;
        let store = store
            .lock()
            .map_err(|_| CoreError::InvalidData("version store lock poisoned".into()))?;
        store.store_version(&version, Some(&snapshot_to_store))?;
    }

    Ok(version)
}

fn snapshot_text_from_automerge_bytes(snapshot_bytes: &[u8]) -> Result<Option<String>, CoreError> {
    if snapshot_bytes.is_empty() {
        return Ok(Some(String::new()));
    }
    if automerge::AutoCommit::load(snapshot_bytes).is_err() {
        return Ok(None);
    }

    match notes_core::doc_store::visible_text_from_snapshot_bytes(snapshot_bytes) {
        Ok(text) => Ok(Some(text)),
        Err(_) => Ok(utf8_snapshot_text(snapshot_bytes)),
    }
}

fn looks_like_legacy_snapshot_text(text: &str) -> bool {
    text.chars()
        .all(|ch| matches!(ch, '\n' | '\r' | '\t') || !ch.is_control())
}

fn utf8_snapshot_text(snapshot_bytes: &[u8]) -> Option<String> {
    if let Ok(text) = String::from_utf8(snapshot_bytes.to_vec()) {
        if looks_like_legacy_snapshot_text(&text) {
            return Some(text);
        }
    }

    None
}

fn ensure_version_matches_request(
    version: &notes_core::Version,
    project: &str,
    doc_id: &DocId,
) -> Result<(), CoreError> {
    if version.project != project || version.doc_id != doc_id.to_string() {
        return Err(CoreError::InvalidData(
            "version does not belong to the requested document".into(),
        ));
    }

    Ok(())
}

fn decode_snapshot_bytes(
    snapshot_bytes: &[u8],
    epoch_mgr: Option<&notes_crypto::EpochKeyManager>,
    doc_id: &DocId,
) -> Vec<u8> {
    if let Some(epoch_mgr) = epoch_mgr {
        if snapshot_bytes.len() >= 28 {
            let epoch = u32::from_be_bytes([
                snapshot_bytes[0],
                snapshot_bytes[1],
                snapshot_bytes[2],
                snapshot_bytes[3],
            ]);
            if let Ok(key) = epoch_mgr.key_for_epoch(epoch) {
                let doc_id_bytes = *doc_id.as_bytes();
                if let Ok((_, plaintext)) =
                    notes_crypto::decrypt_snapshot(key.as_bytes(), &doc_id_bytes, snapshot_bytes)
                {
                    return plaintext;
                }
            }
        }
    }

    snapshot_bytes.to_vec()
}

fn looks_like_encrypted_snapshot_header(snapshot_bytes: &[u8]) -> bool {
    if snapshot_bytes.len() < 28 {
        return false;
    }

    let epoch = u32::from_be_bytes([
        snapshot_bytes[0],
        snapshot_bytes[1],
        snapshot_bytes[2],
        snapshot_bytes[3],
    ]);
    epoch < 1_000_000
}

fn snapshot_preview_text(
    snapshot_bytes: &[u8],
    epoch_mgr: Option<&notes_crypto::EpochKeyManager>,
    doc_id: &DocId,
) -> Result<Option<String>, CoreError> {
    if let Some(text) = snapshot_text_from_automerge_bytes(snapshot_bytes)? {
        return Ok(Some(text));
    }

    if let Some(epoch_mgr) = epoch_mgr {
        if looks_like_encrypted_snapshot_header(snapshot_bytes) {
            let decoded = decode_snapshot_bytes(snapshot_bytes, Some(epoch_mgr), doc_id);
            if decoded.as_slice() != snapshot_bytes {
                return snapshot_text_from_automerge_bytes(&decoded);
            }

            return Ok(utf8_snapshot_text(snapshot_bytes));
        }
    }

    Ok(utf8_snapshot_text(snapshot_bytes))
}

async fn load_version_preview_text(
    project_manager: &ProjectManager,
    version_store: &std::sync::Mutex<notes_core::VersionStore>,
    project: &str,
    doc_id: DocId,
    version_id: &str,
) -> Result<String, CoreError> {
    let (heads, snapshot_data) = {
        let store = version_store
            .lock()
            .map_err(|_| CoreError::InvalidData("version store lock poisoned".into()))?;

        let version = store
            .get_version(version_id)?
            .ok_or_else(|| CoreError::InvalidData("version not found".into()))?;

        ensure_version_matches_request(&version, project, &doc_id)?;

        let heads = notes_core::version::strings_to_heads(&version.heads);
        let snapshot = store.get_snapshot(version_id)?;
        (heads, snapshot)
    };

    project_manager.open_doc(project, &doc_id).await?;
    let doc_arc = project_manager.doc_store().get_doc(&doc_id)?;
    let mut doc = doc_arc.write().await;

    let mut text_from_heads: Option<String> = None;
    if !heads.is_empty() {
        if let Ok(text) = notes_core::version::get_text_at(&mut doc, &heads) {
            if !text.is_empty() || snapshot_data.is_none() {
                return Ok(text);
            }
            text_from_heads = Some(text);
        }
    }

    if let Some(data) = snapshot_data {
        let preview_text = if let Ok(epoch_mgr_arc) = project_manager.get_epoch_keys(project) {
            let mgr = epoch_mgr_arc.read().await;
            snapshot_preview_text(&data, Some(&mgr), &doc_id)?
        } else {
            snapshot_preview_text(&data, None, &doc_id)?
        };

        if let Some(text) = preview_text {
            return Ok(text);
        }
    }

    if let Some(text) = text_from_heads {
        return Ok(text);
    }

    Err(CoreError::InvalidData("version preview unavailable".into()))
}

async fn execute_get_version_text(
    project_manager: &ProjectManager,
    version_store: &std::sync::Mutex<notes_core::VersionStore>,
    project: &str,
    doc_id: DocId,
    version_id: &str,
) -> Result<String, CoreError> {
    load_version_preview_text(project_manager, version_store, project, doc_id, version_id).await
}

async fn execute_restore_to_version(
    project_manager: &ProjectManager,
    version_store: &std::sync::Mutex<notes_core::VersionStore>,
    sync_trigger: &tokio::sync::mpsc::Sender<(String, DocId)>,
    project: &str,
    doc_id: DocId,
    version_id: &str,
) -> Result<(), CoreError> {
    let (heads, snapshot_data) = {
        let store = version_store
            .lock()
            .map_err(|_| CoreError::InvalidData("version store lock poisoned".into()))?;

        let version = store
            .get_version(version_id)?
            .ok_or_else(|| CoreError::InvalidData("version not found".into()))?;

        ensure_version_matches_request(&version, project, &doc_id)?;

        let heads = notes_core::version::strings_to_heads(&version.heads);
        let snapshot_data = store.get_snapshot(version_id)?;
        (heads, snapshot_data)
    };

    if heads.is_empty() && snapshot_data.is_none() {
        return Err(CoreError::InvalidData(
            "version snapshot unavailable".into(),
        ));
    }

    let mut decrypted_snapshot = if let Some(ref data) = snapshot_data {
        if let Ok(epoch_mgr_arc) = project_manager.get_epoch_keys(project) {
            let mgr = epoch_mgr_arc.read().await;
            Some(decode_snapshot_bytes(data, Some(&mgr), &doc_id))
        } else {
            snapshot_data.clone()
        }
    } else {
        None
    };

    if let (Some(original), Some(decoded)) = (snapshot_data.as_ref(), decrypted_snapshot.as_ref()) {
        if looks_like_encrypted_snapshot_header(original)
            && automerge::AutoCommit::load(decoded).is_err()
        {
            if utf8_snapshot_text(original).is_some() {
                decrypted_snapshot = snapshot_data.clone();
            } else if heads.is_empty() {
                return Err(CoreError::InvalidData(
                    "version snapshot unavailable".into(),
                ));
            } else {
                decrypted_snapshot = None;
            }
        }
    }

    project_manager.open_doc(project, &doc_id).await?;
    let doc_arc = project_manager.doc_store().get_doc(&doc_id)?;
    let mut doc = doc_arc.write().await;

    notes_core::version::restore_to_version(&mut doc, &heads, decrypted_snapshot.as_deref())?;
    drop(doc);

    let current_heads = {
        let doc_arc = project_manager.doc_store().get_doc(&doc_id)?;
        let mut doc = doc_arc.write().await;
        doc.get_heads()
            .iter()
            .map(|head| head.to_string())
            .collect::<Vec<_>>()
    };

    mark_seen_heads_best_effort(project_manager, project, doc_id, current_heads).await;
    project_manager.doc_store().mark_dirty(&doc_id);
    let _ = sync_trigger.send((project.to_string(), doc_id)).await;
    Ok(())
}

pub async fn persist_manifest_update_for_sync(
    project_manager: &Arc<ProjectManager>,
    sync_engine: &Arc<SyncEngine>,
    sync_trigger: &tokio::sync::mpsc::Sender<(String, DocId)>,
    local_peer_id: &str,
    project: &str,
    manifest_data: &[u8],
) -> Result<DocId, CoreError> {
    tokio::fs::create_dir_all(
        project_manager
            .persistence()
            .project_dir(project)
            .join(".p2p"),
    )
    .await?;
    project_manager
        .persistence()
        .save_manifest(project, manifest_data)
        .await?;

    let manifest_doc_id = project_manager.ensure_manifest_doc_loaded(project).await?;
    if let Ok(manifest_doc) = project_manager.doc_store().get_doc(&manifest_doc_id) {
        sync_engine.register_doc(manifest_doc_id, manifest_doc);
        if let Ok(local_peer_id) = local_peer_id.parse::<iroh::EndpointId>() {
            populate_doc_acl_from_parts(
                project_manager,
                sync_engine,
                &local_peer_id,
                project,
                manifest_doc_id,
            )
            .await;
        }
    }
    let _ = sync_trigger
        .send((project.to_string(), manifest_doc_id))
        .await;
    Ok(manifest_doc_id)
}

async fn execute_add_project_todo(
    project_manager: &Arc<ProjectManager>,
    sync_engine: &Arc<SyncEngine>,
    sync_trigger: &tokio::sync::mpsc::Sender<(String, DocId)>,
    local_peer_id: &str,
    project: &str,
    text: &str,
    linked_doc_id: Option<&str>,
) -> Result<String, CoreError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(CoreError::InvalidInput("todo text cannot be empty".into()));
    }

    let manifest_arc = project_manager.get_manifest_for_ui(project)?;
    let (todo_id, data) = {
        let mut manifest = manifest_arc.write().await;
        let id = manifest.add_todo(text, local_peer_id, linked_doc_id)?;
        let data = manifest.save();
        (id, data)
    };

    let _ = persist_manifest_update_for_sync(
        project_manager,
        sync_engine,
        sync_trigger,
        local_peer_id,
        project,
        &data,
    )
    .await?;
    Ok(todo_id.to_string())
}

async fn execute_toggle_project_todo(
    project_manager: &Arc<ProjectManager>,
    sync_engine: &Arc<SyncEngine>,
    sync_trigger: &tokio::sync::mpsc::Sender<(String, DocId)>,
    local_peer_id: &str,
    project: &str,
    todo_id: &str,
) -> Result<bool, CoreError> {
    let manifest_arc = project_manager.get_manifest_for_ui(project)?;
    let (new_done, data) = {
        let mut manifest = manifest_arc.write().await;
        let done = manifest.toggle_todo(todo_id)?;
        let data = manifest.save();
        (done, data)
    };

    let _ = persist_manifest_update_for_sync(
        project_manager,
        sync_engine,
        sync_trigger,
        local_peer_id,
        project,
        &data,
    )
    .await?;
    Ok(new_done)
}

async fn execute_remove_project_todo(
    project_manager: &Arc<ProjectManager>,
    sync_engine: &Arc<SyncEngine>,
    sync_trigger: &tokio::sync::mpsc::Sender<(String, DocId)>,
    local_peer_id: &str,
    project: &str,
    todo_id: &str,
) -> Result<(), CoreError> {
    let manifest_arc = project_manager.get_manifest_for_ui(project)?;
    let data = {
        let mut manifest = manifest_arc.write().await;
        manifest.remove_todo(todo_id)?;
        manifest.save()
    };

    let _ = persist_manifest_update_for_sync(
        project_manager,
        sync_engine,
        sync_trigger,
        local_peer_id,
        project,
        &data,
    )
    .await?;
    Ok(())
}

async fn execute_update_project_todo(
    project_manager: &Arc<ProjectManager>,
    sync_engine: &Arc<SyncEngine>,
    sync_trigger: &tokio::sync::mpsc::Sender<(String, DocId)>,
    local_peer_id: &str,
    project: &str,
    todo_id: &str,
    text: &str,
) -> Result<(), CoreError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(CoreError::InvalidInput("todo text cannot be empty".into()));
    }

    let manifest_arc = project_manager.get_manifest_for_ui(project)?;
    let data = {
        let mut manifest = manifest_arc.write().await;
        manifest.update_todo_text(todo_id, text)?;
        manifest.save()
    };

    let _ = persist_manifest_update_for_sync(
        project_manager,
        sync_engine,
        sync_trigger,
        local_peer_id,
        project,
        &data,
    )
    .await?;
    Ok(())
}

/// Get the text content of a document at a specific version's heads.
#[tauri::command]
async fn get_version_text(
    state: State<'_, AppState>,
    project: String,
    doc_id: DocId,
    version_id: String,
) -> Result<String, CoreError> {
    check_role(&state, &project, MinRole::Viewer).await?;
    let store = require_version_store(&state)?;
    execute_get_version_text(
        &state.project_manager,
        &store,
        &project,
        doc_id,
        &version_id,
    )
    .await
}

/// Restore a document to a specific version.
#[tauri::command]
async fn restore_to_version_cmd(
    state: State<'_, AppState>,
    project: String,
    doc_id: DocId,
    version_id: String,
) -> Result<(), CoreError> {
    check_role(&state, &project, MinRole::Editor).await?;
    let store = require_version_store(&state)?;
    execute_restore_to_version(
        &state.project_manager,
        &store,
        &state.sync_trigger,
        &project,
        doc_id,
        &version_id,
    )
    .await
}

// ── Search Commands ─────────────────────────────────────────────────

/// Search across all notes.
#[tauri::command]
async fn search_notes(
    state: State<'_, AppState>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<notes_core::SearchResult>, CoreError> {
    let limit = limit.unwrap_or(20);
    let index = state
        .search_index
        .lock()
        .map_err(|_| CoreError::InvalidData("search index lock poisoned".into()))?;
    index.search(&query, limit)
}

/// Search within a specific project.
#[tauri::command]
async fn search_project_notes(
    state: State<'_, AppState>,
    query: String,
    project: String,
    limit: Option<usize>,
) -> Result<Vec<notes_core::SearchResult>, CoreError> {
    let limit = limit.unwrap_or(20);
    let index = state
        .search_index
        .lock()
        .map_err(|_| CoreError::InvalidData("search index lock poisoned".into()))?;
    index.search_project(&query, &project, limit)
}

// ── P2P Networking Commands ──────────────────────────────────────────

#[tauri::command]
async fn get_peer_id(state: State<'_, AppState>) -> Result<String, CoreError> {
    Ok(state.local_peer_id.clone())
}

#[tauri::command]
async fn get_peer_addr(state: State<'_, AppState>) -> Result<String, CoreError> {
    require_network_ready(&state)?;
    Ok(format!("{:?}", state.endpoint.addr()))
}

#[tauri::command]
async fn sync_with_peer(
    state: State<'_, AppState>,
    peer_addr: String,
    doc_id: DocId,
) -> Result<(), CoreError> {
    require_network_ready(&state)?;
    if state.sync_engine.is_network_blocked() {
        return Err(CoreError::InvalidData("network blocked".into()));
    }

    let peer_id: iroh::EndpointId = peer_addr
        .parse()
        .map_err(|e| CoreError::InvalidInput(format!("invalid peer ID: {e}")))?;

    // Timeout: 30 seconds for the entire sync operation
    let connection = tokio::time::timeout(
        Duration::from_secs(30),
        state.endpoint.connect(peer_id, NOTES_SYNC_ALPN),
    )
    .await
    .map_err(|_| CoreError::InvalidData("connection timed out".into()))?
    .map_err(|e| CoreError::InvalidInput(format!("connection failed: {e}")))?;

    tokio::time::timeout(
        Duration::from_secs(60),
        state.sync_engine.sync_doc_with_peer(&connection, doc_id),
    )
    .await
    .map_err(|_| CoreError::InvalidData("sync timed out".into()))?
    .map_err(|e| CoreError::InvalidData(format!("sync failed: {e}")))?;

    Ok(())
}

/// Add a peer to a project, persist to manifest, and connect.
#[tauri::command]
async fn add_peer(
    state: State<'_, AppState>,
    project: String,
    peer_id_str: String,
) -> Result<(), CoreError> {
    notes_core::validate_project_name(&project)?;
    check_role(&state, &project, MinRole::Owner).await?;
    require_network_ready(&state)?;
    let peer_id: iroh::EndpointId = peer_id_str
        .parse()
        .map_err(|e| CoreError::InvalidInput(format!("invalid peer ID: {e}")))?;

    // Add to PeerManager (deduped)
    state.peer_manager.add_peer_to_project(&project, peer_id);

    // Persist to manifest
    {
        let manifest_arc = state.project_manager.get_manifest_for_ui(&project)?;
        let mut manifest = manifest_arc.write().await;
        let existing = manifest.list_peers().unwrap_or_default();
        if !existing.iter().any(|p| p.peer_id == peer_id_str) {
            manifest.add_peer(&peer_id_str, "editor", "")?;
            let data = manifest.save();
            drop(manifest);
            state
                .project_manager
                .persistence()
                .save_manifest(&project, &data)
                .await?;
        }
    }

    // Connect (best-effort)
    if let Err(e) = state.peer_manager.get_or_connect(peer_id).await {
        log::warn!("Initial connection to peer {peer_id} failed: {e}");
    } else {
        let _ = state.app_handle.emit(
            events::event_names::PEER_STATUS,
            events::PeerStatusEvent {
                peer_id: peer_id_str,
                state: events::PeerConnectionState::Connected,
                alias: None,
            },
        );
    }

    Ok(())
}

/// Remove a peer from a project, update manifest, and clean up ACL.
#[tauri::command]
async fn remove_peer(
    state: State<'_, AppState>,
    project: String,
    peer_id_str: String,
) -> Result<(), CoreError> {
    notes_core::validate_project_name(&project)?;
    check_role(&state, &project, MinRole::Owner).await?;
    require_network_ready(&state)?;
    let peer_id: iroh::EndpointId = peer_id_str
        .parse()
        .map_err(|e| CoreError::InvalidInput(format!("invalid peer ID: {e}")))?;

    // Remove from manifest and save; keep a rollback snapshot so peer removal
    // fails closed if epoch ratcheting cannot complete.
    let previous_manifest_data;
    {
        let manifest_arc = state.project_manager.get_manifest_for_ui(&project)?;
        let mut manifest = manifest_arc.write().await;
        previous_manifest_data = manifest.save();
        let _ = manifest.remove_peer(&peer_id_str);
        let _ = manifest.remove_wrapped_epoch_key(&peer_id_str);
        let data = manifest.save();
        drop(manifest);
        state
            .project_manager
            .persistence()
            .save_manifest(&project, &data)
            .await?;
    }

    // Ratchet epoch keys (forward secrecy — removed peer can't decrypt new data)
    if let Err(e) = state.project_manager.ratchet_epoch_keys(&project).await {
        state
            .project_manager
            .persistence()
            .save_manifest(&project, &previous_manifest_data)
            .await?;
        state.project_manager.reload_manifest(&project).await?;
        return Err(CoreError::InvalidData(format!(
            "failed to rotate project keys while removing peer: {e}"
        )));
    }

    // Removal is now durable; disconnect and de-authorize the peer.
    state
        .peer_manager
        .remove_peer_from_project(&project, &peer_id);

    // Remove ACL entries for all docs in this project, including the manifest.
    for doc_id in project_doc_ids_for_acl(&state, &project).await {
        state.sync_engine.remove_peer_role(doc_id, &peer_id);
    }

    let _ = state.app_handle.emit(
        events::event_names::PEER_STATUS,
        events::PeerStatusEvent {
            peer_id: peer_id_str,
            state: events::PeerConnectionState::Disconnected,
            alias: None,
        },
    );

    Ok(())
}

/// Sync a document with all peers in a project.
#[tauri::command]
async fn sync_doc_with_project(
    state: State<'_, AppState>,
    project: String,
    doc_id: DocId,
) -> Result<serde_json::Value, CoreError> {
    notes_core::validate_project_name(&project)?;
    require_network_ready(&state)?;
    let sync_checkpoint = synced_checkpoint(&state, doc_id);

    emit_sync_status(
        &state.app_handle,
        doc_id,
        events::SyncState::Syncing,
        pending_unsent_changes(&state, doc_id),
    );

    let results = state
        .peer_manager
        .sync_doc_with_project_peers(&project, doc_id)
        .await;

    let success_count = results.iter().filter(|(_, r)| r.is_ok()).count();
    let fail_count = results.iter().filter(|(_, r)| r.is_err()).count();

    // Emit final status
    let unsent_after = if success_count > 0 {
        mark_unsent_changes_synced(&state, doc_id, sync_checkpoint)
    } else {
        pending_unsent_changes(&state, doc_id)
    };

    let sync_state = if success_count > 0 {
        events::SyncState::Synced
    } else if fail_count > 0 {
        events::SyncState::LocalOnly
    } else {
        events::SyncState::LocalOnly
    };

    emit_sync_status(&state.app_handle, doc_id, sync_state, unsent_after);

    Ok(serde_json::json!({
        "synced": success_count,
        "failed": fail_count,
    }))
}

/// Get connection status for all peers in a project.
#[tauri::command]
async fn get_peer_status(
    state: State<'_, AppState>,
    project: String,
) -> Result<Vec<ProjectPeerSummary>, CoreError> {
    notes_core::validate_project_name(&project)?;

    let manifest_arc = state.project_manager.get_manifest_for_ui(&project)?;
    let manifest = manifest_arc.read().await;
    let peers = manifest.list_peers()?;
    let owner = manifest.get_owner().unwrap_or_default();
    let owner_alias = manifest.get_owner_alias()?;
    drop(manifest);

    let live_overlay = active_presence_overlay(&state, &project)
        .await
        .unwrap_or_default();
    let base_roster = ProjectManager::build_project_peer_roster(
        &owner,
        owner_alias,
        &peers,
        &state.local_peer_id,
        &live_overlay,
    );

    let statuses = base_roster
        .into_iter()
        .map(|mut peer| {
            let connected = peer
                .peer_id
                .parse::<iroh::EndpointId>()
                .ok()
                .map(|peer_id| {
                    is_network_ready(&state) && state.peer_manager.is_peer_connected(&peer_id)
                })
                .unwrap_or(false);
            peer.connected = connected;
            if !connected {
                peer.active_doc = None;
            }
            peer
        })
        .collect();

    Ok(statuses)
}

// ── Presence Commands ────────────────────────────────────────────────

/// Broadcast a cursor/presence update to peers in a project.
#[tauri::command]
async fn broadcast_presence(
    state: State<'_, AppState>,
    project: String,
    active_doc: Option<DocId>,
    cursor_pos: Option<u64>,
    selection: Option<(u64, u64)>,
) -> Result<(), CoreError> {
    require_network_ready(&state)?;
    notes_core::validate_project_name(&project)?;
    let settings =
        notes_core::AppSettings::load(state.project_manager.persistence().base_dir()).await;

    let project_id = ensure_project_presence_subscription(&state, &project).await?;
    if let Some(doc_id) = active_doc {
        let doc_project = state.project_manager.get_project_for_doc(&doc_id);
        if doc_project.as_deref() != Some(project.as_str()) {
            return Err(CoreError::InvalidData(
                "presence active doc outside project".into(),
            ));
        }
    }

    let seq = {
        let mut map = state
            .presence_seq
            .lock()
            .map_err(|_| CoreError::InvalidData("presence sequence lock poisoned".into()))?;
        let entry = map.entry(project_id.clone()).or_insert(0);
        *entry += 1;
        *entry
    };
    let update = PresenceUpdate {
        version: PRESENCE_PROTOCOL_VERSION,
        project_id,
        peer_id: state.local_peer_id.clone(),
        session_id: state.presence_session_id.clone(),
        session_started_at: state.presence_session_started_at,
        seq,
        alias: settings.display_name,
        active_doc,
        cursor_pos,
        selection,
        ttl_ms: PRESENCE_TTL_MS,
        timestamp: now_ms(),
    };

    state
        .presence_manager
        .publish(update.clone())
        .await
        .map_err(|err| CoreError::InvalidData(err.to_string()))?;
    state
        .presence_manager
        .apply_update(update.clone(), now_ms());
    emit_presence_event(&state.app_handle, &update);

    Ok(())
}

#[tauri::command]
async fn e2e_set_network_blocked(
    state: State<'_, AppState>,
    blocked: bool,
) -> Result<(), CoreError> {
    ensure_e2e_mode()?;
    require_network_ready(&state)?;
    state.sync_engine.set_network_blocked(blocked);
    state.peer_manager.set_network_blocked(blocked);
    Ok(())
}

#[tauri::command]
fn e2e_is_enabled() -> bool {
    std::env::var("P2P_E2E").ok().as_deref() == Some("1")
}

// ── Settings Commands ────────────────────────────────────────────────

/// Get the current app settings.
#[tauri::command]
async fn get_settings(state: State<'_, AppState>) -> Result<notes_core::AppSettings, CoreError> {
    let notes_dir = state.project_manager.persistence().base_dir();
    Ok(notes_core::AppSettings::load(notes_dir).await)
}

/// Update app settings.
#[tauri::command]
async fn update_settings(
    state: State<'_, AppState>,
    settings: notes_core::AppSettings,
) -> Result<(), CoreError> {
    let notes_dir = state.project_manager.persistence().base_dir().to_path_buf();
    settings.normalized().save(&notes_dir).await
}

/// Get the degradation level for a document based on word count.
#[tauri::command]
async fn get_doc_degradation(
    state: State<'_, AppState>,
    project: String,
    doc_id: DocId,
) -> Result<notes_core::DegradationLevel, CoreError> {
    state.project_manager.open_doc(&project, &doc_id).await?;
    let text = state.project_manager.get_doc_text(&doc_id).await?;
    let notes_dir = state.project_manager.persistence().base_dir();
    let settings = notes_core::AppSettings::load(notes_dir).await;
    Ok(settings.degradation_level(&text))
}

// ── Invite Commands ──────────────────────────────────────────────────

#[tauri::command]
async fn generate_invite(
    state: State<'_, AppState>,
    project: String,
    role: String,
) -> Result<GenerateInviteResult, CoreError> {
    notes_core::validate_project_name(&project)?;
    if role != "editor" && role != "viewer" {
        return Err(CoreError::InvalidInput(
            "invite role must be 'editor' or 'viewer'".into(),
        ));
    }
    require_network_ready(&state)?;
    // Only the owner (or first sharer) can generate invites
    check_role(&state, &project, MinRole::Owner).await?;
    let _files = state.project_manager.list_files(&project).await?;

    // Set owner in manifest (if not already set) and get manifest data
    let my_peer_id = state.local_peer_id.clone();
    let settings =
        notes_core::AppSettings::load(state.project_manager.persistence().base_dir()).await;
    let manifest_arc = state.project_manager.get_manifest_for_ui(&project)?;
    let (manifest_data, project_id) = {
        let mut manifest = manifest_arc.write().await;
        // Ensure owner is set before sharing
        let current_owner = manifest.get_owner().unwrap_or_default();
        if current_owner.is_empty() {
            manifest.set_owner(&my_peer_id)?;
        }
        if manifest.get_owner_alias()?.is_none() {
            manifest.set_owner_alias(&settings.display_name)?;
        }
        let data = manifest.save();
        let pid = manifest.project_id().unwrap_or_default();
        (data, pid)
    };
    // Persist the updated manifest
    state
        .project_manager
        .persistence()
        .save_manifest(&project, &manifest_data)
        .await?;

    let passphrase = notes_sync::invite::generate_passphrase(6);
    let peer_id = state.local_peer_id.clone();
    let invite_ttl = notes_sync::invite::current_invite_ttl();
    let expires_at = chrono::Utc::now()
        + chrono::Duration::from_std(invite_ttl)
            .map_err(|e| CoreError::InvalidData(format!("invalid invite ttl: {e}")))?;

    // Register a lightweight PendingInvite. The actual payload is built later
    // from current state by OwnerInviteCoordinator so it cannot go stale.
    let invite_id = uuid::Uuid::new_v4().to_string();
    let pending = notes_sync::invite::PendingInvite {
        invite_id: invite_id.clone(),
        code: notes_sync::invite::InviteCode {
            passphrase: passphrase.clone(),
            peer_id: peer_id.clone(),
            expires_at,
        },
        created_at: std::time::Instant::now(),
        attempts: 0,
        project_name: project.clone(),
        project_id,
        invite_role: role,
        state: notes_sync::invite::InviteState::Open,
    };
    state
        .invite_handler
        .add_pending_checked(passphrase.clone(), pending)
        .map_err(|e| CoreError::InvalidData(format!("failed to persist invite: {e}")))?;

    log::info!("Generated invite for project {project}");

    Ok(GenerateInviteResult {
        invite_id,
        passphrase,
        peer_id,
        expires_at: expires_at.to_rfc3339(),
    })
}

#[tauri::command]
async fn accept_invite(
    state: State<'_, AppState>,
    passphrase: String,
    owner_peer_id: String,
) -> Result<AcceptInviteResult, CoreError> {
    require_network_ready(&state)?;
    accept_invite_impl(
        Arc::clone(&state.project_manager),
        Arc::clone(&state.sync_engine),
        Arc::clone(&state.peer_manager),
        Arc::clone(&state.join_session_store),
        Arc::clone(&state.session_secret_cache),
        state.endpoint.clone(),
        Some(state.app_handle.clone()),
        passphrase,
        owner_peer_id,
    )
    .await
}

#[tauri::command]
async fn list_pending_join_resumes_cmd(
    state: State<'_, AppState>,
) -> Result<Vec<PendingJoinResumeStatus>, CoreError> {
    list_pending_join_resumes(&state.join_session_store)
}

#[tauri::command]
fn debug_get_secret_read_stats_cmd() -> Result<DebugSecretReadStats, CoreError> {
    if !cfg!(debug_assertions) && !secret_read_debug_enabled() {
        return Err(CoreError::InvalidInput(
            "secret read debug stats are disabled".into(),
        ));
    }
    Ok(snapshot_secret_read_stats())
}

#[tauri::command]
fn debug_reset_secret_read_stats_cmd() -> Result<(), CoreError> {
    if !cfg!(debug_assertions) && !secret_read_debug_enabled() {
        return Err(CoreError::InvalidInput(
            "secret read debug stats are disabled".into(),
        ));
    }
    let should_enable = cfg!(debug_assertions) || secret_read_debug_enabled();
    notes_crypto::debug_reset_secret_read_tracking();
    if should_enable {
        notes_crypto::debug_enable_secret_read_tracking(true);
        notes_crypto::debug_set_secret_read_phase(notes_crypto::SecretReadPhase::Runtime);
    }
    Ok(())
}

#[tauri::command]
async fn resume_pending_joins_cmd(state: State<'_, AppState>) -> Result<(), CoreError> {
    require_network_ready(&state)?;
    resume_join_sessions(
        Arc::clone(&state.join_session_store),
        Arc::clone(&state.session_secret_cache),
        Arc::clone(&state.project_manager),
        Arc::clone(&state.sync_engine),
        Arc::clone(&state.peer_manager),
        state.endpoint.clone(),
        Some(state.app_handle.clone()),
    )
    .await;
    Ok(())
}

#[tauri::command]
async fn list_owner_invites_cmd(
    state: State<'_, AppState>,
    project: Option<String>,
) -> Result<Vec<OwnerInviteStatus>, CoreError> {
    list_owner_invites_from_store(&state.owner_invite_store, project.as_deref())
}
async fn populate_doc_acl(state: &AppState, project: &str, doc_id: DocId) {
    let Ok(local_peer_id) = state.local_peer_id.parse::<iroh::EndpointId>() else {
        log::warn!("Skipping ACL population for doc {doc_id} because local peer id is invalid");
        return;
    };

    populate_doc_acl_from_parts(
        &state.project_manager,
        &state.sync_engine,
        &local_peer_id,
        project,
        doc_id,
    )
    .await;
}

async fn project_doc_ids_for_acl(state: &AppState, project: &str) -> Vec<DocId> {
    let mut doc_ids = state
        .project_manager
        .list_files(project)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|file| file.id)
        .collect::<Vec<_>>();
    if let Ok(manifest_doc_id) = state.project_manager.manifest_doc_id(project).await {
        doc_ids.push(manifest_doc_id);
    }
    doc_ids
}

// ── Update Commands ──────────────────────────────────────────────────

/// Holds a checked update between the "check" and "install" steps.
/// The frontend calls check_for_update first, which stores the Update
/// object here. Then install_update takes it and runs the download+install.
struct PendingUpdate(std::sync::Mutex<Option<tauri_plugin_updater::Update>>);

/// Metadata about an available update, sent to the frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateInfo {
    version: String,
    current_version: String,
    body: Option<String>,
    date: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdaterAvailability {
    enabled: bool,
    reason: Option<String>,
}

fn updater_availability() -> UpdaterAvailability {
    let raw = std::env::var("NOTES_DISABLE_UPDATER").unwrap_or_default();
    let disabled = matches!(
        raw.as_str(),
        "1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON"
    );
    let reason = std::env::var("NOTES_UPDATER_REASON")
        .ok()
        .filter(|value| !value.trim().is_empty());

    UpdaterAvailability {
        enabled: !disabled,
        reason: if disabled {
            Some(reason.unwrap_or_else(|| {
                "Updates are managed by your system package install for this copy of notes."
                    .to_string()
            }))
        } else {
            None
        },
    }
}

/// Progress events streamed to the frontend during download via a Channel.
#[derive(Clone, Serialize)]
#[serde(tag = "event", content = "data")]
enum DownloadEvent {
    /// First chunk received — includes total download size if known.
    #[serde(rename_all = "camelCase")]
    Started { content_length: Option<u64> },
    /// A chunk of bytes was downloaded.
    #[serde(rename_all = "camelCase")]
    Progress { chunk_length: usize },
    /// Download complete, install starting.
    Finished,
}

#[tauri::command]
fn get_updater_availability() -> UpdaterAvailability {
    updater_availability()
}

/// Check if a newer version is available on GitHub Releases.
/// Returns Some(UpdateInfo) if an update exists, None if up to date.
/// The Update object is stored in PendingUpdate for install_update to use.
#[tauri::command]
async fn check_for_update(
    app: AppHandle,
    pending: State<'_, PendingUpdate>,
) -> Result<Option<UpdateInfo>, String> {
    let availability = updater_availability();
    if !availability.enabled {
        return Ok(None);
    }

    let update = app
        .updater_builder()
        .build()
        .map_err(|e| format!("updater init failed: {e}"))?
        .check()
        .await
        .map_err(|e| format!("update check failed: {e}"))?;

    let info = update.as_ref().map(|u| UpdateInfo {
        version: u.version.clone(),
        current_version: u.current_version.clone(),
        body: u.body.clone(),
        date: u.date.map(|d| d.to_string()),
    });

    // Store the Update object so install_update can consume it later
    *pending.0.lock().unwrap() = update;
    Ok(info)
}

/// Download and install a previously-checked update.
/// Streams progress events (Started, Progress, Finished) to the frontend
/// via a Tauri Channel so the UI can show a progress bar.
#[tauri::command]
async fn install_update(
    pending: State<'_, PendingUpdate>,
    on_event: Channel<DownloadEvent>,
) -> Result<(), String> {
    let availability = updater_availability();
    if !availability.enabled {
        return Err(availability.reason.unwrap_or_else(|| {
            "updates are managed outside the app for this install".to_string()
        }));
    }

    // Take the pending Update — this consumes it so it can't be installed twice
    let update = pending
        .0
        .lock()
        .unwrap()
        .take()
        .ok_or_else(|| "no pending update — call check_for_update first".to_string())?;

    // Track whether we've sent the Started event (only on first chunk)
    let started = std::sync::atomic::AtomicBool::new(false);

    let on_chunk = {
        let on_event = on_event.clone();
        move |chunk_length: usize, content_length: Option<u64>| {
            if !started.swap(true, std::sync::atomic::Ordering::Relaxed) {
                let _ = on_event.send(DownloadEvent::Started { content_length });
            }
            let _ = on_event.send(DownloadEvent::Progress { chunk_length });
        }
    };

    let on_finished = {
        let on_event = on_event.clone();
        move || {
            let _ = on_event.send(DownloadEvent::Finished);
        }
    };

    // Downloads the platform-specific updater bundle, verifies the
    // minisign signature against the pubkey in tauri.conf.json, and
    // lets Tauri install it for the current platform.
    update
        .download_and_install(on_chunk, on_finished)
        .await
        .map_err(|e| format!("install failed: {e}"))?;

    Ok(())
}

// ── App Setup ────────────────────────────────────────────────────────

fn resolve_notes_dir() -> Result<std::path::PathBuf, String> {
    if let Ok(dir) = std::env::var("NOTES_DIR") {
        return Ok(std::path::PathBuf::from(dir));
    }
    if std::env::var("P2P_E2E").ok().as_deref() == Some("1") {
        if let Ok(dir) = std::env::var("TAURI_DATA_DIR") {
            return Ok(std::path::PathBuf::from(dir).join("Notes"));
        }
        if let Ok(dir) = std::env::var("XDG_DATA_HOME") {
            return Ok(std::path::PathBuf::from(dir).join("Notes"));
        }
    }
    if let Some(home) = dirs::home_dir() {
        return Ok(home.join("Notes"));
    }
    if let Some(doc_dir) = dirs::document_dir() {
        return Ok(doc_dir.join("Notes"));
    }
    Err("Could not determine a suitable notes directory".to_string())
}

fn sync_debounce_ms() -> u64 {
    if std::env::var("P2P_E2E").ok().as_deref() != Some("1") {
        return 500;
    }

    std::env::var("P2P_SYNC_DEBOUNCE_MS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .filter(|millis| *millis > 0)
        .unwrap_or(500)
}

fn peer_monitor_interval_ms() -> u64 {
    if std::env::var("P2P_E2E").ok().as_deref() != Some("1") {
        return 15_000;
    }

    std::env::var("P2P_MONITOR_INTERVAL_MS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .filter(|millis| *millis > 0)
        .unwrap_or(15_000)
}

fn ensure_e2e_mode() -> Result<(), CoreError> {
    if std::env::var("P2P_E2E").ok().as_deref() == Some("1") {
        Ok(())
    } else {
        Err(CoreError::InvalidInput("e2e mode is not enabled".into()))
    }
}

/// Load or generate a persistent iroh SecretKey using the OS keychain.
/// Falls back to file-based storage with restrictive permissions.
fn load_or_create_secret_key(
    notes_dir: &std::path::Path,
) -> Result<iroh::SecretKey, Box<dyn std::error::Error>> {
    let keys_dir = notes_dir.join(".p2p").join("keys");
    let keystore = notes_crypto::KeyStore::new(keys_dir);
    const KEY_NAME: &str = "peer-identity";

    // Try loading existing key, including legacy keychain service migration.
    match keystore.load_key(KEY_NAME) {
        Ok(bytes) if bytes.len() == 32 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            let key = iroh::SecretKey::from_bytes(&arr);
            log::info!("Loaded peer identity from keystore");
            return Ok(key);
        }
        Ok(_) => {
            log::warn!("Identity key corrupt, generating new one");
        }
        Err(notes_crypto::CryptoError::KeyNotFound(_)) => {}
        Err(err) => return Err(Box::new(err)),
    }

    // Migrate from old plaintext file if it exists
    let old_key_path = notes_dir.join(".p2p-identity");
    if old_key_path.exists() {
        let bytes = std::fs::read(&old_key_path)?;
        if bytes.len() == 32 {
            // Store in keystore and remove old file
            keystore.store_key(KEY_NAME, &bytes)?;
            std::fs::remove_file(&old_key_path).ok();
            log::info!("Migrated peer identity from plaintext file to keystore");
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            return Ok(iroh::SecretKey::from_bytes(&arr));
        }
    }

    // Generate new key
    let mut key_bytes = [0u8; 32];
    getrandom::fill(&mut key_bytes).map_err(|e| format!("failed to generate random key: {e}"))?;
    let key = iroh::SecretKey::from_bytes(&key_bytes);

    // Store in keystore (OS keychain on macOS, file with 0o600 elsewhere)
    keystore.store_key(KEY_NAME, &key.to_bytes())?;

    log::info!("Generated new peer identity, stored in keystore");
    Ok(key)
}

fn start_deferred_networking(app_handle: AppHandle) {
    let (
        endpoint,
        sync_engine,
        peer_manager,
        presence_manager,
        invite_handler,
        project_manager,
        join_session_store,
        session_secret_cache,
        sync_receiver,
        unsent_changes,
        sync_trigger,
        local_peer_id,
        blob_store,
    ) = {
        let state = app_handle.state::<AppState>();
        if !begin_network_startup(&state.network_status) {
            return;
        }

        let sync_receiver = match state.sync_receiver.lock() {
            Ok(mut receiver) => receiver.take(),
            Err(_) => None,
        };

        (
            state.endpoint.clone(),
            Arc::clone(&state.sync_engine),
            Arc::clone(&state.peer_manager),
            Arc::clone(&state.presence_manager),
            Arc::clone(&state.invite_handler),
            Arc::clone(&state.project_manager),
            Arc::clone(&state.join_session_store),
            Arc::clone(&state.session_secret_cache),
            sync_receiver,
            Arc::clone(&state.unsent_changes),
            state.sync_trigger.clone(),
            state.local_peer_id.clone(),
            Arc::clone(&state.blob_store),
        )
    };
    let app_handle_clone = app_handle;

    tauri::async_runtime::spawn(async move {
        let Some(mut sync_rx) = sync_receiver else {
            let managed = app_handle_clone.state::<AppState>();
            set_network_status(
                &managed,
                NetworkStatus::Failed("sync worker was already started".into()),
            );
            log::error!("Deferred networking startup failed: sync worker receiver unavailable");
            return;
        };

        log::info!("Starting deferred networking runtime");

        let router = Router::builder(endpoint.clone())
            .accept(NOTES_SYNC_ALPN, Arc::clone(&sync_engine))
            .accept(INVITE_ALPN, Arc::clone(&invite_handler))
            .accept(GOSSIP_ALPN, presence_manager.gossip().clone())
            .accept(notes_sync::blobs::blob_alpn(), blob_store.protocol())
            .spawn();

        {
            let managed = app_handle_clone.state::<AppState>();
            if let Ok(mut router_slot) = managed.router.lock() {
                *router_slot = Some(router.clone());
            }
            set_network_status(&managed, NetworkStatus::Ready);
        }

        log::info!("iroh router started");

        tauri::async_runtime::spawn(resume_join_sessions(
            Arc::clone(&join_session_store),
            Arc::clone(&session_secret_cache),
            Arc::clone(&project_manager),
            Arc::clone(&sync_engine),
            Arc::clone(&peer_manager),
            endpoint.clone(),
            Some(app_handle_clone.clone()),
        ));

        tauri::async_runtime::spawn({
            let peer_mgr = Arc::clone(&peer_manager);
            let interval_ms = peer_monitor_interval_ms();
            async move {
                peer_mgr
                    .monitoring_loop(Duration::from_millis(interval_ms))
                    .await;
            }
        });

        tauri::async_runtime::spawn({
            let peer_mgr = Arc::clone(&peer_manager);
            let handle = app_handle_clone.clone();
            let debounce_ms = sync_debounce_ms();
            let unsent_changes = Arc::clone(&unsent_changes);
            let pm = Arc::clone(&project_manager);
            async move {
                loop {
                    let first = match sync_rx.recv().await {
                        Some(v) => v,
                        None => break,
                    };
                    tokio::time::sleep(Duration::from_millis(debounce_ms)).await;
                    let mut to_sync = std::collections::HashSet::new();
                    to_sync.insert(first);
                    while let Ok(item) = sync_rx.try_recv() {
                        to_sync.insert(item);
                    }
                    for (project, doc_id) in to_sync {
                        let manifest_doc_id = pm.manifest_doc_id(&project).await.ok();
                        if let Some(manifest_doc_id) = manifest_doc_id.filter(|id| *id != doc_id) {
                            let _ = peer_mgr
                                .sync_doc_with_project_peers(&project, manifest_doc_id)
                                .await;
                        }
                        let sync_checkpoint = unsent_changes
                            .lock()
                            .ok()
                            .and_then(|map| map.get(&doc_id).map(|entry| entry.local_changes))
                            .unwrap_or(0);
                        let pending = unsent_changes
                            .lock()
                            .ok()
                            .and_then(|map| map.get(&doc_id).copied())
                            .map(|entry| entry.local_changes.saturating_sub(entry.synced_changes))
                            .unwrap_or(0);
                        emit_sync_status(&handle, doc_id, events::SyncState::Syncing, pending);

                        let results = peer_mgr.sync_doc_with_project_peers(&project, doc_id).await;
                        let ok = results.iter().filter(|(_, r)| r.is_ok()).count();
                        let fail = results.iter().filter(|(_, r)| r.is_err()).count();

                        if ok > 0 {
                            if let Ok(mut map) = unsent_changes.lock() {
                                let entry = map.entry(doc_id).or_default();
                                entry.synced_changes = entry
                                    .synced_changes
                                    .max(sync_checkpoint)
                                    .min(entry.local_changes);
                                if entry.local_changes.saturating_sub(entry.synced_changes) == 0 {
                                    map.remove(&doc_id);
                                }
                            }
                        }

                        let sync_state = if ok > 0 {
                            events::SyncState::Synced
                        } else if fail > 0 {
                            events::SyncState::LocalOnly
                        } else {
                            events::SyncState::LocalOnly
                        };

                        let pending = unsent_changes
                            .lock()
                            .ok()
                            .and_then(|map| {
                                map.get(&doc_id).map(|entry| {
                                    entry.local_changes.saturating_sub(entry.synced_changes)
                                })
                            })
                            .unwrap_or(0);
                        emit_sync_status(&handle, doc_id, sync_state, pending);

                        if ok > 0 {
                            log::debug!("Auto-synced doc {doc_id} with {ok} peers");
                        }
                    }
                }
            }
        });

        tauri::async_runtime::spawn({
            let mut rx = peer_manager.subscribe_peer_status();
            let handle = app_handle_clone.clone();
            let pm = Arc::clone(&project_manager);
            let peer_mgr = Arc::clone(&peer_manager);
            let presence_mgr = Arc::clone(&presence_manager);
            let sync_tx = sync_trigger.clone();
            async move {
                loop {
                    match rx.recv().await {
                        Ok(status_event) => {
                            let status_for_emit = status_event.clone();
                            let status_alias = status_event.alias.clone();
                            let connected_peer = reconnect_peer_id(&status_event);
                            log::debug!(
                                "Peer status change: {} -> {:?}",
                                status_event.peer_id,
                                status_event.state
                            );
                            let _ = handle.emit(events::event_names::PEER_STATUS, status_for_emit);

                            if let Some(peer_id) = connected_peer {
                                for project_name in peer_mgr.get_projects_for_peer(&peer_id) {
                                    match pm.list_files(&project_name).await {
                                        Ok(files) => {
                                            let stats = try_queue_reconnect_syncs(
                                                &sync_tx,
                                                &project_name,
                                                &files,
                                            );
                                            if stats.dropped > 0 {
                                                log::warn!(
                                                    "Dropped {} reconnect sync jobs for project {}",
                                                    stats.dropped,
                                                    project_name
                                                );
                                            }
                                        }
                                        Err(err) => {
                                            log::warn!(
                                                "Failed to queue reconnect sync for project {}: {}",
                                                project_name,
                                                err
                                            );
                                        }
                                    }
                                }
                            } else if let Ok(peer_id) =
                                status_event.peer_id.parse::<iroh::EndpointId>()
                            {
                                let now = now_ms();
                                for project_name in peer_mgr.get_projects_for_peer(&peer_id) {
                                    if let Ok(project_id) = pm.get_project_id(&project_name).await {
                                        presence_mgr.clear_peer(
                                            &project_id,
                                            &status_event.peer_id,
                                            now,
                                        );
                                        emit_presence_event(
                                            &handle,
                                            &PresenceUpdate {
                                                version: PRESENCE_PROTOCOL_VERSION,
                                                project_id,
                                                peer_id: status_event.peer_id.clone(),
                                                session_id: String::new(),
                                                session_started_at: 0,
                                                seq: 0,
                                                alias: status_alias
                                                    .clone()
                                                    .unwrap_or_else(|| "peer".into()),
                                                active_doc: None,
                                                cursor_pos: None,
                                                selection: None,
                                                ttl_ms: PRESENCE_TTL_MS,
                                                timestamp: now,
                                            },
                                        );
                                    }
                                }
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            log::warn!("Peer status channel lagged by {n}");
                        }
                    }
                }
            }
        });

        tauri::async_runtime::spawn({
            let mut rx = presence_manager.subscribe();
            let handle = app_handle_clone.clone();
            let presence_mgr = Arc::clone(&presence_manager);
            async move {
                loop {
                    match rx.recv().await {
                        Ok(update) => {
                            if matches!(
                                presence_mgr.apply_update(update.clone(), now_ms()),
                                ApplyOutcome::Applied
                            ) {
                                emit_presence_event(&handle, &update);
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            log::warn!("Presence channel lagged by {n}");
                        }
                    }
                }
            }
        });

        tauri::async_runtime::spawn({
            let handle = app_handle_clone.clone();
            let presence_mgr = Arc::clone(&presence_manager);
            async move {
                let mut interval = tokio::time::interval(Duration::from_secs(1));
                loop {
                    interval.tick().await;
                    for expired in presence_mgr.expire_stale(now_ms()) {
                        emit_presence_event(
                            &handle,
                            &PresenceUpdate {
                                active_doc: None,
                                cursor_pos: None,
                                selection: None,
                                ..expired
                            },
                        );
                    }
                }
            }
        });

        tauri::async_runtime::spawn({
            let mut rx = sync_engine.subscribe_remote_changes();
            let handle = app_handle_clone.clone();
            let pm = Arc::clone(&project_manager);
            let local_peer_id = local_peer_id.clone();
            async move {
                loop {
                    match rx.recv().await {
                        Ok(change) => {
                            let doc_id = change.doc_id;
                            log::debug!("Remote change detected for doc {doc_id}");

                            if let Some(project_name) = pm.get_project_for_doc(&doc_id) {
                                let manifest_doc_id = pm.manifest_doc_id(&project_name).await.ok();
                                if manifest_doc_id == Some(doc_id) {
                                    if let Ok(owner) = pm.get_project_owner(&project_name).await {
                                        if !owner.is_empty() {
                                            if let Ok(manifest_arc) =
                                                pm.get_manifest_for_ui(&project_name)
                                            {
                                                let manifest = manifest_arc.read().await;
                                                let owner_actor = manifest
                                                    .get_owner_actor_id()
                                                    .ok()
                                                    .flatten()
                                                    .or_else(|| {
                                                        manifest.get_actor_aliases().ok().and_then(
                                                            |aliases| {
                                                                aliases
                                                                    .iter()
                                                                    .find(|(_, alias)| {
                                                                        alias.as_str() == owner
                                                                    })
                                                                    .map(|(actor, _)| actor.clone())
                                                            },
                                                        )
                                                    });
                                                drop(manifest);
                                                if let Some(actor_hex) = owner_actor {
                                                    if let Err(e) = pm
                                                        .validate_manifest_after_sync(
                                                            &project_name,
                                                            &change.before_heads,
                                                            &actor_hex,
                                                        )
                                                        .await
                                                    {
                                                        log::error!("Manifest validation failed for {project_name}: {e}");
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }

                                let mode = if manifest_doc_id == Some(doc_id) {
                                    events::RemoteChangeMode::MetadataOnly
                                } else {
                                    match pm
                                        .resolve_local_access(&project_name, &local_peer_id)
                                        .await
                                    {
                                        Ok((role, access_state)) => {
                                            if matches!(
                                                access_state,
                                                notes_core::ProjectAccessState::Viewer
                                            ) || matches!(role, Some(PeerRole::Viewer))
                                            {
                                                events::RemoteChangeMode::ViewerSnapshotAvailable
                                            } else {
                                                events::RemoteChangeMode::IncrementalAvailable
                                            }
                                        }
                                        Err(_) => events::RemoteChangeMode::MetadataOnly,
                                    }
                                };

                                let _ = handle.emit(
                                    events::event_names::REMOTE_CHANGE,
                                    events::RemoteChangeEvent {
                                        project_id: project_name,
                                        doc_id,
                                        peer_id: change.peer_id,
                                        mode,
                                    },
                                );
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            log::debug!("Remote change channel closed");
                            break;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            log::warn!("Remote change channel lagged by {n} messages");
                        }
                    }
                }
            }
        });
    });
}

async fn preload_startup_secrets(
    project_manager: Arc<ProjectManager>,
    join_session_store: Arc<JoinSessionStore>,
    session_secret_cache: Arc<SessionSecretCache>,
) {
    match project_manager.preload_all_project_secrets().await {
        Ok((epoch_keys, x25519_identities)) => {
            log::info!(
                "Startup secret preload cached {epoch_keys} epoch key sets and {x25519_identities} project X25519 identities"
            );
        }
        Err(err) => {
            log::warn!("Startup project secret preload failed: {err}");
        }
    }

    match session_secret_cache.preload_join_secrets(&join_session_store) {
        Ok(count) => {
            log::info!("Startup secret preload cached {count} join session secrets");
        }
        Err(err) => {
            log::warn!("Startup join-session secret preload failed: {err}");
        }
    }

    if secret_read_debug_enabled() {
        let stats = snapshot_secret_read_stats();
        log::info!(
            "Secret read tracker after startup preload: startup_reads={}, runtime_reads={}, cache_hits={}, cache_misses={}",
            stats.startup_reads,
            stats.runtime_reads,
            stats.cache_hits,
            stats.cache_misses
        );
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            if secret_read_debug_enabled() {
                notes_crypto::debug_reset_secret_read_tracking();
                notes_crypto::debug_enable_secret_read_tracking(true);
                notes_crypto::debug_set_secret_read_phase(notes_crypto::SecretReadPhase::Startup);
            }
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            // Register the PendingUpdate state for the update commands
            app.manage(PendingUpdate(std::sync::Mutex::new(None)));

            let notes_dir =
                resolve_notes_dir().map_err(|e| anyhow::anyhow!(e))?;

            if notes_dir.exists() {
                let metadata = std::fs::symlink_metadata(&notes_dir)
                    .map_err(|e| anyhow::anyhow!("Could not read notes directory: {e}"))?;
                if metadata.file_type().is_symlink() {
                    return Err(anyhow::anyhow!(
                        "{} is a symlink — refusing to start",
                        notes_dir.display()
                    )
                    .into());
                }
            }

            std::fs::create_dir_all(&notes_dir).map_err(|e| {
                anyhow::anyhow!(
                    "Could not create notes directory at {}: {e}",
                    notes_dir.display()
                )
            })?;

            log::info!("Notes directory: {}", notes_dir.display());

            let secret_key = load_or_create_secret_key(&notes_dir)
                .map_err(|e| anyhow::anyhow!("Failed to load peer identity: {e}"))?;

            log::info!("Peer ID: {}", secret_key.public());

            // Derive a device-level SQLCipher key from the iroh identity key.
            // This protects global databases (search index, version store) against
            // disk theft without requiring per-project epoch keys.
            let device_db_key: [u8; 32] = {
                use hkdf::Hkdf;
                use sha2::Sha256;
                let hk = Hkdf::<Sha256>::new(None, secret_key.to_bytes().as_ref());
                let mut key = [0u8; 32];
                hk.expand(b"p2p-notes/v1/device-db-encryption", &mut key)
                    .expect("HKDF expand should not fail");
                key
            };

            // Initialize the full-text search index (encrypted with device key)
            let search_db_path = notes_dir.join(".p2p").join("search.db");
            std::fs::create_dir_all(search_db_path.parent().unwrap()).ok();
            let search_index = notes_core::SearchIndex::open_with_recovery(&search_db_path, Some(&device_db_key))
                .map_err(|e| anyhow::anyhow!("Failed to open search index: {e}"))?;
            log::info!("Search index opened (encrypted)");

            // Initialize the version store (encrypted with device key).
            // Preserve-first behavior: if an existing versions.db is legacy plaintext,
            // keyed with a different device identity, or otherwise unreadable, back it up
            // and create a fresh store so version history remains available.
            let version_db_path = notes_dir.join(".p2p").join("versions.db");
            let legacy_history_db_path = notes_dir.join(".p2p").join("history.db");
            let version_store = match notes_core::VersionStore::open(&version_db_path, Some(&device_db_key)) {
                Ok(store) => store,
                Err(e) => {
                    log::warn!(
                        "Failed to open version store at {}. Backing it up and creating a fresh store. Error: {}",
                        version_db_path.display(),
                        e
                    );

                    if version_db_path.exists() {
                        let backup_suffix = chrono::Utc::now().timestamp();
                        let backup_path = version_db_path.with_extension(format!("db.bak.{backup_suffix}"));

                        match std::fs::rename(&version_db_path, &backup_path) {
                            Ok(()) => {
                                log::warn!(
                                    "Backed up unreadable version store to {}",
                                    backup_path.display()
                                );
                            }
                            Err(rename_err) => {
                                log::warn!(
                                    "Rename backup failed for unreadable version store at {}: {}. Trying copy+remove fallback.",
                                    version_db_path.display(),
                                    rename_err
                                );
                                std::fs::copy(&version_db_path, &backup_path)
                                    .map_err(|copy_err| anyhow::anyhow!(
                                        "Failed to preserve unreadable version store at {} via rename ({}) and copy ({}).",
                                        version_db_path.display(),
                                        rename_err,
                                        copy_err,
                                    ))?;
                                std::fs::remove_file(&version_db_path)
                                    .map_err(|remove_err| anyhow::anyhow!(
                                        "Copied unreadable version store to {}, but failed to remove original {}: {}",
                                        backup_path.display(),
                                        version_db_path.display(),
                                        remove_err,
                                    ))?;
                                log::warn!(
                                    "Copied unreadable version store to {} and removed original {}",
                                    backup_path.display(),
                                    version_db_path.display()
                                );
                            }
                        }

                        for suffix in ["-wal", "-shm"] {
                            let companion = std::path::PathBuf::from(format!(
                                "{}{}",
                                version_db_path.display(),
                                suffix
                            ));
                            if companion.exists() {
                                let companion_backup = std::path::PathBuf::from(format!(
                                    "{}.bak.{backup_suffix}",
                                    companion.display()
                                ));
                                if let Err(companion_err) =
                                    std::fs::rename(&companion, &companion_backup)
                                {
                                    log::warn!(
                                        "Failed to back up companion version store file {}: {}",
                                        companion.display(),
                                        companion_err
                                    );
                                }
                            }
                        }
                    }

                    notes_core::VersionStore::open(&version_db_path, Some(&device_db_key))
                        .map_err(|fresh_err| anyhow::anyhow!(
                            "Failed to create fresh version store at {}: {}",
                            version_db_path.display(),
                            fresh_err
                        ))?
                }
            };
            log::info!("Version store opened (encrypted)");
            match version_store.migrate_from_legacy_history_db(&legacy_history_db_path) {
                Ok(count) if count > 0 => {
                    log::info!("Migrated {count} old history sessions to versions")
                }
                Ok(_) => {}
                Err(e) => log::warn!("History migration failed (non-fatal): {e}"),
            }
            let version_store = Arc::new(std::sync::Mutex::new(version_store));

            // Load or create stable device actor ID
            let p2p_dir = notes_dir.join(".p2p");
            let device_actor_id = notes_core::version::load_or_create_device_actor_id(&p2p_dir)
                .map_err(|e| anyhow::anyhow!("Failed to load device actor ID: {e}"))?;
            let device_actor_hex = device_actor_id.to_hex_string();
            log::info!("Device actor ID: {}", device_actor_hex);

            let search_index = Arc::new(std::sync::Mutex::new(search_index));
            let project_manager = Arc::new(ProjectManager::with_full_config(
                notes_dir.clone(),
                Arc::clone(&search_index),
                device_actor_id,
            ));
            // Create SyncStateStore for persistent sync states
            let sync_state_store = Arc::new(SyncStateStore::new(notes_dir.join(".p2p")));

            // Create BlobStore for content-addressed image storage
            let blob_store = Arc::new(
                tauri::async_runtime::block_on(
                    notes_sync::blobs::BlobStore::new(notes_dir.join(".p2p").join("blobs"))
                ).map_err(|e| anyhow::anyhow!("Failed to create blob store: {e}"))?,
            );

            let app_handle = app.handle().clone();

            // Load settings for relay configuration before creating the endpoint.
            let startup_settings = tauri::async_runtime::block_on(
                notes_core::AppSettings::load(&notes_dir)
            );

            let endpoint = tauri::async_runtime::block_on(async {
                let mut builder = Endpoint::builder(iroh::endpoint::presets::N0)
                    .secret_key(secret_key)
                    .address_lookup(iroh::address_lookup::MdnsAddressLookupBuilder::default());

                // Apply custom relay servers from settings.
                // This overrides the default N0 relay with user-configured relays,
                // while keeping the N0 DNS address lookup for peer discovery.
                if !startup_settings.custom_relays.is_empty() {
                    let mut relay_urls = Vec::new();
                    for url_str in &startup_settings.custom_relays {
                        match url_str.parse::<iroh::RelayUrl>() {
                            Ok(url) => {
                                log::info!("Using custom relay: {url_str}");
                                relay_urls.push(url);
                            }
                            Err(e) => {
                                log::warn!("Invalid custom relay URL '{url_str}': {e}");
                            }
                        }
                    }
                    if !relay_urls.is_empty() {
                        builder = builder.relay_mode(iroh::RelayMode::custom(relay_urls));
                    }
                }

                builder
                    .bind()
                    .await
                    .map_err(|e| anyhow::anyhow!("failed to bind iroh endpoint: {e}"))
            })?;

            let mut sync_engine = Arc::new(SyncEngine::new());
            Arc::get_mut(&mut sync_engine)
                .expect("sync engine uniquely owned during setup")
                .set_sync_state_store(Arc::clone(&sync_state_store));

            log::info!("iroh endpoint bound, id: {}", endpoint.id());

            let gossip = iroh_gossip::net::Gossip::builder().spawn(endpoint.clone());
            let presence_manager = Arc::new(PresenceManager::new(gossip.clone()));

            // Create the PeerManager for managing persistent connections
            let mut peer_manager = Arc::new(PeerManager::new(
                endpoint.clone(),
                Arc::clone(&sync_engine),
            ));
            Arc::get_mut(&mut peer_manager)
                .expect("peer manager uniquely owned during setup")
                .set_project_sync_resolver(Arc::new(ProjectSyncResolverImpl::new(
                    Arc::clone(&project_manager),
                )));
            sync_engine.set_change_handler(Arc::new(ProjectSyncObserver::new(
                Arc::clone(&project_manager),
                Arc::downgrade(&sync_engine),
                Arc::downgrade(&peer_manager),
                endpoint.id(),
            )));
            let owner_invite_store = Arc::new(OwnerInviteStateStore::new(notes_dir.clone()));
            let join_session_store = Arc::new(JoinSessionStore::new(notes_dir.clone()));
            let session_secret_cache = Arc::new(SessionSecretCache::default());

            let coordinator = Arc::new(OwnerInviteCoordinator::new(
                Arc::clone(&project_manager),
                Arc::clone(&sync_engine),
                Arc::clone(&peer_manager),
                endpoint.id(),
            ));
            let invite_persistence = Arc::new(OwnerInvitePersistence::new(
                notes_dir.clone(),
                endpoint.id().to_string(),
            ));
            let mut invite_handler_raw = InviteHandler::new();
            invite_handler_raw.set_lifecycle_handler(coordinator);
            invite_handler_raw.set_persistence_handler(invite_persistence.clone());
            if let Ok(restored) = invite_persistence.load_runtime_invites_with_manifest_reconcile() {
                for (passphrase, invite) in restored {
                    let _ = invite_handler_raw.add_pending_checked(passphrase, invite);
                }
            }
            tauri::async_runtime::block_on(preload_startup_secrets(
                Arc::clone(&project_manager),
                Arc::clone(&join_session_store),
                Arc::clone(&session_secret_cache),
            ));
            if secret_read_debug_enabled() {
                notes_crypto::debug_set_secret_read_phase(notes_crypto::SecretReadPhase::Runtime);
            }
            let invite_handler = Arc::new(invite_handler_raw);

            // Auto-sync trigger: debounced channel that syncs with peers on local changes
            let (sync_tx, sync_rx) =
                tokio::sync::mpsc::channel::<(String, DocId)>(256);
            let unsent_changes: Arc<Mutex<HashMap<DocId, UnsentChangesState>>> =
                Arc::new(Mutex::new(HashMap::new()));
            let presence_seq: Arc<Mutex<HashMap<String, u64>>> = Arc::new(Mutex::new(HashMap::new()));
            let presence_session_id = uuid::Uuid::new_v4().to_string();
            let presence_session_started_at = now_ms();

            app.manage(AppState {
                project_manager: Arc::clone(&project_manager),
                sync_engine,
                peer_manager,
                invite_handler,
                owner_invite_store,
                join_session_store,
                session_secret_cache: Arc::clone(&session_secret_cache),
                sync_state_store,
                search_index: Arc::clone(&search_index),
                version_store,
                blob_store: Arc::clone(&blob_store),
                presence_manager,
                presence_session_id,
                presence_session_started_at,
                presence_seq,
                device_actor_hex,
                local_peer_id: endpoint.id().to_string(),
                secret_key: endpoint.secret_key().clone(),
                sync_trigger: sync_tx,
                sync_receiver: Mutex::new(Some(sync_rx)),
                unsent_changes,
                endpoint,
                router: Mutex::new(None),
                network_status: RwLock::new(NetworkStatus::NotStarted),
                app_handle,
            });

            // Reindex search on startup (background, non-blocking)
            {
                let pm = Arc::clone(&project_manager);
                tauri::async_runtime::spawn(async move {
                    pm.reindex_search().await;
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_projects,
            list_project_summaries,
            create_project,
            open_project,
            rename_project,
            delete_project,
            purge_project_local_data,
            list_project_eviction_notices,
            dismiss_project_eviction_notice,
            get_project_metadata,
            update_project_metadata,
            archive_project,
            list_project_tree,
            // Project todos
            add_project_todo,
            toggle_project_todo,
            remove_project_todo,
            update_project_todo,
            list_project_todos,
            // Image commands
            import_image,
            get_image,
            has_image,
            list_files,
            create_note,
            open_doc,
            close_doc,
            delete_note,
            rename_note,
            recover_doc_from_markdown_cmd,
            get_doc_binary,
            get_doc_text,
            apply_changes,
            save_doc,
            compact_doc,
            get_doc_incremental,
            get_viewer_doc_snapshot,
            ensure_blob_available,
            get_peer_id,
            get_peer_addr,
            sync_with_peer,
            add_peer,
            remove_peer,
            sync_doc_with_project,
            get_peer_status,
            generate_invite,
            accept_invite,
            list_pending_join_resumes_cmd,
            resume_pending_joins_cmd,
            list_owner_invites_cmd,
            debug_get_secret_read_stats_cmd,
            debug_reset_secret_read_stats_cmd,
            // Blame + Search + Unseen
            get_doc_blame,
            get_actor_aliases,
            // Version system
            get_device_actor_id,
            get_doc_versions,
            create_version,
            get_version_text,
            restore_to_version_cmd,
            search_notes,
            search_project_notes,
            get_unseen_docs,
            mark_doc_seen,
            // Settings
            get_settings,
            update_settings,
            get_doc_degradation,
            broadcast_presence,
            e2e_set_network_blocked,
            e2e_is_enabled,
            // Auto-update
            get_updater_availability,
            check_for_update,
            install_update,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if let RunEvent::Ready = &event {
            start_deferred_networking(app_handle.clone());
        }

        if let RunEvent::ExitRequested { .. } = &event {
            let state = app_handle.state::<AppState>();
            let pm = Arc::clone(&state.project_manager);
            let peer_mgr = Arc::clone(&state.peer_manager);
            let router = state.router.lock().ok().and_then(|slot| slot.clone());
            tauri::async_runtime::block_on(async {
                // 1. Save all documents
                pm.shutdown().await;
                // 2. Close peer connections
                peer_mgr.shutdown().await;
                // 3. Shut down the router (stops accepting new connections)
                if let Some(router) = router {
                    router.shutdown().await.ok();
                }
                log::info!("Graceful shutdown complete");
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    use automerge::ReadDoc as _;
    use notes_core::FileType;
    use notes_crypto::EpochKeyManager;

    fn make_test_version(project: &str, doc_id: DocId) -> notes_core::Version {
        notes_core::Version {
            id: uuid::Uuid::new_v4().to_string(),
            doc_id: doc_id.to_string(),
            project: project.into(),
            version_type: notes_core::version::VersionType::Auto,
            name: "Walrus".into(),
            label: None,
            heads: vec!["a".repeat(64)],
            actor: "actor".into(),
            created_at: 0,
            change_count: 0,
            chars_added: 0,
            chars_removed: 0,
            blocks_changed: 0,
            significance: notes_core::version::VersionSignificance::Significant,
            seq: 1,
        }
    }

    async fn write_doc_text(
        project_manager: &ProjectManager,
        project: &str,
        doc_id: DocId,
        text: &str,
    ) {
        project_manager.open_doc(project, &doc_id).await.unwrap();
        project_manager
            .doc_store()
            .replace_text(&doc_id, text)
            .await
            .unwrap();
    }

    async fn read_doc_text(
        project_manager: &ProjectManager,
        project: &str,
        doc_id: DocId,
    ) -> String {
        project_manager.open_doc(project, &doc_id).await.unwrap();
        let doc_arc = project_manager.doc_store().get_doc(&doc_id).unwrap();
        let doc = doc_arc.write().await;
        if let Some((automerge::Value::Object(automerge::ObjType::Text), text_id)) =
            doc.get(automerge::ROOT, "text").unwrap()
        {
            doc.text(&text_id).unwrap()
        } else {
            String::new()
        }
    }

    async fn current_doc_heads(
        project_manager: &ProjectManager,
        project: &str,
        doc_id: DocId,
    ) -> Vec<String> {
        project_manager.open_doc(project, &doc_id).await.unwrap();
        let doc_arc = project_manager.doc_store().get_doc(&doc_id).unwrap();
        let mut doc = doc_arc.write().await;
        doc.get_heads()
            .iter()
            .map(|head| head.to_string())
            .collect()
    }

    async fn current_doc_snapshot(
        project_manager: &ProjectManager,
        project: &str,
        doc_id: DocId,
    ) -> Vec<u8> {
        project_manager.open_doc(project, &doc_id).await.unwrap();
        let doc_arc = project_manager.doc_store().get_doc(&doc_id).unwrap();
        let mut doc = doc_arc.write().await;
        doc.save()
    }

    #[test]
    fn reconnect_peer_id_only_allows_connected_valid_peers() {
        let mut secret = [0u8; 32];
        getrandom::fill(&mut secret).unwrap();
        let valid = iroh::SecretKey::from_bytes(&secret).public().to_string();

        let connected = events::PeerStatusEvent {
            peer_id: valid.clone(),
            state: events::PeerConnectionState::Connected,
            alias: None,
        };
        assert!(reconnect_peer_id(&connected).is_some());

        let invalid = events::PeerStatusEvent {
            peer_id: "not-a-peer-id".into(),
            state: events::PeerConnectionState::Connected,
            alias: None,
        };
        assert!(reconnect_peer_id(&invalid).is_none());

        let disconnected = events::PeerStatusEvent {
            peer_id: valid,
            state: events::PeerConnectionState::Disconnected,
            alias: None,
        };
        assert!(reconnect_peer_id(&disconnected).is_none());
    }

    #[tokio::test]
    async fn try_queue_reconnect_syncs_counts_queued_and_dropped() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<(String, DocId)>(1);
        tx.try_send(("prefill".into(), uuid::Uuid::new_v4()))
            .unwrap();

        let files = vec![
            DocInfo {
                id: uuid::Uuid::new_v4(),
                path: "a.md".into(),
                file_type: FileType::Note,
                created: chrono::Utc::now(),
            },
            DocInfo {
                id: uuid::Uuid::new_v4(),
                path: "b.md".into(),
                file_type: FileType::Note,
                created: chrono::Utc::now(),
            },
        ];

        let stats = try_queue_reconnect_syncs(&tx, "project", &files);
        assert_eq!(stats.attempted, 2);
        assert_eq!(stats.queued, 0);
        assert_eq!(stats.dropped, 2);
        assert!(rx.recv().await.is_some());
    }

    #[tokio::test]
    async fn try_queue_reconnect_syncs_enqueues_payloads() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<(String, DocId)>(4);
        let files = vec![
            DocInfo {
                id: uuid::Uuid::new_v4(),
                path: "a.md".into(),
                file_type: FileType::Note,
                created: chrono::Utc::now(),
            },
            DocInfo {
                id: uuid::Uuid::new_v4(),
                path: "b.md".into(),
                file_type: FileType::Note,
                created: chrono::Utc::now(),
            },
        ];

        let stats = try_queue_reconnect_syncs(&tx, "project", &files);
        assert_eq!(stats.attempted, 2);
        assert_eq!(stats.queued, 2);
        assert_eq!(stats.dropped, 0);

        let first = rx.recv().await.unwrap();
        let second = rx.recv().await.unwrap();
        let queued = vec![first, second];
        assert!(queued.contains(&("project".to_string(), files[0].id)));
        assert!(queued.contains(&("project".to_string(), files[1].id)));
    }

    #[tokio::test]
    async fn try_queue_reconnect_syncs_mixed_capacity() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<(String, DocId)>(2);
        tx.try_send(("prefill".into(), uuid::Uuid::new_v4()))
            .unwrap();
        let files = vec![
            DocInfo {
                id: uuid::Uuid::new_v4(),
                path: "a.md".into(),
                file_type: FileType::Note,
                created: chrono::Utc::now(),
            },
            DocInfo {
                id: uuid::Uuid::new_v4(),
                path: "b.md".into(),
                file_type: FileType::Note,
                created: chrono::Utc::now(),
            },
        ];

        let stats = try_queue_reconnect_syncs(&tx, "project", &files);
        assert_eq!(stats.attempted, 2);
        assert_eq!(stats.queued, 1);
        assert_eq!(stats.dropped, 1);

        let first = rx.recv().await.unwrap();
        let second = rx.recv().await.unwrap();
        let queued = vec![first, second];
        assert!(
            queued.contains(&("project".to_string(), files[0].id))
                || queued.contains(&("project".to_string(), files[1].id))
        );
    }

    #[test]
    fn begin_network_startup_transitions_only_once() {
        let status = RwLock::new(NetworkStatus::NotStarted);

        assert!(begin_network_startup(&status));
        assert_eq!(*status.read().unwrap(), NetworkStatus::Starting);

        assert!(!begin_network_startup(&status));
        assert_eq!(*status.read().unwrap(), NetworkStatus::Starting);

        *status.write().unwrap() = NetworkStatus::Ready;
        assert!(!begin_network_startup(&status));
        assert_eq!(*status.read().unwrap(), NetworkStatus::Ready);
    }

    #[test]
    fn require_network_ready_status_matches_lifecycle() {
        assert!(require_network_ready_status(&NetworkStatus::Ready).is_ok());

        let not_started = require_network_ready_status(&NetworkStatus::NotStarted)
            .expect_err("not-started networking should be gated");
        assert!(not_started.to_string().contains("still starting"));

        let starting = require_network_ready_status(&NetworkStatus::Starting)
            .expect_err("starting networking should be gated");
        assert!(starting.to_string().contains("still starting"));

        let failed = require_network_ready_status(&NetworkStatus::Failed("boom".into()))
            .expect_err("failed networking should return detailed error");
        assert!(failed.to_string().contains("failed to initialize: boom"));
    }

    #[test]
    fn network_status_message_covers_all_states() {
        assert_eq!(
            network_status_message(&NetworkStatus::NotStarted),
            "networking is still starting"
        );
        assert_eq!(
            network_status_message(&NetworkStatus::Starting),
            "networking is still starting"
        );
        assert_eq!(
            network_status_message(&NetworkStatus::Ready),
            "networking is ready"
        );
        assert_eq!(
            network_status_message(&NetworkStatus::Failed("x".into())),
            "networking failed to initialize"
        );
    }

    #[tokio::test]
    async fn load_seen_state_or_default_recovers_from_malformed_json() {
        let dir = tempfile::tempdir().unwrap();
        let p2p_dir = dir.path().join(".p2p");
        tokio::fs::create_dir_all(&p2p_dir).await.unwrap();
        tokio::fs::write(p2p_dir.join("seen_state.json"), "{not valid json")
            .await
            .unwrap();

        let state = load_seen_state_or_default(dir.path()).await;
        assert_eq!(state.last_seen_at(&uuid::Uuid::new_v4()), None);
    }

    #[tokio::test]
    async fn save_seen_state_best_effort_does_not_fail_for_unwritable_path() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("not-a-project-dir");
        tokio::fs::write(&file_path, "occupied").await.unwrap();

        save_seen_state_best_effort(&file_path, &notes_core::ProjectSeenState::default()).await;
    }

    #[tokio::test]
    async fn malformed_seen_state_does_not_break_unseen_or_mark_seen_flows() {
        let dir = tempfile::tempdir().unwrap();
        let project_manager = ProjectManager::new(dir.path().to_path_buf());
        project_manager.create_project("shared").await.unwrap();
        project_manager.open_project("shared").await.unwrap();
        let doc_id = project_manager
            .create_note("shared", "hello.md")
            .await
            .unwrap();

        let seen_state_path = dir
            .path()
            .join("shared")
            .join(".p2p")
            .join("seen_state.json");
        tokio::fs::write(&seen_state_path, "{bad json")
            .await
            .unwrap();

        let unseen = get_unseen_docs_for_project(&project_manager, "shared")
            .await
            .unwrap();
        assert_eq!(unseen.len(), 1);
        assert_eq!(unseen[0].doc_id, doc_id);

        mark_doc_seen_for_project(&project_manager, "shared", doc_id)
            .await
            .unwrap();

        let repaired = tokio::fs::read_to_string(&seen_state_path).await.unwrap();
        assert!(repaired.contains(&doc_id.to_string()));
    }

    #[test]
    fn owner_min_role_accepts_shared_owner_and_rejects_mismatch() {
        assert!(is_authorized_for_min_role(
            Some(PeerRole::Owner),
            notes_core::ProjectAccessState::Owner,
            MinRole::Owner,
        )
        .is_ok());

        assert!(matches!(
            is_authorized_for_min_role(
                None,
                notes_core::ProjectAccessState::IdentityMismatch,
                MinRole::Owner,
            ),
            Err(CoreError::ProjectIdentityMismatch)
        ));
    }

    #[test]
    fn viewer_and_editor_role_checks_match_todo_expectations() {
        assert!(is_authorized_for_min_role(
            Some(PeerRole::Viewer),
            notes_core::ProjectAccessState::Viewer,
            MinRole::Viewer,
        )
        .is_ok());

        assert!(matches!(
            is_authorized_for_min_role(
                Some(PeerRole::Viewer),
                notes_core::ProjectAccessState::Viewer,
                MinRole::Editor,
            ),
            Err(CoreError::InvalidInput(_))
        ));

        assert!(is_authorized_for_min_role(
            Some(PeerRole::Editor),
            notes_core::ProjectAccessState::Editor,
            MinRole::Editor,
        )
        .is_ok());
    }

    #[tokio::test]
    async fn persist_manifest_update_for_sync_loads_manifest_doc_and_queues_sync() {
        let dir = tempfile::tempdir().unwrap();
        let project_manager = Arc::new(ProjectManager::new(dir.path().to_path_buf()));
        project_manager.create_project("shared").await.unwrap();
        project_manager.open_project("shared").await.unwrap();

        let manifest_arc = project_manager.get_manifest_for_ui("shared").unwrap();
        let manifest_data = {
            let mut manifest = manifest_arc.write().await;
            manifest
                .add_todo("backend todo", "peer-1", Some("doc-1"))
                .unwrap();
            manifest.save()
        };

        let sync_engine = Arc::new(SyncEngine::new());
        let (sync_tx, mut sync_rx) = tokio::sync::mpsc::channel::<(String, DocId)>(4);

        let manifest_doc_id = persist_manifest_update_for_sync(
            &project_manager,
            &sync_engine,
            &sync_tx,
            "peer-1",
            "shared",
            &manifest_data,
        )
        .await
        .unwrap();

        let queued = sync_rx.recv().await.unwrap();
        assert_eq!(queued, ("shared".to_string(), manifest_doc_id));
        assert!(project_manager.doc_store().contains(&manifest_doc_id));

        let reloaded = project_manager.get_manifest_for_ui("shared").unwrap();
        let todos = reloaded.read().await.list_todos().unwrap();
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].text, "backend todo");
        assert_eq!(todos[0].linked_doc_id.as_deref(), Some("doc-1"));
    }

    #[tokio::test]
    async fn execute_add_project_todo_trims_text_persists_and_queues_sync() {
        let dir = tempfile::tempdir().unwrap();
        let project_manager = Arc::new(ProjectManager::new(dir.path().to_path_buf()));
        project_manager.create_project("shared").await.unwrap();
        project_manager.open_project("shared").await.unwrap();
        let sync_engine = Arc::new(SyncEngine::new());
        let (sync_tx, mut sync_rx) = tokio::sync::mpsc::channel::<(String, DocId)>(4);

        let todo_id = execute_add_project_todo(
            &project_manager,
            &sync_engine,
            &sync_tx,
            "peer-1",
            "shared",
            "  backend todo  ",
            Some("doc-1"),
        )
        .await
        .unwrap();

        let queued = sync_rx.recv().await.unwrap();
        let manifest_doc_id = project_manager.manifest_doc_id("shared").await.unwrap();
        assert_eq!(queued, ("shared".to_string(), manifest_doc_id));

        let todos = project_manager
            .get_manifest_for_ui("shared")
            .unwrap()
            .read()
            .await
            .list_todos()
            .unwrap();
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].id.to_string(), todo_id);
        assert_eq!(todos[0].text, "backend todo");
        assert_eq!(todos[0].linked_doc_id.as_deref(), Some("doc-1"));
    }

    #[tokio::test]
    async fn execute_update_and_remove_project_todo_validate_inputs_and_persist() {
        let dir = tempfile::tempdir().unwrap();
        let project_manager = Arc::new(ProjectManager::new(dir.path().to_path_buf()));
        project_manager.create_project("shared").await.unwrap();
        project_manager.open_project("shared").await.unwrap();
        let sync_engine = Arc::new(SyncEngine::new());
        let (sync_tx, _sync_rx) = tokio::sync::mpsc::channel::<(String, DocId)>(4);

        let todo_id = execute_add_project_todo(
            &project_manager,
            &sync_engine,
            &sync_tx,
            "peer-1",
            "shared",
            "seed todo",
            None,
        )
        .await
        .unwrap();

        assert!(matches!(
            execute_update_project_todo(
                &project_manager,
                &sync_engine,
                &sync_tx,
                "peer-1",
                "shared",
                &todo_id,
                "   ",
            )
            .await,
            Err(CoreError::InvalidInput(_))
        ));

        execute_update_project_todo(
            &project_manager,
            &sync_engine,
            &sync_tx,
            "peer-1",
            "shared",
            &todo_id,
            "updated todo",
        )
        .await
        .unwrap();

        let done = execute_toggle_project_todo(
            &project_manager,
            &sync_engine,
            &sync_tx,
            "peer-1",
            "shared",
            &todo_id,
        )
        .await
        .unwrap();
        assert!(done);

        let todos = project_manager
            .get_manifest_for_ui("shared")
            .unwrap()
            .read()
            .await
            .list_todos()
            .unwrap();
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].text, "updated todo");
        assert!(todos[0].done);

        execute_remove_project_todo(
            &project_manager,
            &sync_engine,
            &sync_tx,
            "peer-1",
            "shared",
            &todo_id,
        )
        .await
        .unwrap();

        let todos = project_manager
            .get_manifest_for_ui("shared")
            .unwrap()
            .read()
            .await
            .list_todos()
            .unwrap();
        assert!(todos.is_empty());
    }

    #[tokio::test]
    async fn execute_toggle_and_remove_project_todo_reject_unknown_ids() {
        let dir = tempfile::tempdir().unwrap();
        let project_manager = Arc::new(ProjectManager::new(dir.path().to_path_buf()));
        project_manager.create_project("shared").await.unwrap();
        project_manager.open_project("shared").await.unwrap();
        let sync_engine = Arc::new(SyncEngine::new());
        let (sync_tx, _sync_rx) = tokio::sync::mpsc::channel::<(String, DocId)>(4);

        assert!(matches!(
            execute_toggle_project_todo(
                &project_manager,
                &sync_engine,
                &sync_tx,
                "peer-1",
                "shared",
                "missing",
            )
            .await,
            Err(CoreError::InvalidData(message)) if message.contains("todo not found")
        ));

        assert!(matches!(
            execute_remove_project_todo(
                &project_manager,
                &sync_engine,
                &sync_tx,
                "peer-1",
                "shared",
                "missing",
            )
            .await,
            Ok(())
        ));
    }

    #[test]
    fn snapshot_preview_text_decrypts_using_snapshot_epoch() {
        let doc_id = uuid::Uuid::new_v4();
        let epoch0 = [7u8; 32];
        let mut mgr = EpochKeyManager::from_key(0, &epoch0);
        mgr.ratchet().expect("ratchet epoch");

        use automerge::transaction::Transactable as _;

        let mut snapshot_doc = automerge::AutoCommit::new();
        let text_id = snapshot_doc
            .put_object(automerge::ROOT, "text", automerge::ObjType::Text)
            .expect("create text object");
        snapshot_doc
            .splice_text(&text_id, 0, 0, "hello from epoch zero")
            .expect("set text");
        let snapshot = snapshot_doc.save();
        let encrypted = notes_crypto::encrypt_snapshot(&epoch0, doc_id.as_bytes(), 0, &snapshot)
            .expect("encrypt snapshot");

        let text = snapshot_preview_text(&encrypted, Some(&mgr), &doc_id).expect("preview text");
        assert_eq!(text.as_deref(), Some("hello from epoch zero"));
    }

    #[test]
    fn snapshot_preview_text_supports_plaintext_legacy_snapshots() {
        let doc_id = uuid::Uuid::new_v4();
        let text = snapshot_preview_text(b"legacy markdown body", None, &doc_id).unwrap();

        assert_eq!(text.as_deref(), Some("legacy markdown body"));
    }

    #[test]
    fn snapshot_preview_text_still_supports_legacy_utf8_when_header_is_not_encrypted() {
        let doc_id = uuid::Uuid::new_v4();
        let mgr = EpochKeyManager::from_key(0, &[7u8; 32]);
        let raw = b"this looks like utf8 markdown but should not bypass encrypted handling";

        let text = snapshot_preview_text(raw, Some(&mgr), &doc_id).unwrap();

        assert_eq!(text.as_deref(), Some(std::str::from_utf8(raw).unwrap()));
    }

    #[test]
    fn snapshot_preview_text_reports_unavailable_for_garbage_data() {
        let doc_id = uuid::Uuid::new_v4();
        let text = snapshot_preview_text(&[0, 159, 146, 150], None, &doc_id).unwrap();

        assert!(text.is_none());
    }

    #[test]
    fn utf8_fallback_rejects_binary_looking_text() {
        let encrypted_like = vec![0, 0, 0, 5];
        let mut encrypted_like = encrypted_like;
        encrypted_like.extend(std::iter::repeat_n(0, 25));

        assert!(!looks_like_legacy_snapshot_text("\u{0001}\u{0002}\u{0003}"));
        assert!(looks_like_legacy_snapshot_text("# heading\n- bullet"));
        assert!(looks_like_encrypted_snapshot_header(&encrypted_like));
        assert!(!looks_like_encrypted_snapshot_header(
            b"this is plain markdown snapshot text"
        ));
    }

    #[test]
    fn ensure_version_matches_request_rejects_mismatched_doc() {
        let doc_id = uuid::Uuid::new_v4();
        let mut version = make_test_version("project-a", uuid::Uuid::new_v4());
        version.heads = vec![];

        let err = ensure_version_matches_request(&version, "project-b", &doc_id).unwrap_err();
        assert!(err
            .to_string()
            .contains("version does not belong to the requested document"));
    }

    #[tokio::test]
    async fn load_version_preview_text_rejects_mismatched_versions() {
        let dir = tempfile::tempdir().unwrap();
        let project_manager = ProjectManager::new(dir.path().to_path_buf());
        project_manager.create_project("project-a").await.unwrap();
        project_manager.open_project("project-a").await.unwrap();
        let doc_id = project_manager
            .create_note("project-a", "hello.md")
            .await
            .unwrap();
        let version_store =
            std::sync::Mutex::new(notes_core::VersionStore::open_in_memory().unwrap());
        let mut version = make_test_version("project-a", doc_id);
        version.project = "project-b".into();

        version_store
            .lock()
            .unwrap()
            .store_version(&version, None)
            .unwrap();

        let err = load_version_preview_text(
            &project_manager,
            &version_store,
            "project-a",
            doc_id,
            &version.id,
        )
        .await
        .unwrap_err();

        assert!(err
            .to_string()
            .contains("version does not belong to the requested document"));
    }

    #[tokio::test]
    async fn execute_get_version_text_returns_current_heads_preview() {
        let dir = tempfile::tempdir().unwrap();
        let project_manager = ProjectManager::new(dir.path().to_path_buf());
        project_manager.create_project("project-a").await.unwrap();
        project_manager.open_project("project-a").await.unwrap();
        let doc_id = project_manager
            .create_note("project-a", "hello.md")
            .await
            .unwrap();
        write_doc_text(
            &project_manager,
            "project-a",
            doc_id,
            "hello from live heads",
        )
        .await;

        let version_store =
            std::sync::Mutex::new(notes_core::VersionStore::open_in_memory().unwrap());
        let mut version = make_test_version("project-a", doc_id);
        version.heads = current_doc_heads(&project_manager, "project-a", doc_id).await;
        version_store
            .lock()
            .unwrap()
            .store_version(&version, None)
            .unwrap();

        let text = execute_get_version_text(
            &project_manager,
            &version_store,
            "project-a",
            doc_id,
            &version.id,
        )
        .await
        .unwrap();

        assert_eq!(text, "hello from live heads");
    }

    #[tokio::test]
    async fn execute_get_version_text_falls_back_to_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let project_manager = ProjectManager::new(dir.path().to_path_buf());
        project_manager.create_project("project-a").await.unwrap();
        project_manager.open_project("project-a").await.unwrap();
        let doc_id = project_manager
            .create_note("project-a", "hello.md")
            .await
            .unwrap();
        write_doc_text(&project_manager, "project-a", doc_id, "hello from snapshot").await;
        let snapshot = current_doc_snapshot(&project_manager, "project-a", doc_id).await;

        let version_store =
            std::sync::Mutex::new(notes_core::VersionStore::open_in_memory().unwrap());
        let mut version = make_test_version("project-a", doc_id);
        version.heads = vec![];
        version_store
            .lock()
            .unwrap()
            .store_version(&version, Some(snapshot.as_slice()))
            .unwrap();

        let text = execute_get_version_text(
            &project_manager,
            &version_store,
            "project-a",
            doc_id,
            &version.id,
        )
        .await
        .unwrap();

        assert_eq!(text, "hello from snapshot");
    }

    #[tokio::test]
    async fn execute_get_version_text_reports_unavailable_when_version_has_no_heads_or_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let project_manager = ProjectManager::new(dir.path().to_path_buf());
        project_manager.create_project("project-a").await.unwrap();
        project_manager.open_project("project-a").await.unwrap();
        let doc_id = project_manager
            .create_note("project-a", "hello.md")
            .await
            .unwrap();

        let version_store =
            std::sync::Mutex::new(notes_core::VersionStore::open_in_memory().unwrap());
        let mut version = make_test_version("project-a", doc_id);
        version.heads = vec![];
        version_store
            .lock()
            .unwrap()
            .store_version(&version, None)
            .unwrap();

        let err = execute_get_version_text(
            &project_manager,
            &version_store,
            "project-a",
            doc_id,
            &version.id,
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("version preview unavailable"));
    }

    #[tokio::test]
    async fn execute_get_version_text_supports_plaintext_legacy_snapshots() {
        let dir = tempfile::tempdir().unwrap();
        let project_manager = ProjectManager::new(dir.path().to_path_buf());
        project_manager.create_project("project-a").await.unwrap();
        project_manager.open_project("project-a").await.unwrap();
        let doc_id = project_manager
            .create_note("project-a", "hello.md")
            .await
            .unwrap();

        let version_store =
            std::sync::Mutex::new(notes_core::VersionStore::open_in_memory().unwrap());
        let mut version = make_test_version("project-a", doc_id);
        version.heads = vec![];
        version_store
            .lock()
            .unwrap()
            .store_version(&version, Some(b"legacy markdown body"))
            .unwrap();

        let text = execute_get_version_text(
            &project_manager,
            &version_store,
            "project-a",
            doc_id,
            &version.id,
        )
        .await
        .unwrap();

        assert_eq!(text, "legacy markdown body");
    }

    #[tokio::test]
    async fn execute_get_version_text_supports_empty_plaintext_legacy_snapshots() {
        let dir = tempfile::tempdir().unwrap();
        let project_manager = ProjectManager::new(dir.path().to_path_buf());
        project_manager.create_project("project-a").await.unwrap();
        project_manager.open_project("project-a").await.unwrap();
        let doc_id = project_manager
            .create_note("project-a", "hello.md")
            .await
            .unwrap();

        let version_store =
            std::sync::Mutex::new(notes_core::VersionStore::open_in_memory().unwrap());
        let mut version = make_test_version("project-a", doc_id);
        version.heads = vec![];
        version_store
            .lock()
            .unwrap()
            .store_version(&version, Some(b""))
            .unwrap();

        let text = execute_get_version_text(
            &project_manager,
            &version_store,
            "project-a",
            doc_id,
            &version.id,
        )
        .await
        .unwrap();

        assert_eq!(text, "");
    }

    #[tokio::test]
    async fn execute_get_version_text_decrypts_old_epoch_snapshots() {
        let dir = tempfile::tempdir().unwrap();
        let project_manager = ProjectManager::new(dir.path().to_path_buf());
        project_manager.create_project("project-a").await.unwrap();
        project_manager.open_project("project-a").await.unwrap();
        project_manager.init_epoch_keys("project-a").await.unwrap();
        let doc_id = project_manager
            .create_note("project-a", "hello.md")
            .await
            .unwrap();
        write_doc_text(&project_manager, "project-a", doc_id, "epoch zero body").await;
        let snapshot = current_doc_snapshot(&project_manager, "project-a", doc_id).await;

        let epoch_mgr_arc = project_manager.get_epoch_keys("project-a").unwrap();
        let epoch0_key = {
            let mgr = epoch_mgr_arc.read().await;
            mgr.current_key().unwrap()
        };
        {
            let mut mgr = epoch_mgr_arc.write().await;
            mgr.ratchet().unwrap();
        }

        let encrypted_snapshot =
            notes_crypto::encrypt_snapshot(epoch0_key.as_bytes(), doc_id.as_bytes(), 0, &snapshot)
                .unwrap();

        let version_store =
            std::sync::Mutex::new(notes_core::VersionStore::open_in_memory().unwrap());
        let mut version = make_test_version("project-a", doc_id);
        version.heads = vec![];
        version_store
            .lock()
            .unwrap()
            .store_version(&version, Some(encrypted_snapshot.as_slice()))
            .unwrap();

        let text = execute_get_version_text(
            &project_manager,
            &version_store,
            "project-a",
            doc_id,
            &version.id,
        )
        .await
        .unwrap();

        assert_eq!(text, "epoch zero body");
    }

    #[tokio::test]
    async fn execute_get_version_text_falls_back_to_heads_when_snapshot_is_undecryptable() {
        let dir = tempfile::tempdir().unwrap();
        let project_manager = ProjectManager::new(dir.path().to_path_buf());
        project_manager.create_project("project-a").await.unwrap();
        project_manager.open_project("project-a").await.unwrap();
        let doc_id = project_manager
            .create_note("project-a", "hello.md")
            .await
            .unwrap();
        write_doc_text(&project_manager, "project-a", doc_id, "heads preview body").await;
        let heads = current_doc_heads(&project_manager, "project-a", doc_id).await;
        let wrong_key = [19u8; 32];
        let encrypted_snapshot =
            notes_crypto::encrypt_snapshot(&wrong_key, doc_id.as_bytes(), 0, b"not a snapshot")
                .unwrap();

        let version_store =
            std::sync::Mutex::new(notes_core::VersionStore::open_in_memory().unwrap());
        let mut version = make_test_version("project-a", doc_id);
        version.heads = heads;
        version_store
            .lock()
            .unwrap()
            .store_version(&version, Some(encrypted_snapshot.as_slice()))
            .unwrap();

        let text = execute_get_version_text(
            &project_manager,
            &version_store,
            "project-a",
            doc_id,
            &version.id,
        )
        .await
        .unwrap();

        assert_eq!(text, "heads preview body");
    }

    #[tokio::test]
    async fn execute_restore_to_version_restores_snapshot_and_queues_sync() {
        let dir = tempfile::tempdir().unwrap();
        let project_manager = ProjectManager::new(dir.path().to_path_buf());
        project_manager.create_project("project-a").await.unwrap();
        project_manager.open_project("project-a").await.unwrap();
        let doc_id = project_manager
            .create_note("project-a", "hello.md")
            .await
            .unwrap();
        write_doc_text(&project_manager, "project-a", doc_id, "restored body").await;
        let snapshot = current_doc_snapshot(&project_manager, "project-a", doc_id).await;
        write_doc_text(&project_manager, "project-a", doc_id, "live body").await;

        let version_store =
            std::sync::Mutex::new(notes_core::VersionStore::open_in_memory().unwrap());
        let mut version = make_test_version("project-a", doc_id);
        version.heads = vec![];
        version_store
            .lock()
            .unwrap()
            .store_version(&version, Some(snapshot.as_slice()))
            .unwrap();

        let (sync_tx, mut sync_rx) = tokio::sync::mpsc::channel::<(String, DocId)>(1);
        execute_restore_to_version(
            &project_manager,
            &version_store,
            &sync_tx,
            "project-a",
            doc_id,
            &version.id,
        )
        .await
        .unwrap();

        assert_eq!(
            read_doc_text(&project_manager, "project-a", doc_id).await,
            "restored body"
        );
        assert_eq!(
            sync_rx.recv().await,
            Some(("project-a".to_string(), doc_id))
        );
    }

    #[tokio::test]
    async fn execute_restore_to_version_restores_from_heads_without_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let project_manager = ProjectManager::new(dir.path().to_path_buf());
        project_manager.create_project("project-a").await.unwrap();
        project_manager.open_project("project-a").await.unwrap();
        let doc_id = project_manager
            .create_note("project-a", "hello.md")
            .await
            .unwrap();
        write_doc_text(&project_manager, "project-a", doc_id, "heads restore body").await;
        let heads = current_doc_heads(&project_manager, "project-a", doc_id).await;
        write_doc_text(&project_manager, "project-a", doc_id, "live body").await;

        let version_store =
            std::sync::Mutex::new(notes_core::VersionStore::open_in_memory().unwrap());
        let mut version = make_test_version("project-a", doc_id);
        version.heads = heads;
        version_store
            .lock()
            .unwrap()
            .store_version(&version, None)
            .unwrap();

        let (sync_tx, _sync_rx) = tokio::sync::mpsc::channel::<(String, DocId)>(1);
        execute_restore_to_version(
            &project_manager,
            &version_store,
            &sync_tx,
            "project-a",
            doc_id,
            &version.id,
        )
        .await
        .unwrap();

        assert_eq!(
            read_doc_text(&project_manager, "project-a", doc_id).await,
            "heads restore body"
        );
    }

    #[tokio::test]
    async fn execute_restore_to_version_decrypts_old_epoch_snapshots() {
        let dir = tempfile::tempdir().unwrap();
        let project_manager = ProjectManager::new(dir.path().to_path_buf());
        project_manager.create_project("project-a").await.unwrap();
        project_manager.open_project("project-a").await.unwrap();
        project_manager.init_epoch_keys("project-a").await.unwrap();
        let doc_id = project_manager
            .create_note("project-a", "hello.md")
            .await
            .unwrap();
        write_doc_text(&project_manager, "project-a", doc_id, "epoch zero body").await;
        let snapshot = current_doc_snapshot(&project_manager, "project-a", doc_id).await;

        let epoch_mgr_arc = project_manager.get_epoch_keys("project-a").unwrap();
        let epoch0_key = {
            let mgr = epoch_mgr_arc.read().await;
            mgr.current_key().unwrap()
        };
        {
            let mut mgr = epoch_mgr_arc.write().await;
            mgr.ratchet().unwrap();
        }

        let encrypted_snapshot =
            notes_crypto::encrypt_snapshot(epoch0_key.as_bytes(), doc_id.as_bytes(), 0, &snapshot)
                .unwrap();

        write_doc_text(&project_manager, "project-a", doc_id, "live body").await;

        let version_store =
            std::sync::Mutex::new(notes_core::VersionStore::open_in_memory().unwrap());
        let mut version = make_test_version("project-a", doc_id);
        version.heads = vec![];
        version_store
            .lock()
            .unwrap()
            .store_version(&version, Some(encrypted_snapshot.as_slice()))
            .unwrap();

        let (sync_tx, _sync_rx) = tokio::sync::mpsc::channel::<(String, DocId)>(1);
        execute_restore_to_version(
            &project_manager,
            &version_store,
            &sync_tx,
            "project-a",
            doc_id,
            &version.id,
        )
        .await
        .unwrap();

        assert_eq!(
            read_doc_text(&project_manager, "project-a", doc_id).await,
            "epoch zero body"
        );
    }

    #[tokio::test]
    async fn execute_restore_to_version_rejects_undecryptable_encrypted_snapshots() {
        let dir = tempfile::tempdir().unwrap();
        let project_manager = ProjectManager::new(dir.path().to_path_buf());
        project_manager.create_project("project-a").await.unwrap();
        project_manager.open_project("project-a").await.unwrap();
        let doc_id = project_manager
            .create_note("project-a", "hello.md")
            .await
            .unwrap();
        write_doc_text(&project_manager, "project-a", doc_id, "live body").await;
        let snapshot = current_doc_snapshot(&project_manager, "project-a", doc_id).await;
        let wrong_key = [19u8; 32];
        let encrypted_snapshot =
            notes_crypto::encrypt_snapshot(&wrong_key, doc_id.as_bytes(), 0, &snapshot).unwrap();

        let version_store =
            std::sync::Mutex::new(notes_core::VersionStore::open_in_memory().unwrap());
        let mut version = make_test_version("project-a", doc_id);
        version.heads = vec![];
        version_store
            .lock()
            .unwrap()
            .store_version(&version, Some(encrypted_snapshot.as_slice()))
            .unwrap();

        let (sync_tx, mut sync_rx) = tokio::sync::mpsc::channel::<(String, DocId)>(1);
        let err = execute_restore_to_version(
            &project_manager,
            &version_store,
            &sync_tx,
            "project-a",
            doc_id,
            &version.id,
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("version snapshot unavailable"));
        assert_eq!(
            read_doc_text(&project_manager, "project-a", doc_id).await,
            "live body"
        );
        assert!(sync_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn execute_restore_to_version_falls_back_to_heads_when_snapshot_is_undecryptable() {
        let dir = tempfile::tempdir().unwrap();
        let project_manager = ProjectManager::new(dir.path().to_path_buf());
        project_manager.create_project("project-a").await.unwrap();
        project_manager.open_project("project-a").await.unwrap();
        let doc_id = project_manager
            .create_note("project-a", "hello.md")
            .await
            .unwrap();
        write_doc_text(&project_manager, "project-a", doc_id, "heads restore body").await;
        let heads = current_doc_heads(&project_manager, "project-a", doc_id).await;
        let wrong_key = [19u8; 32];
        let encrypted_snapshot =
            notes_crypto::encrypt_snapshot(&wrong_key, doc_id.as_bytes(), 0, b"not a snapshot")
                .unwrap();
        write_doc_text(&project_manager, "project-a", doc_id, "live body").await;

        let version_store =
            std::sync::Mutex::new(notes_core::VersionStore::open_in_memory().unwrap());
        let mut version = make_test_version("project-a", doc_id);
        version.heads = heads;
        version_store
            .lock()
            .unwrap()
            .store_version(&version, Some(encrypted_snapshot.as_slice()))
            .unwrap();

        let (sync_tx, _sync_rx) = tokio::sync::mpsc::channel::<(String, DocId)>(1);
        execute_restore_to_version(
            &project_manager,
            &version_store,
            &sync_tx,
            "project-a",
            doc_id,
            &version.id,
        )
        .await
        .unwrap();

        assert_eq!(
            read_doc_text(&project_manager, "project-a", doc_id).await,
            "heads restore body"
        );
    }

    #[tokio::test]
    async fn execute_restore_to_version_rejects_mismatched_version_request() {
        let dir = tempfile::tempdir().unwrap();
        let project_manager = ProjectManager::new(dir.path().to_path_buf());
        project_manager.create_project("project-a").await.unwrap();
        project_manager.open_project("project-a").await.unwrap();
        let doc_id = project_manager
            .create_note("project-a", "hello.md")
            .await
            .unwrap();

        let version_store =
            std::sync::Mutex::new(notes_core::VersionStore::open_in_memory().unwrap());
        let mut version = make_test_version("project-b", doc_id);
        version.project = "project-b".into();
        version_store
            .lock()
            .unwrap()
            .store_version(&version, None)
            .unwrap();

        let (sync_tx, _sync_rx) = tokio::sync::mpsc::channel::<(String, DocId)>(1);
        let err = execute_restore_to_version(
            &project_manager,
            &version_store,
            &sync_tx,
            "project-a",
            doc_id,
            &version.id,
        )
        .await
        .unwrap_err();

        assert!(err
            .to_string()
            .contains("version does not belong to the requested document"));
    }

    #[tokio::test]
    async fn execute_restore_to_version_rejects_missing_heads_and_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let project_manager = ProjectManager::new(dir.path().to_path_buf());
        project_manager.create_project("project-a").await.unwrap();
        project_manager.open_project("project-a").await.unwrap();
        let doc_id = project_manager
            .create_note("project-a", "hello.md")
            .await
            .unwrap();

        let version_store =
            std::sync::Mutex::new(notes_core::VersionStore::open_in_memory().unwrap());
        let mut version = make_test_version("project-a", doc_id);
        version.heads = vec![];
        version_store
            .lock()
            .unwrap()
            .store_version(&version, None)
            .unwrap();

        let (sync_tx, _sync_rx) = tokio::sync::mpsc::channel::<(String, DocId)>(1);
        let err = execute_restore_to_version(
            &project_manager,
            &version_store,
            &sync_tx,
            "project-a",
            doc_id,
            &version.id,
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("version snapshot unavailable"));
    }

    #[tokio::test]
    async fn execute_restore_to_version_supports_plaintext_legacy_snapshots() {
        let dir = tempfile::tempdir().unwrap();
        let project_manager = ProjectManager::new(dir.path().to_path_buf());
        project_manager.create_project("project-a").await.unwrap();
        project_manager.open_project("project-a").await.unwrap();
        let doc_id = project_manager
            .create_note("project-a", "hello.md")
            .await
            .unwrap();
        write_doc_text(&project_manager, "project-a", doc_id, "live body").await;

        let version_store =
            std::sync::Mutex::new(notes_core::VersionStore::open_in_memory().unwrap());
        let mut version = make_test_version("project-a", doc_id);
        version.heads = vec![];
        version_store
            .lock()
            .unwrap()
            .store_version(&version, Some(b"legacy markdown body"))
            .unwrap();

        let (sync_tx, _sync_rx) = tokio::sync::mpsc::channel::<(String, DocId)>(1);
        execute_restore_to_version(
            &project_manager,
            &version_store,
            &sync_tx,
            "project-a",
            doc_id,
            &version.id,
        )
        .await
        .unwrap();

        assert_eq!(
            read_doc_text(&project_manager, "project-a", doc_id).await,
            "legacy markdown body"
        );
    }

    #[tokio::test]
    async fn execute_restore_to_version_supports_empty_plaintext_legacy_snapshots() {
        let dir = tempfile::tempdir().unwrap();
        let project_manager = ProjectManager::new(dir.path().to_path_buf());
        project_manager.create_project("project-a").await.unwrap();
        project_manager.open_project("project-a").await.unwrap();
        let doc_id = project_manager
            .create_note("project-a", "hello.md")
            .await
            .unwrap();
        write_doc_text(&project_manager, "project-a", doc_id, "live body").await;

        let version_store =
            std::sync::Mutex::new(notes_core::VersionStore::open_in_memory().unwrap());
        let mut version = make_test_version("project-a", doc_id);
        version.heads = vec![];
        version_store
            .lock()
            .unwrap()
            .store_version(&version, Some(b""))
            .unwrap();

        let (sync_tx, _sync_rx) = tokio::sync::mpsc::channel::<(String, DocId)>(1);
        execute_restore_to_version(
            &project_manager,
            &version_store,
            &sync_tx,
            "project-a",
            doc_id,
            &version.id,
        )
        .await
        .unwrap();

        assert_eq!(
            read_doc_text(&project_manager, "project-a", doc_id).await,
            ""
        );
    }

    #[tokio::test]
    async fn preload_startup_secrets_warms_project_and_join_caches() {
        std::env::set_var("NOTES_KEYSTORE_MODE", "file-only");
        notes_crypto::debug_reset_secret_read_tracking();
        notes_crypto::debug_enable_secret_read_tracking(true);
        notes_crypto::debug_set_secret_read_phase(notes_crypto::SecretReadPhase::Startup);
        let dir = tempfile::tempdir().unwrap();
        let project_manager = Arc::new(ProjectManager::new(dir.path().to_path_buf()));
        project_manager.create_project("shared").await.unwrap();
        project_manager.init_epoch_keys("shared").await.unwrap();
        project_manager
            .get_or_create_project_x25519_identity("shared")
            .await
            .unwrap();

        let join_session_store = Arc::new(JoinSessionStore::new(dir.path().to_path_buf()));
        join_session_store
            .save(&notes_core::PersistedJoinSession {
                schema_version: 1,
                session_id: "startup-session".into(),
                owner_peer_id: "owner-peer".into(),
                project_id: "project-id".into(),
                project_name: "shared".into(),
                local_project_name: "shared".into(),
                role: "editor".into(),
                payload: "{}".into(),
                stage: notes_core::PersistedJoinStage::PayloadStaged {
                    staged_at: chrono::Utc::now(),
                },
                updated_at: chrono::Utc::now(),
            })
            .unwrap();
        join_session_store
            .save_secret_bundle(
                "startup-session",
                &notes_core::PersistedJoinSecret {
                    passphrase: "startup-secret".into(),
                    epoch_key_hex: None,
                },
            )
            .unwrap();

        let restarted = Arc::new(ProjectManager::new(dir.path().to_path_buf()));
        let session_secret_cache = Arc::new(SessionSecretCache::default());
        preload_startup_secrets(
            Arc::clone(&restarted),
            Arc::clone(&join_session_store),
            Arc::clone(&session_secret_cache),
        )
        .await;
        let stats = snapshot_secret_read_stats();

        assert!(restarted.has_cached_epoch_keys("shared"));
        assert!(restarted.has_cached_project_x25519_identity("shared"));
        assert!(session_secret_cache.has_join_passphrase("startup-session"));
        assert!(stats.startup_reads > 0);
        assert_eq!(stats.runtime_reads, 0);
        notes_crypto::debug_reset_secret_read_tracking();
    }

    #[test]
    fn debug_reset_secret_read_stats_keeps_runtime_tracking_enabled() {
        notes_crypto::debug_enable_secret_read_tracking(true);
        notes_crypto::debug_set_secret_read_phase(notes_crypto::SecretReadPhase::Runtime);
        notes_crypto::debug_note_secret_cache_hit();
        notes_crypto::debug_note_secret_cache_miss();
        notes_crypto::debug_record_secret_read(
            "peer-identity",
            notes_crypto::SecretReadBackend::File,
            notes_crypto::SecretReadOutcome::Hit,
        );

        debug_reset_secret_read_stats_cmd().unwrap();

        let stats = snapshot_secret_read_stats();
        assert!(stats.enabled);
        assert_eq!(stats.phase, "runtime");
        assert_eq!(stats.startup_reads, 0);
        assert_eq!(stats.runtime_reads, 0);
        assert_eq!(stats.cache_hits, 0);
        assert_eq!(stats.cache_misses, 0);
        assert!(stats.events.is_empty());
    }
}
