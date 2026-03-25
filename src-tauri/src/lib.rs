use std::sync::Arc;
use std::time::Duration;

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
    state.project_manager.open_project(&name).await
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
    Ok(())
}

#[tauri::command]
async fn close_doc(
    state: State<'_, AppState>,
    project: String,
    doc_id: DocId,
) -> Result<(), CoreError> {
    state.sync_engine.unregister_doc(&doc_id);
    state.project_manager.close_doc(&project, &doc_id).await
}

#[tauri::command]
async fn delete_note(
    state: State<'_, AppState>,
    project: String,
    doc_id: DocId,
) -> Result<(), CoreError> {
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
    state
        .project_manager
        .apply_changes(&project, &doc_id, &data)
        .await?;

    // Trigger auto-sync with peers (debounced by the receiver)
    let _ = state.sync_trigger.send((project, doc_id)).await;

    Ok(())
}

#[tauri::command]
async fn save_doc(
    state: State<'_, AppState>,
    project: String,
    doc_id: DocId,
) -> Result<(), CoreError> {
    state.project_manager.save_doc(&project, &doc_id).await
}

#[tauri::command]
async fn compact_doc(
    state: State<'_, AppState>,
    project: String,
    doc_id: DocId,
) -> Result<(), CoreError> {
    state
        .project_manager
        .compact_doc(&project, &doc_id)
        .await
}

// ── Unseen Changes Commands ──────────────────────────────────────────

/// Get a list of documents in a project with unseen-change indicators.
/// Returns `[{ docId, path, hasUnseenChanges, lastSeenAt }]`.
#[tauri::command]
async fn get_unseen_docs(
    state: State<'_, AppState>,
    project: String,
) -> Result<Vec<notes_core::UnseenDocInfo>, CoreError> {
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

// ── History Commands ─────────────────────────────────────────────────

/// Get the version history of a document, grouped into editing sessions.
#[tauri::command]
async fn get_doc_history(
    state: State<'_, AppState>,
    project: String,
    doc_id: DocId,
) -> Result<Vec<notes_core::HistorySession>, CoreError> {
    state.project_manager.open_doc(&project, &doc_id).await?;
    let doc_arc = state.project_manager.doc_store().get_doc(&doc_id)?;
    let mut doc = doc_arc.write().await;
    Ok(notes_core::history::get_document_history(&mut doc))
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

/// Add a peer to a project and connect to them.
#[tauri::command]
async fn add_peer(
    state: State<'_, AppState>,
    project: String,
    peer_id_str: String,
) -> Result<(), CoreError> {
    let peer_id: iroh::EndpointId = peer_id_str
        .parse()
        .map_err(|e| CoreError::InvalidInput(format!("invalid peer ID: {e}")))?;

    state.peer_manager.add_peer_to_project(&project, peer_id);

    // Try to connect immediately (best-effort)
    if let Err(e) = state.peer_manager.get_or_connect(peer_id).await {
        log::warn!("Initial connection to peer {peer_id} failed: {e}");
    } else {
        // Emit peer connected event
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

/// Remove a peer from a project.
#[tauri::command]
async fn remove_peer(
    state: State<'_, AppState>,
    project: String,
    peer_id_str: String,
) -> Result<(), CoreError> {
    let peer_id: iroh::EndpointId = peer_id_str
        .parse()
        .map_err(|e| CoreError::InvalidInput(format!("invalid peer ID: {e}")))?;

    state
        .peer_manager
        .remove_peer_from_project(&project, &peer_id);

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
    let _files = state.project_manager.list_files(&project).await?;

    // Get the manifest data to include in the invite
    let manifest_arc = state.project_manager.get_manifest_for_ui(&project)?;
    let mut manifest = manifest_arc.write().await;
    let manifest_data = manifest.save();
    let project_id = manifest.project_id().unwrap_or_default();
    drop(manifest);

    let passphrase = notes_sync::invite::generate_passphrase(6);
    let peer_id = state.endpoint.id().to_string();
    let expires_at = chrono::Utc::now() + chrono::Duration::minutes(10);

    // Register a PendingInvite with real manifest data
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
        derived_key: notes_sync::invite::derive_key_from_passphrase(&passphrase),
        manifest_data,
        invite_role: "editor".to_string(),
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

        // Challenge-response handshake
        let our_challenge = invite::generate_challenge();
        send.write_all(&our_challenge)
            .await
            .map_err(|e| CoreError::InvalidData(format!("send failed: {e}")))?;

        let mut owner_challenge = [0u8; 32];
        recv.read_exact(&mut owner_challenge)
            .await
            .map_err(|e| CoreError::InvalidData(format!("read failed: {e}")))?;

        let our_response = invite::compute_challenge_response(&passphrase, &owner_challenge);
        send.write_all(&our_response)
            .await
            .map_err(|e| CoreError::InvalidData(format!("send failed: {e}")))?;

        let mut owner_response = [0u8; 32];
        recv.read_exact(&mut owner_response)
            .await
            .map_err(|e| CoreError::InvalidData(format!("read failed: {e}")))?;

        if !invite::verify_challenge_response(&passphrase, &our_challenge, &owner_response) {
            return Err(CoreError::InvalidData(
                "Invite verification failed — wrong code".into(),
            ));
        }

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

        let derived_key = invite::derive_key_from_passphrase(&passphrase);
        let plaintext = invite::decrypt_payload(&derived_key, &encrypted)
            .map_err(|e| CoreError::InvalidData(format!("decrypt failed: {e}")))?;

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
            let search_index = notes_core::SearchIndex::open(&search_db_path)
                .map_err(|e| anyhow::anyhow!("Failed to open search index: {e}"))?;
            log::info!("Search index opened at {}", search_db_path.display());

            let search_index = Arc::new(std::sync::Mutex::new(search_index));
            let project_manager = Arc::new(ProjectManager::with_search_index(
                notes_dir.clone(),
                Arc::clone(&search_index),
            ));
            let sync_engine = Arc::new(SyncEngine::new());
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
            {
                let pm = Arc::clone(&peer_manager);
                tauri::async_runtime::spawn(async move {
                    pm.start_monitoring(Duration::from_secs(30));
                });
            }

            // Create the SyncStateStore for persistent sync state
            let sync_state_store = Arc::new(SyncStateStore::new(
                notes_dir.join(".p2p"),
            ));

            // Auto-sync trigger: debounced channel that syncs with peers on local changes
            let (sync_tx, mut sync_rx) =
                tokio::sync::mpsc::channel::<(String, DocId)>(256);
            {
                let peer_mgr = Arc::clone(&peer_manager);
                tauri::async_runtime::spawn(async move {
                    // Simple debounce: drain all pending messages, then sync
                    loop {
                        // Wait for at least one trigger
                        let first = match sync_rx.recv().await {
                            Some(v) => v,
                            None => break, // Channel closed
                        };

                        // Wait 500ms to collect more triggers (debounce)
                        tokio::time::sleep(Duration::from_millis(500)).await;

                        // Drain any buffered triggers into a set
                        let mut to_sync = std::collections::HashSet::new();
                        to_sync.insert(first);
                        while let Ok(item) = sync_rx.try_recv() {
                            to_sync.insert(item);
                        }

                        // Sync each unique (project, doc_id) with peers
                        for (project, doc_id) in to_sync {
                            let results = peer_mgr
                                .sync_doc_with_project_peers(&project, doc_id)
                                .await;
                            let ok = results.iter().filter(|(_, r)| r.is_ok()).count();
                            if ok > 0 {
                                log::debug!("Auto-synced doc {doc_id} with {ok} peers");
                            }
                        }
                    }
                });
            }

            // Spawn a task that forwards SyncEngine remote-change notifications to Tauri events
            {
                let mut rx = sync_engine.subscribe_remote_changes();
                let handle = app_handle.clone();
                tauri::async_runtime::spawn(async move {
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

            app.manage(AppState {
                project_manager: Arc::clone(&project_manager),
                sync_engine,
                peer_manager,
                invite_handler,
                sync_state_store,
                search_index: Arc::clone(&search_index),
                sync_trigger: sync_tx,
                endpoint,
                router,
                app_handle,
            });

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
            get_peer_id,
            get_peer_addr,
            sync_with_peer,
            add_peer,
            remove_peer,
            sync_doc_with_project,
            get_peer_status,
            generate_invite,
            accept_invite,
            // History + Search + Unseen
            get_doc_history,
            search_notes,
            search_project_notes,
            get_unseen_docs,
            mark_doc_seen,
            // Settings
            get_settings,
            update_settings,
            get_doc_degradation,
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
