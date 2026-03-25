use std::sync::Arc;

use automerge::AutoCommit;
use dashmap::DashMap;
use iroh::endpoint::Connection;
use iroh::protocol::AcceptError;
use tokio::sync::{RwLock, Semaphore};
use uuid::Uuid;

use crate::protocol;
use crate::sync_session::{self, SyncError, SyncSession};

/// The ALPN protocol identifier for notes sync.
pub const NOTES_SYNC_ALPN: &[u8] = b"/p2p-notes/sync/1";

/// Peer role for ACL enforcement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerRole {
    Owner,
    Editor,
    Viewer,
    /// Unknown peer — not in the ACL.
    Unauthorized,
}

/// The sync engine manages document sync over iroh connections.
///
/// It implements `iroh::protocol::ProtocolHandler` to accept incoming sync requests,
/// and provides methods to initiate outgoing sync with peers.
///
/// Remote change notifications are sent via a `tokio::sync::broadcast` channel.
/// Subscribe with `SyncEngine::subscribe_remote_changes()` to receive `Uuid` doc IDs.
/// Maximum concurrent connections handled by the sync engine.
const MAX_CONNECTIONS: usize = 32;

/// Maximum concurrent bidirectional streams per connection.
const MAX_STREAMS_PER_CONNECTION: usize = 16;

pub struct SyncEngine {
    /// Documents available for sync, keyed by their 32-byte wire ID.
    docs: Arc<DashMap<[u8; 32], Arc<RwLock<AutoCommit>>>>,

    /// ACL: maps (doc_key, peer_id) -> role. If no entry, peer is unauthorized.
    acl: Arc<DashMap<([u8; 32], iroh::EndpointId), PeerRole>>,

    /// Persistent sync state store (optional — if None, fresh state per session).
    sync_state_store: Option<Arc<crate::SyncStateStore>>,

    /// Channel for notifying about remote changes.
    change_tx: tokio::sync::broadcast::Sender<Uuid>,

    /// Semaphore limiting concurrent incoming connections.
    connection_semaphore: Arc<Semaphore>,
}

impl std::fmt::Debug for SyncEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyncEngine")
            .field("docs_count", &self.docs.len())
            .finish()
    }
}

impl SyncEngine {
    pub fn new() -> Self {
        let (change_tx, _) = tokio::sync::broadcast::channel(256);
        Self {
            docs: Arc::new(DashMap::new()),
            acl: Arc::new(DashMap::new()),
            sync_state_store: None,
            change_tx,
            connection_semaphore: Arc::new(Semaphore::new(MAX_CONNECTIONS)),
        }
    }

    /// Set the sync state store for persistent sync states.
    pub fn set_sync_state_store(&mut self, store: Arc<crate::SyncStateStore>) {
        self.sync_state_store = Some(store);
    }

    /// Subscribe to remote change notifications.
    /// Returns a receiver that yields the DocId (as Uuid) of changed documents.
    pub fn subscribe_remote_changes(&self) -> tokio::sync::broadcast::Receiver<Uuid> {
        self.change_tx.subscribe()
    }


    /// Register a document for sync.
    pub fn register_doc(&self, doc_id: Uuid, doc: Arc<RwLock<AutoCommit>>) {
        let key = uuid_to_doc_key(doc_id);
        self.docs.insert(key, doc);
    }

    /// Unregister a document from sync.
    pub fn unregister_doc(&self, doc_id: &Uuid) {
        let key = uuid_to_doc_key(*doc_id);
        self.docs.remove(&key);
    }

    /// Set the ACL role for a peer on a document.
    pub fn set_peer_role(
        &self,
        doc_id: Uuid,
        peer_id: iroh::EndpointId,
        role: PeerRole,
    ) {
        let key = uuid_to_doc_key(doc_id);
        self.acl.insert((key, peer_id), role);
    }

    /// Remove a peer's ACL entry for a document.
    pub fn remove_peer_role(&self, doc_id: Uuid, peer_id: &iroh::EndpointId) {
        let key = uuid_to_doc_key(doc_id);
        self.acl.remove(&(key, *peer_id));
    }

    /// Check a peer's role for a document.
    fn check_peer_role(
        &self,
        doc_key: &[u8; 32],
        peer_id: &iroh::EndpointId,
    ) -> PeerRole {
        self.acl
            .get(&(*doc_key, *peer_id))
            .map(|entry| *entry.value())
            .unwrap_or(PeerRole::Unauthorized)
    }

    /// Get a document by its wire-protocol key.
    fn get_doc(&self, key: &[u8; 32]) -> Option<Arc<RwLock<AutoCommit>>> {
        self.docs.get(key).map(|entry| Arc::clone(entry.value()))
    }

    /// Initiate a sync with a remote peer for a specific document.
    pub async fn sync_doc_with_peer(
        &self,
        connection: &Connection,
        doc_id: Uuid,
    ) -> Result<(), SyncError> {
        let key = uuid_to_doc_key(doc_id);
        let doc = self
            .get_doc(&key)
            .ok_or_else(|| SyncError::Protocol(format!("document {doc_id} not registered")))?;

        // Load persisted sync state if available
        let peer_id = connection.remote_id();
        let initial_state = if let Some(ref store) = self.sync_state_store {
            Some(store.load_or_create(&peer_id, &doc_id).await)
        } else {
            None
        };

        let mut session = SyncSession::new_with_state(key, initial_state);
        session.run_initiator(connection, doc).await?;

        // Save sync state after successful sync
        if let Some(ref store) = self.sync_state_store {
            if let Err(e) = store.save(&peer_id, &doc_id, session.sync_state()).await {
                log::warn!("Failed to save sync state for {doc_id}: {e}");
            }
        }

        // Notify that remote changes may have been applied
        let _ = self.change_tx.send(doc_id);

        log::info!("Outgoing sync complete for doc {doc_id}");
        Ok(())
    }

    /// Handle an incoming sync connection (called by the ProtocolHandler).
    async fn handle_connection(&self, connection: Connection) -> Result<(), SyncError> {
        log::debug!(
            "Incoming sync connection from {:?}",
            connection.remote_id()
        );

        let stream_semaphore = Semaphore::new(MAX_STREAMS_PER_CONNECTION);

        loop {
            // Limit concurrent streams per connection
            let _stream_permit = stream_semaphore.acquire().await.map_err(|_| {
                SyncError::Protocol("stream semaphore closed".to_string())
            })?;

            // Accept the next bidirectional stream
            let (mut send, mut recv) = match connection.accept_bi().await {
                Ok(streams) => streams,
                Err(iroh::endpoint::ConnectionError::ApplicationClosed(_)) => {
                    log::debug!("Sync connection closed by peer");
                    break;
                }
                Err(e) => return Err(SyncError::Connection(e)),
            };

            // Read stream header
            let (msg_type, doc_id) = sync_session::read_stream_header(&mut recv).await?;

            // ACL check: verify the remote peer has access to this document
            let remote_id = connection.remote_id();
            let role = self.check_peer_role(&doc_id, &remote_id);

            if role == PeerRole::Unauthorized {
                log::warn!(
                    "Unauthorized peer {:?} tried to sync doc {:?}",
                    remote_id,
                    &doc_id[..4]
                );
                let _ = send.finish();
                continue;
            }

            match msg_type {
                protocol::MessageType::SyncMessage => {
                    // Only Editors and Owners can do bidirectional sync
                    if role == PeerRole::Viewer {
                        // Viewer gets a read-only snapshot instead
                        log::debug!(
                            "Viewer {:?} requested sync for {:?}, sending snapshot",
                            remote_id,
                            &doc_id[..4]
                        );
                        if let Some(doc) = self.get_doc(&doc_id) {
                            let mut doc = doc.write().await;
                            let snapshot = doc.save();
                            // Send snapshot as length-prefixed data
                            let len = (snapshot.len() as u32).to_be_bytes();
                            let _ = send.write_all(&len).await;
                            let _ = send.write_all(&snapshot).await;
                        }
                        let _ = send.finish();
                        continue;
                    }

                    let doc = match self.get_doc(&doc_id) {
                        Some(doc) => doc,
                        None => {
                            log::warn!(
                                "Sync requested for unknown doc {:?}, ignoring stream",
                                &doc_id[..4]
                            );
                            let _ = send.finish();
                            continue;
                        }
                    };

                    let mut session = SyncSession::new(doc_id);
                    if let Err(e) = session.run_responder(&mut send, &mut recv, doc).await {
                        log::error!("Sync session error for doc {:?}: {e}", &doc_id[..4]);
                    }

                    // Notify about potential changes
                    if let Some(uuid) = doc_key_to_uuid(&doc_id) {
                        let _ = self.change_tx.send(uuid);
                    }

                    let _ = send.finish();
                }
                protocol::MessageType::ViewerSnapshot => {
                    // Push a read-only snapshot to the viewer
                    if let Some(doc) = self.get_doc(&doc_id) {
                        let mut doc = doc.write().await;
                        let snapshot = doc.save();
                        let len = (snapshot.len() as u32).to_be_bytes();
                        let _ = send.write_all(&len).await;
                        let _ = send.write_all(&snapshot).await;
                    }
                    let _ = send.finish();
                }
                protocol::MessageType::PresenceUpdate => {
                    log::debug!("Presence update handled via gossip, not sync stream");
                    let _ = send.finish();
                }
            }
        }

        Ok(())
    }
}

impl Default for SyncEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Implement iroh's ProtocolHandler for accepting incoming sync connections.
impl iroh::protocol::ProtocolHandler for SyncEngine {
    fn accept(
        &self,
        connection: Connection,
    ) -> impl std::future::Future<Output = Result<(), AcceptError>> + Send {
        let engine = SyncEngine {
            docs: Arc::clone(&self.docs),
            acl: Arc::clone(&self.acl),
            sync_state_store: self.sync_state_store.clone(),
            change_tx: self.change_tx.clone(),
            connection_semaphore: Arc::clone(&self.connection_semaphore),
        };
        let semaphore = Arc::clone(&self.connection_semaphore);

        async move {
            // Limit concurrent connections
            let _permit = match semaphore.try_acquire() {
                Ok(permit) => permit,
                Err(_) => {
                    log::warn!("Connection limit reached, rejecting incoming sync connection");
                    return Ok(());
                }
            };

            if let Err(e) = engine.handle_connection(connection).await {
                log::error!("Sync protocol error: {e}");
            }
            Ok(())
        }
    }
}

/// Convert a UUID to a 32-byte document key for the wire protocol.
fn uuid_to_doc_key(id: Uuid) -> [u8; 32] {
    let mut key = [0u8; 32];
    key[..16].copy_from_slice(id.as_bytes());
    key
}

/// Convert a 32-byte document key back to a UUID.
fn doc_key_to_uuid(key: &[u8; 32]) -> Option<Uuid> {
    if key[16..].iter().any(|&b| b != 0) {
        return None;
    }
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&key[..16]);
    Some(Uuid::from_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uuid_key_roundtrip() {
        let id = Uuid::new_v4();
        let key = uuid_to_doc_key(id);
        let recovered = doc_key_to_uuid(&key).unwrap();
        assert_eq!(id, recovered);
    }

    #[test]
    fn test_register_and_unregister() {
        let engine = SyncEngine::new();
        let id = Uuid::new_v4();
        let doc = Arc::new(RwLock::new(AutoCommit::new()));

        engine.register_doc(id, doc);
        assert!(engine.get_doc(&uuid_to_doc_key(id)).is_some());

        engine.unregister_doc(&id);
        assert!(engine.get_doc(&uuid_to_doc_key(id)).is_none());
    }
}
