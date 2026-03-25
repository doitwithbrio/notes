//! Durable history storage in SQLite.
//!
//! Archives session metadata and text snapshots before compaction
//! so that version history survives Automerge document compaction.
//!
//! **v1 security limitation**: Text snapshots are stored in plaintext in SQLite,
//! whereas the main Automerge documents are encrypted at rest with XChaCha20-Poly1305.
//! This is a known gap — encrypting snapshots with the current epoch key before insertion
//! is planned for v2. The trade-off is acceptable for v1 because the history.db file
//! lives alongside the main data directory and has the same filesystem permissions.

use std::path::Path;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::history::HistorySession;
use crate::types::DocId;

/// Lightweight snapshot metadata (returned to frontend for UI).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotInfo {
    pub id: i64,
    pub doc_id: String,
    pub captured_at: i64,
    pub change_count: usize,
    pub word_count: usize,
    pub trigger: String,
}

/// SQLite-backed durable history archive.
pub struct HistoryStore {
    conn: Connection,
}

impl HistoryStore {
    /// Open or create the history database at the given path.
    pub fn open(db_path: &Path) -> Result<Self, CoreError> {
        let conn = Connection::open(db_path)
            .map_err(|e| CoreError::InvalidData(format!("history db open failed: {e}")))?;

        // Enable WAL mode for better concurrent read performance
        conn.execute_batch("PRAGMA journal_mode=WAL;")
            .map_err(|e| CoreError::InvalidData(format!("WAL mode failed: {e}")))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS history_sessions (
                id          TEXT NOT NULL,
                doc_id      TEXT NOT NULL,
                project     TEXT NOT NULL,
                actor       TEXT NOT NULL,
                started_at  INTEGER NOT NULL,
                ended_at    INTEGER NOT NULL,
                change_count INTEGER NOT NULL,
                op_count    INTEGER NOT NULL,
                first_change_hash TEXT NOT NULL,
                last_change_hash TEXT NOT NULL,
                PRIMARY KEY (id, doc_id)
            );
            CREATE INDEX IF NOT EXISTS idx_hs_doc_time
                ON history_sessions(doc_id, started_at);

            CREATE TABLE IF NOT EXISTS history_snapshots (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                doc_id          TEXT NOT NULL,
                project         TEXT NOT NULL,
                captured_at     INTEGER NOT NULL,
                change_count    INTEGER NOT NULL,
                word_count      INTEGER NOT NULL,
                text_content    TEXT NOT NULL,
                trigger         TEXT NOT NULL DEFAULT 'compaction'
            );
            CREATE INDEX IF NOT EXISTS idx_snap_doc_time
                ON history_snapshots(doc_id, captured_at);

            CREATE TABLE IF NOT EXISTS compaction_log (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                doc_id          TEXT NOT NULL,
                compacted_at    INTEGER NOT NULL,
                changes_before  INTEGER NOT NULL,
                bytes_before    INTEGER NOT NULL,
                bytes_after     INTEGER NOT NULL,
                snapshot_id     INTEGER REFERENCES history_snapshots(id)
            );",
        )
        .map_err(|e| CoreError::InvalidData(format!("history schema creation failed: {e}")))?;

        Ok(Self { conn })
    }

    /// Open an in-memory history store (for testing).
    pub fn open_in_memory() -> Result<Self, CoreError> {
        let conn = Connection::open_in_memory()
            .map_err(|e| CoreError::InvalidData(format!("in-memory db failed: {e}")))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS history_sessions (
                id          TEXT NOT NULL,
                doc_id      TEXT NOT NULL,
                project     TEXT NOT NULL,
                actor       TEXT NOT NULL,
                started_at  INTEGER NOT NULL,
                ended_at    INTEGER NOT NULL,
                change_count INTEGER NOT NULL,
                op_count    INTEGER NOT NULL,
                first_change_hash TEXT NOT NULL,
                last_change_hash TEXT NOT NULL,
                PRIMARY KEY (id, doc_id)
            );
            CREATE INDEX IF NOT EXISTS idx_hs_doc_time
                ON history_sessions(doc_id, started_at);

            CREATE TABLE IF NOT EXISTS history_snapshots (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                doc_id          TEXT NOT NULL,
                project         TEXT NOT NULL,
                captured_at     INTEGER NOT NULL,
                change_count    INTEGER NOT NULL,
                word_count      INTEGER NOT NULL,
                text_content    TEXT NOT NULL,
                trigger         TEXT NOT NULL DEFAULT 'compaction'
            );

            CREATE TABLE IF NOT EXISTS compaction_log (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                doc_id          TEXT NOT NULL,
                compacted_at    INTEGER NOT NULL,
                changes_before  INTEGER NOT NULL,
                bytes_before    INTEGER NOT NULL,
                bytes_after     INTEGER NOT NULL,
                snapshot_id     INTEGER REFERENCES history_snapshots(id)
            );",
        )
        .map_err(|e| CoreError::InvalidData(format!("history schema creation failed: {e}")))?;

        Ok(Self { conn })
    }

    /// Archive sessions extracted from Automerge before compaction.
    /// Uses INSERT OR REPLACE keyed on (id, doc_id) to handle duplicates.
    pub fn archive_sessions(
        &self,
        sessions: &[HistorySession],
        project: &str,
        doc_id: &DocId,
    ) -> Result<(), CoreError> {
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| CoreError::InvalidData(format!("tx begin failed: {e}")))?;

        {
            let mut stmt = tx
                .prepare(
                    "INSERT OR REPLACE INTO history_sessions
                 (id, doc_id, project, actor, started_at, ended_at, change_count, op_count, first_change_hash, last_change_hash)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                )
                .map_err(|e| CoreError::InvalidData(format!("prepare failed: {e}")))?;

            let doc_id_str = doc_id.to_string();
            for session in sessions {
                stmt.execute(params![
                    session.id,
                    doc_id_str,
                    project,
                    session.actor,
                    session.started_at,
                    session.ended_at,
                    session.change_count,
                    session.op_count,
                    session.first_change_hash,
                    session.last_change_hash,
                ])
                .map_err(|e| CoreError::InvalidData(format!("session insert failed: {e}")))?;
            }
        }

        tx.commit()
            .map_err(|e| CoreError::InvalidData(format!("tx commit failed: {e}")))?;
        Ok(())
    }

    /// Store a text snapshot (typically at compaction time or session boundaries).
    /// Returns the snapshot row ID.
    pub fn store_snapshot(
        &self,
        doc_id: &DocId,
        project: &str,
        text: &str,
        change_count: usize,
        trigger: &str,
    ) -> Result<i64, CoreError> {
        let now = chrono::Utc::now().timestamp();
        let word_count = text.split_whitespace().count();

        self.conn
            .execute(
                "INSERT INTO history_snapshots
                 (doc_id, project, captured_at, change_count, word_count, text_content, trigger)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    doc_id.to_string(),
                    project,
                    now,
                    change_count as i64,
                    word_count as i64,
                    text,
                    trigger,
                ],
            )
            .map_err(|e| CoreError::InvalidData(format!("snapshot insert failed: {e}")))?;

        Ok(self.conn.last_insert_rowid())
    }

    /// Log a compaction event.
    pub fn log_compaction(
        &self,
        doc_id: &DocId,
        changes_before: usize,
        bytes_before: usize,
        bytes_after: usize,
        snapshot_id: i64,
    ) -> Result<(), CoreError> {
        let now = chrono::Utc::now().timestamp();
        self.conn
            .execute(
                "INSERT INTO compaction_log
                 (doc_id, compacted_at, changes_before, bytes_before, bytes_after, snapshot_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    doc_id.to_string(),
                    now,
                    changes_before as i64,
                    bytes_before as i64,
                    bytes_after as i64,
                    snapshot_id,
                ],
            )
            .map_err(|e| CoreError::InvalidData(format!("compaction log failed: {e}")))?;
        Ok(())
    }

    /// Get all archived sessions for a document (most recent first).
    pub fn get_archived_sessions(&self, doc_id: &DocId) -> Result<Vec<HistorySession>, CoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, actor, started_at, ended_at, change_count, op_count, first_change_hash, last_change_hash
                 FROM history_sessions
                 WHERE doc_id = ?1
                 ORDER BY started_at DESC",
            )
            .map_err(|e| CoreError::InvalidData(format!("query failed: {e}")))?;

        let sessions = stmt
            .query_map(params![doc_id.to_string()], |row| {
                Ok(HistorySession {
                    id: row.get(0)?,
                    actor: row.get(1)?,
                    started_at: row.get(2)?,
                    ended_at: row.get(3)?,
                    change_count: row.get(4)?,
                    op_count: row.get(5)?,
                    first_change_hash: row.get(6)?,
                    last_change_hash: row.get(7)?,
                })
            })
            .map_err(|e| CoreError::InvalidData(format!("query failed: {e}")))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(sessions)
    }

    /// Get the full history of a document, merging archived SQLite sessions with live Automerge sessions.
    /// Deduplicates by session ID (deterministic, so identical across both sources).
    pub fn get_merged_history(
        &self,
        doc_id: &DocId,
        live_sessions: Vec<HistorySession>,
    ) -> Result<Vec<HistorySession>, CoreError> {
        let archived = self.get_archived_sessions(doc_id)?;

        let mut seen_ids = std::collections::HashSet::new();
        let mut merged = Vec::new();

        // Live sessions take priority (they have the most current data)
        for session in live_sessions {
            seen_ids.insert(session.id.clone());
            merged.push(session);
        }

        // Add archived sessions not present in live (destroyed by compaction)
        for session in archived {
            if !seen_ids.contains(&session.id) {
                merged.push(session);
            }
        }

        // Sort most recent first
        merged.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        Ok(merged)
    }

    /// Get snapshots for a document (for restore UI).
    pub fn get_snapshots(
        &self,
        doc_id: &DocId,
        limit: usize,
    ) -> Result<Vec<SnapshotInfo>, CoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, doc_id, captured_at, change_count, word_count, trigger
                 FROM history_snapshots
                 WHERE doc_id = ?1
                 ORDER BY captured_at DESC
                 LIMIT ?2",
            )
            .map_err(|e| CoreError::InvalidData(format!("query failed: {e}")))?;

        let snapshots = stmt
            .query_map(params![doc_id.to_string(), limit as i64], |row| {
                Ok(SnapshotInfo {
                    id: row.get(0)?,
                    doc_id: row.get(1)?,
                    captured_at: row.get(2)?,
                    change_count: row.get(3)?,
                    word_count: row.get(4)?,
                    trigger: row.get(5)?,
                })
            })
            .map_err(|e| CoreError::InvalidData(format!("query failed: {e}")))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(snapshots)
    }

    /// Get the text content of a specific snapshot (for restore or preview).
    pub fn get_snapshot_text(&self, snapshot_id: i64) -> Result<String, CoreError> {
        self.conn
            .query_row(
                "SELECT text_content FROM history_snapshots WHERE id = ?1",
                params![snapshot_id],
                |row| row.get(0),
            )
            .map_err(|e| CoreError::InvalidData(format!("snapshot not found: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_session(id: &str, actor: &str, started_at: i64) -> HistorySession {
        HistorySession {
            id: id.to_string(),
            actor: actor.to_string(),
            started_at,
            ended_at: started_at + 300,
            change_count: 10,
            op_count: 50,
            first_change_hash: format!("{:0>64}", id),
            last_change_hash: format!("{:0>64}", format!("{id}_last")),
        }
    }

    #[test]
    fn test_open_in_memory() {
        let store = HistoryStore::open_in_memory().unwrap();
        let doc_id = uuid::Uuid::new_v4();
        let sessions = store.get_archived_sessions(&doc_id).unwrap();
        assert!(sessions.is_empty());
    }

    #[test]
    fn test_archive_and_retrieve_sessions() {
        let store = HistoryStore::open_in_memory().unwrap();
        let doc_id = uuid::Uuid::new_v4();

        let sessions = vec![
            make_test_session("sess1", "actor_a", 1000),
            make_test_session("sess2", "actor_b", 2000),
        ];

        store
            .archive_sessions(&sessions, "test-project", &doc_id)
            .unwrap();

        let retrieved = store.get_archived_sessions(&doc_id).unwrap();
        assert_eq!(retrieved.len(), 2);
        // Most recent first
        assert_eq!(retrieved[0].id, "sess2");
        assert_eq!(retrieved[1].id, "sess1");
    }

    #[test]
    fn test_archive_idempotent() {
        let store = HistoryStore::open_in_memory().unwrap();
        let doc_id = uuid::Uuid::new_v4();

        let sessions = vec![make_test_session("sess1", "actor_a", 1000)];

        store
            .archive_sessions(&sessions, "test-project", &doc_id)
            .unwrap();
        // Archive again — should not duplicate
        store
            .archive_sessions(&sessions, "test-project", &doc_id)
            .unwrap();

        let retrieved = store.get_archived_sessions(&doc_id).unwrap();
        assert_eq!(retrieved.len(), 1);
    }

    #[test]
    fn test_store_and_get_snapshot() {
        let store = HistoryStore::open_in_memory().unwrap();
        let doc_id = uuid::Uuid::new_v4();

        let snapshot_id = store
            .store_snapshot(
                &doc_id,
                "test-project",
                "Hello world content",
                50,
                "compaction",
            )
            .unwrap();

        let text = store.get_snapshot_text(snapshot_id).unwrap();
        assert_eq!(text, "Hello world content");

        let snapshots = store.get_snapshots(&doc_id, 10).unwrap();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].word_count, 3);
        assert_eq!(snapshots[0].trigger, "compaction");
    }

    #[test]
    fn test_merged_history() {
        let store = HistoryStore::open_in_memory().unwrap();
        let doc_id = uuid::Uuid::new_v4();

        // Archive some old sessions
        let archived = vec![
            make_test_session("old_sess", "actor_a", 500),
            make_test_session("common_sess", "actor_a", 1000),
        ];
        store
            .archive_sessions(&archived, "test-project", &doc_id)
            .unwrap();

        // Live sessions from Automerge (after compaction, only recent ones)
        let live = vec![
            make_test_session("common_sess", "actor_a", 1000), // overlaps with archived
            make_test_session("new_sess", "actor_a", 2000),
        ];

        let merged = store.get_merged_history(&doc_id, live).unwrap();

        // Should have 3 unique sessions: old_sess (archived only), common_sess (deduplicated), new_sess (live only)
        assert_eq!(merged.len(), 3);
        // Most recent first
        assert_eq!(merged[0].id, "new_sess");
        assert_eq!(merged[1].id, "common_sess");
        assert_eq!(merged[2].id, "old_sess");
    }

    #[test]
    fn test_compaction_log() {
        let store = HistoryStore::open_in_memory().unwrap();
        let doc_id = uuid::Uuid::new_v4();

        let snapshot_id = store
            .store_snapshot(&doc_id, "test-project", "content", 100, "compaction")
            .unwrap();

        store
            .log_compaction(&doc_id, 100, 50000, 10000, snapshot_id)
            .unwrap();

        // Verify no error (log is audit-only, no retrieval API needed for v1)
    }
}
