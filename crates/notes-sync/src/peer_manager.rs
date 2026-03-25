//! Manages persistent peer connections per project.
//!
//! Tracks which peers belong to which project, maintains iroh connections,
//! and provides methods for syncing docs with peers.

use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use iroh::endpoint::{Connection, Endpoint};
use iroh::EndpointId;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::events::{PeerConnectionState, PeerStatusEvent};
use crate::sync_engine::{SyncEngine, NOTES_SYNC_ALPN};

/// Manages peer connections and live sync for all projects.
pub struct PeerManager {
    endpoint: Endpoint,
    sync_engine: Arc<SyncEngine>,

    /// Active connections per peer ID.
    connections: DashMap<EndpointId, Connection>,

    /// Peers registered per project (project name -> set of peer IDs).
    project_peers: DashMap<String, Vec<EndpointId>>,

    /// Cancellation token for background tasks.
    cancel: CancellationToken,

    /// Broadcast channel for peer status changes (connect/disconnect).
    status_tx: tokio::sync::broadcast::Sender<PeerStatusEvent>,
}

impl PeerManager {
    pub fn new(endpoint: Endpoint, sync_engine: Arc<SyncEngine>) -> Self {
        let (status_tx, _) = tokio::sync::broadcast::channel(64);
        Self {
            endpoint,
            sync_engine,
            connections: DashMap::new(),
            project_peers: DashMap::new(),
            cancel: CancellationToken::new(),
            status_tx,
        }
    }

    /// Subscribe to peer status change events (connect/disconnect).
    pub fn subscribe_peer_status(&self) -> tokio::sync::broadcast::Receiver<PeerStatusEvent> {
        self.status_tx.subscribe()
    }

    /// Register a peer for a project.
    pub fn add_peer_to_project(&self, project: &str, peer_id: EndpointId) {
        let mut peers = self
            .project_peers
            .entry(project.to_string())
            .or_default();
        if !peers.contains(&peer_id) {
            peers.push(peer_id);
            log::info!("Added peer {peer_id} to project {project}");
        }
    }

    /// Remove a peer from a project.
    pub fn remove_peer_from_project(&self, project: &str, peer_id: &EndpointId) {
        if let Some(mut peers) = self.project_peers.get_mut(project) {
            peers.retain(|p| p != peer_id);
        }
        // If this peer isn't in any project, close the connection
        let still_needed = self
            .project_peers
            .iter()
            .any(|entry| entry.value().contains(peer_id));
        if !still_needed {
            if let Some((_, conn)) = self.connections.remove(peer_id) {
                conn.close(0u32.into(), b"removed");
            }
        }
    }

    /// Get or establish a connection to a peer.
    pub async fn get_or_connect(
        &self,
        peer_id: EndpointId,
    ) -> Result<Connection, PeerError> {
        // Check for existing live connection
        if let Some(entry) = self.connections.get(&peer_id) {
            // close_reason() returns Some if closed, None if alive
            if entry.close_reason().is_none() {
                return Ok(entry.clone());
            }
            // Dead connection — drop ref before removing
            drop(entry);
            self.connections.remove(&peer_id);
        }

        // Establish new connection
        log::info!("Connecting to peer {peer_id}");
        let connection = tokio::time::timeout(
            Duration::from_secs(15),
            self.endpoint.connect(peer_id, NOTES_SYNC_ALPN),
        )
        .await
        .map_err(|_| PeerError::ConnectionTimeout)?
        .map_err(|e| PeerError::Connection(e.to_string()))?;

        self.connections.insert(peer_id, connection.clone());
        log::info!("Connected to peer {peer_id}");
        Ok(connection)
    }

    /// Sync a specific document with all peers in a project.
    pub async fn sync_doc_with_project_peers(
        &self,
        project: &str,
        doc_id: Uuid,
    ) -> Vec<(EndpointId, Result<(), PeerError>)> {
        let peer_ids: Vec<EndpointId> = self
            .project_peers
            .get(project)
            .map(|entry| entry.value().clone())
            .unwrap_or_default();

        let mut results = Vec::new();

        for peer_id in peer_ids {
            let result = async {
                let connection = self.get_or_connect(peer_id).await?;
                tokio::time::timeout(
                    Duration::from_secs(60),
                    self.sync_engine.sync_doc_with_peer(&connection, doc_id),
                )
                .await
                .map_err(|_| PeerError::SyncTimeout)?
                .map_err(|e| PeerError::Sync(e.to_string()))?;
                Ok(())
            }
            .await;

            if let Err(ref e) = result {
                log::warn!("Sync with peer {peer_id} failed: {e}");
            }
            results.push((peer_id, result));
        }

        results
    }

    /// Get list of peers for a project.
    pub fn get_project_peers(&self, project: &str) -> Vec<EndpointId> {
        self.project_peers
            .get(project)
            .map(|entry| entry.value().clone())
            .unwrap_or_default()
    }

    /// Get connection status for a peer.
    pub fn is_peer_connected(&self, peer_id: &EndpointId) -> bool {
        self.connections
            .get(peer_id)
            .map(|entry| entry.close_reason().is_none())
            .unwrap_or(false)
    }

    /// Get the number of active connections.
    pub fn active_connection_count(&self) -> usize {
        self.connections
            .iter()
            .filter(|entry| entry.close_reason().is_none())
            .count()
    }

    /// Start a background task that monitors connections and auto-reconnects.
    /// Checks every `interval` and reconnects disconnected peers.
    /// Returns a JoinHandle for the monitoring task.
    pub fn start_monitoring(
        self: &Arc<Self>,
        interval: Duration,
    ) -> tokio::task::JoinHandle<()> {
        let this = Arc::clone(self);
        let cancel = this.cancel.clone();

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        log::debug!("Peer monitoring task cancelled");
                        break;
                    }
                    _ = ticker.tick() => {
                        // Collect all registered peers across all projects
                        let all_peers: Vec<EndpointId> = this
                            .project_peers
                            .iter()
                            .flat_map(|entry| entry.value().clone())
                            .collect();

                        for peer_id in all_peers {
                            if !this.is_peer_connected(&peer_id) {
                                log::debug!("Auto-reconnecting to peer {peer_id}");
                                match this.get_or_connect(peer_id).await {
                                    Ok(_) => {
                                        log::info!("Auto-reconnected to peer {peer_id}");
                                        let _ = this.status_tx.send(PeerStatusEvent {
                                            peer_id: peer_id.to_string(),
                                            state: PeerConnectionState::Connected,
                                            alias: None,
                                        });
                                    }
                                    Err(e) => {
                                        log::debug!("Auto-reconnect failed for {peer_id}: {e}");
                                    }
                                }
                            }
                        }

                        // Clean up dead connections from the map
                        let dead_peers: Vec<EndpointId> = this
                            .connections
                            .iter()
                            .filter(|entry| entry.close_reason().is_some())
                            .map(|entry| *entry.key())
                            .collect();

                        for peer_id in dead_peers {
                            this.connections.remove(&peer_id);
                            let _ = this.status_tx.send(PeerStatusEvent {
                                peer_id: peer_id.to_string(),
                                state: PeerConnectionState::Disconnected,
                                alias: None,
                            });
                        }
                    }
                }
            }
        })
    }

    /// Shut down all connections and cancel background tasks.
    pub async fn shutdown(&self) {
        self.cancel.cancel();

        let keys: Vec<EndpointId> = self
            .connections
            .iter()
            .map(|entry| *entry.key())
            .collect();

        for peer_id in &keys {
            if let Some((_, conn)) = self.connections.remove(peer_id) {
                conn.close(0u32.into(), b"shutdown");
            }
        }

        log::info!("PeerManager shutdown: closed {} connections", keys.len());
    }

    /// Get the cancellation token (for coordinating with external tasks).
    pub fn cancel_token(&self) -> &CancellationToken {
        &self.cancel
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PeerError {
    #[error("Connection timed out")]
    ConnectionTimeout,

    #[error("Connection failed: {0}")]
    Connection(String),

    #[error("Sync timed out")]
    SyncTimeout,

    #[error("Sync failed: {0}")]
    Sync(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_project_peers_data_structure() {
        let peers: DashMap<String, Vec<EndpointId>> = DashMap::new();
        let peer_id: EndpointId =
            "b27ef3e7a4c94bac1daa3f233e0dd19c6f69d88ad9d833e593da93c57f75e6dd"
                .parse()
                .unwrap();

        peers
            .entry("test-project".to_string())
            .or_default()
            .push(peer_id);

        assert_eq!(peers.get("test-project").unwrap().len(), 1);
        assert!(peers.get("nonexistent").is_none());
    }
}
