use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::editor_schema::EditorDocument;

/// Unique identifier for a document within a project.
pub type DocId = Uuid;

/// Unique identifier for a project.
pub type ProjectId = Uuid;

/// Role a peer can have in a shared project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PeerRole {
    Owner,
    Editor,
    Viewer,
}

/// How the current local device is related to a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectAccessState {
    LocalOwner,
    Owner,
    Editor,
    Viewer,
    IdentityMismatch,
}

/// Type of file in the project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileType {
    Note,
    Asset,
}

/// Information about a peer in a shared project.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerInfo {
    pub peer_id: String,
    pub role: PeerRole,
    pub alias: String,
    pub since: chrono::DateTime<chrono::Utc>,
}

/// A lightweight description of a document, returned to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocInfo {
    pub id: DocId,
    pub path: String,
    pub file_type: FileType,
    pub created: chrono::DateTime<chrono::Utc>,
}

/// Represents the sync status of a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncStatus {
    #[serde(rename = "synced")]
    Synced,
    #[serde(rename = "syncing")]
    Syncing,
    #[serde(rename = "local-only")]
    LocalOnly,
}

/// Frontend-facing summary of a project.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub name: String,
    pub path: String,
    pub shared: bool,
    pub role: Option<PeerRole>,
    pub access_state: ProjectAccessState,
    pub can_edit: bool,
    pub can_manage_peers: bool,
    pub peer_count: usize,
    pub file_count: usize,
}

/// Frontend-facing peer status record.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPeerSummary {
    pub peer_id: String,
    pub connected: bool,
    pub alias: Option<String>,
    pub role: PeerRole,
    pub active_doc: Option<String>,
    pub is_self: bool,
}

// ── Blame Types ──────────────────────────────────────────────────────

/// A contiguous run of text written by the same author.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlameSpan {
    /// Character offset where this span starts.
    pub start: usize,
    /// Character offset where this span ends (exclusive).
    pub end: usize,
    /// Automerge ActorId hex string.
    pub actor: String,
    /// Human-readable alias if known, else None.
    pub alias: Option<String>,
    /// Approximate timestamp (latest change timestamp from this actor).
    pub timestamp: Option<i64>,
}

/// Full blame result for a document.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocBlame {
    /// Total text length in characters.
    pub text_length: usize,
    /// Ordered, non-overlapping spans covering the full text.
    pub spans: Vec<BlameSpan>,
    /// Map of actor hex -> info (alias, color index).
    pub actors: std::collections::HashMap<String, ActorInfo>,
}

/// Metadata about an Automerge actor (author).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorInfo {
    /// Human-readable alias (from manifest or "You").
    pub alias: Option<String>,
    /// Assigned color index (0..N for N distinct actors).
    pub color_index: usize,
}

// ── Todo Types ───────────────────────────────────────────────────────

/// A project-level todo item (stored in manifest, synced via CRDT).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoItem {
    /// UUID of this todo.
    pub id: String,
    /// The todo text.
    pub text: String,
    /// Whether the todo is done.
    pub done: bool,
    /// Peer ID of who created this todo.
    pub created_by: String,
    /// ISO 8601 timestamp.
    pub created_at: String,
    /// Optional link to a specific document.
    pub linked_doc_id: Option<String>,
}

/// Project metadata returned to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMetadata {
    pub name: String,
    pub project_id: String,
    pub emoji: Option<String>,
    pub description: Option<String>,
    pub color: Option<String>,
    pub archived: bool,
    pub created: Option<String>,
    pub owner: Option<String>,
    pub peer_count: usize,
    pub file_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DocumentSourceSchema {
    LegacyText,
    GraphV2,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UnsupportedNodeSummary {
    pub count: usize,
    pub node_types: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocReadSnapshot {
    pub schema_version: u32,
    pub source_schema: DocumentSourceSchema,
    pub needs_migration: bool,
    pub visible_text: String,
    pub editor_document: EditorDocument,
    pub unsupported_nodes: UnsupportedNodeSummary,
    pub can_edit_rich_text: bool,
    pub can_edit_plain_text: bool,
}
