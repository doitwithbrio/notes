//! SQLite-backed durable version storage.
//!
//! Replaces the old history_sessions/history_snapshots tables with a single
//! unified `versions` table. Stores Automerge binary snapshots (BLOB) instead
//! of plaintext, preserving rich text formatting for restores.

use std::path::Path;

use crate::error::CoreError;
use crate::types::DocId;
use crate::version::{Version, VersionSignificance, VersionType};
use rusqlite::{params, Connection};

/// SQLite-backed version store.
pub struct VersionStore {
    conn: Connection,
}

impl VersionStore {
    /// Open or create the version store at the given path.
    pub fn open(db_path: &Path, encryption_key: Option<&[u8; 32]>) -> Result<Self, CoreError> {
        let conn = Connection::open(db_path)
            .map_err(|e| CoreError::InvalidData(format!("version db open failed: {e}")))?;

        if let Some(key) = encryption_key {
            let hex_key: String = key.iter().map(|b| format!("{:02x}", b)).collect();
            conn.execute_batch(&format!("PRAGMA key = \"x'{hex_key}'\";"))
                .map_err(|e| CoreError::InvalidData(format!("SQLCipher key failed: {e}")))?;
        }

        conn.execute_batch("PRAGMA journal_mode=WAL;")
            .map_err(|e| CoreError::InvalidData(format!("WAL mode failed: {e}")))?;

        // Create the new versions table
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS versions (
                id              TEXT PRIMARY KEY,
                doc_id          TEXT NOT NULL,
                project         TEXT NOT NULL,
                type            TEXT NOT NULL DEFAULT 'auto',
                name            TEXT NOT NULL,
                label           TEXT,
                heads_json      TEXT NOT NULL,
                actor           TEXT NOT NULL,
                created_at      INTEGER NOT NULL,
                change_count    INTEGER NOT NULL DEFAULT 0,
                chars_added     INTEGER NOT NULL DEFAULT 0,
                chars_removed   INTEGER NOT NULL DEFAULT 0,
                blocks_changed  INTEGER NOT NULL DEFAULT 0,
                significance    TEXT NOT NULL DEFAULT 'significant',
                automerge_snapshot BLOB,
                seq             INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_versions_doc_seq
                ON versions(doc_id, seq);
            CREATE INDEX IF NOT EXISTS idx_versions_doc_time
                ON versions(doc_id, created_at);

            -- Keep the old tables for migration purposes (read-only)
            -- They will be dropped after successful migration.

            -- Compaction log (still useful)
            CREATE TABLE IF NOT EXISTS compaction_log (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                doc_id          TEXT NOT NULL,
                compacted_at    INTEGER NOT NULL,
                changes_before  INTEGER NOT NULL,
                bytes_before    INTEGER NOT NULL,
                bytes_after     INTEGER NOT NULL
            );",
        )
        .map_err(|e| CoreError::InvalidData(format!("version schema creation failed: {e}")))?;

        Ok(Self { conn })
    }

    /// Open an in-memory version store (for testing).
    pub fn open_in_memory() -> Result<Self, CoreError> {
        let conn = Connection::open_in_memory()
            .map_err(|e| CoreError::InvalidData(format!("in-memory db failed: {e}")))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS versions (
                id              TEXT PRIMARY KEY,
                doc_id          TEXT NOT NULL,
                project         TEXT NOT NULL,
                type            TEXT NOT NULL DEFAULT 'auto',
                name            TEXT NOT NULL,
                label           TEXT,
                heads_json      TEXT NOT NULL,
                actor           TEXT NOT NULL,
                created_at      INTEGER NOT NULL,
                change_count    INTEGER NOT NULL DEFAULT 0,
                chars_added     INTEGER NOT NULL DEFAULT 0,
                chars_removed   INTEGER NOT NULL DEFAULT 0,
                blocks_changed  INTEGER NOT NULL DEFAULT 0,
                significance    TEXT NOT NULL DEFAULT 'significant',
                automerge_snapshot BLOB,
                seq             INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_versions_doc_seq
                ON versions(doc_id, seq);

            CREATE TABLE IF NOT EXISTS compaction_log (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                doc_id          TEXT NOT NULL,
                compacted_at    INTEGER NOT NULL,
                changes_before  INTEGER NOT NULL,
                bytes_before    INTEGER NOT NULL,
                bytes_after     INTEGER NOT NULL
            );",
        )
        .map_err(|e| CoreError::InvalidData(format!("version schema creation failed: {e}")))?;

        Ok(Self { conn })
    }

    /// Store a new version entry.
    pub fn store_version(
        &self,
        version: &Version,
        snapshot: Option<&[u8]>,
    ) -> Result<(), CoreError> {
        let heads_json = serde_json::to_string(&version.heads).unwrap_or_else(|_| "[]".to_string());

        self.conn
            .execute(
                "INSERT OR REPLACE INTO versions
                 (id, doc_id, project, type, name, label, heads_json, actor,
                  created_at, change_count, chars_added, chars_removed, blocks_changed,
                  significance, automerge_snapshot, seq)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
                params![
                    version.id,
                    version.doc_id,
                    version.project,
                    version.version_type.as_str(),
                    version.name,
                    version.label,
                    heads_json,
                    version.actor,
                    version.created_at,
                    version.change_count as i64,
                    version.chars_added as i64,
                    version.chars_removed as i64,
                    version.blocks_changed as i64,
                    version.significance.as_str(),
                    snapshot,
                    version.seq,
                ],
            )
            .map_err(|e| CoreError::InvalidData(format!("version insert failed: {e}")))?;

        Ok(())
    }

    /// Get all versions for a document, ordered by sequence (most recent first).
    pub fn get_versions(&self, doc_id: &DocId) -> Result<Vec<Version>, CoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, doc_id, project, type, name, label, heads_json, actor,
                        created_at, change_count, chars_added, chars_removed, blocks_changed,
                        significance, seq
                 FROM versions
                 WHERE doc_id = ?1
                 ORDER BY seq DESC",
            )
            .map_err(|e| CoreError::InvalidData(format!("query failed: {e}")))?;

        let versions = stmt
            .query_map(params![doc_id.to_string()], |row| {
                let heads_json: String = row.get(6)?;
                let heads: Vec<String> = serde_json::from_str(&heads_json).unwrap_or_default();

                Ok(Version {
                    id: row.get(0)?,
                    doc_id: row.get(1)?,
                    project: row.get(2)?,
                    version_type: VersionType::from_str(&row.get::<_, String>(3)?),
                    name: row.get(4)?,
                    label: row.get(5)?,
                    heads,
                    actor: row.get(7)?,
                    created_at: row.get(8)?,
                    change_count: row.get::<_, i64>(9)? as usize,
                    chars_added: row.get::<_, i64>(10)? as usize,
                    chars_removed: row.get::<_, i64>(11)? as usize,
                    blocks_changed: row.get::<_, i64>(12)? as usize,
                    significance: VersionSignificance::from_str(&row.get::<_, String>(13)?),
                    seq: row.get(14)?,
                })
            })
            .map_err(|e| CoreError::InvalidData(format!("query failed: {e}")))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(versions)
    }

    /// Get the Automerge snapshot blob for a specific version (for rich text restore).
    pub fn get_snapshot(&self, version_id: &str) -> Result<Option<Vec<u8>>, CoreError> {
        let result = self
            .conn
            .query_row(
                "SELECT automerge_snapshot FROM versions WHERE id = ?1",
                params![version_id],
                |row| row.get::<_, Option<Vec<u8>>>(0),
            )
            .map_err(|e| CoreError::InvalidData(format!("snapshot query failed: {e}")))?;

        Ok(result)
    }

    /// Get the next sequence number for a document.
    pub fn next_seq(&self, doc_id: &DocId) -> Result<i64, CoreError> {
        let max_seq: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(seq), 0) FROM versions WHERE doc_id = ?1",
                params![doc_id.to_string()],
                |row| row.get(0),
            )
            .map_err(|e| CoreError::InvalidData(format!("seq query failed: {e}")))?;

        Ok(max_seq + 1)
    }

    /// Get all version names used for a document (for unique name generation).
    pub fn get_used_names(&self, doc_id: &DocId) -> Result<Vec<String>, CoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT name FROM versions WHERE doc_id = ?1")
            .map_err(|e| CoreError::InvalidData(format!("query failed: {e}")))?;

        let names = stmt
            .query_map(params![doc_id.to_string()], |row| row.get(0))
            .map_err(|e| CoreError::InvalidData(format!("query failed: {e}")))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(names)
    }

    /// Get a specific version by ID.
    pub fn get_version(&self, version_id: &str) -> Result<Option<Version>, CoreError> {
        let result = self.conn.query_row(
            "SELECT id, doc_id, project, type, name, label, heads_json, actor,
                        created_at, change_count, chars_added, chars_removed, blocks_changed,
                        significance, seq
                 FROM versions WHERE id = ?1",
            params![version_id],
            |row| {
                let heads_json: String = row.get(6)?;
                let heads: Vec<String> = serde_json::from_str(&heads_json).unwrap_or_default();

                Ok(Version {
                    id: row.get(0)?,
                    doc_id: row.get(1)?,
                    project: row.get(2)?,
                    version_type: VersionType::from_str(&row.get::<_, String>(3)?),
                    name: row.get(4)?,
                    label: row.get(5)?,
                    heads,
                    actor: row.get(7)?,
                    created_at: row.get(8)?,
                    change_count: row.get::<_, i64>(9)? as usize,
                    chars_added: row.get::<_, i64>(10)? as usize,
                    chars_removed: row.get::<_, i64>(11)? as usize,
                    blocks_changed: row.get::<_, i64>(12)? as usize,
                    significance: VersionSignificance::from_str(&row.get::<_, String>(13)?),
                    seq: row.get(14)?,
                })
            },
        );

        match result {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(CoreError::InvalidData(format!("version query failed: {e}"))),
        }
    }

    /// Get the latest version for a document (the most recent by seq).
    pub fn get_latest_version(&self, doc_id: &DocId) -> Result<Option<Version>, CoreError> {
        let versions = self.get_versions(doc_id)?;
        Ok(versions.into_iter().next())
    }

    /// Log a compaction event.
    pub fn log_compaction(
        &self,
        doc_id: &DocId,
        changes_before: usize,
        bytes_before: usize,
        bytes_after: usize,
    ) -> Result<(), CoreError> {
        let now = chrono::Utc::now().timestamp();
        self.conn
            .execute(
                "INSERT INTO compaction_log
                 (doc_id, compacted_at, changes_before, bytes_before, bytes_after)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    doc_id.to_string(),
                    now,
                    changes_before as i64,
                    bytes_before as i64,
                    bytes_after as i64,
                ],
            )
            .map_err(|e| CoreError::InvalidData(format!("compaction log failed: {e}")))?;
        Ok(())
    }

    /// Migrate data from old history_sessions and history_snapshots tables.
    /// This is a one-time operation called during app startup.
    pub fn migrate_from_old_history(&self) -> Result<usize, CoreError> {
        // Check if old tables exist
        let has_old_sessions: bool = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='history_sessions'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(0)
            > 0;

        if !has_old_sessions {
            return Ok(0);
        }

        // Count existing old sessions
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM history_sessions", [], |row| {
                row.get(0)
            })
            .unwrap_or(0);

        if count == 0 {
            return Ok(0);
        }

        log::info!(
            "Migrating {} old history sessions to new versions table",
            count
        );

        // Migrate each old session as an auto version
        self.conn
            .execute_batch(
                "INSERT OR IGNORE INTO versions
                 (id, doc_id, project, type, name, label, heads_json, actor,
                  created_at, change_count, chars_added, chars_removed, blocks_changed,
                  significance, seq)
                 SELECT
                    id,
                    doc_id,
                    project,
                    'auto',
                    'Migrated',  -- Will need creature name assignment
                    NULL,
                    json_array(last_change_hash),
                    actor,
                    started_at,
                    change_count,
                    0, 0, 0,
                    CASE WHEN change_count > 5 THEN 'significant' ELSE 'minor' END,
                    ROW_NUMBER() OVER (PARTITION BY doc_id ORDER BY started_at)
                 FROM history_sessions",
            )
            .map_err(|e| CoreError::InvalidData(format!("migration failed: {e}")))?;

        // Drop old tables after successful migration
        self.conn
            .execute_batch(
                "DROP TABLE IF EXISTS history_sessions;
                 DROP TABLE IF EXISTS history_snapshots;",
            )
            .ok(); // Don't fail if drop fails

        Ok(count as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_version(id: &str, doc_id: &str, seq: i64) -> Version {
        Version {
            id: id.to_string(),
            doc_id: doc_id.to_string(),
            project: "test-project".to_string(),
            version_type: VersionType::Auto,
            name: "Nautilus".to_string(),
            label: None,
            heads: vec!["a".repeat(64)],
            actor: "test-actor".to_string(),
            created_at: 1000 + seq * 100,
            change_count: 10,
            chars_added: 50,
            chars_removed: 5,
            blocks_changed: 2,
            significance: VersionSignificance::Significant,
            seq,
        }
    }

    #[test]
    fn test_open_in_memory() {
        let store = VersionStore::open_in_memory().unwrap();
        let doc_id = uuid::Uuid::new_v4();
        let versions = store.get_versions(&doc_id).unwrap();
        assert!(versions.is_empty());
    }

    #[test]
    fn test_store_and_retrieve_version() {
        let store = VersionStore::open_in_memory().unwrap();
        let doc_id = uuid::Uuid::new_v4();
        let doc_id_str = doc_id.to_string();

        let version = make_test_version("v1", &doc_id_str, 1);
        store.store_version(&version, None).unwrap();

        let versions = store.get_versions(&doc_id).unwrap();
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].name, "Nautilus");
        assert_eq!(versions[0].chars_added, 50);
    }

    #[test]
    fn test_store_with_snapshot() {
        let store = VersionStore::open_in_memory().unwrap();
        let doc_id = uuid::Uuid::new_v4();
        let doc_id_str = doc_id.to_string();

        let version = make_test_version("v1", &doc_id_str, 1);
        let snapshot = b"fake automerge data";
        store.store_version(&version, Some(snapshot)).unwrap();

        let loaded = store.get_snapshot("v1").unwrap();
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap(), snapshot.to_vec());
    }

    #[test]
    fn test_ordering() {
        let store = VersionStore::open_in_memory().unwrap();
        let doc_id = uuid::Uuid::new_v4();
        let doc_id_str = doc_id.to_string();

        store
            .store_version(&make_test_version("v1", &doc_id_str, 1), None)
            .unwrap();
        store
            .store_version(&make_test_version("v2", &doc_id_str, 2), None)
            .unwrap();
        store
            .store_version(&make_test_version("v3", &doc_id_str, 3), None)
            .unwrap();

        let versions = store.get_versions(&doc_id).unwrap();
        assert_eq!(versions.len(), 3);
        // Most recent first
        assert_eq!(versions[0].id, "v3");
        assert_eq!(versions[1].id, "v2");
        assert_eq!(versions[2].id, "v1");
    }

    #[test]
    fn test_next_seq() {
        let store = VersionStore::open_in_memory().unwrap();
        let doc_id = uuid::Uuid::new_v4();
        let doc_id_str = doc_id.to_string();

        assert_eq!(store.next_seq(&doc_id).unwrap(), 1);

        store
            .store_version(&make_test_version("v1", &doc_id_str, 1), None)
            .unwrap();
        assert_eq!(store.next_seq(&doc_id).unwrap(), 2);
    }

    #[test]
    fn test_used_names() {
        let store = VersionStore::open_in_memory().unwrap();
        let doc_id = uuid::Uuid::new_v4();
        let doc_id_str = doc_id.to_string();

        let mut v1 = make_test_version("v1", &doc_id_str, 1);
        v1.name = "Seahorse".to_string();
        store.store_version(&v1, None).unwrap();

        let mut v2 = make_test_version("v2", &doc_id_str, 2);
        v2.name = "Octopus".to_string();
        store.store_version(&v2, None).unwrap();

        let names = store.get_used_names(&doc_id).unwrap();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"Seahorse".to_string()));
        assert!(names.contains(&"Octopus".to_string()));
    }

    #[test]
    fn test_named_version() {
        let store = VersionStore::open_in_memory().unwrap();
        let doc_id = uuid::Uuid::new_v4();
        let doc_id_str = doc_id.to_string();

        let mut v = make_test_version("v1", &doc_id_str, 1);
        v.version_type = VersionType::Named;
        v.label = Some("Final draft".to_string());
        v.significance = VersionSignificance::Named;
        store.store_version(&v, None).unwrap();

        let versions = store.get_versions(&doc_id).unwrap();
        assert_eq!(versions[0].version_type, VersionType::Named);
        assert_eq!(versions[0].label.as_deref(), Some("Final draft"));
        assert_eq!(versions[0].significance, VersionSignificance::Named);
    }

    #[test]
    fn test_get_version_by_id() {
        let store = VersionStore::open_in_memory().unwrap();
        let doc_id = uuid::Uuid::new_v4();
        let doc_id_str = doc_id.to_string();

        store
            .store_version(&make_test_version("v1", &doc_id_str, 1), None)
            .unwrap();

        let found = store.get_version("v1").unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, "v1");

        let not_found = store.get_version("nonexistent").unwrap();
        assert!(not_found.is_none());
    }
}
