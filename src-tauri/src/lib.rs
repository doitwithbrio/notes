use std::sync::Arc;
use std::time::Duration;

use automerge::ReadDoc;
use iroh::endpoint::Endpoint;
use iroh::protocol::Router;
use notes_core::{
    CoreError, DocId, DocInfo, PeerRole, PeerStatusSummary, ProjectManager, ProjectSummary,
};
use notes_sync::events;
use notes_sync::invite::{InviteHandler, INVITE_ALPN};
use notes_sync::peer_manager::PeerManager;
use notes_sync::sync_engine::{SyncEngine, NOTES_SYNC_ALPN};
use notes_sync::SyncStateStore;
use serde::{Deserialize, Serialize};
use tauri::ipc::{InvokeResponseBody, Response};
use tauri::{Emitter, Manager, RunEvent, State};

/// Shared app state accessible from all Tauri commands.
struct AppState {
    project_manager: Arc<ProjectManager>,
    sync_engine: Arc<SyncEngine>,
    peer_manager: Arc<PeerManager>,
    invite_handler: Arc<InviteHandler>,
    #[allow(dead_code)] // Used by sync sessions — wired via SyncEngine in Phase 2+
    sync_state_store: Arc<SyncStateStore>,
    search_index: Arc<std::sync::Mutex<notes_core::SearchIndex>>,
    version_store: Arc<std::sync::Mutex<notes_core::VersionStore>>,
    /// Stable device actor ID (hex string) for the frontend to use.
    device_actor_hex: String,
    /// Channel to trigger auto-sync when documents change.
    sync_trigger: tokio::sync::mpsc::Sender<(String, DocId)>,
    endpoint: Endpoint,
    router: Router,
    app_handle: tauri::AppHandle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GenerateInviteResult {
    passphrase: String,
    peer_id: String,
    expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AcceptInviteResult {
    project_id: String,
    project_name: String,
    role: String,
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
    let my_peer_id = state.endpoint.id().to_string();
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
    state.project_manager.create_project(&name).await
}

#[tauri::command]
async fn list_project_summaries(
    state: State<'_, AppState>,
) -> Result<Vec<ProjectSummary>, CoreError> {
    let my_peer_id = state.endpoint.id().to_string();
    let projects = state.project_manager.list_projects().await?;
    let mut summaries = Vec::new();

    for name in projects {
        let _ = state.project_manager.open_project(&name).await;
        let file_count = state
            .project_manager
            .list_files(&name)
            .await
            .map(|f| f.len())
            .unwrap_or(0);

        let peer_count = state.peer_manager.get_project_peers(&name).len();
        let shared = peer_count > 0;

        // Check ownership from manifest
        let role = if let Ok(owner) = state.project_manager.get_project_owner(&name).await {
            if owner == my_peer_id { PeerRole::Owner } else { PeerRole::Editor }
        } else {
            PeerRole::Owner // Local-only project, user is owner
        };

        let path = state
            .project_manager
            .persistence()
            .project_dir(&name)
            .to_string_lossy()
            .to_string();

        summaries.push(ProjectSummary {
            name,
            path,
            shared,
            role,
            peer_count,
            file_count,
        });
    }

    Ok(summaries)
}

#[tauri::command]
async fn open_project(
    state: State<'_, AppState>,
    name: String,
) -> Result<(), CoreError> {
    // Load epoch keys FIRST so open_project_databases() can derive SQLCipher keys
    let _ = state.project_manager.load_epoch_keys(&name).await;

    state.project_manager.open_project(&name).await?;

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

    // Emit initial sync status for this document
    let peer_count = state.peer_manager.get_project_peers(&project).len();
    let connected_count = state.peer_manager.active_connection_count();
    let sync_state = if peer_count == 0 {
        events::SyncState::LocalOnly // No peers → local project
    } else if connected_count > 0 {
        events::SyncState::Synced    // Peers connected → synced
    } else {
        events::SyncState::Syncing   // Peers registered but not yet connected
    };
    let _ = state.app_handle.emit(
        events::event_names::SYNC_STATUS,
        events::SyncStatusEvent {
            doc_id,
            state: sync_state,
            unsent_changes: 0,
        },
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
    // Capture heads before applying so we can sign new changes
    let heads_before = {
        let doc_arc = state.project_manager.doc_store().get_doc(&doc_id)?;
        let mut doc = doc_arc.write().await;
        doc.get_heads().to_vec()
    };

    state
        .project_manager
        .apply_changes(&project, &doc_id, &data)
        .await?;

    // Sign locally-created changes with the device's Ed25519 key.
    // These signatures are stored in the SyncEngine and transmitted
    // as sidecar SignatureBatch messages during sync.
    {
        let doc_arc = state.project_manager.doc_store().get_doc(&doc_id)?;
        let mut doc = doc_arc.write().await;
        let secret_key = state.endpoint.secret_key();
        let new_changes = doc.get_changes(&heads_before);
        for change in &new_changes {
            let hash = change.hash();
            let raw_bytes = change.raw_bytes();
            let signed = notes_crypto::SignedChange::sign(&secret_key, raw_bytes);
            let sig = notes_sync::protocol::ChangeSignature {
                change_hash: hash.to_string(),
                author: signed.author,
                signature: signed.signature,
            };
            state
                .sync_engine
                .store_signature(doc_id, hash.to_string(), sig);
        }

        // Mark doc as seen after local edits
        let project_dir = state.project_manager.persistence().project_dir(&project);
        let mut seen_state = notes_core::SeenStateManager::load(&project_dir).await?;
        seen_state.mark_seen(&doc_id, &mut doc);
        notes_core::SeenStateManager::save(&project_dir, &seen_state).await?;
    }

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
    // Invalidate persisted sync states for this doc (compaction changes internal state)
    state.sync_state_store.delete_all_for_doc(&doc_id).await;
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
    notes_core::validate_project_name(&project)?;
    // Load seen state for this project
    let project_dir = state
        .project_manager
        .persistence()
        .project_dir(&project);
    let seen_state = notes_core::SeenStateManager::load(&project_dir).await?;

    // List all files
    let files = state.project_manager.list_files(&project).await?;

    let mut results = Vec::new();
    for file in files {
        // We need the doc loaded to check heads — try to load if not already
        if let Err(_) = state.project_manager.open_doc(&project, &file.id).await {
            // Can't load — report as no unseen
            results.push(notes_core::UnseenDocInfo {
                doc_id: file.id,
                path: file.path,
                has_unseen_changes: false,
                last_seen_at: seen_state.last_seen_at(&file.id),
            });
            continue;
        }

        let doc_arc = state.project_manager.doc_store().get_doc(&file.id)?;
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

/// Mark a document as "seen" (user has opened and viewed it).
/// Call this when the frontend opens a document.
#[tauri::command]
async fn mark_doc_seen(
    state: State<'_, AppState>,
    project: String,
    doc_id: DocId,
) -> Result<(), CoreError> {
    state.project_manager.open_doc(&project, &doc_id).await?;

    let doc_arc = state.project_manager.doc_store().get_doc(&doc_id)?;
    let mut doc = doc_arc.write().await;

    let project_dir = state
        .project_manager
        .persistence()
        .project_dir(&project);

    let mut seen_state = notes_core::SeenStateManager::load(&project_dir).await?;
    seen_state.mark_seen(&doc_id, &mut doc);
    notes_core::SeenStateManager::save(&project_dir, &seen_state).await?;

    Ok(())
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
    let store = state
        .version_store
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
    let store = state
        .version_store
        .lock()
        .map_err(|_| CoreError::InvalidData("version store lock poisoned".into()))?;

    let prev_heads = store
        .get_latest_version(&doc_id)?
        .map(|v| notes_core::version::strings_to_heads(&v.heads))
        .unwrap_or_default();

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
    let used_names = store.get_used_names(&doc_id)?;
    let name = notes_core::version::unique_creature_name(&version_id, &used_names);

    let seq = store.next_seq(&doc_id)?;
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

    // Save an Automerge snapshot for rich text restore
    let snapshot = {
        let mut snapshot_doc = doc.clone();
        snapshot_doc.save()
    };

    store.store_version(&version, Some(&snapshot))?;

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
        let store = state
            .version_store
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

    // Fall back to snapshot
    if let Some(data) = snapshot_data {
        if let Ok(snapshot_doc) = automerge::AutoCommit::load(&data) {
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
        let store = state
            .version_store
            .lock()
            .map_err(|_| CoreError::InvalidData("version store lock poisoned".into()))?;

        let version = store
            .get_version(&version_id)?
            .ok_or_else(|| CoreError::InvalidData("version not found".into()))?;

        let heads = notes_core::version::strings_to_heads(&version.heads);
        let snapshot_data = store.get_snapshot(&version_id)?;
        (heads, snapshot_data)
    };

    state.project_manager.open_doc(&project, &doc_id).await?;
    let doc_arc = state.project_manager.doc_store().get_doc(&doc_id)?;
    let mut doc = doc_arc.write().await;

    notes_core::version::restore_to_version(
        &mut doc,
        &heads,
        snapshot_data.as_deref(),
    )?;

    // Mark seen so restore doesn't appear as "unseen changes"
    let project_dir = state.project_manager.persistence().project_dir(&project);
    let mut seen_state = notes_core::SeenStateManager::load(&project_dir).await?;
    seen_state.mark_seen(&doc_id, &mut doc);
    notes_core::SeenStateManager::save(&project_dir, &seen_state).await?;

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
    Ok(state.endpoint.id().to_string())
}

#[tauri::command]
async fn get_peer_addr(state: State<'_, AppState>) -> Result<String, CoreError> {
    Ok(format!("{:?}", state.endpoint.addr()))
}

#[tauri::command]
async fn sync_with_peer(
    state: State<'_, AppState>,
    peer_addr: String,
    doc_id: DocId,
) -> Result<(), CoreError> {
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
    let peer_id: iroh::EndpointId = peer_id_str
        .parse()
        .map_err(|e| CoreError::InvalidInput(format!("invalid peer ID: {e}")))?;

    // Remove from PeerManager
    state
        .peer_manager
        .remove_peer_from_project(&project, &peer_id);

    // Remove from manifest and save
    {
        let manifest_arc = state.project_manager.get_manifest_for_ui(&project)?;
        let mut manifest = manifest_arc.write().await;
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
        log::warn!("Epoch key ratchet failed for {project}: {e}");
    }

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
    // Emit syncing status
    let _ = state.app_handle.emit(
        events::event_names::SYNC_STATUS,
        events::SyncStatusEvent {
            doc_id,
            state: events::SyncState::Syncing,
            unsent_changes: 0,
        },
    );

    let results = state
        .peer_manager
        .sync_doc_with_project_peers(&project, doc_id)
        .await;

    let success_count = results.iter().filter(|(_, r)| r.is_ok()).count();
    let fail_count = results.iter().filter(|(_, r)| r.is_err()).count();

    // Emit final status
    let sync_state = if success_count > 0 {
        events::SyncState::Synced
    } else if fail_count > 0 {
        events::SyncState::LocalOnly
    } else {
        events::SyncState::LocalOnly
    };

    let _ = state.app_handle.emit(
        events::event_names::SYNC_STATUS,
        events::SyncStatusEvent {
            doc_id,
            state: sync_state,
            unsent_changes: 0,
        },
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
    let peers = state.peer_manager.get_project_peers(&project);

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
    for peer_id in peers {
        let connected = state.peer_manager.is_peer_connected(&peer_id);
        let peer_id_str = peer_id.to_string();
        let meta = peer_map.get(&peer_id_str);

        statuses.push(PeerStatusSummary {
            peer_id: peer_id_str,
            connected,
            alias: meta.map(|m| m.alias.clone()),
            role: meta.map(|m| m.role),
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
    let settings = notes_core::AppSettings::load(
        state.project_manager.persistence().base_dir(),
    )
    .await;

    // Emit presence event for the frontend
    let _ = state.app_handle.emit(
        events::event_names::PRESENCE_UPDATE,
        events::PresenceEvent {
            peer_id: state.endpoint.id().to_string(),
            alias: settings.display_name,
            active_doc,
            cursor_pos,
            selection,
        },
    );

    Ok(())
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
    settings.save(&notes_dir).await
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
) -> Result<GenerateInviteResult, CoreError> {
    notes_core::validate_project_name(&project)?;
    // Only the owner (or first sharer) can generate invites
    check_role(&state, &project, MinRole::Owner).await?;
    let _files = state.project_manager.list_files(&project).await?;

    // Set owner in manifest (if not already set) and get manifest data
    let my_peer_id = state.endpoint.id().to_string();
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

    // Initialize epoch keys for the project (if not already done)
    state
        .project_manager
        .init_epoch_keys(&project)
        .await?;

    // Get or create X25519 keypair for key wrapping
    let keys_dir = state
        .project_manager
        .persistence()
        .project_dir(&project)
        .join(".p2p")
        .join("keys");
    std::fs::create_dir_all(&keys_dir).ok();
    let keystore = notes_crypto::KeyStore::new(keys_dir);
    let (_owner_x25519_secret, owner_x25519_public) = keystore
        .get_or_create_x25519("x25519-identity")
        .map_err(|e| CoreError::InvalidData(format!("X25519 key generation failed: {e}")))?;

    // Wrap the current epoch key for the invitee.
    // Get the current epoch key to include in the invite payload.
    // The epoch key is transmitted inside the SPAKE2-encrypted payload, so it's
    // protected by the session key. For subsequent key rotations (after peer removal),
    // X25519 ECDH wrapping is used to distribute new keys.
    let (current_epoch, epoch_key_bytes) = if let Ok(epoch_mgr_arc) = state.project_manager.get_epoch_keys(&project) {
        let mgr = epoch_mgr_arc.read().await;
        let epoch = mgr.current_epoch();
        let key = mgr.current_key().ok().map(|k| *k.as_bytes());
        (epoch, key)
    } else {
        (0, None)
    };

    let passphrase = notes_sync::invite::generate_passphrase(6);
    let peer_id = state.endpoint.id().to_string();
    let expires_at = chrono::Utc::now() + chrono::Duration::minutes(10);

    // Register a PendingInvite with real manifest data + X25519 info
    let pending = notes_sync::invite::PendingInvite {
        code: notes_sync::invite::InviteCode {
            passphrase: passphrase.clone(),
            peer_id: peer_id.clone(),
            expires_at,
        },
        created_at: std::time::Instant::now(),
        attempts: 0,
        project_name: project.clone(),
        project_id,
        manifest_data,
        invite_role: "editor".to_string(),
        owner_x25519_public: Some(*owner_x25519_public.as_bytes()),
        epoch_key: epoch_key_bytes,
        current_epoch,
    };
    state
        .invite_handler
        .add_pending(passphrase.clone(), pending);

    log::info!("Generated invite for project {project}");

    Ok(GenerateInviteResult {
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
    use notes_sync::invite;

    let peer_id: iroh::EndpointId = owner_peer_id
        .parse()
        .map_err(|e| CoreError::InvalidInput(format!("invalid owner peer ID: {e}")))?;

    // Timeout the entire invite flow (30s)
    let result = tokio::time::timeout(Duration::from_secs(30), async {
        let connection = state
            .endpoint
            .connect(peer_id, invite::INVITE_ALPN)
            .await
            .map_err(|e| CoreError::InvalidInput(format!("connection failed: {e}")))?;

        let (mut send, mut recv) = connection
            .open_bi()
            .await
            .map_err(|e| CoreError::InvalidData(format!("stream open failed: {e}")))?;

        // SPAKE2 handshake
        let (invitee_state, invitee_msg) = invite::start_invitee_handshake(&passphrase);

        // Send our SPAKE2 message (length-prefixed)
        let len = (invitee_msg.len() as u32).to_be_bytes();
        send.write_all(&len)
            .await
            .map_err(|e| CoreError::InvalidData(format!("send spake2 len failed: {e}")))?;
        send.write_all(&invitee_msg)
            .await
            .map_err(|e| CoreError::InvalidData(format!("send spake2 msg failed: {e}")))?;

        // Read owner's SPAKE2 message (length-prefixed)
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

        // Finish handshake to derive shared session key
        let shared_key = invite::finish_handshake(invitee_state, &owner_msg)
            .map_err(|_| CoreError::InvalidData("SPAKE2 handshake failed — wrong code".into()))?;

        // Read encrypted payload
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

        let payload: invite::InvitePayload = serde_json::from_slice(&plaintext)
            .map_err(|e| CoreError::InvalidData(format!("invalid payload: {e}")))?;

        let _ = send.finish();

        // Create the project locally from the received manifest
        let project_name = payload.project_name.clone();
        let pm = Arc::clone(&state.project_manager);

        // Decode manifest hex to bytes
        let manifest_bytes: Vec<u8> = (0..payload.manifest_hex.len())
            .step_by(2)
            .map(|i| {
                u8::from_str_radix(&payload.manifest_hex[i..i + 2], 16)
                    .unwrap_or(0)
            })
            .collect();

        // Create the project directory structure
        pm.create_project(&project_name).await.or_else(|e| {
            // If project already exists, that's OK (might be re-joining)
            if matches!(e, CoreError::ProjectAlreadyExists(_)) {
                Ok(())
            } else {
                Err(e)
            }
        })?;

        // Save the received manifest (overwriting the fresh one)
        if !manifest_bytes.is_empty() {
            pm.persistence()
                .save_manifest(&project_name, &manifest_bytes)
                .await?;
            // Reload the manifest in memory
            let _ = pm.open_project(&project_name).await;
            log::info!("Saved received manifest for project {project_name}");
        }

        // Add the owner as a peer in PeerManager for sync
        state
            .peer_manager
            .add_peer_to_project(&project_name, peer_id);

        // Store owner's X25519 public key, generate our own, and import epoch key
        {
            let keys_dir = pm
                .persistence()
                .project_dir(&project_name)
                .join(".p2p")
                .join("keys");
            std::fs::create_dir_all(&keys_dir).ok();
            let keystore = notes_crypto::KeyStore::new(keys_dir);

            // Store the owner's X25519 public key for future key rotation wrapping
            if !payload.owner_x25519_public_hex.is_empty() {
                if let Ok(owner_pk_bytes) = hex_decode_32(&payload.owner_x25519_public_hex) {
                    keystore.store_key("owner-x25519-public", &owner_pk_bytes).ok();
                }
            }

            // Generate our own X25519 keypair
            let _ = keystore.get_or_create_x25519("x25519-identity");

            // Import the epoch key received from the owner (transmitted inside SPAKE2-encrypted payload)
            if !payload.epoch_key_hex.is_empty() {
                if let Ok(epoch_key_bytes) = hex_decode_32(&payload.epoch_key_hex) {
                    // Create an EpochKeyManager with the received key and store it
                    let mgr = notes_crypto::EpochKeyManager::from_key(
                        payload.epoch,
                        &epoch_key_bytes,
                    );
                    if let Ok(data) = mgr.serialize() {
                        let keychain_name = format!("epoch-keys-{project_name}");
                        keystore.store_key(&keychain_name, &data).ok();
                        log::info!(
                            "Imported epoch key (epoch {}) for project {project_name}",
                            payload.epoch
                        );
                    }
                }
            }
        }

        Ok(AcceptInviteResult {
            project_id: payload.project_id,
            project_name: payload.project_name,
            role: payload.role,
        })
    })
    .await;

    match result {
        Ok(inner) => inner,
        Err(_) => Err(CoreError::InvalidData("invite timed out after 30s".into())),
    }
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Decode a hex string to a 32-byte array.
fn hex_decode_32(hex: &str) -> Result<[u8; 32], CoreError> {
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

/// Populate the SyncEngine ACL for a document from the project's manifest peer list.
/// Also sets Owner role for the local device's peer ID.
async fn populate_doc_acl(state: &AppState, project: &str, doc_id: DocId) {
    use notes_sync::sync_engine::PeerRole as SyncPeerRole;

    // Always allow the local peer (we're always Owner or Editor of our own open docs)
    state
        .sync_engine
        .set_peer_role(doc_id, state.endpoint.id(), SyncPeerRole::Owner);

    // Add all project peers from the manifest
    if let Ok(peers) = state.project_manager.get_project_peers(project).await {
        for peer in peers {
            if let Ok(peer_id) = peer.peer_id.parse::<iroh::EndpointId>() {
                let sync_role = match peer.role {
                    PeerRole::Owner => SyncPeerRole::Owner,
                    PeerRole::Editor => SyncPeerRole::Editor,
                    PeerRole::Viewer => SyncPeerRole::Viewer,
                };
                state.sync_engine.set_peer_role(doc_id, peer_id, sync_role);
            }
        }
    }
}

// ── App Setup ────────────────────────────────────────────────────────

fn resolve_notes_dir() -> Result<std::path::PathBuf, String> {
    if let Ok(dir) = std::env::var("NOTES_DIR") {
        return Ok(std::path::PathBuf::from(dir));
    }
    if let Some(home) = dirs::home_dir() {
        return Ok(home.join("Notes"));
    }
    if let Some(doc_dir) = dirs::document_dir() {
        return Ok(doc_dir.join("P2P Notes"));
    }
    Err("Could not determine a suitable notes directory".to_string())
}

/// Load or generate a persistent iroh SecretKey using the OS keychain.
/// Falls back to file-based storage with restrictive permissions.
fn load_or_create_secret_key(
    notes_dir: &std::path::Path,
) -> Result<iroh::SecretKey, Box<dyn std::error::Error>> {
    let keys_dir = notes_dir.join(".p2p").join("keys");
    let keystore = notes_crypto::KeyStore::new(keys_dir);
    const KEY_NAME: &str = "peer-identity";

    // Try loading existing key
    if keystore.has_key(KEY_NAME) {
        let bytes = keystore.load_key(KEY_NAME)?;
        if bytes.len() == 32 {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            let key = iroh::SecretKey::from_bytes(&arr);
            log::info!("Loaded peer identity from keystore");
            return Ok(key);
        }
        log::warn!("Identity key corrupt, generating new one");
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

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

            // Initialize the full-text search index
            let search_db_path = notes_dir.join(".p2p").join("search.db");
            std::fs::create_dir_all(search_db_path.parent().unwrap()).ok();
            // Note: encryption_key=None for initial open. When a shared project
            // with epoch keys is loaded, the DBs should be re-keyed with
            // PRAGMA rekey. For v1, SQLCipher provides the infrastructure;
            // per-project key derivation is wired when projects are opened.
            let search_index = notes_core::SearchIndex::open(&search_db_path, None)
                .map_err(|e| anyhow::anyhow!("Failed to open search index: {e}"))?;
            log::info!("Search index opened at {}", search_db_path.display());

            // Initialize the version store (SQLite)
            let version_db_path = notes_dir.join(".p2p").join("versions.db");
            let version_store = notes_core::VersionStore::open(&version_db_path, None)
                .map_err(|e| anyhow::anyhow!("Failed to open version store: {e}"))?;
            log::info!("Version store opened at {}", version_db_path.display());

            // Migrate old history data to new version store (one-time)
            match version_store.migrate_from_old_history() {
                Ok(count) if count > 0 => log::info!("Migrated {count} old history sessions to versions"),
                Ok(_) => {},
                Err(e) => log::warn!("History migration failed (non-fatal): {e}"),
            }

            // Load or create stable device actor ID
            let p2p_dir = notes_dir.join(".p2p");
            let device_actor_id = notes_core::version::load_or_create_device_actor_id(&p2p_dir)
                .map_err(|e| anyhow::anyhow!("Failed to load device actor ID: {e}"))?;
            let device_actor_hex = device_actor_id.to_hex_string();
            log::info!("Device actor ID: {}", device_actor_hex);

            let search_index = Arc::new(std::sync::Mutex::new(search_index));
            let version_store = Arc::new(std::sync::Mutex::new(version_store));
            let project_manager = Arc::new(ProjectManager::with_full_config(
                notes_dir.clone(),
                Arc::clone(&search_index),
                device_actor_id,
            ));
            // Create SyncStateStore for persistent sync states
            let sync_state_store = Arc::new(SyncStateStore::new(notes_dir.join(".p2p")));

            let mut sync_engine_raw = SyncEngine::new();
            sync_engine_raw.set_sync_state_store(Arc::clone(&sync_state_store));
            let sync_engine = Arc::new(sync_engine_raw);
            let invite_handler = Arc::new(InviteHandler::new());

            let sync_engine_for_router = Arc::clone(&sync_engine);
            let invite_handler_for_router = Arc::clone(&invite_handler);
            let app_handle = app.handle().clone();

            let (endpoint, router) = tauri::async_runtime::block_on(async {
                let endpoint = Endpoint::builder(iroh::endpoint::presets::N0)
                    .secret_key(secret_key)
                    .address_lookup(iroh::address_lookup::MdnsAddressLookupBuilder::default())
                    .bind()
                    .await
                    .expect("failed to bind iroh endpoint");

                log::info!("iroh endpoint bound, id: {}", endpoint.id());

                let router = Router::builder(endpoint.clone())
                    .accept(NOTES_SYNC_ALPN, sync_engine_for_router)
                    .accept(INVITE_ALPN, invite_handler_for_router)
                    .spawn();

                log::info!("iroh router started");

                (endpoint, router)
            });

            // Create the PeerManager for managing persistent connections
            let peer_manager = Arc::new(PeerManager::new(
                endpoint.clone(),
                Arc::clone(&sync_engine),
            ));
            // Start peer monitoring (15s interval, first tick immediate)
            let _monitor_handle = peer_manager.start_monitoring(Duration::from_secs(15));

            // Auto-sync trigger: debounced channel that syncs with peers on local changes
            let (sync_tx, mut sync_rx) =
                tokio::sync::mpsc::channel::<(String, DocId)>(256);

            // ── Supervised background tasks ──────────────────────────────
            // All long-running tasks are tracked in a JoinSet. A supervisor
            // task monitors for panics/exits and logs them. Restart on panic
            // is not implemented yet (requires factory closures for channel
            // re-subscription), but monitoring ensures failures are visible.
            let mut task_set = tokio::task::JoinSet::new();

            // Task 1: Auto-sync debounce loop
            {
                let peer_mgr = Arc::clone(&peer_manager);
                let handle = app_handle.clone();
                task_set.spawn(async move {
                    loop {
                        let first = match sync_rx.recv().await {
                            Some(v) => v,
                            None => break,
                        };
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        let mut to_sync = std::collections::HashSet::new();
                        to_sync.insert(first);
                        while let Ok(item) = sync_rx.try_recv() {
                            to_sync.insert(item);
                        }
                        for (project, doc_id) in to_sync {
                            let _ = handle.emit(
                                events::event_names::SYNC_STATUS,
                                events::SyncStatusEvent {
                                    doc_id,
                                    state: events::SyncState::Syncing,
                                    unsent_changes: 0,
                                },
                            );

                            let results = peer_mgr
                                .sync_doc_with_project_peers(&project, doc_id)
                                .await;
                            let ok = results.iter().filter(|(_, r)| r.is_ok()).count();
                            let fail = results.iter().filter(|(_, r)| r.is_err()).count();

                            let sync_state = if ok > 0 {
                                events::SyncState::Synced
                            } else if fail > 0 {
                                events::SyncState::LocalOnly
                            } else {
                                events::SyncState::LocalOnly
                            };

                            let _ = handle.emit(
                                events::event_names::SYNC_STATUS,
                                events::SyncStatusEvent {
                                    doc_id,
                                    state: sync_state,
                                    unsent_changes: 0,
                                },
                            );

                            if ok > 0 {
                                log::debug!("Auto-synced doc {doc_id} with {ok} peers");
                            }
                        }
                    }
                });
            }

            // Task 2: Invite accepted handler
            {
                let mut rx = invite_handler.subscribe_accepted();
                let pm = Arc::clone(&project_manager);
                let peer_mgr = Arc::clone(&peer_manager);
                task_set.spawn(async move {
                    loop {
                        match rx.recv().await {
                            Ok(accepted) => {
                                log::info!(
                                    "Invite accepted: adding peer {} to project {}",
                                    accepted.invitee_peer_id,
                                    accepted.project_name
                                );
                                if let Ok(manifest_arc) =
                                    pm.get_manifest_for_ui(&accepted.project_name)
                                {
                                    let mut manifest = manifest_arc.write().await;
                                    let _ = manifest.add_peer(
                                        &accepted.invitee_peer_id,
                                        &accepted.role,
                                        "",
                                    );
                                    let data = manifest.save();
                                    drop(manifest);
                                    let _ = pm
                                        .persistence()
                                        .save_manifest(&accepted.project_name, &data)
                                        .await;
                                }
                                if let Ok(peer_id) =
                                    accepted.invitee_peer_id.parse::<iroh::EndpointId>()
                                {
                                    peer_mgr.add_peer_to_project(
                                        &accepted.project_name,
                                        peer_id,
                                    );
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                                log::warn!("Invite accepted channel lagged by {n}");
                            }
                        }
                    }
                });
            }

            // Task 3: Peer status event forwarder (from PeerManager monitoring loop)
            {
                let mut rx = peer_manager.subscribe_peer_status();
                let handle = app_handle.clone();
                task_set.spawn(async move {
                    loop {
                        match rx.recv().await {
                            Ok(status_event) => {
                                log::debug!(
                                    "Peer status change: {} -> {:?}",
                                    status_event.peer_id,
                                    status_event.state
                                );
                                let _ = handle.emit(
                                    events::event_names::PEER_STATUS,
                                    status_event,
                                );
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                                log::warn!("Peer status channel lagged by {n}");
                            }
                        }
                    }
                });
            }

            // Task 4: Remote change event forwarder
            {
                let mut rx = sync_engine.subscribe_remote_changes();
                let handle = app_handle.clone();
                task_set.spawn(async move {
                    loop {
                        match rx.recv().await {
                            Ok(doc_id) => {
                                log::debug!("Remote change detected for doc {doc_id}");
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
                });
            }

            // Task supervisor: monitors all background tasks for panics/exits
            tauri::async_runtime::spawn(async move {
                while let Some(result) = task_set.join_next().await {
                    match result {
                        Ok(()) => {
                            log::warn!("Background task exited normally (unexpected)");
                        }
                        Err(e) => {
                            if e.is_panic() {
                                log::error!(
                                    "Background task panicked: {}. \
                                     The corresponding subsystem (sync/presence/invite) \
                                     may no longer function until app restart.",
                                    e
                                );
                            } else {
                                log::error!("Background task cancelled: {e}");
                            }
                        }
                    }
                }
                log::info!("All supervised background tasks have exited");
            });

            app.manage(AppState {
                project_manager: Arc::clone(&project_manager),
                sync_engine,
                peer_manager,
                invite_handler,
                sync_state_store,
                search_index: Arc::clone(&search_index),
                version_store: Arc::clone(&version_store),
                device_actor_hex,
                sync_trigger: sync_tx,
                endpoint,
                router,
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
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if let RunEvent::ExitRequested { .. } = &event {
            let state = app_handle.state::<AppState>();
            let pm = Arc::clone(&state.project_manager);
            let peer_mgr = Arc::clone(&state.peer_manager);
            let router = state.router.clone();
            tauri::async_runtime::block_on(async {
                // 1. Save all documents
                pm.shutdown().await;
                // 2. Close peer connections
                peer_mgr.shutdown().await;
                // 3. Shut down the router (stops accepting new connections)
                router.shutdown().await.ok();
                log::info!("Graceful shutdown complete");
            });
        }
    });
}
