use std::sync::Arc;

use automerge::sync::SyncDoc;
use automerge::{transaction::Transactable, AutoCommit, ObjType, ReadDoc};
use dashmap::DashMap;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::error::CoreError;
use crate::types::DocId;

/// Thread-safe store for Automerge documents.
///
/// Uses DashMap for lock-free concurrent access to different documents,
/// and tokio RwLock per document for concurrent reads / exclusive writes.
pub struct DocStore {
    docs: DashMap<DocId, Arc<RwLock<AutoCommit>>>,
}

impl DocStore {
    pub fn new() -> Self {
        Self {
            docs: DashMap::new(),
        }
    }

    /// Create a new empty Automerge document and insert it into the store.
    /// Returns the new document's ID.
    pub fn create_doc(&self) -> DocId {
        let id = Uuid::new_v4();
        let mut doc = AutoCommit::new();

        doc.put(automerge::ROOT, "schemaVersion", 1_u64)
            .expect("failed to set schemaVersion");
        doc.put_object(automerge::ROOT, "text", ObjType::Text)
            .expect("failed to create text object");

        self.docs.insert(id, Arc::new(RwLock::new(doc)));
        id
    }

    /// Load an existing Automerge document from binary data.
    pub fn load_doc(&self, id: DocId, data: &[u8]) -> Result<(), CoreError> {
        let doc = AutoCommit::load(data)?;
        self.docs.insert(id, Arc::new(RwLock::new(doc)));
        Ok(())
    }

    /// Get a clone of the Arc for a document. Caller holds Arc, DashMap ref is dropped.
    pub fn get_doc(&self, id: &DocId) -> Result<Arc<RwLock<AutoCommit>>, CoreError> {
        self.docs
            .get(id)
            .map(|entry| Arc::clone(entry.value()))
            .ok_or(CoreError::DocNotFound(*id))
    }

    /// Check if a document exists in the store.
    pub fn contains(&self, id: &DocId) -> bool {
        self.docs.contains_key(id)
    }

    /// Remove a document from the store, returning it if present.
    pub fn remove_doc(&self, id: &DocId) -> Option<Arc<RwLock<AutoCommit>>> {
        self.docs.remove(id).map(|(_, v)| v)
    }

    /// Get a full binary save of the document (for persistence or sync).
    pub async fn save_doc(&self, id: &DocId) -> Result<Vec<u8>, CoreError> {
        let doc_arc = self.get_doc(id)?;
        let mut doc = doc_arc.write().await;
        Ok(doc.save())
    }

    /// Get an incremental save since the last save (for efficient IPC to frontend).
    pub async fn save_incremental(&self, id: &DocId) -> Result<Vec<u8>, CoreError> {
        let doc_arc = self.get_doc(id)?;
        let mut doc = doc_arc.write().await;
        Ok(doc.save_incremental())
    }

    /// Apply incremental changes from the frontend WASM Automerge instance.
    pub async fn apply_incremental(
        &self,
        id: &DocId,
        data: &[u8],
    ) -> Result<(), CoreError> {
        let doc_arc = self.get_doc(id)?;
        let mut doc = doc_arc.write().await;
        doc.load_incremental(data)?;
        Ok(())
    }

    /// Generate a sync message for a peer.
    pub async fn generate_sync_message(
        &self,
        id: &DocId,
        sync_state: &mut automerge::sync::State,
    ) -> Result<Option<automerge::sync::Message>, CoreError> {
        let doc_arc = self.get_doc(id)?;
        let mut doc = doc_arc.write().await;
        let msg = doc.sync().generate_sync_message(sync_state);
        Ok(msg)
    }

    /// Receive a sync message from a peer.
    pub async fn receive_sync_message(
        &self,
        id: &DocId,
        sync_state: &mut automerge::sync::State,
        message: automerge::sync::Message,
    ) -> Result<(), CoreError> {
        let doc_arc = self.get_doc(id)?;
        let mut doc = doc_arc.write().await;
        doc.sync()
            .receive_sync_message(sync_state, message)?;
        Ok(())
    }

    /// Get the text content of a document as a String.
    pub async fn get_text(&self, id: &DocId) -> Result<String, CoreError> {
        let doc_arc = self.get_doc(id)?;
        let doc = doc_arc.read().await;

        let text_obj = doc
            .get(automerge::ROOT, "text")?
            .ok_or_else(|| CoreError::InvalidData("document has no text field".into()))?;

        match text_obj {
            (automerge::Value::Object(ObjType::Text), text_id) => {
                Ok(doc.text(&text_id)?)
            }
            _ => Err(CoreError::InvalidData("text field is not Text type".into())),
        }
    }

    /// Update the text content of a document (used for manual .md import).
    pub async fn set_text(&self, id: &DocId, content: &str) -> Result<(), CoreError> {
        let doc_arc = self.get_doc(id)?;
        let mut doc = doc_arc.write().await;

        let text_obj = doc
            .get(automerge::ROOT, "text")?
            .ok_or_else(|| CoreError::InvalidData("document has no text field".into()))?;

        match text_obj {
            (automerge::Value::Object(ObjType::Text), text_id) => {
                let len = doc.length(&text_id);
                if len > 0 {
                    doc.splice_text(&text_id, 0, len as isize, "")?;
                }
                doc.splice_text(&text_id, 0, 0, content)?;
                Ok(())
            }
            _ => Err(CoreError::InvalidData("text field is not Text type".into())),
        }
    }

    /// Compact a document by saving and reloading.
    pub async fn compact(&self, id: &DocId) -> Result<(), CoreError> {
        let doc_arc = self.get_doc(id)?;
        let mut doc = doc_arc.write().await;
        let bytes = doc.save();
        *doc = AutoCommit::load(&bytes)?;
        log::info!("Compacted document {id}: {} bytes", bytes.len());
        Ok(())
    }

    /// List all document IDs currently loaded.
    pub fn loaded_doc_ids(&self) -> Vec<DocId> {
        self.docs.iter().map(|entry| *entry.key()).collect()
    }

    pub fn len(&self) -> usize {
        self.docs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }
}

impl Default for DocStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_and_get_text() {
        let store = DocStore::new();
        let id = store.create_doc();
        let text = store.get_text(&id).await.unwrap();
        assert_eq!(text, "");
    }

    #[tokio::test]
    async fn test_set_and_get_text() {
        let store = DocStore::new();
        let id = store.create_doc();
        store.set_text(&id, "Hello, world!").await.unwrap();
        let text = store.get_text(&id).await.unwrap();
        assert_eq!(text, "Hello, world!");
    }

    #[tokio::test]
    async fn test_save_and_load() {
        let store = DocStore::new();
        let id = store.create_doc();
        store.set_text(&id, "Persistent content").await.unwrap();
        let data = store.save_doc(&id).await.unwrap();

        let store2 = DocStore::new();
        store2.load_doc(id, &data).unwrap();
        let text = store2.get_text(&id).await.unwrap();
        assert_eq!(text, "Persistent content");
    }

    #[tokio::test]
    async fn test_compact() {
        let store = DocStore::new();
        let id = store.create_doc();
        store.set_text(&id, "Before compact").await.unwrap();
        store.compact(&id).await.unwrap();
        let text = store.get_text(&id).await.unwrap();
        assert_eq!(text, "Before compact");
    }

    #[tokio::test]
    async fn test_incremental_sync() {
        let store = DocStore::new();
        let id = store.create_doc();
        store.set_text(&id, "Hello").await.unwrap();

        let full = store.save_doc(&id).await.unwrap();
        let store2 = DocStore::new();
        store2.load_doc(id, &full).unwrap();

        store.set_text(&id, "Hello, updated").await.unwrap();
        let inc2 = store.save_incremental(&id).await.unwrap();

        store2.apply_incremental(&id, &inc2).await.unwrap();
        let text = store2.get_text(&id).await.unwrap();
        assert_eq!(text, "Hello, updated");
    }

    #[tokio::test]
    async fn test_remove_doc() {
        let store = DocStore::new();
        let id = store.create_doc();
        assert!(store.contains(&id));
        store.remove_doc(&id);
        assert!(!store.contains(&id));
    }
}
