//! Tracks which document changes each device has "seen" (opened and viewed).
//!
//! When a user opens a document, we record the current Automerge heads.
//! When they return to the app, we compare the current heads against the
//! last-seen heads. If they differ, there are unseen changes.
//!
//! Persisted as JSON in `.p2p/seen_state.json` per project.

use std::collections::HashMap;
use std::path::Path;

use automerge::AutoCommit;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::types::DocId;

/// Per-document seen state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocSeenState {
    /// The Automerge heads when the user last viewed this document.
    pub last_seen_heads: Vec<String>,
    /// When the user last viewed this document.
    pub last_seen_at: DateTime<Utc>,
}

/// Per-project seen state (all documents).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectSeenState {
    docs: HashMap<String, DocSeenState>,
}

impl ProjectSeenState {
    fn collect_heads(doc: &mut AutoCommit) -> Vec<String> {
        doc.get_heads()
            .iter()
            .map(|h| hex_encode(h.as_ref()))
            .collect()
    }

    /// Mark a document as "seen" by recording its current heads.
    pub fn mark_seen(&mut self, doc_id: &DocId, doc: &mut AutoCommit) {
        let heads = Self::collect_heads(doc);
        self.mark_seen_heads(doc_id, heads);
    }

    pub fn mark_seen_heads(&mut self, doc_id: &DocId, heads: Vec<String>) {
        self.docs.insert(
            doc_id.to_string(),
            DocSeenState {
                last_seen_heads: heads,
                last_seen_at: Utc::now(),
            },
        );
    }

    /// Check if a document has unseen changes.
    /// Returns true if the doc's current heads differ from the last-seen heads.
    pub fn has_unseen_changes(&self, doc_id: &DocId, doc: &mut AutoCommit) -> bool {
        let current_heads = Self::collect_heads(doc);
        self.has_unseen_changes_from_heads(doc_id, &current_heads)
    }

    pub fn has_unseen_changes_from_heads(&self, doc_id: &DocId, current_heads: &[String]) -> bool {
        match self.docs.get(&doc_id.to_string()) {
            Some(seen) => {
                // Compare head sets (order-independent)
                let mut current_sorted = current_heads.to_vec();
                current_sorted.sort();
                let mut seen_sorted = seen.last_seen_heads.clone();
                seen_sorted.sort();
                current_sorted != seen_sorted
            }
            None => {
                // Never seen — has unseen changes if the doc has any changes at all
                !current_heads.is_empty()
            }
        }
    }

    /// Get the last-seen timestamp for a document.
    pub fn last_seen_at(&self, doc_id: &DocId) -> Option<DateTime<Utc>> {
        self.docs.get(&doc_id.to_string()).map(|s| s.last_seen_at)
    }

    /// Remove tracking for a document (e.g., when deleted).
    pub fn remove(&mut self, doc_id: &DocId) {
        self.docs.remove(&doc_id.to_string());
    }
}

/// Manages seen state persistence for a project.
pub struct SeenStateManager;

impl SeenStateManager {
    /// Load seen state from disk.
    pub async fn load(project_dir: &Path) -> Result<ProjectSeenState, CoreError> {
        let path = project_dir.join(".p2p").join("seen_state.json");
        match tokio::fs::read_to_string(&path).await {
            Ok(json) => serde_json::from_str(&json)
                .map_err(|e| CoreError::InvalidData(format!("bad seen_state.json: {e}"))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ProjectSeenState::default()),
            Err(e) => Err(CoreError::Io(e)),
        }
    }

    /// Save seen state to disk.
    pub async fn save(project_dir: &Path, state: &ProjectSeenState) -> Result<(), CoreError> {
        let path = project_dir.join(".p2p").join("seen_state.json");
        let json = serde_json::to_string_pretty(state)?;
        tokio::fs::create_dir_all(path.parent().unwrap()).await?;
        // Atomic write: tmp + rename
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        let tmp_path = std::path::PathBuf::from(tmp);
        tokio::fs::write(&tmp_path, json).await?;
        tokio::fs::rename(&tmp_path, &path).await?;
        Ok(())
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Information about unseen changes in a document (returned to frontend).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnseenDocInfo {
    pub doc_id: DocId,
    pub path: String,
    pub has_unseen_changes: bool,
    pub last_seen_at: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use automerge::{transaction::Transactable, ObjType, ReadDoc};

    fn make_doc() -> (uuid::Uuid, AutoCommit) {
        let id = uuid::Uuid::new_v4();
        let mut doc = AutoCommit::new();
        doc.put_object(automerge::ROOT, "text", ObjType::Text)
            .unwrap();
        (id, doc)
    }

    #[test]
    fn test_mark_seen_and_check() {
        let (id, mut doc) = make_doc();
        let mut state = ProjectSeenState::default();

        // Before marking seen, should have unseen changes (doc has initial commit)
        assert!(state.has_unseen_changes(&id, &mut doc));

        // Mark as seen
        state.mark_seen(&id, &mut doc);

        // Now should NOT have unseen changes
        assert!(!state.has_unseen_changes(&id, &mut doc));
    }

    #[test]
    fn test_unseen_after_remote_change() {
        let (id, mut doc) = make_doc();
        let mut state = ProjectSeenState::default();

        // Mark as seen
        state.mark_seen(&id, &mut doc);
        assert!(!state.has_unseen_changes(&id, &mut doc));

        // Simulate a remote change
        let text_id = doc.get(automerge::ROOT, "text").unwrap().unwrap().1;
        doc.splice_text(&text_id, 0, 0, "new text from remote")
            .unwrap();

        // Now should have unseen changes
        assert!(state.has_unseen_changes(&id, &mut doc));
    }

    #[test]
    fn test_last_seen_at() {
        let (id, mut doc) = make_doc();
        let mut state = ProjectSeenState::default();

        assert!(state.last_seen_at(&id).is_none());

        state.mark_seen(&id, &mut doc);
        assert!(state.last_seen_at(&id).is_some());
    }

    #[test]
    fn test_remove_tracking() {
        let (id, mut doc) = make_doc();
        let mut state = ProjectSeenState::default();

        state.mark_seen(&id, &mut doc);
        assert!(state.last_seen_at(&id).is_some());

        state.remove(&id);
        assert!(state.last_seen_at(&id).is_none());
    }

    #[test]
    fn test_serde_roundtrip() {
        let (id, mut doc) = make_doc();
        let mut state = ProjectSeenState::default();
        state.mark_seen(&id, &mut doc);

        let json = serde_json::to_string(&state).unwrap();
        let loaded: ProjectSeenState = serde_json::from_str(&json).unwrap();

        assert!(!loaded.has_unseen_changes(&id, &mut doc));
    }

    #[test]
    fn test_empty_doc_no_unseen() {
        let id = uuid::Uuid::new_v4();
        let mut doc = AutoCommit::new();
        let state = ProjectSeenState::default();

        assert!(!state.has_unseen_changes(&id, &mut doc));
    }
}
