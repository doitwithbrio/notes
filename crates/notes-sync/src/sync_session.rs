use std::sync::Arc;

use automerge::sync::{Message as SyncMessage, SyncDoc, State as SyncState};
use automerge::AutoCommit;

/// Generate a sync message, working around borrow lifetime issues with sync().
fn gen_sync_msg(
    doc: &mut AutoCommit,
    sync_state: &mut SyncState,
) -> Option<Vec<u8>> {
    let msg = doc.sync().generate_sync_message(sync_state);
    msg.map(|m| m.encode())
}

/// Receive a sync message and generate a response.
fn recv_and_gen(
    doc: &mut AutoCommit,
    sync_state: &mut SyncState,
    message: SyncMessage,
) -> Result<(bool, Option<Vec<u8>>), automerge::AutomergeError> {
    let heads_before = doc.get_heads();
    doc.sync().receive_sync_message(sync_state, message)?;
    let heads_after = doc.get_heads();
    let had_changes = heads_before != heads_after;
    let response = doc.sync().generate_sync_message(sync_state).map(|m| m.encode());
    Ok((had_changes, response))
}
use tokio::sync::RwLock;

use crate::protocol::{self, PROTOCOL_VERSION};

/// A single document sync session between two peers.
///
/// Manages the Automerge SyncState and handles reading/writing
/// framed sync messages over a QUIC bidirectional stream.
pub struct SyncSession {
    doc_id: [u8; 32],
    sync_state: SyncState,
}

impl SyncSession {
    pub fn new(doc_id: [u8; 32]) -> Self {
        Self {
            doc_id,
            sync_state: SyncState::new(),
        }
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

            let (had_changes, response) = {
                let mut doc = doc.write().await;
                recv_and_gen(&mut doc, &mut self.sync_state, peer_msg)?
            };

            if had_changes {
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
                    log::debug!(
                        "Sync complete for doc {:?}",
                        &self.doc_id[..4]
                    );
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
async fn read_framed(recv: &mut iroh::endpoint::RecvStream) -> Result<Vec<u8>, SyncError> {
    let mut len_buf = [0u8; 4];
    match recv.read_exact(&mut len_buf).await {
        Ok(()) => {}
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("finished") || msg.contains("STREAM_FIN") || msg.contains("ClosedStream") {
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
}
