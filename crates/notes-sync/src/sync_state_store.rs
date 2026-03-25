//! Persistent sync state per-peer per-document.
//!
//! Stores Automerge SyncState to avoid full re-sync on reconnection.
//! Persisted to `.p2p/sync_states/<peer_id_short>/<doc_id>.syncstate`.

use std::path::PathBuf;

use automerge::sync::State as SyncState;
use iroh::EndpointId;
use uuid::Uuid;

/// Manages persistent sync states.
pub struct SyncStateStore {
    base_dir: PathBuf,
}

impl SyncStateStore {
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    /// Get the file path for a sync state.
    fn state_path(&self, peer_id: &EndpointId, doc_id: &Uuid) -> PathBuf {
        let peer_short = &peer_id.to_string()[..10];
        self.base_dir
            .join("sync_states")
            .join(peer_short)
            .join(format!("{}.syncstate", doc_id))
    }

    /// Load a persisted sync state, or return a fresh one.
    pub async fn load_or_create(
        &self,
        peer_id: &EndpointId,
        doc_id: &Uuid,
    ) -> SyncState {
        let path = self.state_path(peer_id, doc_id);
        match tokio::fs::read(&path).await {
            Ok(data) => {
                match SyncState::decode(&data) {
                    Ok(state) => {
                        log::debug!("Loaded sync state for peer {peer_id} doc {doc_id}");
                        state
                    }
                    Err(e) => {
                        log::warn!("Corrupt sync state for {peer_id}/{doc_id}: {e}");
                        SyncState::new()
                    }
                }
            }
            Err(_) => SyncState::new(),
        }
    }

    /// Persist a sync state after successful sync.
    pub async fn save(
        &self,
        peer_id: &EndpointId,
        doc_id: &Uuid,
        state: &SyncState,
    ) -> Result<(), std::io::Error> {
        let path = self.state_path(peer_id, doc_id);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let data = state.encode();
        tokio::fs::write(&path, &data).await?;
        log::debug!("Saved sync state for peer {peer_id} doc {doc_id}");
        Ok(())
    }

    /// Delete a sync state (e.g., after compaction invalidates it).
    pub async fn delete(
        &self,
        peer_id: &EndpointId,
        doc_id: &Uuid,
    ) -> Result<(), std::io::Error> {
        let path = self.state_path(peer_id, doc_id);
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Delete all sync states for a document (e.g., after compaction).
    pub async fn delete_all_for_doc(&self, doc_id: &Uuid) {
        let dir = self.base_dir.join("sync_states");
        if let Ok(mut entries) = tokio::fs::read_dir(&dir).await {
            while let Ok(Some(peer_dir)) = entries.next_entry().await {
                let state_file = peer_dir.path().join(format!("{}.syncstate", doc_id));
                let _ = tokio::fs::remove_file(&state_file).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_load_nonexistent_returns_fresh() {
        let dir = tempfile::tempdir().unwrap();
        let store = SyncStateStore::new(dir.path().join(".p2p"));

        let peer_id: EndpointId =
            "b27ef3e7a4c94bac1daa3f233e0dd19c6f69d88ad9d833e593da93c57f75e6dd"
                .parse()
                .unwrap();
        let doc_id = Uuid::new_v4();

        let state = store.load_or_create(&peer_id, &doc_id).await;
        // Fresh state — just verify it doesn't panic
        let _ = state.encode();
    }

    #[tokio::test]
    async fn test_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let store = SyncStateStore::new(dir.path().join(".p2p"));

        let peer_id: EndpointId =
            "b27ef3e7a4c94bac1daa3f233e0dd19c6f69d88ad9d833e593da93c57f75e6dd"
                .parse()
                .unwrap();
        let doc_id = Uuid::new_v4();

        let state = SyncState::new();
        store.save(&peer_id, &doc_id, &state).await.unwrap();

        let loaded = store.load_or_create(&peer_id, &doc_id).await;
        // Both should encode to the same bytes
        assert_eq!(state.encode(), loaded.encode());
    }

    #[tokio::test]
    async fn test_delete() {
        let dir = tempfile::tempdir().unwrap();
        let store = SyncStateStore::new(dir.path().join(".p2p"));

        let peer_id: EndpointId =
            "b27ef3e7a4c94bac1daa3f233e0dd19c6f69d88ad9d833e593da93c57f75e6dd"
                .parse()
                .unwrap();
        let doc_id = Uuid::new_v4();

        let state = SyncState::new();
        store.save(&peer_id, &doc_id, &state).await.unwrap();
        store.delete(&peer_id, &doc_id).await.unwrap();

        // Should return fresh state after deletion
        let loaded = store.load_or_create(&peer_id, &doc_id).await;
        let _ = loaded.encode(); // Just verify no panic
    }
}
