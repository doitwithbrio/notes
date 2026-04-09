//! SQLite-backed durable version storage.
//!
//! Replaces the old history_sessions/history_snapshots tables with a single
//! unified `versions` table. Stores Automerge binary snapshots (BLOB) instead
//! of plaintext, preserving rich text formatting for restores.

use std::path::Path;

use crate::error::CoreError;
use crate::types::DocId;
use crate::version::{Version, VersionSignificance, VersionType};
use rusqlite::{params, Connection, OptionalExtension};

const VERSION_SCHEMA_SQL: &str = "CREATE TABLE IF NOT EXISTS versions (
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
);";

const COMPACTION_LOG_SCHEMA_SQL: &str = "CREATE TABLE IF NOT EXISTS compaction_log (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    doc_id          TEXT NOT NULL,
    compacted_at    INTEGER NOT NULL,
    changes_before  INTEGER NOT NULL,
    bytes_before    INTEGER NOT NULL,
    bytes_after     INTEGER NOT NULL
);";

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
        Self::initialize_schema(&conn)?;

        Ok(Self { conn })
    }

    /// Open an in-memory version store (for testing).
    pub fn open_in_memory() -> Result<Self, CoreError> {
        let conn = Connection::open_in_memory()
            .map_err(|e| CoreError::InvalidData(format!("in-memory db failed: {e}")))?;
        Self::initialize_schema(&conn)?;

        Ok(Self { conn })
    }

    fn initialize_schema(conn: &Connection) -> Result<(), CoreError> {
        conn.execute_batch(VERSION_SCHEMA_SQL)
            .map_err(|e| CoreError::InvalidData(format!("version schema creation failed: {e}")))?;
        conn.execute_batch(COMPACTION_LOG_SCHEMA_SQL).map_err(|e| {
            CoreError::InvalidData(format!("compaction schema creation failed: {e}"))
        })?;
        Self::upgrade_versions_schema(conn)?;
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_versions_doc_seq ON versions(doc_id, seq);
             CREATE INDEX IF NOT EXISTS idx_versions_doc_time ON versions(doc_id, created_at);",
        )
        .map_err(|e| CoreError::InvalidData(format!("version index creation failed: {e}")))?;
        Ok(())
    }

    fn upgrade_versions_schema(conn: &Connection) -> Result<(), CoreError> {
        let mut stmt = conn
            .prepare("PRAGMA table_info(versions)")
            .map_err(|e| CoreError::InvalidData(format!("schema inspect failed: {e}")))?;
        let columns: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|e| CoreError::InvalidData(format!("schema inspect failed: {e}")))?
            .collect::<Result<_, _>>()
            .map_err(|e| CoreError::InvalidData(format!("schema inspect failed: {e}")))?;

        for (column, sql) in [
            (
                "type",
                "ALTER TABLE versions ADD COLUMN type TEXT NOT NULL DEFAULT 'auto'",
            ),
            ("label", "ALTER TABLE versions ADD COLUMN label TEXT"),
            (
                "heads_json",
                "ALTER TABLE versions ADD COLUMN heads_json TEXT NOT NULL DEFAULT '[]'",
            ),
            (
                "change_count",
                "ALTER TABLE versions ADD COLUMN change_count INTEGER NOT NULL DEFAULT 0",
            ),
            (
                "chars_added",
                "ALTER TABLE versions ADD COLUMN chars_added INTEGER NOT NULL DEFAULT 0",
            ),
            (
                "chars_removed",
                "ALTER TABLE versions ADD COLUMN chars_removed INTEGER NOT NULL DEFAULT 0",
            ),
            (
                "blocks_changed",
                "ALTER TABLE versions ADD COLUMN blocks_changed INTEGER NOT NULL DEFAULT 0",
            ),
            (
                "significance",
                "ALTER TABLE versions ADD COLUMN significance TEXT NOT NULL DEFAULT 'significant'",
            ),
            (
                "automerge_snapshot",
                "ALTER TABLE versions ADD COLUMN automerge_snapshot BLOB",
            ),
            (
                "seq",
                "ALTER TABLE versions ADD COLUMN seq INTEGER NOT NULL DEFAULT 0",
            ),
        ] {
            if !columns.iter().any(|existing| existing == column) {
                conn.execute(sql, [])
                    .map_err(|e| CoreError::InvalidData(format!("schema upgrade failed: {e}")))?;
            }
        }

        Self::backfill_heads_json(conn, &columns)?;
        Self::backfill_seq(conn)?;
        Ok(())
    }

    fn backfill_heads_json(
        conn: &Connection,
        existing_columns: &[String],
    ) -> Result<(), CoreError> {
        if existing_columns
            .iter()
            .any(|column| column == "last_change_hash")
        {
            conn.execute(
                "UPDATE versions
                 SET heads_json = json_array(last_change_hash)
                 WHERE (heads_json IS NULL OR heads_json = '' OR heads_json = '[]')
                   AND last_change_hash IS NOT NULL
                   AND last_change_hash != ''",
                [],
            )
            .map_err(|e| CoreError::InvalidData(format!("heads backfill failed: {e}")))?;
        }

        conn.execute(
            "UPDATE versions
             SET heads_json = '[]'
             WHERE heads_json IS NULL OR heads_json = ''",
            [],
        )
        .map_err(|e| CoreError::InvalidData(format!("heads normalization failed: {e}")))?;

        Ok(())
    }

    fn backfill_seq(conn: &Connection) -> Result<(), CoreError> {
        let mut stmt = conn
            .prepare(
                "SELECT id, doc_id
                 FROM versions
                 WHERE seq <= 0
                 ORDER BY doc_id, created_at, rowid",
            )
            .map_err(|e| CoreError::InvalidData(format!("seq backfill failed: {e}")))?;
        let rows: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| CoreError::InvalidData(format!("seq backfill failed: {e}")))?
            .collect::<Result<_, _>>()
            .map_err(|e| CoreError::InvalidData(format!("seq backfill failed: {e}")))?;

        if rows.is_empty() {
            return Ok(());
        }

        let tx = conn
            .unchecked_transaction()
            .map_err(|e| CoreError::InvalidData(format!("seq backfill failed: {e}")))?;
        let mut current_doc = String::new();
        let mut next_seq = 0_i64;
        for (id, doc_id) in rows {
            if doc_id != current_doc {
                current_doc = doc_id;
                next_seq = tx
                    .query_row(
                        "SELECT COALESCE(MAX(seq), 0) FROM versions WHERE doc_id = ?1",
                        params![current_doc.clone()],
                        |row| row.get(0),
                    )
                    .map_err(|e| CoreError::InvalidData(format!("seq backfill failed: {e}")))?;
            }
            next_seq += 1;

            tx.execute(
                "UPDATE versions SET seq = ?1 WHERE id = ?2",
                params![next_seq, id],
            )
            .map_err(|e| CoreError::InvalidData(format!("seq backfill failed: {e}")))?;
        }
        tx.commit()
            .map_err(|e| CoreError::InvalidData(format!("seq backfill failed: {e}")))?;
        Ok(())
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

    /// Remove all stored versions for a project.
    pub fn delete_project(&self, project: &str) -> Result<(), CoreError> {
        self.conn
            .execute("DELETE FROM versions WHERE project = ?1", params![project])
            .map_err(|e| CoreError::InvalidData(format!("version delete failed: {e}")))?;
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
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| CoreError::InvalidData(format!("query failed: {e}")))?;

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
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| CoreError::InvalidData(format!("query failed: {e}")))?;

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

    /// Migrate data from an old `history.db` into the new versions store.
    /// Safe to run repeatedly.
    pub fn migrate_from_legacy_history_db(
        &self,
        legacy_db_path: &Path,
    ) -> Result<usize, CoreError> {
        if !legacy_db_path.exists() {
            return Ok(0);
        }

        let legacy = Connection::open(legacy_db_path)
            .map_err(|e| CoreError::InvalidData(format!("legacy history db open failed: {e}")))?;

        let has_old_sessions: bool = legacy
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
        let count: i64 = legacy
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

        #[derive(Debug)]
        struct LegacySession {
            id: String,
            doc_id: String,
            project: String,
            actor: String,
            started_at: i64,
            change_count: i64,
            last_change_hash: String,
        }

        let mut stmt = legacy
            .prepare(
                "SELECT id, doc_id, project, actor, started_at, change_count, last_change_hash
                 FROM history_sessions
                 ORDER BY doc_id, started_at, id",
            )
            .map_err(|e| CoreError::InvalidData(format!("migration failed: {e}")))?;
        let sessions: Vec<LegacySession> = stmt
            .query_map([], |row| {
                Ok(LegacySession {
                    id: row.get(0)?,
                    doc_id: row.get(1)?,
                    project: row.get(2)?,
                    actor: row.get(3)?,
                    started_at: row.get(4)?,
                    change_count: row.get(5)?,
                    last_change_hash: row.get(6)?,
                })
            })
            .map_err(|e| CoreError::InvalidData(format!("migration failed: {e}")))?
            .collect::<Result<_, _>>()
            .map_err(|e| CoreError::InvalidData(format!("migration failed: {e}")))?;

        let mut snapshot_stmt = legacy
            .prepare(
                "SELECT content
                 FROM history_snapshots
                 WHERE doc_id = ?1 AND captured_at <= ?2
                 ORDER BY captured_at DESC
                 LIMIT 1",
            )
            .map_err(|e| CoreError::InvalidData(format!("migration failed: {e}")))?;
        let mut fallback_snapshot_stmt = legacy
            .prepare(
                "SELECT content
                 FROM history_snapshots
                 WHERE doc_id = ?1
                 ORDER BY captured_at DESC
                 LIMIT 1",
            )
            .map_err(|e| CoreError::InvalidData(format!("migration failed: {e}")))?;

        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| CoreError::InvalidData(format!("migration failed: {e}")))?;
        let mut current_doc = String::new();
        let mut next_seq = 0_i64;
        let mut migrated = 0_usize;

        for session in sessions {
            let exists: Option<String> = tx
                .query_row(
                    "SELECT id FROM versions WHERE id = ?1",
                    params![session.id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|e| CoreError::InvalidData(format!("migration failed: {e}")))?;
            if exists.is_some() {
                continue;
            }

            if session.doc_id != current_doc {
                current_doc = session.doc_id.clone();
                let existing_max: i64 = tx
                    .query_row(
                        "SELECT COALESCE(MAX(seq), 0) FROM versions WHERE doc_id = ?1",
                        params![session.doc_id],
                        |row| row.get(0),
                    )
                    .map_err(|e| CoreError::InvalidData(format!("migration failed: {e}")))?;
                next_seq = existing_max + 1;
            } else {
                next_seq += 1;
            }

            let used_names: Vec<String> = {
                let mut used_stmt = tx
                    .prepare("SELECT name FROM versions WHERE doc_id = ?1")
                    .map_err(|e| CoreError::InvalidData(format!("migration failed: {e}")))?;
                let used_names = used_stmt
                    .query_map(params![session.doc_id.clone()], |row| row.get(0))
                    .map_err(|e| CoreError::InvalidData(format!("migration failed: {e}")))?
                    .collect::<Result<_, _>>()
                    .map_err(|e| CoreError::InvalidData(format!("migration failed: {e}")))?;
                used_names
            };

            let version = Version {
                id: session.id.clone(),
                doc_id: session.doc_id.clone(),
                project: session.project,
                version_type: VersionType::Auto,
                name: crate::version::unique_creature_name(&session.id, &used_names),
                label: None,
                heads: vec![session.last_change_hash],
                actor: session.actor,
                created_at: session.started_at,
                change_count: session.change_count.max(0) as usize,
                chars_added: 0,
                chars_removed: 0,
                blocks_changed: 0,
                significance: if session.change_count > 5 {
                    VersionSignificance::Significant
                } else {
                    VersionSignificance::Minor
                },
                seq: next_seq,
            };

            let snapshot = snapshot_stmt
                .query_row(params![version.doc_id.clone(), version.created_at], |row| {
                    row.get::<_, Vec<u8>>(0)
                })
                .optional()
                .map_err(|e| CoreError::InvalidData(format!("migration failed: {e}")))?
                .or(fallback_snapshot_stmt
                    .query_row(params![version.doc_id.clone()], |row| {
                        row.get::<_, Vec<u8>>(0)
                    })
                    .optional()
                    .map_err(|e| CoreError::InvalidData(format!("migration failed: {e}")))?);

            let heads_json =
                serde_json::to_string(&version.heads).unwrap_or_else(|_| "[]".to_string());
            tx.execute(
                "INSERT INTO versions
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
            .map_err(|e| CoreError::InvalidData(format!("migration failed: {e}")))?;

            migrated += 1;
        }

        tx.commit()
            .map_err(|e| CoreError::InvalidData(format!("migration failed: {e}")))?;

        Ok(migrated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use rusqlite::Connection;

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

    #[test]
    fn test_open_upgrades_existing_versions_schema() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("versions.db");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE versions (
                id TEXT PRIMARY KEY,
                doc_id TEXT NOT NULL,
                project TEXT NOT NULL,
                name TEXT NOT NULL,
                actor TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                last_change_hash TEXT NOT NULL
            );
            INSERT INTO versions (id, doc_id, project, name, actor, created_at, last_change_hash)
            VALUES ('v1', 'doc-1', 'project', 'Legacy', 'actor', 123, 'abc123');",
        )
        .unwrap();
        drop(conn);

        let store = VersionStore::open(&db_path, None).unwrap();

        let conn = Connection::open(&db_path).unwrap();
        let columns: Vec<String> = conn
            .prepare("PRAGMA table_info(versions)")
            .unwrap()
            .query_map([], |row| row.get(1))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(columns.contains(&"seq".to_string()));
        assert!(columns.contains(&"heads_json".to_string()));

        let heads_json: String = conn
            .query_row(
                "SELECT heads_json FROM versions WHERE id = 'v1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let seq: i64 = conn
            .query_row("SELECT seq FROM versions WHERE id = 'v1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(heads_json, "[\"abc123\"]");
        assert_eq!(seq, 1);

        drop(store);
    }

    #[test]
    fn test_migrate_from_legacy_history_db() {
        let dir = tempfile::tempdir().unwrap();
        let versions_path = dir.path().join("versions.db");
        let history_path = dir.path().join("history.db");

        let store = VersionStore::open(&versions_path, None).unwrap();

        let legacy = Connection::open(&history_path).unwrap();
        legacy
            .execute_batch(
                "CREATE TABLE history_sessions (
                    id TEXT NOT NULL,
                    doc_id TEXT NOT NULL,
                    project TEXT NOT NULL,
                    actor TEXT NOT NULL,
                    started_at INTEGER NOT NULL,
                    ended_at INTEGER NOT NULL,
                    change_count INTEGER NOT NULL,
                    op_count INTEGER NOT NULL,
                    first_change_hash TEXT NOT NULL,
                    last_change_hash TEXT NOT NULL,
                    PRIMARY KEY (id, doc_id)
                );
                CREATE TABLE history_snapshots (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    doc_id TEXT NOT NULL,
                    project TEXT NOT NULL,
                    captured_at INTEGER NOT NULL,
                    change_count INTEGER NOT NULL,
                    word_count INTEGER NOT NULL,
                    content BLOB NOT NULL,
                    epoch INTEGER NOT NULL DEFAULT 0,
                    trigger TEXT NOT NULL DEFAULT 'compaction'
                );
                INSERT INTO history_sessions
                    (id, doc_id, project, actor, started_at, ended_at, change_count, op_count, first_change_hash, last_change_hash)
                VALUES
                    ('legacy-1', '11111111-1111-1111-1111-111111111111', 'project', 'actor-a', 10, 11, 2, 2, 'hash-a1', 'hash-a2'),
                    ('legacy-2', '11111111-1111-1111-1111-111111111111', 'project', 'actor-a', 20, 21, 8, 3, 'hash-b1', 'hash-b2');
                INSERT INTO history_snapshots
                    (doc_id, project, captured_at, change_count, word_count, content, epoch, trigger)
                VALUES
                    ('11111111-1111-1111-1111-111111111111', 'project', 9, 2, 10, X'616263', 0, 'idle'),
                    ('11111111-1111-1111-1111-111111111111', 'project', 19, 8, 20, X'646566', 0, 'idle');",
            )
            .unwrap();
        drop(legacy);

        let migrated = store.migrate_from_legacy_history_db(&history_path).unwrap();
        assert_eq!(migrated, 2);

        let doc_id = uuid::Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
        let versions = store.get_versions(&doc_id).unwrap();
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].seq, 2);
        assert_eq!(versions[1].seq, 1);
        assert_eq!(versions[0].heads, vec!["hash-b2".to_string()]);
        assert_eq!(
            store.get_snapshot("legacy-1").unwrap(),
            Some(b"abc".to_vec())
        );
        assert_eq!(
            store.get_snapshot("legacy-2").unwrap(),
            Some(b"def".to_vec())
        );

        let migrated_again = store.migrate_from_legacy_history_db(&history_path).unwrap();
        assert_eq!(migrated_again, 0);
    }
}
