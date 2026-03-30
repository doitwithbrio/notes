use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

pub mod invite_accept;

use iroh::endpoint::Endpoint;
use iroh::protocol::Router;
use notes_core::{
    CoreError, DocId, DocInfo, JoinSessionStore, OwnerInviteStateStore, PeerRole,
    PeerStatusSummary, ProjectManager, ProjectSummary,
};
use notes_sync::events;
use notes_sync::invite::{
    InviteHandler, INVITE_ALPN,
};
use notes_sync::peer_manager::PeerManager;
use notes_sync::sync_engine::{SyncEngine, NOTES_SYNC_ALPN};
use notes_sync::SyncStateStore;
use serde::{Deserialize, Serialize};
use tauri::ipc::{Channel, InvokeResponseBody, Response};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State};
use tauri_plugin_updater::UpdaterExt;

use crate::invite_accept::{
    accept_invite_impl, list_owner_invites_from_store, list_pending_join_resumes,
    populate_doc_acl_from_parts, resume_join_sessions, AcceptInviteResult, OwnerInviteCoordinator,
    OwnerInvitePersistence, OwnerInviteStatus, PendingJoinResumeStatus,
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
    #[allow(dead_code)] // Used by sync sessions — wired via SyncEngine in Phase 2+
    sync_state_store: Arc<SyncStateStore>,
    search_index: Arc<std::sync::Mutex<notes_core::SearchIndex>>,
    version_store: Arc<std::sync::Mutex<notes_core::VersionStore>>,
    blob_store: Arc<notes_sync::blobs::BlobStore>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum NetworkStatus {
    NotStarted,
    Starting,
    Ready,
    Failed(String),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReconnectQueueStats {
    attempted: usize,
    queued: usize,
    dropped: usize,
}

fn reconnect_peer_id(
    status_event: &events::PeerStatusEvent,
) -> Option<iroh::EndpointId> {
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
        if sync_tx.try_send((project_name.to_string(), file.id)).is_ok() {
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
    entry.synced_changes = entry.synced_changes.max(synced_through).min(entry.local_changes);
    let pending = entry.local_changes.saturating_sub(entry.synced_changes);
    if pending == 0 {
        map.remove(&doc_id);
    }
    pending
}

fn sync_state_for_project(peer_manager: &PeerManager, project: &str) -> events::SyncState {
    let peer_count = peer_manager.get_project_peers(project).len();
    let connected_count = peer_manager.active_connection_count();

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
    let mut current = status
        .write()
        .expect("network status lock poisoned");
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
    /// At least Editor (rejects Viewers).
    Editor,
    /// Must be Owner.
    Owner,
}

/// Check the local device's role in a project. Returns Ok(()) if authorized,
/// Err if the role is insufficient. For local-only projects (no owner set),
/// all operations are allowed.
async fn check_role(
    state: &AppState,
    project: &str,
    min_role: MinRole,
) -> Result<(), CoreError> {
    let my_peer_id = state.local_peer_id.clone();
    let owner = state
        .project_manager
        .get_project_owner(project)
        .await
        .unwrap_or_default();

    // Local-only project (no sharing configured) — all operations allowed
    if owner.is_empty() {
        return Ok(());
    }

    // Owner can do everything
    if owner == my_peer_id {
        return Ok(());
    }

    // Look up our role
    let peers = state.project_manager.get_project_peers(project).await?;
    let my_role = peers
        .iter()
        .find(|p| p.peer_id == my_peer_id)
        .map(|p| p.role);

    match min_role {
        MinRole::Owner => {
            Err(CoreError::InvalidInput("only the project owner can perform this action".into()))
        }
        MinRole::Editor => {
            match my_role {
                Some(PeerRole::Owner) | Some(PeerRole::Editor) => Ok(()),
                Some(PeerRole::Viewer) => {
                    Err(CoreError::InvalidInput("viewers cannot modify documents".into()))
                }
                None => {
                    // Unknown/removed peer — fail closed (deny access)
                    Err(CoreError::InvalidInput("peer not authorized for this project".into()))
                }
            }
        }
    }
}

// ── Project Commands ─────────────────────────────────────────────────

#[tauri::command]
async fn list_projects(state: State<'_, AppState>) -> Result<Vec<String>, CoreError> {
    state.project_manager.list_projects().await
}

#[tauri::command]
async fn create_project(
    state: State<'_, AppState>,
    name: String,
) -> Result<(), CoreError> {
    state.project_manager.create_project(&name).await?;

    let my_peer_id = state.local_peer_id.clone();
    let manifest_arc = state.project_manager.get_manifest_for_ui(&name)?;
    let manifest_data = {
        let mut manifest = manifest_arc.write().await;
        manifest.set_owner(&my_peer_id)?;
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
    state.project_manager.list_project_summaries(&my_peer_id).await
}

#[tauri::command]
async fn open_project(
    state: State<'_, AppState>,
    name: String,
    connect_peers: Option<bool>,
) -> Result<(), CoreError> {
    // Load epoch keys FIRST so open_project_databases() can derive SQLCipher keys
    let _ = state.project_manager.load_epoch_keys(&name).await;

    state.project_manager.open_project(&name).await?;

    if !connect_peers.unwrap_or(false) {
        return Ok(());
    }

    // Restore peers from manifest into PeerManager and connect immediately
    if let Ok(peers) = state.project_manager.get_project_peers(&name).await {
        let peer_ids: Vec<(iroh::EndpointId, String)> = peers
            .iter()
            .filter_map(|p| {
                p.peer_id
                    .parse::<iroh::EndpointId>()
                    .ok()
                    .map(|id| (id, p.peer_id.clone()))
            })
            .collect();

        if !is_network_ready(&state) {
            log::debug!("Skipping peer restore for project {name} because networking is not ready yet");
            return Ok(());
        }

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
    // Unregister all docs from sync engine
    if let Ok(files) = state.project_manager.list_files(&old_name).await {
        for file in &files {
            state.sync_engine.unregister_doc(&file.id);
        }
    }
    state
        .project_manager
        .rename_project(&old_name, &new_name)
        .await
}

#[tauri::command]
async fn delete_project(
    state: State<'_, AppState>,
    name: String,
) -> Result<(), CoreError> {
    // Unregister all docs from sync engine
    if let Ok(files) = state.project_manager.list_files(&name).await {
        for file in &files {
            state.sync_engine.unregister_doc(&file.id);
        }
    }
    // Remove all peers for this project from peer manager
    let peers = state.peer_manager.get_project_peers(&name);
    for peer_id in &peers {
        state
            .peer_manager
            .remove_peer_from_project(&name, peer_id);
    }
    state.project_manager.delete_project(&name).await
}

#[tauri::command]
async fn get_project_metadata(
    state: State<'_, AppState>,
    project: String,
) -> Result<notes_core::ProjectMetadata, CoreError> {
    state.project_manager.open_project(&project).await?;
    let manifest_arc = state.project_manager.get_manifest_for_ui(&project)?;
    let manifest = manifest_arc.read().await;

    let files = state.project_manager.list_files(&project).await.unwrap_or_default();
    let peers = state.project_manager.get_project_peers(&project).await.unwrap_or_default();

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
    let my_peer_id = state.local_peer_id.clone();
    let manifest_arc = state.project_manager.get_manifest_for_ui(&project)?;
    let (todo_id, data) = {
        let mut manifest = manifest_arc.write().await;
        let id = manifest.add_todo(&text, &my_peer_id, linked_doc_id.as_deref())?;
        let data = manifest.save();
        (id, data)
    };
    state
        .project_manager
        .persistence()
        .save_manifest(&project, &data)
        .await?;
    Ok(todo_id.to_string())
}

#[tauri::command]
async fn toggle_project_todo(
    state: State<'_, AppState>,
    project: String,
    todo_id: String,
) -> Result<bool, CoreError> {
    check_role(&state, &project, MinRole::Editor).await?;
    let manifest_arc = state.project_manager.get_manifest_for_ui(&project)?;
    let (new_done, data) = {
        let mut manifest = manifest_arc.write().await;
        let done = manifest.toggle_todo(&todo_id)?;
        let data = manifest.save();
        (done, data)
    };
    state
        .project_manager
        .persistence()
        .save_manifest(&project, &data)
        .await?;
    Ok(new_done)
}

#[tauri::command]
async fn remove_project_todo(
    state: State<'_, AppState>,
    project: String,
    todo_id: String,
) -> Result<(), CoreError> {
    let manifest_arc = state.project_manager.get_manifest_for_ui(&project)?;
    let data = {
        let mut manifest = manifest_arc.write().await;
        manifest.remove_todo(&todo_id)?;
        manifest.save()
    };
    state
        .project_manager
        .persistence()
        .save_manifest(&project, &data)
        .await
}

#[tauri::command]
async fn update_project_todo(
    state: State<'_, AppState>,
    project: String,
    todo_id: String,
    text: String,
) -> Result<(), CoreError> {
    let manifest_arc = state.project_manager.get_manifest_for_ui(&project)?;
    let data = {
        let mut manifest = manifest_arc.write().await;
        manifest.update_todo_text(&todo_id, &text)?;
        manifest.save()
    };
    state
        .project_manager
        .persistence()
        .save_manifest(&project, &data)
        .await
}

#[tauri::command]
async fn list_project_todos(
    state: State<'_, AppState>,
    project: String,
) -> Result<Vec<notes_core::TodoItem>, CoreError> {
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
async fn get_image(
    state: State<'_, AppState>,
    hash: String,
) -> Result<Response, CoreError> {
    let data = state
        .blob_store
        .read(&hash)
        .await
        .map_err(|e| CoreError::InvalidData(format!("image read failed: {e}")))?;
    Ok(Response::new(InvokeResponseBody::Raw(data)))
}

/// Check if a blob exists locally.
#[tauri::command]
async fn has_image(
    state: State<'_, AppState>,
    hash: String,
) -> Result<bool, CoreError> {
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
    let doc_arc = state.project_manager.doc_store().get_doc(&doc_id)?;
    state.sync_engine.register_doc(doc_id, doc_arc);
    // Populate ACL for this doc from the project's peer list
    populate_doc_acl(&state, &project, doc_id).await;
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
    state.sync_engine.unregister_doc(&doc_id);
    state.project_manager.delete_note(&project, &doc_id).await
}

#[tauri::command]
async fn rename_note(
    state: State<'_, AppState>,
    project: String,
    doc_id: DocId,
    new_path: String,
) -> Result<(), CoreError> {
    check_role(&state, &project, MinRole::Editor).await?;
    state
        .project_manager
        .rename_note(&project, &doc_id, &new_path)
        .await
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
            state
                .sync_engine
                .store_signature(doc_id, hash.clone(), sig);
        }
    }

    // Mark doc as seen after local edits, but keep disk I/O outside the doc lock.
    mark_seen_heads_best_effort(&state.project_manager, &project, doc_id, applied.current_heads).await;

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
    state
        .project_manager
        .compact_doc(&project, &doc_id)
        .await?;
    // Invalidate persisted sync states and signatures for this doc (compaction changes internal state)
    state.sync_state_store.delete_all_for_doc(&doc_id).await;
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
async fn get_device_actor_id(
    state: State<'_, AppState>,
) -> Result<String, CoreError> {
    Ok(state.device_actor_hex.clone())
}

/// Get all versions for a document.
#[tauri::command]
async fn get_doc_versions(
    state: State<'_, AppState>,
    doc_id: DocId,
) -> Result<Vec<notes_core::Version>, CoreError> {
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
        return Err(CoreError::InvalidInput("no significant changes to version".into()));
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

    let snapshot_to_store = if let Ok(epoch_mgr_arc) = state.project_manager.get_epoch_keys(&project) {
        let mgr = epoch_mgr_arc.read().await;
        if let Ok(key) = mgr.current_key() {
            let doc_id_bytes = *doc_id.as_bytes();
            match notes_crypto::encrypt_snapshot(key.as_bytes(), &doc_id_bytes, mgr.current_epoch(), &snapshot_raw) {
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

/// Get the text content of a document at a specific version's heads.
#[tauri::command]
async fn get_version_text(
    state: State<'_, AppState>,
    project: String,
    doc_id: DocId,
    version_id: String,
) -> Result<String, CoreError> {
    // Get version info and snapshot from store (short lock, no await)
    let (heads, snapshot_data) = {
        let store = require_version_store(&state)?;
        let store = store
            .lock()
            .map_err(|_| CoreError::InvalidData("version store lock poisoned".into()))?;

        let version = store
            .get_version(&version_id)?
            .ok_or_else(|| CoreError::InvalidData("version not found".into()))?;

        let heads = notes_core::version::strings_to_heads(&version.heads);
        let snapshot = store.get_snapshot(&version_id)?;
        (heads, snapshot)
    }; // store lock dropped here

    // Try from live Automerge doc first
    state.project_manager.open_doc(&project, &doc_id).await?;
    let doc_arc = state.project_manager.doc_store().get_doc(&doc_id)?;
    let mut doc = doc_arc.write().await;

    if let Ok(text) = notes_core::version::get_text_at(&mut doc, &heads) {
        if !text.is_empty() || heads.is_empty() {
            return Ok(text);
        }
    }

    // Fall back to snapshot (try decrypting if it's encrypted)
    if let Some(data) = snapshot_data {
        // Try to decrypt the snapshot if epoch keys are available
        let snapshot_bytes = if let Ok(epoch_mgr_arc) = state.project_manager.get_epoch_keys(&project) {
            let mgr = epoch_mgr_arc.read().await;
            if let Ok(key) = mgr.current_key() {
                let doc_id_bytes = *doc_id.as_bytes();
                notes_crypto::decrypt_snapshot(key.as_bytes(), &doc_id_bytes, &data)
                    .map(|(_, plaintext)| plaintext)
                    .unwrap_or(data) // Fall back to raw (pre-encryption snapshot)
            } else {
                data
            }
        } else {
            data
        };

        if let Ok(snapshot_doc) = automerge::AutoCommit::load(&snapshot_bytes) {
            use automerge::ReadDoc as _;
            if let Some((automerge::Value::Object(automerge::ObjType::Text), text_id)) =
                snapshot_doc.get(automerge::ROOT, "text")?
            {
                return Ok(snapshot_doc.text(&text_id)?);
            }
        }
    }

    Ok(String::new())
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

    let (heads, snapshot_data) = {
        let store = require_version_store(&state)?;
        let store = store
            .lock()
            .map_err(|_| CoreError::InvalidData("version store lock poisoned".into()))?;

        let version = store
            .get_version(&version_id)?
            .ok_or_else(|| CoreError::InvalidData("version not found".into()))?;

        let heads = notes_core::version::strings_to_heads(&version.heads);
        let snapshot_data = store.get_snapshot(&version_id)?;
        (heads, snapshot_data)
    };

    // Decrypt snapshot if encrypted
    let decrypted_snapshot = if let Some(ref data) = snapshot_data {
        if let Ok(epoch_mgr_arc) = state.project_manager.get_epoch_keys(&project) {
            let mgr = epoch_mgr_arc.read().await;
            if let Ok(key) = mgr.current_key() {
                let doc_id_bytes = *doc_id.as_bytes();
                Some(
                    notes_crypto::decrypt_snapshot(key.as_bytes(), &doc_id_bytes, data)
                        .map(|(_, plaintext)| plaintext)
                        .unwrap_or_else(|_| data.clone()), // fallback to raw
                )
            } else {
                snapshot_data.clone()
            }
        } else {
            snapshot_data.clone()
        }
    } else {
        None
    };

    state.project_manager.open_doc(&project, &doc_id).await?;
    let doc_arc = state.project_manager.doc_store().get_doc(&doc_id)?;
    let mut doc = doc_arc.write().await;

    notes_core::version::restore_to_version(
        &mut doc,
        &heads,
        decrypted_snapshot.as_deref(),
    )?;
    drop(doc);

    // Mark seen so restore doesn't appear as "unseen changes"
    let current_heads = {
        let doc_arc = state.project_manager.doc_store().get_doc(&doc_id)?;
        let mut doc = doc_arc.write().await;
        doc.get_heads()
            .iter()
            .map(|head| head.to_string())
            .collect::<Vec<_>>()
    };

    mark_seen_heads_best_effort(&state.project_manager, &project, doc_id, current_heads).await;

    state.project_manager.doc_store().mark_dirty(&doc_id);
    let _ = state.sync_trigger.send((project, doc_id)).await;
    Ok(())
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

    // Remove ACL entries for all docs in this project
    if let Ok(files) = state.project_manager.list_files(&project).await {
        for file in files {
            state.sync_engine.remove_peer_role(file.id, &peer_id);
        }
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

    emit_sync_status(
        &state.app_handle,
        doc_id,
        sync_state,
        unsent_after,
    );

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
) -> Result<Vec<PeerStatusSummary>, CoreError> {
    notes_core::validate_project_name(&project)?;

    // Get manifest peer metadata for alias/role lookup
    let manifest_peers = state
        .project_manager
        .get_project_peers(&project)
        .await
        .unwrap_or_default();
    let peer_map: std::collections::HashMap<String, notes_core::PeerInfo> = manifest_peers
        .into_iter()
        .map(|p| (p.peer_id.clone(), p))
        .collect();

    let mut statuses = Vec::new();
    for meta in peer_map.values() {
        let connected = meta
            .peer_id
            .parse::<iroh::EndpointId>()
            .ok()
            .map(|peer_id| is_network_ready(&state) && state.peer_manager.is_peer_connected(&peer_id))
            .unwrap_or(false);
        statuses.push(PeerStatusSummary {
            peer_id: meta.peer_id.clone(),
            connected,
            alias: Some(meta.alias.clone()),
            role: Some(meta.role),
            active_doc: None,
        });
    }

    Ok(statuses)
}

// ── Presence Commands ────────────────────────────────────────────────

/// Broadcast a cursor/presence update to peers in a project.
#[tauri::command]
async fn broadcast_presence(
    state: State<'_, AppState>,
    _project: String,
    active_doc: Option<DocId>,
    cursor_pos: Option<u64>,
    selection: Option<(u64, u64)>,
) -> Result<(), CoreError> {
    require_network_ready(&state)?;
    let settings = notes_core::AppSettings::load(
        state.project_manager.persistence().base_dir(),
    )
    .await;

    // Emit presence event for the frontend
    let _ = state.app_handle.emit(
        events::event_names::PRESENCE_UPDATE,
        events::PresenceEvent {
            peer_id: state.local_peer_id.clone(),
            alias: settings.display_name,
            active_doc,
            cursor_pos,
            selection,
        },
    );

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
async fn get_settings(
    state: State<'_, AppState>,
) -> Result<notes_core::AppSettings, CoreError> {
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
    let manifest_arc = state.project_manager.get_manifest_for_ui(&project)?;
    let (manifest_data, project_id) = {
        let mut manifest = manifest_arc.write().await;
        // Ensure owner is set before sharing
        let current_owner = manifest.get_owner().unwrap_or_default();
        if current_owner.is_empty() {
            manifest.set_owner(&my_peer_id)?;
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
    let expires_at = chrono::Utc::now() + chrono::Duration::from_std(invite_ttl)
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
async fn resume_pending_joins_cmd(state: State<'_, AppState>) -> Result<(), CoreError> {
    require_network_ready(&state)?;
    resume_join_sessions(
        Arc::clone(&state.join_session_store),
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

/// Check if a newer version is available on GitHub Releases.
/// Returns Some(UpdateInfo) if an update exists, None if up to date.
/// The Update object is stored in PendingUpdate for install_update to use.
#[tauri::command]
async fn check_for_update(
    app: AppHandle,
    pending: State<'_, PendingUpdate>,
) -> Result<Option<UpdateInfo>, String> {
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

    // Downloads the .app.tar.gz, verifies the minisign signature
    // against the pubkey in tauri.conf.json, extracts it, and replaces
    // the running .app bundle on macOS.
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
    getrandom::fill(&mut key_bytes)
        .map_err(|e| format!("failed to generate random key: {e}"))?;
    let key = iroh::SecretKey::from_bytes(&key_bytes);

    // Store in keystore (OS keychain on macOS, file with 0o600 elsewhere)
    keystore.store_key(KEY_NAME, &key.to_bytes())?;

    log::info!("Generated new peer identity, stored in keystore");
    Ok(key)
}

fn start_deferred_networking(app_handle: AppHandle) {
    let (endpoint, sync_engine, peer_manager, invite_handler, project_manager, join_session_store, sync_receiver, unsent_changes, sync_trigger) = {
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
            Arc::clone(&state.invite_handler),
            Arc::clone(&state.project_manager),
            Arc::clone(&state.join_session_store),
            sync_receiver,
            Arc::clone(&state.unsent_changes),
            state.sync_trigger.clone(),
        )
    };
    let app_handle_clone = app_handle;

    tauri::async_runtime::spawn(async move {
        let Some(mut sync_rx) = sync_receiver else {
            let managed = app_handle_clone.state::<AppState>();
            set_network_status(&managed, NetworkStatus::Failed("sync worker was already started".into()));
            log::error!("Deferred networking startup failed: sync worker receiver unavailable");
            return;
        };

        log::info!("Starting deferred networking runtime");

        let router = Router::builder(endpoint.clone())
            .accept(NOTES_SYNC_ALPN, Arc::clone(&sync_engine))
            .accept(INVITE_ALPN, Arc::clone(&invite_handler))
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
                peer_mgr.monitoring_loop(Duration::from_millis(interval_ms)).await;
            }
        });

        tauri::async_runtime::spawn({
            let peer_mgr = Arc::clone(&peer_manager);
            let handle = app_handle_clone.clone();
            let debounce_ms = sync_debounce_ms();
            let unsent_changes = Arc::clone(&unsent_changes);
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
                                entry.synced_changes = entry.synced_changes.max(sync_checkpoint).min(entry.local_changes);
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
                                map.get(&doc_id)
                                    .map(|entry| entry.local_changes.saturating_sub(entry.synced_changes))
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
            let sync_tx = sync_trigger.clone();
            async move {
                loop {
                    match rx.recv().await {
                        Ok(status_event) => {
                            let connected_peer = reconnect_peer_id(&status_event);
                            log::debug!(
                                "Peer status change: {} -> {:?}",
                                status_event.peer_id,
                                status_event.state
                            );
                            let _ = handle.emit(events::event_names::PEER_STATUS, status_event);

                            if let Some(peer_id) = connected_peer {
                                for project_name in peer_mgr.get_projects_for_peer(&peer_id) {
                                    match pm.list_files(&project_name).await {
                                        Ok(files) => {
                                            let stats = try_queue_reconnect_syncs(&sync_tx, &project_name, &files);
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
            let mut rx = sync_engine.subscribe_remote_changes();
            let handle = app_handle_clone.clone();
            let pm = Arc::clone(&project_manager);
            async move {
                loop {
                    match rx.recv().await {
                        Ok(doc_id) => {
                            log::debug!("Remote change detected for doc {doc_id}");

                            if let Some(project_name) = pm.get_project_for_doc(&doc_id) {
                                if let Ok(owner) = pm.get_project_owner(&project_name).await {
                                    if !owner.is_empty() {
                                        if let Ok(manifest_arc) = pm.get_manifest_for_ui(&project_name) {
                                            let manifest = manifest_arc.read().await;
                                            if let Ok(aliases) = manifest.get_actor_aliases() {
                                                let owner_actor = aliases
                                                    .iter()
                                                    .find(|(_, alias)| alias.as_str() == owner)
                                                    .map(|(actor, _)| actor.clone());
                                                drop(manifest);
                                                if let Some(actor_hex) = owner_actor {
                                                    if let Err(e) = pm
                                                        .validate_manifest_after_sync(&project_name, &[], &actor_hex)
                                                        .await
                                                    {
                                                        log::error!("Manifest validation failed for {project_name}: {e}");
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            let _ = handle.emit(
                                events::event_names::REMOTE_CHANGE,
                                events::RemoteChangeEvent {
                                    doc_id,
                                    peer_id: None,
                                },
                            );
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
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

            let mut sync_engine_raw = SyncEngine::new();
            sync_engine_raw.set_sync_state_store(Arc::clone(&sync_state_store));
            let sync_engine = Arc::new(sync_engine_raw);

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

            log::info!("iroh endpoint bound, id: {}", endpoint.id());

            // Create the PeerManager for managing persistent connections
            let peer_manager = Arc::new(PeerManager::new(
                endpoint.clone(),
                Arc::clone(&sync_engine),
            ));
            let owner_invite_store = Arc::new(OwnerInviteStateStore::new(notes_dir.clone()));
            let join_session_store = Arc::new(JoinSessionStore::new(notes_dir.clone()));

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
            let invite_handler = Arc::new(invite_handler_raw);

            // Auto-sync trigger: debounced channel that syncs with peers on local changes
            let (sync_tx, sync_rx) =
                tokio::sync::mpsc::channel::<(String, DocId)>(256);
            let unsent_changes: Arc<Mutex<HashMap<DocId, UnsentChangesState>>> =
                Arc::new(Mutex::new(HashMap::new()));

            app.manage(AppState {
                project_manager: Arc::clone(&project_manager),
                sync_engine,
                peer_manager,
                invite_handler,
                owner_invite_store,
                join_session_store,
                sync_state_store,
                search_index: Arc::clone(&search_index),
                version_store,
                blob_store: Arc::clone(&blob_store),
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
            get_doc_binary,
            get_doc_text,
            apply_changes,
            save_doc,
            compact_doc,
            get_doc_incremental,
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

    use notes_core::FileType;

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
        tx.try_send(("prefill".into(), uuid::Uuid::new_v4())).unwrap();

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
        tx.try_send(("prefill".into(), uuid::Uuid::new_v4())).unwrap();
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
        assert!(queued.contains(&("project".to_string(), files[0].id)) || queued.contains(&("project".to_string(), files[1].id)));
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
        assert_eq!(network_status_message(&NetworkStatus::NotStarted), "networking is still starting");
        assert_eq!(network_status_message(&NetworkStatus::Starting), "networking is still starting");
        assert_eq!(network_status_message(&NetworkStatus::Ready), "networking is ready");
        assert_eq!(network_status_message(&NetworkStatus::Failed("x".into())), "networking failed to initialize");
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
        let doc_id = project_manager.create_note("shared", "hello.md").await.unwrap();

        let seen_state_path = dir.path().join("shared").join(".p2p").join("seen_state.json");
        tokio::fs::write(&seen_state_path, "{bad json").await.unwrap();

        let unseen = get_unseen_docs_for_project(&project_manager, "shared").await.unwrap();
        assert_eq!(unseen.len(), 1);
        assert_eq!(unseen[0].doc_id, doc_id);

        mark_doc_seen_for_project(&project_manager, "shared", doc_id)
            .await
            .unwrap();

        let repaired = tokio::fs::read_to_string(&seen_state_path).await.unwrap();
        assert!(repaired.contains(&doc_id.to_string()));
    }
}
