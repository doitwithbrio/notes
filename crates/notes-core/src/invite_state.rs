use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::CoreError;

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum PersistedOwnerInvitePhase {
    Open,
    Reserved {
        session_id: String,
        invitee_peer_id: String,
        reserved_at: DateTime<Utc>,
        phase: String,
    },
    PreparedAckReceived {
        session_id: String,
        invitee_peer_id: String,
        prepared_at: DateTime<Utc>,
    },
    CommittedPendingAck {
        session_id: String,
        invitee_peer_id: String,
        committed_at: DateTime<Utc>,
    },
    Consumed {
        session_id: String,
        invitee_peer_id: String,
        consumed_at: DateTime<Utc>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedOwnerInviteRecord {
    pub schema_version: u32,
    pub invite_id: String,
    pub project_name: String,
    pub project_id: String,
    pub owner_peer_id: String,
    pub role: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub attempts: u32,
    pub phase: PersistedOwnerInvitePhase,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "stage", rename_all = "camelCase")]
pub enum PersistedJoinStage {
    PayloadStaged { staged_at: DateTime<Utc> },
    CommitConfirmed { confirmed_at: DateTime<Utc> },
    Finalized { finalized_at: DateTime<Utc> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedJoinSession {
    pub schema_version: u32,
    pub session_id: String,
    pub owner_peer_id: String,
    pub project_id: String,
    pub project_name: String,
    pub local_project_name: String,
    pub role: String,
    pub payload: String,
    pub stage: PersistedJoinStage,
    pub updated_at: DateTime<Utc>,
}

pub struct OwnerInviteStateStore {
    dir: PathBuf,
}

impl OwnerInviteStateStore {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: base_dir.into().join(".p2p").join("owner-invites"),
        }
    }

    pub fn save(&self, record: &PersistedOwnerInviteRecord) -> Result<(), CoreError> {
        let mut record = record.clone();
        record.schema_version = SCHEMA_VERSION;
        atomic_write_json(&self.path_for(&record.invite_id), &record)
    }

    pub fn load_all(&self) -> Result<Vec<PersistedOwnerInviteRecord>, CoreError> {
        load_all_json(&self.dir)
    }

    pub fn delete(&self, invite_id: &str) -> Result<(), CoreError> {
        remove_file_if_exists(&self.path_for(invite_id))
    }

    fn path_for(&self, invite_id: &str) -> PathBuf {
        self.dir.join(format!("{invite_id}.json"))
    }
}

pub struct JoinSessionStore {
    dir: PathBuf,
}

impl JoinSessionStore {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: base_dir.into().join(".p2p").join("join-sessions"),
        }
    }

    pub fn save(&self, session: &PersistedJoinSession) -> Result<(), CoreError> {
        let mut session = session.clone();
        session.schema_version = SCHEMA_VERSION;
        atomic_write_json(&self.path_for(&session.session_id), &session)
    }

    pub fn load_all(&self) -> Result<Vec<PersistedJoinSession>, CoreError> {
        load_all_json(&self.dir)
    }

    pub fn delete(&self, session_id: &str) -> Result<(), CoreError> {
        remove_file_if_exists(&self.path_for(session_id))?;
        remove_file_if_exists(&self.secret_path_for(session_id))
    }

    pub fn save_secret(&self, session_id: &str, secret: &str) -> Result<(), CoreError> {
        let path = self.secret_path_for(session_id);
        let Some(parent) = path.parent() else {
            return Err(CoreError::InvalidData("missing parent directory".into()));
        };
        fs::create_dir_all(parent)?;
        fs::write(path, secret.as_bytes())?;
        Ok(())
    }

    pub fn load_secret(&self, session_id: &str) -> Result<Option<String>, CoreError> {
        let path = self.secret_path_for(session_id);
        match fs::read_to_string(path) {
            Ok(secret) => Ok(Some(secret)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(CoreError::Io(err)),
        }
    }

    fn path_for(&self, session_id: &str) -> PathBuf {
        self.dir.join(format!("{session_id}.json"))
    }

    fn secret_path_for(&self, session_id: &str) -> PathBuf {
        self.dir.join(format!("{session_id}.secret"))
    }
}

fn load_all_json<T: for<'de> Deserialize<'de>>(dir: &Path) -> Result<Vec<T>, CoreError> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut out = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let raw = fs::read_to_string(&path)?;
        out.push(serde_json::from_str(&raw)?);
    }
    Ok(out)
}

fn atomic_write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), CoreError> {
    let Some(parent) = path.parent() else {
        return Err(CoreError::InvalidData("missing parent directory".into()));
    };
    fs::create_dir_all(parent)?;
    let raw = serde_json::to_vec_pretty(value)?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, raw)?;
    fs::rename(tmp, path)?;
    Ok(())
}

fn remove_file_if_exists(path: &Path) -> Result<(), CoreError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(CoreError::Io(err)),
    }
}
