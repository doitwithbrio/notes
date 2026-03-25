use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
    pub role: PeerRole,
    pub peer_count: usize,
    pub file_count: usize,
}

/// Frontend-facing peer status record.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerStatusSummary {
    pub peer_id: String,
    pub connected: bool,
    pub alias: Option<String>,
    pub role: Option<PeerRole>,
    pub active_doc: Option<String>,
}
