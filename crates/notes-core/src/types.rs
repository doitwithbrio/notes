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

/// Metadata about a file tracked in the project manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub created: chrono::DateTime<chrono::Utc>,
    #[serde(rename = "type")]
    pub file_type: FileType,
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
pub struct PeerInfo {
    pub role: PeerRole,
    pub alias: String,
    pub since: chrono::DateTime<chrono::Utc>,
}

/// A lightweight description of a document, returned to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocInfo {
    pub id: DocId,
    pub path: String,
    pub file_type: FileType,
    pub created: chrono::DateTime<chrono::Utc>,
}

/// Represents the sync status of a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncStatus {
    Synced,
    Syncing,
    LocalOnly,
}
