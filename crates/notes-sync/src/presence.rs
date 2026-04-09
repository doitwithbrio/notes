//! Cursor presence and ephemeral status via iroh-gossip.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use bytes::Bytes;
use dashmap::DashMap;
use futures_lite::StreamExt;
use iroh::EndpointId;
use iroh_gossip::api::{Event, GossipSender};
use iroh_gossip::net::Gossip;
use iroh_gossip::TopicId;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, Mutex};
use tokio::task::JoinHandle;
use uuid::Uuid;

pub const PRESENCE_PROTOCOL_VERSION: u8 = 1;
pub const MAX_PRESENCE_RATE: u32 = 10;
pub const PRESENCE_TTL_MS: u64 = 6_000;

const MIN_UPDATE_INTERVAL_MS: u128 = 1000 / MAX_PRESENCE_RATE as u128;
const MAX_PRESENCE_MESSAGE_SIZE: usize = 1024;
const MAX_ALIAS_LEN: usize = 64;
const MAX_PEER_ID_LEN: usize = 128;
const MAX_PROJECT_ID_LEN: usize = 128;
const MAX_SESSION_ID_LEN: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PresenceUpdate {
    pub version: u8,
    pub project_id: String,
    pub peer_id: String,
    pub session_id: String,
    pub session_started_at: u64,
    pub seq: u64,
    pub alias: String,
    pub active_doc: Option<Uuid>,
    pub cursor_pos: Option<u64>,
    pub selection: Option<(u64, u64)>,
    pub ttl_ms: u64,
    pub timestamp: u64,
}

impl PresenceUpdate {
    pub fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("presence serialization should not fail")
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        serde_json::from_slice(data).ok()
    }

    pub fn is_clear(&self) -> bool {
        self.active_doc.is_none() && self.cursor_pos.is_none() && self.selection.is_none()
    }
}

#[derive(Debug, Clone)]
pub struct CachedPresence {
    pub update: PresenceUpdate,
    pub received_at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyOutcome {
    Applied,
    IgnoredStale,
}

struct JoinedTopic {
    sender: GossipSender,
    task: JoinHandle<()>,
}

/// Derives a gossip TopicId from a canonical project ID.
pub fn project_topic_id(project_id: &str) -> TopicId {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(format!("p2p-notes/gossip/{project_id}").as_bytes());
    let mut topic = [0u8; 32];
    topic.copy_from_slice(&hash[..32]);
    TopicId::from(topic)
}

pub struct PresenceManager {
    gossip: Gossip,
    presence_tx: broadcast::Sender<PresenceUpdate>,
    rate_limiters: Arc<DashMap<(String, String), Instant>>,
    joined_topics: Arc<Mutex<HashMap<String, JoinedTopic>>>,
    cache: Arc<DashMap<(String, String), CachedPresence>>,
}

impl PresenceManager {
    pub fn new(gossip: Gossip) -> Self {
        let (presence_tx, _) = broadcast::channel(256);
        Self {
            gossip,
            presence_tx,
            rate_limiters: Arc::new(DashMap::new()),
            joined_topics: Arc::new(Mutex::new(HashMap::new())),
            cache: Arc::new(DashMap::new()),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<PresenceUpdate> {
        self.presence_tx.subscribe()
    }

    pub fn gossip(&self) -> &Gossip {
        &self.gossip
    }

    pub fn cached_presence(&self, project_id: &str) -> HashMap<String, CachedPresence> {
        self.cache
            .iter()
            .filter(|entry| entry.key().0 == project_id)
            .map(|entry| (entry.key().1.clone(), entry.value().clone()))
            .collect()
    }

    pub async fn ensure_joined(
        &self,
        project_id: &str,
        bootstrap_peers: Vec<EndpointId>,
    ) -> Result<(), PresenceError> {
        let mut joined_topics = self.joined_topics.lock().await;
        if let Some(joined) = joined_topics.get(project_id) {
            if !bootstrap_peers.is_empty() {
                joined.sender.join_peers(bootstrap_peers).await?;
            }
            return Ok(());
        }

        let topic_id = project_topic_id(project_id);
        let topic = self.gossip.subscribe(topic_id, bootstrap_peers).await?;
        let (sender, mut receiver) = topic.split();
        let tx = self.presence_tx.clone();
        let expected_project_id = project_id.to_string();
        let rate_limiters = Arc::clone(&self.rate_limiters);
        let task = tokio::spawn(async move {
            while let Some(event) = receiver.next().await {
                match event {
                    Ok(Event::Received(message)) => {
                        match Self::validate_incoming_inner(
                            &rate_limiters,
                            message.delivered_from,
                            &message.content,
                        ) {
                            Ok(update) => {
                                if update.project_id == expected_project_id {
                                    let _ = tx.send(update);
                                }
                            }
                            Err(PresenceError::RateLimited) => {}
                            Err(err) => {
                                log::debug!("Dropping invalid presence update: {err}");
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(err) => {
                        log::warn!("Presence topic receive failed: {err}");
                        break;
                    }
                }
            }
        });
        joined_topics.insert(project_id.to_string(), JoinedTopic { sender, task });
        Ok(())
    }

    pub async fn leave_project(&self, project_id: &str) {
        let mut joined_topics = self.joined_topics.lock().await;
        if let Some(joined) = joined_topics.remove(project_id) {
            joined.task.abort();
        }
        self.cache
            .retain(|(cached_project_id, _), _| cached_project_id != project_id);
    }

    pub async fn publish(&self, update: PresenceUpdate) -> Result<(), PresenceError> {
        let joined_topics = self.joined_topics.lock().await;
        let joined = joined_topics
            .get(&update.project_id)
            .ok_or_else(|| PresenceError::ProjectNotJoined(update.project_id.clone()))?;
        joined
            .sender
            .broadcast(Bytes::from(update.encode()))
            .await
            .map_err(PresenceError::from)
    }

    pub fn validate_incoming(
        &self,
        sender: EndpointId,
        data: &[u8],
    ) -> Result<PresenceUpdate, PresenceError> {
        Self::validate_incoming_inner(&self.rate_limiters, sender, data)
    }

    fn validate_incoming_inner(
        rate_limiters: &DashMap<(String, String), Instant>,
        sender: EndpointId,
        data: &[u8],
    ) -> Result<PresenceUpdate, PresenceError> {
        if data.len() > MAX_PRESENCE_MESSAGE_SIZE {
            return Err(PresenceError::InvalidMessage(
                "presence message too large".into(),
            ));
        }

        let update = PresenceUpdate::decode(data)
            .ok_or_else(|| PresenceError::InvalidMessage("invalid presence payload".into()))?;

        if update.version != PRESENCE_PROTOCOL_VERSION {
            return Err(PresenceError::InvalidMessage(
                "unsupported presence version".into(),
            ));
        }
        if update.peer_id != sender.to_string() {
            return Err(PresenceError::InvalidMessage(
                "presence sender mismatch".into(),
            ));
        }
        if update.alias.len() > MAX_ALIAS_LEN
            || update.peer_id.len() > MAX_PEER_ID_LEN
            || update.project_id.is_empty()
            || update.project_id.len() > MAX_PROJECT_ID_LEN
            || update.session_id.is_empty()
            || update.session_id.len() > MAX_SESSION_ID_LEN
        {
            return Err(PresenceError::InvalidMessage(
                "oversized presence fields".into(),
            ));
        }
        if (update.cursor_pos.is_some() || update.selection.is_some())
            && update.active_doc.is_none()
        {
            return Err(PresenceError::InvalidMessage(
                "cursor/selection requires active_doc".into(),
            ));
        }
        if let Some((from, to)) = update.selection {
            if from > to {
                return Err(PresenceError::InvalidMessage(
                    "selection range invalid".into(),
                ));
            }
        }

        let rate_key = (update.project_id.clone(), update.peer_id.clone());
        let now = Instant::now();
        if let Some(last) = rate_limiters.get(&rate_key) {
            if now.duration_since(*last).as_millis() < MIN_UPDATE_INTERVAL_MS {
                return Err(PresenceError::RateLimited);
            }
        }
        rate_limiters.insert(rate_key, now);
        Ok(update)
    }

    pub fn apply_update(&self, update: PresenceUpdate, received_at_ms: u64) -> ApplyOutcome {
        let key = (update.project_id.clone(), update.peer_id.clone());
        if let Some(existing) = self.cache.get(&key) {
            if !is_newer_presence(&existing.update, &update) {
                return ApplyOutcome::IgnoredStale;
            }
        }
        self.cache.insert(
            key,
            CachedPresence {
                update,
                received_at_ms,
            },
        );
        ApplyOutcome::Applied
    }

    pub fn clear_peer(&self, project_id: &str, peer_id: &str, received_at_ms: u64) {
        let key = (project_id.to_string(), peer_id.to_string());
        if let Some(mut existing) = self.cache.get_mut(&key) {
            existing.update.active_doc = None;
            existing.update.cursor_pos = None;
            existing.update.selection = None;
            existing.received_at_ms = received_at_ms;
        }
    }

    pub fn expire_stale(&self, now_ms: u64) -> Vec<PresenceUpdate> {
        let mut expired = Vec::new();
        let keys: Vec<_> = self
            .cache
            .iter()
            .filter_map(|entry| {
                let ttl_ms = entry.value().update.ttl_ms;
                if now_ms.saturating_sub(entry.value().received_at_ms) >= ttl_ms {
                    Some(entry.key().clone())
                } else {
                    None
                }
            })
            .collect();
        for key in keys {
            if let Some((_, cached)) = self.cache.remove(&key) {
                expired.push(cached.update);
            }
        }
        expired
    }
}

fn is_newer_presence(current: &PresenceUpdate, next: &PresenceUpdate) -> bool {
    if current.session_id != next.session_id {
        return next.session_started_at >= current.session_started_at;
    }
    next.seq > current.seq
}

#[derive(Debug, thiserror::Error)]
pub enum PresenceError {
    #[error("Gossip error: {0}")]
    Gossip(String),
    #[error("Invalid message: {0}")]
    InvalidMessage(String),
    #[error("Rate limited")]
    RateLimited,
    #[error("Project not joined: {0}")]
    ProjectNotJoined(String),
}

impl From<iroh_gossip::api::ApiError> for PresenceError {
    fn from(value: iroh_gossip::api::ApiError) -> Self {
        PresenceError::Gossip(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_presence_manager() -> PresenceManager {
        let endpoint = iroh::Endpoint::builder(iroh::endpoint::presets::N0)
            .bind()
            .await
            .expect("endpoint");
        let gossip = iroh_gossip::net::Gossip::builder().spawn(endpoint);
        PresenceManager::new(gossip)
    }

    fn sample_update() -> PresenceUpdate {
        PresenceUpdate {
            version: PRESENCE_PROTOCOL_VERSION,
            project_id: "project-1".into(),
            peer_id: "peer-1".into(),
            session_id: "session-a".into(),
            session_started_at: 100,
            seq: 1,
            alias: "Alice".into(),
            active_doc: Some(Uuid::new_v4()),
            cursor_pos: Some(42),
            selection: Some((10, 20)),
            ttl_ms: PRESENCE_TTL_MS,
            timestamp: 1234567890,
        }
    }

    #[test]
    fn test_presence_update_roundtrip() {
        let update = sample_update();
        let encoded = update.encode();
        let decoded = PresenceUpdate::decode(&encoded).unwrap();
        assert_eq!(decoded, update);
    }

    #[test]
    fn test_project_topic_id_deterministic() {
        let t1 = project_topic_id("my-project");
        let t2 = project_topic_id("my-project");
        assert_eq!(t1, t2);
    }

    #[test]
    fn test_project_topic_id_unique() {
        let t1 = project_topic_id("project-a");
        let t2 = project_topic_id("project-b");
        assert_ne!(t1, t2);
    }

    #[test]
    fn test_presence_decode_invalid() {
        assert!(PresenceUpdate::decode(b"not json").is_none());
    }

    #[test]
    fn test_ordering_prefers_newer_seq_same_session() {
        let current = sample_update();
        let mut next = current.clone();
        next.seq = current.seq + 1;
        assert!(is_newer_presence(&current, &next));
        assert!(!is_newer_presence(&next, &current));
    }

    #[test]
    fn test_clear_detection() {
        let mut update = sample_update();
        update.active_doc = None;
        update.cursor_pos = None;
        update.selection = None;
        assert!(update.is_clear());
    }

    #[tokio::test]
    async fn test_apply_update_rejects_stale_sequence() {
        let manager = test_presence_manager().await;
        let current = sample_update();
        assert_eq!(
            manager.apply_update(current.clone(), 10),
            ApplyOutcome::Applied
        );

        let mut stale = current.clone();
        stale.seq = 0;
        assert_eq!(manager.apply_update(stale, 12), ApplyOutcome::IgnoredStale);
    }

    #[tokio::test]
    async fn test_apply_update_accepts_newer_session_started_at() {
        let manager = test_presence_manager().await;
        let current = sample_update();
        assert_eq!(
            manager.apply_update(current.clone(), 10),
            ApplyOutcome::Applied
        );

        let mut next_session = current.clone();
        next_session.session_id = "session-b".into();
        next_session.session_started_at = 200;
        next_session.seq = 1;
        next_session.active_doc = Some(Uuid::new_v4());
        assert_eq!(
            manager.apply_update(next_session, 12),
            ApplyOutcome::Applied
        );
    }

    #[tokio::test]
    async fn test_clear_peer_keeps_entry_but_clears_doc_state() {
        let manager = test_presence_manager().await;
        let current = sample_update();
        let project_id = current.project_id.clone();
        let peer_id = current.peer_id.clone();
        assert_eq!(manager.apply_update(current, 10), ApplyOutcome::Applied);

        manager.clear_peer(&project_id, &peer_id, 15);
        let cached = manager
            .cached_presence(&project_id)
            .get(&peer_id)
            .cloned()
            .expect("cache entry");
        assert_eq!(cached.update.active_doc, None);
        assert_eq!(cached.update.cursor_pos, None);
        assert_eq!(cached.update.selection, None);
        assert_eq!(cached.update.alias, "Alice");
    }
}
