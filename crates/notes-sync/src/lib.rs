//! # notes-sync
//!
//! P2P synchronization layer for the notes app.
//!
//! - `sync_engine` — iroh ProtocolHandler for Automerge doc sync with ACL enforcement
//! - `sync_session` — per-document sync over QUIC bidirectional streams
//! - `sync_state_store` — persistent sync state per-peer per-document
//! - `invite` — HKDF + challenge-response invite flow (owner + invitee handlers)
//! - `peer_manager` — persistent peer connections, auto-reconnect, monitoring
//! - `presence` — cursor/presence via iroh-gossip
//! - `blobs` — image/asset sync via iroh-blobs
//! - `events` — Tauri event types for the frontend
//! - `protocol` — wire protocol types and framing

pub mod blobs;
pub mod events;
pub mod invite;
pub mod peer_manager;
pub mod presence;
pub mod protocol;
pub mod sync_engine;
pub mod sync_session;
pub mod sync_state_store;

pub use iroh_gossip::net::GOSSIP_ALPN;
pub use presence::PresenceManager;
pub use protocol::*;
pub use sync_engine::SyncEngine;
pub use sync_state_store::SyncStateStore;
