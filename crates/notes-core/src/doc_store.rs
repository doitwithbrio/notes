use std::sync::Arc;

use automerge::sync::SyncDoc;
use automerge::transaction::{CommitOptions, Transactable};
use automerge::{AutoCommit, ObjType, ReadDoc};
use dashmap::DashMap;
use tokio::sync::RwLock;

use crate::error::CoreError;
use crate::types::DocId;

/// Get the current Unix timestamp in seconds.
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Thread-safe store for Automerge documents.
///
/// Uses DashMap for lock-free concurrent access to different documents,
/// and tokio RwLock per document for concurrent reads / exclusive writes.
pub struct DocStore {
    docs: DashMap<DocId, Arc<RwLock<AutoCommit>>>,
    /// Set of document IDs that have unsaved changes.
    dirty: DashMap<DocId, ()>,
    /// Stable device actor ID. When set, all loaded/created documents use this actor.
    device_actor_id: Option<automerge::ActorId>,
}

impl DocStore {
    pub fn new() -> Self {
        Self {
            docs: DashMap::new(),
            dirty: DashMap::new(),
            device_actor_id: None,
        }
    }

    /// Create a new DocStore with a stable device actor ID.
    /// All documents created or loaded will use this actor ID.
    pub fn with_actor_id(actor_id: automerge::ActorId) -> Self {
        Self {
            docs: DashMap::new(),
            dirty: DashMap::new(),
            device_actor_id: Some(actor_id),
        }
    }

    /// Set the stable device actor ID.
    pub fn set_device_actor_id(&mut self, actor_id: automerge::ActorId) {
        self.device_actor_id = Some(actor_id);
    }

    /// Get the device actor ID as a hex string.
    pub fn device_actor_hex(&self) -> Option<String> {
        self.device_actor_id.as_ref().map(|id| id.to_hex_string())
    }

    /// Create a new empty Automerge document with a specific ID.
    /// Returns an error if a document with this ID already exists.
    pub fn create_doc_with_id(&self, id: DocId) -> Result<(), CoreError> {
        let mut doc = AutoCommit::new();
        if let Some(ref actor_id) = self.device_actor_id {
            doc.set_actor(actor_id.clone());
        }
        doc.put(automerge::ROOT, "schemaVersion", 1_u64)?;
        doc.put_object(automerge::ROOT, "text", ObjType::Text)?;

        // Use entry API to avoid TOCTOU race
        use dashmap::mapref::entry::Entry;
        match self.docs.entry(id) {
            Entry::Occupied(_) => Err(CoreError::DocAlreadyExists(id)),
            Entry::Vacant(e) => {
                e.insert(Arc::new(RwLock::new(doc)));
                Ok(())
            }
        }
    }

    /// Load an existing Automerge document from binary data.
    /// If a document with this ID already exists, the entry is kept (no overwrite).
    pub fn load_doc(&self, id: DocId, data: &[u8]) -> Result<(), CoreError> {
        let mut doc = AutoCommit::load(data)?;
        if let Some(ref actor_id) = self.device_actor_id {
            doc.set_actor(actor_id.clone());
        }
        // Atomic insert-if-absent to avoid TOCTOU race
        self.docs
            .entry(id)
            .or_insert_with(|| Arc::new(RwLock::new(doc)));
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

    /// Remove a document from the store.
    pub fn remove_doc(&self, id: &DocId) {
        self.docs.remove(id);
        self.dirty.remove(id);
    }

    /// Serialize the full document to binary.
    ///
    /// Takes a read lock and clones the document before serializing
    /// to minimize lock hold time. The clone is cheap relative to
    /// the serialization.
    pub async fn save_doc(&self, id: &DocId) -> Result<Vec<u8>, CoreError> {
        let doc_arc = self.get_doc(id)?;
        let mut snapshot = {
            let doc = doc_arc.read().await;
            doc.clone()
        };
        // Serialize outside the lock
        Ok(snapshot.save())
    }

    /// Apply incremental changes from the frontend WASM Automerge instance.
    /// Marks the document as dirty for the background save loop.
    pub async fn apply_incremental(
        &self,
        id: &DocId,
        data: &[u8],
    ) -> Result<(), CoreError> {
        if data.is_empty() {
            return Ok(());
        }
        const MAX_INCREMENTAL_SIZE: usize = 16 * 1024 * 1024; // 16 MB
        if data.len() > MAX_INCREMENTAL_SIZE {
            return Err(CoreError::InvalidInput(format!(
                "incremental data too large: {} bytes (max {MAX_INCREMENTAL_SIZE})",
                data.len()
            )));
        }
        let doc_arc = self.get_doc(id)?;
        let mut doc = doc_arc.write().await;
        doc.load_incremental(data)?;
        self.dirty.insert(*id, ());
        Ok(())
    }

    /// Generate a sync message for a peer.
    ///
    /// Requires write lock because AutoCommit::sync() needs &mut self in automerge 0.5.
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
        doc.sync().receive_sync_message(sync_state, message)?;
        self.dirty.insert(*id, ());
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

    /// Replace the text content of a document (used for manual .md import).
    /// WARNING: This is a destructive operation — it tombstones all existing text.
    pub async fn replace_text(&self, id: &DocId, content: &str) -> Result<(), CoreError> {
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
                doc.commit_with(CommitOptions::default().with_time(now_secs()));
                self.dirty.insert(*id, ());
                Ok(())
            }
            _ => Err(CoreError::InvalidData("text field is not Text type".into())),
        }
    }

    /// Compact a document by saving and reloading.
    /// Sheds intermediate ops to reduce memory usage.
    /// Note: resets incremental save state — next save will be a full payload.
    pub async fn compact(&self, id: &DocId) -> Result<(), CoreError> {
        let doc_arc = self.get_doc(id)?;
        let mut doc = doc_arc.write().await;
        let bytes = doc.save();
        let mut reloaded = AutoCommit::load(&bytes)?;
        if let Some(ref actor_id) = self.device_actor_id {
            reloaded.set_actor(actor_id.clone());
        }
        *doc = reloaded;
        self.dirty.insert(*id, ());
        log::info!("Compacted document {id}: {} bytes", bytes.len());
        Ok(())
    }

    /// Check if a document has unsaved changes and clear the dirty flag.
    /// Returns true if the document was dirty.
    pub fn take_dirty(&self, id: &DocId) -> bool {
        self.dirty.remove(id).is_some()
    }

    /// Mark a document as dirty (has unsaved changes).
    pub fn mark_dirty(&self, id: &DocId) {
        self.dirty.insert(*id, ());
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

impl std::fmt::Debug for DocStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DocStore")
            .field("doc_count", &self.docs.len())
            .field("has_device_actor", &self.device_actor_id.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_create_and_get_text() {
        let store = DocStore::new();
        let id = Uuid::new_v4();
        store.create_doc_with_id(id).unwrap();
        let text = store.get_text(&id).await.unwrap();
        assert_eq!(text, "");
    }

    #[tokio::test]
    async fn test_create_duplicate_id_rejected() {
        let store = DocStore::new();
        let id = Uuid::new_v4();
        store.create_doc_with_id(id).unwrap();
        assert!(matches!(
            store.create_doc_with_id(id),
            Err(CoreError::DocAlreadyExists(_))
        ));
    }

    #[tokio::test]
    async fn test_set_and_get_text() {
        let store = DocStore::new();
        let id = Uuid::new_v4();
        store.create_doc_with_id(id).unwrap();
        store.replace_text(&id, "Hello, world!").await.unwrap();
        let text = store.get_text(&id).await.unwrap();
        assert_eq!(text, "Hello, world!");
    }

    #[tokio::test]
    async fn test_save_and_load() {
        let store = DocStore::new();
        let id = Uuid::new_v4();
        store.create_doc_with_id(id).unwrap();
        store.replace_text(&id, "Persistent content").await.unwrap();
        let data = store.save_doc(&id).await.unwrap();

        let store2 = DocStore::new();
        store2.load_doc(id, &data).unwrap();
        let text = store2.get_text(&id).await.unwrap();
        assert_eq!(text, "Persistent content");
    }

    #[tokio::test]
    async fn test_compact() {
        let store = DocStore::new();
        let id = Uuid::new_v4();
        store.create_doc_with_id(id).unwrap();
        store.replace_text(&id, "Before compact").await.unwrap();
        store.compact(&id).await.unwrap();
        let text = store.get_text(&id).await.unwrap();
        assert_eq!(text, "Before compact");
    }

    #[tokio::test]
    async fn test_dirty_tracking() {
        let store = DocStore::new();
        let id = Uuid::new_v4();
        store.create_doc_with_id(id).unwrap();

        // New doc is not dirty
        assert!(!store.take_dirty(&id));

        // After modification, it is dirty
        store.replace_text(&id, "modified").await.unwrap();
        assert!(store.take_dirty(&id));

        // After taking dirty, it's clean
        assert!(!store.take_dirty(&id));
    }

    #[tokio::test]
    async fn test_get_nonexistent_doc() {
        let store = DocStore::new();
        let id = Uuid::new_v4();
        assert!(matches!(
            store.get_text(&id).await,
            Err(CoreError::DocNotFound(_))
        ));
    }

    #[tokio::test]
    async fn test_load_corrupted_data() {
        let store = DocStore::new();
        let id = Uuid::new_v4();
        let result = store.load_doc(id, &[0xFF; 100]);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_apply_incremental_oversized() {
        let store = DocStore::new();
        let id = Uuid::new_v4();
        store.create_doc_with_id(id).unwrap();
        let big = vec![0u8; 17 * 1024 * 1024]; // > 16 MB
        assert!(matches!(
            store.apply_incremental(&id, &big).await,
            Err(CoreError::InvalidInput(_))
        ));
    }

    #[tokio::test]
    async fn test_remove_doc() {
        let store = DocStore::new();
        let id = Uuid::new_v4();
        store.create_doc_with_id(id).unwrap();
        assert!(store.contains(&id));
        store.remove_doc(&id);
        assert!(!store.contains(&id));
    }

    #[tokio::test]
    async fn test_unicode_text() {
        let store = DocStore::new();
        let id = Uuid::new_v4();
        store.create_doc_with_id(id).unwrap();
        store.replace_text(&id, "Hello 🌍🔥 日本語テスト").await.unwrap();
        let text = store.get_text(&id).await.unwrap();
        assert_eq!(text, "Hello 🌍🔥 日本語テスト");
    }
}
