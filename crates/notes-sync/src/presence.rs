//! Cursor presence and ephemeral status via iroh-gossip.
//!
//! Each project has a gossip topic. Peers broadcast their cursor
//! positions, active document, and online status. Messages are
//! ephemeral — not persisted, not synced, just real-time.

use std::time::Instant;

use dashmap::DashMap;
use iroh::EndpointId;
use iroh_gossip::net::Gossip;
use iroh_gossip::TopicId;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use uuid::Uuid;

/// Maximum presence updates per second per peer (throttle).
pub const MAX_PRESENCE_RATE: u32 = 10;

/// Minimum interval between updates from the same peer (100ms = 10/sec).
const MIN_UPDATE_INTERVAL_MS: u128 = 1000 / MAX_PRESENCE_RATE as u128;

/// Maximum size of a single presence message in bytes.
const MAX_PRESENCE_MESSAGE_SIZE: usize = 1024;

/// A presence update broadcast to peers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresenceUpdate {
    /// The peer sending this update.
    pub peer_id: String,
    /// Display name / alias.
    pub alias: String,
    /// Which document the peer has open (if any).
    pub active_doc: Option<Uuid>,
    /// Cursor position (ProseMirror offset).
    pub cursor_pos: Option<u64>,
    /// Selection anchor + head.
    pub selection: Option<(u64, u64)>,
    /// Timestamp (millis since epoch).
    pub timestamp: u64,
}

impl PresenceUpdate {
    pub fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("presence serialization should not fail")
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        serde_json::from_slice(data).ok()
    }
}

/// Derives a gossip TopicId from a project ID.
pub fn project_topic_id(project_id: &str) -> TopicId {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(format!("p2p-notes/gossip/{project_id}").as_bytes());
    let mut topic = [0u8; 32];
    topic.copy_from_slice(&hash[..32]);
    TopicId::from(topic)
}

/// Manages gossip-based presence for projects.
pub struct PresenceManager {
    gossip: Gossip,
    /// Channel for broadcasting received presence updates to the frontend.
    presence_tx: broadcast::Sender<PresenceUpdate>,
    /// Per-peer rate limiter: tracks the last accepted update time per peer.
    rate_limiters: DashMap<String, Instant>,
}

impl PresenceManager {
    pub fn new(gossip: Gossip) -> Self {
        let (presence_tx, _) = broadcast::channel(256);
        Self {
            gossip,
            presence_tx,
            rate_limiters: DashMap::new(),
        }
    }

    /// Check if a presence update from a peer should be accepted (rate limiting).
    /// Returns true if the update passes the rate limit, false to drop.
    pub fn should_accept_update(&self, peer_id: &str) -> bool {
        let now = Instant::now();
        if let Some(last) = self.rate_limiters.get(peer_id) {
            if now.duration_since(*last).as_millis() < MIN_UPDATE_INTERVAL_MS {
                return false; // Rate limited — drop this update
            }
        }
        self.rate_limiters.insert(peer_id.to_string(), now);
        true
    }

    /// Process an incoming presence update with rate limiting and validation.
    /// Returns Some(update) if accepted, None if rate-limited or invalid.
    pub fn process_incoming(&self, data: &[u8]) -> Option<PresenceUpdate> {
        // Size check
        if data.len() > MAX_PRESENCE_MESSAGE_SIZE {
            log::warn!("Oversized presence message ({} bytes), dropping", data.len());
            return None;
        }

        let update = PresenceUpdate::decode(data)?;

        // Validate field lengths
        if update.alias.len() > 64 || update.peer_id.len() > 128 {
            log::warn!("Presence update with oversized fields, dropping");
            return None;
        }

        // Rate limit
        if !self.should_accept_update(&update.peer_id) {
            return None;
        }

        Some(update)
    }

    /// Subscribe to presence updates (for the frontend event loop).
    pub fn subscribe(&self) -> broadcast::Receiver<PresenceUpdate> {
        self.presence_tx.subscribe()
    }

    /// Join a project's gossip topic and start receiving presence updates.
    pub async fn join_project(
        &self,
        project_id: &str,
        bootstrap_peers: Vec<EndpointId>,
    ) -> Result<iroh_gossip::api::GossipTopic, PresenceError> {
        let topic = project_topic_id(project_id);
        let sub = self
            .gossip
            .subscribe(topic, bootstrap_peers)
            .await
            .map_err(|e| PresenceError::Gossip(e.to_string()))?;
        Ok(sub)
    }

    /// Get the broadcast sender for forwarding presence to frontend.
    pub fn presence_sender(&self) -> &broadcast::Sender<PresenceUpdate> {
        &self.presence_tx
    }

    /// Reference to the underlying gossip instance.
    pub fn gossip(&self) -> &Gossip {
        &self.gossip
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PresenceError {
    #[error("Gossip error: {0}")]
    Gossip(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_presence_update_roundtrip() {
        let update = PresenceUpdate {
            peer_id: "abc123".to_string(),
            alias: "Alice".to_string(),
            active_doc: Some(Uuid::new_v4()),
            cursor_pos: Some(42),
            selection: Some((10, 20)),
            timestamp: 1234567890,
        };

        let encoded = update.encode();
        let decoded = PresenceUpdate::decode(&encoded).unwrap();
        assert_eq!(decoded.alias, "Alice");
        assert_eq!(decoded.cursor_pos, Some(42));
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
}
