//! # notes-sync
//!
//! Sync protocol crate: Automerge sync over iroh QUIC connections.
//!
//! This crate will be implemented in Phase 2 (P2P Foundation).
//! For now, it defines the wire protocol types and message framing.

pub mod protocol;

pub use protocol::*;
