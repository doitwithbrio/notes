//! Event types emitted to the frontend via Tauri events.
//!
//! These are serialized as JSON and pushed from the Rust backend
//! to the Svelte frontend via `app_handle.emit()`.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Event names (Tauri event channel names).
pub mod event_names {
    /// A remote peer changed a document. Frontend should fetch the appropriate update path.
    pub const REMOTE_CHANGE: &str = "p2p:remote-change";
    /// Sync status changed for a document.
    pub const SYNC_STATUS: &str = "p2p:sync-status";
    /// A peer's presence was updated (cursor, active doc).
    pub const PRESENCE_UPDATE: &str = "p2p:presence-update";
    /// A peer connected or disconnected.
    pub const PEER_STATUS: &str = "p2p:peer-status";
    /// Invite accept / resume lifecycle updates.
    pub const INVITE_ACCEPT_STATUS: &str = "p2p:invite-accept";
    /// A project was evicted locally (e.g. revocation purge).
    pub const PROJECT_EVICTED: &str = "p2p:project-evicted";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InviteAcceptStage {
    Resuming,
    PayloadStaged,
    CommitConfirmed,
    Finalized,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InviteAcceptSource {
    Interactive,
    Resume,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InviteAcceptEvent {
    pub stage: InviteAcceptStage,
    pub source: InviteAcceptSource,
    pub session_id: String,
    pub owner_peer_id: String,
    pub project_id: String,
    pub project_name: String,
    pub local_project_name: Option<String>,
    pub role: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RemoteChangeMode {
    IncrementalAvailable,
    ViewerSnapshotAvailable,
    MetadataOnly,
}

/// Emitted when a remote peer changes a document.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteChangeEvent {
    /// Project containing the changed document.
    pub project_id: String,
    /// The document that was changed.
    pub doc_id: Uuid,
    /// The peer that made the change (if known).
    pub peer_id: Option<String>,
    /// How the frontend should refresh this change.
    pub mode: RemoteChangeMode,
}

/// Sync status for a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncState {
    /// Document is synced with all peers.
    #[serde(rename = "synced")]
    Synced,
    /// Document is currently syncing.
    #[serde(rename = "syncing")]
    Syncing,
    /// No peers connected — document is local only.
    #[serde(rename = "local-only")]
    LocalOnly,
}

/// Emitted when sync status changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatusEvent {
    pub doc_id: Uuid,
    pub state: SyncState,
    /// Number of unsent changes (0 if synced).
    pub unsent_changes: u32,
}

/// Emitted when a peer's presence changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresenceEvent {
    pub project_id: String,
    pub peer_id: String,
    pub session_id: String,
    pub session_started_at: u64,
    pub seq: u64,
    pub alias: String,
    /// Document the peer is viewing.
    pub active_doc: Option<Uuid>,
    /// Cursor position in the document.
    pub cursor_pos: Option<u64>,
    /// Selection range (anchor, head).
    pub selection: Option<(u64, u64)>,
}

/// Peer connection status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PeerConnectionState {
    Connected,
    Disconnected,
}

/// Emitted when a peer connects or disconnects.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerStatusEvent {
    pub peer_id: String,
    pub state: PeerConnectionState,
    pub alias: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectEvictedEvent {
    pub project_id: String,
    pub project_name: String,
    pub reason: String,
}
