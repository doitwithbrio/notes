//! Version history powered by Automerge's built-in change tracking.
//!
//! Changes are grouped into "sessions" — continuous edits by one author
//! with <5 minute gaps between changes.
//!
//! Session IDs are deterministic: derived from the hash of the first
//! Automerge change in the session. This means:
//! - Same session always gets the same ID across calls
//! - Same session gets the same ID on different peers (after sync)
//! - IDs are stable even when new changes extend a session

use automerge::{AutoCommit, ChangeHash, ReadDoc};
use serde::{Deserialize, Serialize};

/// A grouped editing session in the document history.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistorySession {
    /// Deterministic ID derived from the first change's hash (16 hex chars).
    pub id: String,
    /// Actor (author) who made these changes.
    pub actor: String,
    /// When the session started (Unix timestamp in seconds).
    pub started_at: i64,
    /// When the session ended (Unix timestamp in seconds).
    pub ended_at: i64,
    /// Number of individual changes in this session.
    pub change_count: usize,
    /// Total operations across all changes.
    pub op_count: usize,
    /// Hash of the first change in this session (full 64-char hex, for restore targeting).
    pub first_change_hash: String,
    /// Hash of the last change in this session (full 64-char hex, for text_at/diff).
    pub last_change_hash: String,
}

/// Maximum gap between changes to be considered the same session (5 minutes).
const SESSION_GAP_SECS: i64 = 5 * 60;

/// Number of hex characters to use for the session ID (16 = 64 bits).
/// Birthday bound: ~4 billion sessions before 50% collision probability.
const SESSION_ID_HEX_LEN: usize = 16;

/// Derive a deterministic session ID from the first change's hash.
fn session_id_from_hash(hash: &ChangeHash) -> String {
    let full_hex = hash.to_string();
    full_hex[..SESSION_ID_HEX_LEN.min(full_hex.len())].to_string()
}

/// Get the edit history of a document, grouped into sessions.
///
/// Session grouping rules:
/// - Changes from the same actor with <5min gaps are grouped together.
/// - Changes are processed in Automerge's topological order (deterministic).
/// - Session ID is derived from the first change's hash (stable, deterministic).
///
/// Note: requires `&mut AutoCommit` because Automerge 0.7's `get_changes` needs `&mut self`.
pub fn get_document_history(doc: &mut AutoCommit) -> Vec<HistorySession> {
    let changes = doc.get_changes(&[]);

    if changes.is_empty() {
        return vec![];
    }

    let mut sessions: Vec<HistorySession> = Vec::new();

    for change in &changes {
        let actor = change.actor_id().to_hex_string();
        let timestamp = change.timestamp() as i64;
        let ops = change.len();
        let hash = change.hash();

        // Check if this change belongs to the current (most recent) session
        let should_merge = sessions.last().map_or(false, |last: &HistorySession| {
            last.actor == actor && (timestamp - last.ended_at).abs() < SESSION_GAP_SECS
        });

        if should_merge {
            let last = sessions.last_mut().unwrap();
            last.ended_at = timestamp;
            last.change_count += 1;
            last.op_count += ops;
            last.last_change_hash = hash.to_string();
        } else {
            let id = session_id_from_hash(&hash);
            let hash_str = hash.to_string();
            sessions.push(HistorySession {
                id,
                actor: actor.clone(),
                started_at: timestamp,
                ended_at: timestamp,
                change_count: 1,
                op_count: ops,
                first_change_hash: hash_str.clone(),
                last_change_hash: hash_str,
            });
        }
    }

    // Most recent first
    sessions.reverse();
    sessions
}

/// Find a session by its ID and return the change hashes needed for restore/diff.
/// Returns `(heads_before, heads_after)` where:
/// - `heads_before` = the deps of the first change in the session
/// - `heads_after` = [last_change_hash]
pub fn find_session_heads(
    doc: &mut AutoCommit,
    session_id: &str,
) -> Option<(Vec<ChangeHash>, Vec<ChangeHash>)> {
    let changes = doc.get_changes(&[]);
    let sessions = get_document_history(doc);

    let session = sessions.iter().find(|s| s.id == session_id)?;

    // Parse the last change hash for after-heads
    let after_hash: ChangeHash = session.last_change_hash.parse().ok()?;
    let after_heads = vec![after_hash];

    // Find the first change's deps for before-heads
    let first_hash: ChangeHash = session.first_change_hash.parse().ok()?;
    let before_heads = changes
        .iter()
        .find(|c| c.hash() == first_hash)
        .map(|c| c.deps().to_vec())
        .unwrap_or_default();

    Some((before_heads, after_heads))
}

/// Get the text content of a document at specific heads.
/// Uses Automerge's `text_at()` for efficient read-only reconstruction.
pub fn get_text_at_heads(
    doc: &mut AutoCommit,
    heads: &[ChangeHash],
) -> Result<String, automerge::AutomergeError> {
    // Find the text object
    if let Some((automerge::Value::Object(automerge::ObjType::Text), text_id)) =
        doc.get(automerge::ROOT, "text")?
    {
        doc.text_at(&text_id, heads)
    } else {
        Ok(String::new())
    }
}

/// Restore a document to the state at given heads by creating a new change.
/// This is non-destructive: it creates a NEW Automerge change on top of the current heads.
///
/// **v1 limitation**: This restores plain text only. Rich text marks (bold, italic, links,
/// headings, code blocks, etc.) stored as Automerge marks are NOT preserved during restore.
/// When `@automerge/prosemirror` bridge is integrated, this should be updated to use
/// `spans_at()` or `hydrate()` + `update_object()` to preserve rich text structure.
pub fn restore_to_heads(
    doc: &mut AutoCommit,
    target_heads: &[ChangeHash],
) -> Result<(), automerge::AutomergeError> {
    use automerge::transaction::Transactable;

    // Get the text at the target heads
    let old_text = get_text_at_heads(doc, target_heads)?;

    // Find the text object in the current document
    if let Some((automerge::Value::Object(automerge::ObjType::Text), text_id)) =
        doc.get(automerge::ROOT, "text")?
    {
        // Get current text
        let current_text = doc.text(&text_id)?;

        if current_text != old_text {
            // Use a single splice to atomically replace all content.
            // This is better than delete-then-insert for concurrent edit safety:
            // a single splice produces fewer interleaving issues if two peers
            // restore concurrently.
            let current_len = doc.length(&text_id);
            doc.splice_text(&text_id, 0, current_len as isize, &old_text)?;
        }
    }

    // Commit with a descriptive message
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    doc.commit_with(
        automerge::transaction::CommitOptions::default()
            .with_message("Restored to previous version".to_string())
            .with_time(now),
    );

    Ok(())
}

/// Get the number of changes in a document.
pub fn get_change_count(doc: &mut AutoCommit) -> usize {
    doc.get_changes(&[]).len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use automerge::{transaction::Transactable, ObjType};

    fn make_doc_with_text(text: &str) -> AutoCommit {
        let mut doc = AutoCommit::new();
        doc.put(automerge::ROOT, "schemaVersion", 1_u64).unwrap();
        let text_id = doc
            .put_object(automerge::ROOT, "text", ObjType::Text)
            .unwrap();
        doc.splice_text(&text_id, 0, 0, text).unwrap();
        doc
    }

    #[test]
    fn test_empty_history() {
        let mut doc = AutoCommit::new();
        let history = get_document_history(&mut doc);
        assert!(history.is_empty());
    }

    #[test]
    fn test_single_change() {
        let mut doc = make_doc_with_text("Hello");
        let history = get_document_history(&mut doc);
        assert!(!history.is_empty());
        // Session should have a deterministic ID
        assert_eq!(history[0].id.len(), SESSION_ID_HEX_LEN);
        assert!(history[0].id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_session_id_is_deterministic() {
        let mut doc = make_doc_with_text("Hello");
        let history1 = get_document_history(&mut doc);
        let history2 = get_document_history(&mut doc);
        assert_eq!(history1.len(), history2.len());
        for (s1, s2) in history1.iter().zip(history2.iter()) {
            assert_eq!(s1.id, s2.id, "Session IDs must be stable across calls");
        }
    }

    #[test]
    fn test_session_id_survives_new_changes() {
        let mut doc = AutoCommit::new();
        let text_id = doc
            .put_object(automerge::ROOT, "text", ObjType::Text)
            .unwrap();
        doc.splice_text(&text_id, 0, 0, "Hello").unwrap();
        doc.commit();

        let history_before = get_document_history(&mut doc);
        let original_id = history_before[0].id.clone();

        // Add a new change (within same session — timestamp gap is 0)
        doc.splice_text(&text_id, 5, 0, " world").unwrap();
        doc.commit();

        let history_after = get_document_history(&mut doc);

        // The original session should still have the same ID
        let matching = history_after.iter().find(|s| s.id == original_id);
        assert!(
            matching.is_some(),
            "Original session ID must survive new changes being added"
        );
    }

    #[test]
    fn test_first_change_hash_is_full() {
        let mut doc = make_doc_with_text("Hello");
        let history = get_document_history(&mut doc);
        let session = &history[0];
        // Full SHA-256 hash = 64 hex chars
        assert_eq!(session.first_change_hash.len(), 64);
        assert_eq!(session.last_change_hash.len(), 64);
    }

    #[test]
    fn test_change_count() {
        let mut doc = make_doc_with_text("Hello world");
        let count = get_change_count(&mut doc);
        assert!(count > 0);
    }

    #[test]
    fn test_multiple_edits_same_session() {
        let mut doc = AutoCommit::new();
        let text_id = doc
            .put_object(automerge::ROOT, "text", ObjType::Text)
            .unwrap();
        doc.splice_text(&text_id, 0, 0, "Hello").unwrap();
        doc.commit();
        doc.splice_text(&text_id, 5, 0, " world").unwrap();
        doc.commit();

        let history = get_document_history(&mut doc);
        assert!(!history.is_empty());
        let total_changes: usize = history.iter().map(|s| s.change_count).sum();
        assert!(total_changes >= 2);
    }

    #[test]
    fn test_cross_peer_session_ids_match() {
        // Simulate two peers: create doc on peer A, sync to peer B
        let mut doc_a = AutoCommit::new();
        let text_id = doc_a
            .put_object(automerge::ROOT, "text", ObjType::Text)
            .unwrap();
        doc_a.splice_text(&text_id, 0, 0, "Hello from A").unwrap();
        doc_a.commit();

        // "Sync" by saving and loading
        let bytes = doc_a.save();
        let mut doc_b = AutoCommit::load(&bytes).unwrap();

        let history_a = get_document_history(&mut doc_a);
        let history_b = get_document_history(&mut doc_b);

        assert_eq!(history_a.len(), history_b.len());
        for (sa, sb) in history_a.iter().zip(history_b.iter()) {
            assert_eq!(
                sa.id, sb.id,
                "Session IDs must match across peers after sync"
            );
            assert_eq!(sa.first_change_hash, sb.first_change_hash);
        }
    }

    #[test]
    fn test_get_text_at_heads() {
        let mut doc = AutoCommit::new();
        let text_id = doc
            .put_object(automerge::ROOT, "text", ObjType::Text)
            .unwrap();
        doc.splice_text(&text_id, 0, 0, "Version 1").unwrap();
        doc.commit();

        let heads_v1: Vec<ChangeHash> = doc.get_heads().to_vec();

        doc.splice_text(&text_id, 9, 0, " - Version 2 additions")
            .unwrap();
        doc.commit();

        // Current text should be the updated version
        let current = doc.text(&text_id).unwrap();
        assert!(current.contains("Version 2"));

        // Text at old heads should be the original
        let old_text = get_text_at_heads(&mut doc, &heads_v1).unwrap();
        assert_eq!(old_text, "Version 1");
    }

    #[test]
    fn test_restore_to_heads() {
        let mut doc = AutoCommit::new();
        let text_id = doc
            .put_object(automerge::ROOT, "text", ObjType::Text)
            .unwrap();
        doc.splice_text(&text_id, 0, 0, "Original content").unwrap();
        doc.commit();

        let heads_v1: Vec<ChangeHash> = doc.get_heads().to_vec();

        doc.splice_text(&text_id, 16, 0, " with modifications")
            .unwrap();
        doc.commit();

        // Restore to v1
        restore_to_heads(&mut doc, &heads_v1).unwrap();

        let restored_text = doc.text(&text_id).unwrap();
        assert_eq!(restored_text, "Original content");

        // Should have more changes than before (restore is additive)
        assert!(doc.get_changes(&[]).len() >= 3);
    }

    #[test]
    fn test_find_session_heads() {
        let mut doc = AutoCommit::new();
        let text_id = doc
            .put_object(automerge::ROOT, "text", ObjType::Text)
            .unwrap();
        doc.splice_text(&text_id, 0, 0, "Hello").unwrap();
        doc.commit();

        let history = get_document_history(&mut doc);
        assert!(!history.is_empty());

        let result = find_session_heads(&mut doc, &history[0].id);
        assert!(result.is_some());
        let (before, after) = result.unwrap();
        assert!(!after.is_empty());
        // before might be empty for the first session (no deps)
        let _ = before;
    }
}
