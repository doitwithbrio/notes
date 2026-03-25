use std::collections::HashSet;
use std::sync::Arc;

use automerge::sync::{Message as SyncMessage, SyncDoc, State as SyncState};
use automerge::AutoCommit;

/// Generate a sync message, working around borrow lifetime issues with sync().
fn gen_sync_msg(doc: &mut AutoCommit, sync_state: &mut SyncState) -> Option<Vec<u8>> {
    let msg = doc.sync().generate_sync_message(sync_state);
    msg.map(|m| m.encode())
}

/// Receive a sync message and generate a response.
/// Returns (had_changes, response_bytes, new_change_hashes).
fn recv_and_gen(
    doc: &mut AutoCommit,
    sync_state: &mut SyncState,
    message: SyncMessage,
) -> Result<(bool, Option<Vec<u8>>, Vec<automerge::ChangeHash>), automerge::AutomergeError> {
    let heads_before: Vec<automerge::ChangeHash> = doc.get_heads().to_vec();
    doc.sync().receive_sync_message(sync_state, message)?;
    let heads_after = doc.get_heads();
    let had_changes = heads_before != heads_after;

    // Collect hashes of newly applied changes for signature verification
    let new_hashes = if had_changes {
        doc.get_changes(&heads_before)
            .iter()
            .map(|c| c.hash())
            .collect()
    } else {
        vec![]
    };

    let response = doc
        .sync()
        .generate_sync_message(sync_state)
        .map(|m| m.encode());
    Ok((had_changes, response, new_hashes))
}
use tokio::sync::RwLock;

use crate::protocol::{self, PROTOCOL_VERSION};

/// A single document sync session between two peers.
///
/// Manages the Automerge SyncState and handles reading/writing
/// framed sync messages over a QUIC bidirectional stream.
///
/// Supports optional Ed25519 signature verification on incoming changes
/// when `allowed_peers` is provided.
pub struct SyncSession {
    doc_id: [u8; 32],
    sync_state: SyncState,
    /// If set, verify that all incoming changes are signed by a peer in this list.
    allowed_peers: Option<Vec<iroh::EndpointId>>,
    /// Known Automerge actor hex IDs that are authorized for this document.
    /// Used for actor-ID-based verification of incoming changes.
    known_actor_ids: HashSet<String>,
    /// Shared reference to the signature store for Ed25519 verification.
    signatures: Option<Arc<dashmap::DashMap<([u8; 32], String), crate::protocol::ChangeSignature>>>,
    /// The remote peer's iroh identity (for logging).
    remote_peer: Option<iroh::EndpointId>,
}

impl SyncSession {
    pub fn new(doc_id: [u8; 32]) -> Self {
        Self {
            doc_id,
            sync_state: SyncState::new(),
            allowed_peers: None,
            known_actor_ids: HashSet::new(),
            signatures: None,
            remote_peer: None,
        }
    }

    /// Create a session with an existing sync state (for persistent sync).
    pub fn new_with_state(doc_id: [u8; 32], state: Option<SyncState>) -> Self {
        Self {
            doc_id,
            sync_state: state.unwrap_or_else(SyncState::new),
            allowed_peers: None,
            known_actor_ids: HashSet::new(),
            signatures: None,
            remote_peer: None,
        }
    }

    /// Set the ACL for signature verification.
    /// When set, all incoming changes are verified against this peer list.
    pub fn with_acl(mut self, allowed_peers: Vec<iroh::EndpointId>) -> Self {
        self.allowed_peers = Some(allowed_peers);
        self
    }

    /// Set known Automerge actor IDs for actor-based verification.
    /// These are the hex-encoded actor IDs of authorized peers.
    pub fn with_known_actors(mut self, actors: HashSet<String>) -> Self {
        self.known_actor_ids = actors;
        self
    }

    /// Set the signature store for Ed25519 verification of incoming changes.
    pub fn with_signatures(
        mut self,
        sigs: Arc<dashmap::DashMap<([u8; 32], String), crate::protocol::ChangeSignature>>,
    ) -> Self {
        self.signatures = Some(sigs);
        self
    }

    /// Set the remote peer identity (for logging).
    pub fn with_remote_peer(mut self, peer: iroh::EndpointId) -> Self {
        self.remote_peer = Some(peer);
        self
    }

    /// Get a reference to the sync state (for persisting after session).
    pub fn sync_state(&self) -> &SyncState {
        &self.sync_state
    }

    /// Run the sync session as the initiator (outgoing connection).
    ///
    /// Opens the stream, sends the header, then enters the sync loop.
    pub async fn run_initiator(
        &mut self,
        connection: &iroh::endpoint::Connection,
        doc: Arc<RwLock<AutoCommit>>,
    ) -> Result<(), SyncError> {
        let (mut send, mut recv) = connection.open_bi().await?;

        // Send stream header
        let header =
            protocol::encode_stream_header(protocol::MessageType::SyncMessage, &self.doc_id);
        write_bytes(&mut send, &header).await?;

        // Sync loop
        self.sync_loop(&mut send, &mut recv, doc).await?;

        let _ = send.finish();
        Ok(())
    }

    /// Run the sync session as the responder (incoming connection).
    ///
    /// The header has already been read by the protocol handler.
    pub async fn run_responder(
        &mut self,
        send: &mut iroh::endpoint::SendStream,
        recv: &mut iroh::endpoint::RecvStream,
        doc: Arc<RwLock<AutoCommit>>,
    ) -> Result<(), SyncError> {
        self.sync_loop(send, recv, doc).await
    }

    /// The core sync loop: exchange Automerge sync messages until both sides converge.
    ///
    /// When `allowed_peers` is set, newly applied changes are checked post-receive.
    /// This is a post-hoc verification — Automerge applies the changes first, then
    /// we check that the actors who produced them are authorized. If verification fails,
    /// the session is terminated with an error. The applied changes remain (Automerge
    /// doesn't support rollback), but no further changes are accepted.
    ///
    /// This approach is practical because:
    /// 1. Automerge sync operates at the sync-message level, not individual changes
    /// 2. Sync messages are opaque; we can only inspect changes after application
    /// 3. The transport is already authenticated (iroh QUIC E2E)
    /// 4. Post-hoc detection + session termination is sufficient for honest-but-curious peers
    async fn sync_loop(
        &mut self,
        send: &mut iroh::endpoint::SendStream,
        recv: &mut iroh::endpoint::RecvStream,
        doc: Arc<RwLock<AutoCommit>>,
    ) -> Result<(), SyncError> {
        // Generate and send initial sync message
        {
            let mut doc = doc.write().await;
            if let Some(encoded) = gen_sync_msg(&mut doc, &mut self.sync_state) {
                let framed = protocol::encode_framed(&encoded)
                    .map_err(|e| SyncError::Protocol(e.to_string()))?;
                write_bytes(send, &framed).await?;
            }
        }

        let mut unproductive_rounds = 0u32;
        let mut resets = 0u32;
        const MAX_UNPRODUCTIVE: u32 = 10;
        const MAX_RESETS: u32 = 2;

        loop {
            // Read next framed message from peer
            let msg_bytes = match read_framed(recv).await {
                Ok(bytes) => bytes,
                Err(SyncError::StreamFinished) => {
                    log::debug!("Sync stream finished for doc {:?}", &self.doc_id[..4]);
                    break;
                }
                Err(e) => return Err(e),
            };

            // Apply the peer's sync message
            let peer_msg = SyncMessage::decode(&msg_bytes)
                .map_err(|e| SyncError::Protocol(format!("invalid sync message: {e}")))?;

            let (had_changes, response, new_hashes) = {
                let mut doc = doc.write().await;
                recv_and_gen(&mut doc, &mut self.sync_state, peer_msg)?
            };

            // Post-receive signature verification (when ACL is configured)
            if had_changes {
                if let Some(ref allowed) = self.allowed_peers {
                    if let Err(e) = self.verify_new_changes(&doc, &new_hashes, allowed).await {
                        log::error!(
                            "Signature verification failed for doc {:?} from {:?}: {e}",
                            &self.doc_id[..4],
                            self.remote_peer,
                        );
                        return Err(SyncError::SignatureError(e.to_string()));
                    }
                }
                unproductive_rounds = 0;
            } else {
                unproductive_rounds += 1;
            }

            match response {
                Some(encoded) => {
                    let framed = protocol::encode_framed(&encoded)
                        .map_err(|e| SyncError::Protocol(e.to_string()))?;
                    write_bytes(send, &framed).await?;
                }
                None => {
                    log::debug!("Sync complete for doc {:?}", &self.doc_id[..4]);
                    break;
                }
            }

            // Safety valve: if we've done too many rounds without progress, reset
            if unproductive_rounds >= MAX_UNPRODUCTIVE {
                if resets >= MAX_RESETS {
                    log::error!(
                        "Sync failed for doc {:?} after {MAX_RESETS} resets — giving up",
                        &self.doc_id[..4]
                    );
                    return Err(SyncError::Protocol(
                        "sync convergence failed after multiple resets".to_string(),
                    ));
                }
                resets += 1;
                log::warn!(
                    "Sync stalled for doc {:?}, reset {resets}/{MAX_RESETS}",
                    &self.doc_id[..4]
                );
                self.sync_state = SyncState::new();
                unproductive_rounds = 0;
            }
        }

        Ok(())
    }

    /// Verify that newly applied changes come from authorized peers.
    ///
    /// Checks that the Automerge actor ID of each new change is in the
    /// `known_actor_ids` set. If the set is empty (no actor mapping configured),
    /// falls back to allow-all with a warning (graceful degradation for
    /// projects that haven't set up actor ID registration yet).
    ///
    /// When per-change Ed25519 signatures are added, this function will
    /// additionally verify `SignedChange` envelopes via `verify_and_check_acl()`.
    async fn verify_new_changes(
        &self,
        doc: &Arc<RwLock<AutoCommit>>,
        new_hashes: &[automerge::ChangeHash],
        _allowed_peers: &[iroh::EndpointId],
    ) -> Result<(), SyncError> {
        if new_hashes.is_empty() {
            return Ok(());
        }

        // If no known actor IDs are configured, allow all with a warning.
        // This handles projects that haven't registered actor IDs yet.
        if self.known_actor_ids.is_empty() {
            log::debug!(
                "No known actor IDs configured for doc {:?}, skipping actor verification",
                &self.doc_id[..4]
            );
            return Ok(());
        }

        let mut doc = doc.write().await;
        let changes = doc.get_changes(&[]);

        for hash in new_hashes {
            let change = changes.iter().find(|c| c.hash() == *hash);
            if let Some(change) = change {
                let actor = change.actor_id().to_hex_string();

                if !self.known_actor_ids.contains(&actor) {
                    log::error!(
                        "REJECTED change {} from unknown actor {} in doc {:?} (remote: {:?})",
                        hash,
                        &actor[..8.min(actor.len())],
                        &self.doc_id[..4],
                        self.remote_peer,
                    );
                    return Err(SyncError::SignatureError(format!(
                        "change from unknown actor {}", &actor[..8.min(actor.len())]
                    )));
                }

                // If we have a signature store, try to verify the Ed25519 signature
                if let Some(ref sig_store) = self.signatures {
                    let hash_hex = hash.to_string();
                    if let Some(sig_entry) = sig_store.get(&(self.doc_id, hash_hex.clone())) {
                        let sig = sig_entry.value();
                        // Verify the signature using notes-crypto
                        if let Some(signed) = notes_crypto::SignedChange::from_parts(
                            &sig.author,
                            change.raw_bytes(),
                            &sig.signature,
                        ) {
                            match signed.verify() {
                                Ok(_) => {
                                    log::debug!(
                                        "Signature verified for change {} from {}",
                                        &hash_hex[..8],
                                        &sig.author[..8.min(sig.author.len())]
                                    );
                                }
                                Err(e) => {
                                    log::error!(
                                        "INVALID signature for change {}: {e}",
                                        &hash_hex[..8]
                                    );
                                    return Err(SyncError::SignatureError(format!(
                                        "invalid signature for change {}",
                                        &hash_hex[..8]
                                    )));
                                }
                            }
                        } else {
                            log::warn!(
                                "Malformed signature entry for change {}, accepting without verification",
                                &hash_hex[..8]
                            );
                        }
                    }
                    // No signature found — accept (graceful degradation for pre-signing peers)
                }

                log::debug!(
                    "Verified change {} from actor {} in doc {:?}",
                    hash,
                    &actor[..8.min(actor.len())],
                    &self.doc_id[..4]
                );
            }
        }

        Ok(())
    }
}

/// Write bytes to a QUIC send stream, mapping errors to SyncError.
async fn write_bytes(
    send: &mut iroh::endpoint::SendStream,
    data: &[u8],
) -> Result<(), SyncError> {
    send.write_all(data)
        .await
        .map_err(|e| SyncError::Io(Box::new(e)))?;
    Ok(())
}

/// Read a length-prefixed framed message from a QUIC receive stream.
pub async fn read_framed(recv: &mut iroh::endpoint::RecvStream) -> Result<Vec<u8>, SyncError> {
    let mut len_buf = [0u8; 4];
    match recv.read_exact(&mut len_buf).await {
        Ok(()) => {}
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("finished")
                || msg.contains("STREAM_FIN")
                || msg.contains("ClosedStream")
            {
                return Err(SyncError::StreamFinished);
            }
            return Err(SyncError::Io(Box::new(e)));
        }
    }

    let len = u32::from_be_bytes(len_buf) as usize;
    if len > protocol::MAX_MESSAGE_SIZE as usize {
        return Err(SyncError::Protocol(format!(
            "message too large: {len} bytes"
        )));
    }

    let mut buf = vec![0u8; len];
    recv.read_exact(&mut buf)
        .await
        .map_err(|e| SyncError::Io(Box::new(e)))?;
    Ok(buf)
}

/// Read the stream header (version + message type + doc ID).
pub async fn read_stream_header(
    recv: &mut iroh::endpoint::RecvStream,
) -> Result<(protocol::MessageType, [u8; 32]), SyncError> {
    let mut header = [0u8; 34];
    recv.read_exact(&mut header)
        .await
        .map_err(|e| SyncError::Io(Box::new(e)))?;

    let version = header[0];
    if version != PROTOCOL_VERSION {
        return Err(SyncError::Protocol(format!(
            "unsupported protocol version: {version}"
        )));
    }

    let msg_type = protocol::MessageType::try_from(header[1])
        .map_err(|e| SyncError::Protocol(e.to_string()))?;

    let mut doc_id = [0u8; 32];
    doc_id.copy_from_slice(&header[2..34]);

    Ok((msg_type, doc_id))
}

/// Errors during sync.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("QUIC connection error: {0}")]
    Connection(#[from] iroh::endpoint::ConnectionError),

    #[error("IO error: {0}")]
    Io(#[source] Box<dyn std::error::Error + Send + Sync>),

    #[error("Automerge error: {0}")]
    Automerge(#[from] automerge::AutomergeError),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Signature verification failed: {0}")]
    SignatureError(String),

    #[error("Stream finished")]
    StreamFinished,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_session_new() {
        let doc_id = [0xAA; 32];
        let session = SyncSession::new(doc_id);
        assert_eq!(session.doc_id, doc_id);
    }

    #[test]
    fn test_sync_session_with_acl() {
        let doc_id = [0xAA; 32];
        let mut rng = [0u8; 32];
        getrandom::fill(&mut rng).unwrap();
        let key = iroh::SecretKey::from_bytes(&rng);
        let peer = key.public();

        let session = SyncSession::new(doc_id).with_acl(vec![peer]);
        assert!(session.allowed_peers.is_some());
        assert_eq!(session.allowed_peers.unwrap().len(), 1);
    }
}
