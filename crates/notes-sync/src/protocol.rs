use serde::{Deserialize, Serialize};

/// Current wire protocol version.
pub const PROTOCOL_VERSION: u8 = 0x01;

/// Message types for the sync wire protocol.
///
/// Wire format (stream open):
///   [1 byte: protocol version]
///   [1 byte: message type]
///   [32 bytes: document ID]
///
/// Then repeated:
///   [4 bytes: big-endian length]
///   [N bytes: Automerge sync message]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum MessageType {
    /// Automerge sync protocol message (bidirectional, for Editors).
    SyncMessage = 0x01,
    /// Read-only snapshot delivery (unidirectional, for Viewers).
    ViewerSnapshot = 0x02,
    /// Presence/cursor update (ephemeral, via gossip).
    PresenceUpdate = 0x03,
}

impl TryFrom<u8> for MessageType {
    type Error = ProtocolError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x01 => Ok(Self::SyncMessage),
            0x02 => Ok(Self::ViewerSnapshot),
            0x03 => Ok(Self::PresenceUpdate),
            _ => Err(ProtocolError::UnknownMessageType(value)),
        }
    }
}

/// Errors in the sync protocol.
#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("Unknown protocol version: {0}")]
    UnknownVersion(u8),

    #[error("Unknown message type: {0}")]
    UnknownMessageType(u8),

    #[error("Message too large: {0} bytes (max {1})")]
    MessageTooLarge(u32, u32),

    #[error("Connection closed unexpectedly")]
    ConnectionClosed,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Maximum sync message size (16 MB — generous for Automerge messages).
pub const MAX_MESSAGE_SIZE: u32 = 16 * 1024 * 1024;

/// Encode a framed message: [4-byte big-endian length][payload].
pub fn encode_framed(payload: &[u8]) -> Vec<u8> {
    let len = payload.len() as u32;
    let mut buf = Vec::with_capacity(4 + payload.len());
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(payload);
    buf
}

/// Encode a stream header.
pub fn encode_stream_header(msg_type: MessageType, doc_id: &[u8; 32]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(34);
    buf.push(PROTOCOL_VERSION);
    buf.push(msg_type as u8);
    buf.extend_from_slice(doc_id);
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_type_roundtrip() {
        assert_eq!(
            MessageType::try_from(0x01).unwrap(),
            MessageType::SyncMessage
        );
        assert_eq!(
            MessageType::try_from(0x02).unwrap(),
            MessageType::ViewerSnapshot
        );
        assert!(MessageType::try_from(0xFF).is_err());
    }

    #[test]
    fn test_encode_framed() {
        let payload = b"hello";
        let framed = encode_framed(payload);
        assert_eq!(framed.len(), 4 + 5);
        assert_eq!(&framed[0..4], &5_u32.to_be_bytes());
        assert_eq!(&framed[4..], b"hello");
    }

    #[test]
    fn test_encode_stream_header() {
        let doc_id = [0xAA; 32];
        let header = encode_stream_header(MessageType::SyncMessage, &doc_id);
        assert_eq!(header.len(), 34);
        assert_eq!(header[0], PROTOCOL_VERSION);
        assert_eq!(header[1], 0x01);
        assert_eq!(&header[2..], &doc_id);
    }
}
