use std::sync::Arc;

use automerge::sync::SyncDoc;
use automerge::transaction::{CommitOptions, Transactable};
use automerge::{AutoCommit, ObjType, ReadDoc, ValueRef};
use dashmap::DashMap;
use serde_json::{Map, Number, Value};
use tokio::sync::RwLock;

use crate::editor_migration::migrate_legacy_text_to_v2;
use crate::editor_schema::{
    validate_document, EditorDocument, EditorMark, EditorNode, ValidationMode,
};
use crate::editor_text::visible_text;
use crate::error::CoreError;
use crate::types::{DocId, DocReadSnapshot, DocumentSourceSchema, UnsupportedNodeSummary};

/// Get the current Unix timestamp in seconds.
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn initialize_structured_doc(doc: &mut AutoCommit) -> Result<(), automerge::AutomergeError> {
    doc.put(automerge::ROOT, "schemaVersion", 2_u64)?;
    doc.put_object(automerge::ROOT, "text", ObjType::Text)?;

    let root = doc.put_object(automerge::ROOT, "doc", ObjType::Map)?;
    doc.put(&root, "id", "root")?;
    doc.put(&root, "type", "doc")?;
    doc.put(&root, "nodeVersion", 1_u64)?;
    let content = doc.put_object(&root, "content", ObjType::List)?;
    let paragraph = doc.insert_object(&content, 0, ObjType::Map)?;
    doc.put(&paragraph, "id", "paragraph-1")?;
    doc.put(&paragraph, "type", "paragraph")?;
    doc.put(&paragraph, "nodeVersion", 1_u64)?;
    doc.put_object(&paragraph, "content", ObjType::List)?;
    Ok(())
}

fn validate_stored_doc_shape(doc: &AutoCommit) -> Result<(), CoreError> {
    let schema_version = doc
        .get(automerge::ROOT, "schemaVersion")?
        .and_then(|(value, _)| value.to_u64())
        .unwrap_or(1);

    if schema_version < 2 {
        return Ok(());
    }

    let editor_document = load_editor_document_from_doc(doc)?;
    let mirror_text = extract_text_mirror(doc)?;
    let derived_text = visible_text(&editor_document);
    if mirror_text != derived_text {
        return Err(CoreError::InvalidData(
            "schema v2 text mirror is out of sync with structured doc".into(),
        ));
    }

    Ok(())
}

fn extract_text_mirror(doc: &AutoCommit) -> Result<String, CoreError> {
    let text_obj = doc
        .get(automerge::ROOT, "text")?
        .ok_or_else(|| CoreError::InvalidData("document has no text field".into()))?;

    match text_obj {
        (automerge::Value::Object(ObjType::Text), text_id) => Ok(doc.text(&text_id)?),
        (automerge::Value::Scalar(scalar), _)
            if matches!(scalar.as_ref(), automerge::ScalarValue::Str(_)) =>
        {
            scalar
                .to_str()
                .map(|value| value.to_string())
                .ok_or_else(|| CoreError::InvalidData("text field is not string-like".into()))
        }
        _ => Err(CoreError::InvalidData(
            "schema v2 documents must contain a text mirror".into(),
        )),
    }
}

fn write_scalar_value(
    doc: &mut AutoCommit,
    obj: &automerge::ObjId,
    key: &str,
    value: &Value,
) -> Result<(), CoreError> {
    match value {
        Value::String(v) => {
            doc.put(obj, key, v.as_str())?;
        }
        Value::Bool(v) => {
            doc.put(obj, key, *v)?;
        }
        Value::Number(v) => {
            if let Some(n) = v.as_u64() {
                doc.put(obj, key, n)?;
            } else if let Some(n) = v.as_i64() {
                doc.put(obj, key, n)?;
            } else if let Some(n) = v.as_f64() {
                doc.put(obj, key, n)?;
            }
        }
        Value::Null => {
            doc.put(obj, key, ())?;
        }
        Value::Array(bytes) => {
            let data = bytes
                .iter()
                .map(|item| item.as_u64().map(|v| v as u8))
                .collect::<Option<Vec<u8>>>()
                .ok_or_else(|| CoreError::InvalidData("unsupported array value in attrs".into()))?;
            doc.put(obj, key, data)?;
        }
        Value::Object(_) => {
            return Err(CoreError::InvalidData(
                "nested object attrs are not supported yet".into(),
            ))
        }
    }
    Ok(())
}

fn write_mark(
    doc: &mut AutoCommit,
    marks_obj: &automerge::ObjId,
    mark: &EditorMark,
) -> Result<(), CoreError> {
    let mark_obj = doc.insert_object(marks_obj, doc.length(marks_obj), ObjType::Map)?;
    doc.put(&mark_obj, "type", mark.mark_type.as_str())?;
    if !mark.attrs.is_empty() {
        let attrs_obj = doc.put_object(&mark_obj, "attrs", ObjType::Map)?;
        for (key, value) in &mark.attrs {
            write_scalar_value(doc, &attrs_obj, key, value)?;
        }
    }
    Ok(())
}

fn write_editor_node(
    doc: &mut AutoCommit,
    parent: &automerge::ObjId,
    node: &EditorNode,
) -> Result<(), CoreError> {
    let node_obj = doc.insert_object(parent, doc.length(parent), ObjType::Map)?;
    doc.put(&node_obj, "id", node.id.as_str())?;
    doc.put(&node_obj, "type", node.node_type.as_str())?;
    doc.put(&node_obj, "nodeVersion", node.node_version as u64)?;
    if !node.attrs.is_empty() {
        let attrs_obj = doc.put_object(&node_obj, "attrs", ObjType::Map)?;
        for (key, value) in &node.attrs {
            write_scalar_value(doc, &attrs_obj, key, value)?;
        }
    }
    if let Some(text) = &node.text {
        doc.put(&node_obj, "text", text.as_str())?;
    }
    if !node.marks.is_empty() {
        let marks_obj = doc.put_object(&node_obj, "marks", ObjType::List)?;
        for mark in &node.marks {
            write_mark(doc, &marks_obj, mark)?;
        }
    }
    if !node.content.is_empty() {
        let content_obj = doc.put_object(&node_obj, "content", ObjType::List)?;
        for child in &node.content {
            write_editor_node(doc, &content_obj, child)?;
        }
    }
    Ok(())
}

fn replace_text_mirror(doc: &mut AutoCommit, content: &str) -> Result<(), CoreError> {
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
        (automerge::Value::Scalar(_), _) => {
            doc.put(automerge::ROOT, "text", content)?;
            Ok(())
        }
        _ => Err(CoreError::InvalidData("text field is not Text type".into())),
    }
}

fn ensure_text_mirror(doc: &mut AutoCommit) -> Result<(), CoreError> {
    if doc.get(automerge::ROOT, "text")?.is_none() {
        doc.put_object(automerge::ROOT, "text", ObjType::Text)?;
    }

    Ok(())
}

fn replace_structured_doc(
    doc: &mut AutoCommit,
    editor_document: &EditorDocument,
) -> Result<(), CoreError> {
    if let Some((_, existing_doc)) = doc.get(automerge::ROOT, "doc")? {
        doc.delete(automerge::ROOT, "doc")?;
        let _ = existing_doc;
    }
    let root = doc.put_object(automerge::ROOT, "doc", ObjType::Map)?;
    doc.put(&root, "id", editor_document.doc.id.as_str())?;
    doc.put(&root, "type", editor_document.doc.node_type.as_str())?;
    doc.put(
        &root,
        "nodeVersion",
        editor_document.doc.node_version as u64,
    )?;
    if !editor_document.doc.attrs.is_empty() {
        let attrs_obj = doc.put_object(&root, "attrs", ObjType::Map)?;
        for (key, value) in &editor_document.doc.attrs {
            write_scalar_value(doc, &attrs_obj, key, value)?;
        }
    }
    if !editor_document.doc.content.is_empty() {
        let content_obj = doc.put_object(&root, "content", ObjType::List)?;
        for child in &editor_document.doc.content {
            write_editor_node(doc, &content_obj, child)?;
        }
    }
    if let Some(text) = &editor_document.doc.text {
        doc.put(&root, "text", text.as_str())?;
    }
    Ok(())
}

pub(crate) fn apply_editor_document_to_doc(
    doc: &mut AutoCommit,
    editor_document: &EditorDocument,
) -> Result<(), CoreError> {
    doc.put(
        automerge::ROOT,
        "schemaVersion",
        editor_document.schema_version as u64,
    )?;
    ensure_text_mirror(doc)?;
    replace_structured_doc(doc, editor_document)?;
    replace_text_mirror(doc, &visible_text(editor_document))?;
    validate_stored_doc_shape(doc)?;
    Ok(())
}

fn scalar_to_json(value: &automerge::ScalarValueRef<'_>) -> Result<Value, CoreError> {
    match value {
        automerge::ScalarValueRef::Str(v) => Ok(Value::String(v.to_string())),
        automerge::ScalarValueRef::Uint(v) => Ok(Value::Number(Number::from(*v))),
        automerge::ScalarValueRef::Int(v) => Ok(Value::Number(Number::from(*v))),
        automerge::ScalarValueRef::F64(v) => Number::from_f64(*v)
            .map(Value::Number)
            .ok_or_else(|| CoreError::InvalidData("unsupported NaN/inf float value".into())),
        automerge::ScalarValueRef::Boolean(v) => Ok(Value::Bool(*v)),
        automerge::ScalarValueRef::Null => Ok(Value::Null),
        automerge::ScalarValueRef::Timestamp(v) => Ok(Value::Number(Number::from(*v))),
        automerge::ScalarValueRef::Counter(v) => Ok(Value::Number(Number::from(*v))),
        automerge::ScalarValueRef::Bytes(v) => Ok(Value::Array(
            v.iter()
                .map(|byte| Value::Number(Number::from(*byte)))
                .collect(),
        )),
        _ => Err(CoreError::InvalidData(
            "unsupported scalar value in structured note".into(),
        )),
    }
}

fn object_to_json(
    doc: &AutoCommit,
    obj: &automerge::ObjId,
    kind: ObjType,
) -> Result<Value, CoreError> {
    match kind {
        ObjType::Map => {
            let mut map = Map::new();
            for item in doc.map_range(obj, ..) {
                let value = match item.value {
                    ValueRef::Scalar(scalar) => scalar_to_json(&scalar)?,
                    ValueRef::Object(kind) => object_to_json(doc, &item.id(), kind)?,
                };
                map.insert(item.key.to_string(), value);
            }
            Ok(Value::Object(map))
        }
        ObjType::List => {
            let mut list = Vec::new();
            for item in doc.list_range(obj, ..) {
                let value = match item.value {
                    ValueRef::Scalar(scalar) => scalar_to_json(&scalar)?,
                    ValueRef::Object(kind) => object_to_json(doc, &item.id(), kind)?,
                };
                list.push(value);
            }
            Ok(Value::Array(list))
        }
        ObjType::Text => Ok(Value::String(doc.text(obj)?)),
        ObjType::Table => Err(CoreError::InvalidData(
            "table objects are not supported in structured notes yet".into(),
        )),
    }
}

fn parse_marks(value: Value) -> Result<Vec<EditorMark>, CoreError> {
    match value {
        Value::Null => Ok(Vec::new()),
        Value::Array(items) => items
            .into_iter()
            .map(|item| serde_json::from_value(item).map_err(CoreError::from))
            .collect(),
        _ => Err(CoreError::InvalidData(
            "editor node marks must be an array".into(),
        )),
    }
}

fn parse_content(value: Value) -> Result<Vec<EditorNode>, CoreError> {
    match value {
        Value::Null => Ok(Vec::new()),
        Value::Array(items) => items
            .into_iter()
            .map(|item| serde_json::from_value(item).map_err(CoreError::from))
            .collect(),
        _ => Err(CoreError::InvalidData(
            "editor node content must be an array".into(),
        )),
    }
}

fn parse_editor_node_from_value(value: Value) -> Result<EditorNode, CoreError> {
    let Value::Object(mut map) = value else {
        return Err(CoreError::InvalidData(
            "structured editor node must be an object".into(),
        ));
    };

    let id = map
        .remove("id")
        .and_then(|value| value.as_str().map(str::to_string))
        .ok_or_else(|| CoreError::InvalidData("structured editor node is missing id".into()))?;
    let node_type = map
        .remove("type")
        .and_then(|value| value.as_str().map(str::to_string))
        .ok_or_else(|| CoreError::InvalidData("structured editor node is missing type".into()))?;
    let node_version = map
        .remove("nodeVersion")
        .and_then(|value| value.as_u64())
        .unwrap_or(1) as u32;
    let attrs = match map.remove("attrs") {
        Some(Value::Object(attrs)) => attrs,
        Some(_) => {
            return Err(CoreError::InvalidData(
                "structured editor node attrs must be an object".into(),
            ))
        }
        None => Map::new(),
    };
    let content = parse_content(map.remove("content").unwrap_or(Value::Null))?;
    let text = match map.remove("text") {
        Some(Value::String(text)) => Some(text),
        Some(Value::Null) | None => None,
        Some(_) => {
            return Err(CoreError::InvalidData(
                "structured editor node text must be a string".into(),
            ))
        }
    };
    let marks = parse_marks(map.remove("marks").unwrap_or(Value::Null))?;

    Ok(EditorNode {
        id,
        node_type,
        node_version,
        attrs,
        content,
        text,
        marks,
    })
}

fn load_editor_document_from_doc(doc: &AutoCommit) -> Result<EditorDocument, CoreError> {
    let schema_version = doc
        .get(automerge::ROOT, "schemaVersion")?
        .and_then(|(value, _)| value.to_u64())
        .unwrap_or(1);

    if schema_version < 2 {
        let text_obj = doc
            .get(automerge::ROOT, "text")?
            .ok_or_else(|| CoreError::InvalidData("document has no text field".into()))?;

        let text = match text_obj {
            (automerge::Value::Object(ObjType::Text), text_id) => doc.text(&text_id)?,
            (automerge::Value::Scalar(scalar), _) => scalar
                .to_str()
                .map(|value| value.to_string())
                .ok_or_else(|| CoreError::InvalidData("text field is not string-like".into()))?,
            _ => return Err(CoreError::InvalidData("text field is not Text type".into())),
        };

        return Ok(migrate_legacy_text_to_v2(&text));
    }

    let root = match doc.get(automerge::ROOT, "doc")? {
        Some((automerge::Value::Object(ObjType::Map), root)) => root,
        _ => {
            return Err(CoreError::InvalidData(
                "schema v2 documents must contain a structured doc root".into(),
            ))
        }
    };

    let root_value = object_to_json(doc, &root, ObjType::Map)?;
    let editor_document = EditorDocument {
        schema_version: schema_version as u32,
        doc: parse_editor_node_from_value(root_value)?,
    };
    validate_document(&editor_document, ValidationMode::Permissive)?;
    Ok(editor_document)
}

fn get_source_schema(doc: &AutoCommit) -> Result<DocumentSourceSchema, CoreError> {
    let schema_version = doc
        .get(automerge::ROOT, "schemaVersion")?
        .and_then(|(value, _)| value.to_u64())
        .unwrap_or(1);

    Ok(if schema_version >= 2 {
        DocumentSourceSchema::GraphV2
    } else {
        DocumentSourceSchema::LegacyText
    })
}

fn collect_unsupported_node_types(node: &EditorNode, types: &mut Vec<String>) {
    match node.node_type.as_str() {
        "doc" | "paragraph" | "text" | "heading" | "blockquote" | "bullet_list"
        | "ordered_list" | "list_item" | "task_list" | "task_item" | "code_block"
        | "horizontal_rule" | "image" | "hard_break" => {}
        other => {
            if !types.iter().any(|existing| existing == other) {
                types.push(other.to_string());
            }
        }
    }

    for child in &node.content {
        collect_unsupported_node_types(child, types);
    }
}

fn build_read_snapshot_from_doc(doc: &AutoCommit) -> Result<DocReadSnapshot, CoreError> {
    let source_schema = get_source_schema(doc)?;
    let editor_document = load_editor_document_from_doc(doc)?;
    let visible_text = visible_text(&editor_document);
    let mut unsupported_types = Vec::new();
    collect_unsupported_node_types(&editor_document.doc, &mut unsupported_types);
    unsupported_types.sort();
    let unsupported_count = unsupported_types.len();
    let supports_plain_text = unsupported_count == 0;
    let supports_rich_text = source_schema == DocumentSourceSchema::GraphV2 && supports_plain_text;

    Ok(DocReadSnapshot {
        schema_version: editor_document.schema_version,
        source_schema,
        needs_migration: source_schema == DocumentSourceSchema::LegacyText,
        visible_text,
        editor_document,
        unsupported_nodes: UnsupportedNodeSummary {
            count: unsupported_count,
            node_types: unsupported_types,
        },
        can_edit_rich_text: supports_rich_text,
        can_edit_plain_text: supports_plain_text,
    })
}

pub fn read_snapshot_from_bytes(data: &[u8]) -> Result<DocReadSnapshot, CoreError> {
    let doc = AutoCommit::load(data)?;
    validate_stored_doc_shape(&doc)?;
    build_read_snapshot_from_doc(&doc)
}

pub fn visible_text_from_snapshot_bytes(data: &[u8]) -> Result<String, CoreError> {
    Ok(read_snapshot_from_bytes(data)?.visible_text)
}

pub(crate) fn restore_doc_from_snapshot_bytes(
    doc: &mut AutoCommit,
    data: &[u8],
) -> Result<(), CoreError> {
    let snapshot = read_snapshot_from_bytes(data)?;
    apply_editor_document_to_doc(doc, &snapshot.editor_document)
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

pub struct AppliedIncremental {
    pub current_heads: Vec<String>,
    pub new_changes: Vec<(String, Vec<u8>)>,
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
        initialize_structured_doc(&mut doc)?;

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
        validate_stored_doc_shape(&doc)?;
        if let Some(ref actor_id) = self.device_actor_id {
            doc.set_actor(actor_id.clone());
        }
        // Atomic insert-if-absent to avoid TOCTOU race
        self.docs
            .entry(id)
            .or_insert_with(|| Arc::new(RwLock::new(doc)));
        Ok(())
    }

    /// Replace a document in memory with the provided binary state.
    pub async fn replace_doc(&self, id: DocId, data: &[u8]) -> Result<(), CoreError> {
        let mut doc = AutoCommit::load(data)?;
        validate_stored_doc_shape(&doc)?;
        if let Some(ref actor_id) = self.device_actor_id {
            doc.set_actor(actor_id.clone());
        }
        if let Some(existing) = self.docs.get(&id) {
            let current = Arc::clone(existing.value());
            drop(existing);
            let mut guard = current.write().await;
            *guard = doc;
        } else {
            self.docs.insert(id, Arc::new(RwLock::new(doc)));
        }
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
    pub async fn apply_incremental(&self, id: &DocId, data: &[u8]) -> Result<(), CoreError> {
        self.apply_incremental_and_collect(id, data).await?;
        Ok(())
    }

    pub async fn apply_incremental_and_collect(
        &self,
        id: &DocId,
        data: &[u8],
    ) -> Result<AppliedIncremental, CoreError> {
        let doc_arc = self.get_doc(id)?;
        let mut doc = doc_arc.write().await;
        let heads_before = doc.get_heads().to_vec();

        if data.is_empty() {
            return Ok(AppliedIncremental {
                current_heads: heads_before.iter().map(|head| head.to_string()).collect(),
                new_changes: Vec::new(),
            });
        }
        const MAX_INCREMENTAL_SIZE: usize = 16 * 1024 * 1024; // 16 MB
        if data.len() > MAX_INCREMENTAL_SIZE {
            return Err(CoreError::InvalidInput(format!(
                "incremental data too large: {} bytes (max {MAX_INCREMENTAL_SIZE})",
                data.len()
            )));
        }
        let mut updated = doc.clone();
        updated.load_incremental(data)?;
        validate_stored_doc_shape(&updated)?;
        let new_changes = updated
            .get_changes(&heads_before)
            .into_iter()
            .map(|change| (change.hash().to_string(), change.raw_bytes().to_vec()))
            .collect();
        let current_heads = updated
            .get_heads()
            .iter()
            .map(|head| head.to_string())
            .collect();
        *doc = updated;
        self.dirty.insert(*id, ());
        Ok(AppliedIncremental {
            current_heads,
            new_changes,
        })
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
            (automerge::Value::Object(ObjType::Text), text_id) => Ok(doc.text(&text_id)?),
            (automerge::Value::Scalar(scalar), _) => scalar
                .to_str()
                .map(|value| value.to_string())
                .ok_or_else(|| CoreError::InvalidData("text field is not string-like".into())),
            _ => Err(CoreError::InvalidData("text field is not Text type".into())),
        }
    }

    pub async fn get_editor_document(&self, id: &DocId) -> Result<EditorDocument, CoreError> {
        let doc_arc = self.get_doc(id)?;
        let doc = doc_arc.read().await;
        load_editor_document_from_doc(&doc)
    }

    pub async fn get_visible_text(&self, id: &DocId) -> Result<String, CoreError> {
        let document = self.get_editor_document(id).await?;
        Ok(visible_text(&document))
    }

    pub async fn get_read_snapshot(&self, id: &DocId) -> Result<DocReadSnapshot, CoreError> {
        let doc_arc = self.get_doc(id)?;
        let doc = doc_arc.read().await;
        build_read_snapshot_from_doc(&doc)
    }

    /// Replace the text content of a document (used for manual .md import).
    /// WARNING: This is a destructive operation — it tombstones all existing text.
    pub async fn replace_text(&self, id: &DocId, content: &str) -> Result<(), CoreError> {
        let doc_arc = self.get_doc(id)?;
        let mut doc = doc_arc.write().await;
        let schema_version = doc
            .get(automerge::ROOT, "schemaVersion")?
            .and_then(|(value, _)| value.to_u64())
            .unwrap_or(1);

        replace_text_mirror(&mut doc, content)?;
        if schema_version >= 2 {
            let editor_document = migrate_legacy_text_to_v2(content);
            replace_structured_doc(&mut doc, &editor_document)?;
        }

        doc.commit_with(CommitOptions::default().with_time(now_secs()));
        self.dirty.insert(*id, ());
        Ok(())
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
    use crate::editor_schema::KnownNodeKind;
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
        assert_eq!(store.get_visible_text(&id).await.unwrap(), "Hello, world!");
    }

    #[tokio::test]
    async fn test_replace_text_keeps_structured_doc_and_mirror_in_sync() {
        let store = DocStore::new();
        let id = Uuid::new_v4();
        store.create_doc_with_id(id).unwrap();

        store.replace_text(&id, "Alpha\n\nBeta").await.unwrap();

        let editor_doc = store.get_editor_document(&id).await.unwrap();
        assert_eq!(store.get_text(&id).await.unwrap(), "Alpha\n\nBeta");
        assert_eq!(store.get_visible_text(&id).await.unwrap(), "Alpha\n\nBeta");
        assert_eq!(editor_doc.doc.content.len(), 2);
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
        store
            .replace_text(&id, "Hello 🌍🔥 日本語テスト")
            .await
            .unwrap();
        let text = store.get_text(&id).await.unwrap();
        assert_eq!(text, "Hello 🌍🔥 日本語テスト");
    }

    #[tokio::test]
    async fn test_get_text_accepts_scalar_string_field() {
        let store = DocStore::new();
        let id = Uuid::new_v4();
        let mut doc = AutoCommit::new();
        doc.put(automerge::ROOT, "schemaVersion", 2_u64).unwrap();
        doc.put(automerge::ROOT, "text", "graph text mirror")
            .unwrap();
        let root = doc
            .put_object(automerge::ROOT, "doc", ObjType::Map)
            .unwrap();
        doc.put(&root, "id", "root").unwrap();
        doc.put(&root, "type", "doc").unwrap();
        doc.put(&root, "nodeVersion", 1_u64).unwrap();
        let content = doc.put_object(&root, "content", ObjType::List).unwrap();
        let paragraph = doc.insert_object(&content, 0, ObjType::Map).unwrap();
        doc.put(&paragraph, "id", "paragraph-1").unwrap();
        doc.put(&paragraph, "type", "paragraph").unwrap();
        doc.put(&paragraph, "nodeVersion", 1_u64).unwrap();
        let paragraph_content = doc
            .put_object(&paragraph, "content", ObjType::List)
            .unwrap();
        let text = doc
            .insert_object(&paragraph_content, 0, ObjType::Map)
            .unwrap();
        doc.put(&text, "id", "text-1").unwrap();
        doc.put(&text, "type", "text").unwrap();
        doc.put(&text, "nodeVersion", 1_u64).unwrap();
        doc.put(&text, "text", "graph text mirror").unwrap();
        let data = doc.save();

        store.load_doc(id, &data).unwrap();

        let text = store.get_text(&id).await.unwrap();
        assert_eq!(text, "graph text mirror");
    }

    #[tokio::test]
    async fn test_apply_incremental_accepts_structured_note_updates() {
        let store = DocStore::new();
        let id = Uuid::new_v4();
        store.create_doc_with_id(id).unwrap();

        let initial = store.save_doc(&id).await.unwrap();
        let mut edited = AutoCommit::load(&initial).unwrap();
        edited.put(automerge::ROOT, "schemaVersion", 2_u64).unwrap();
        edited
            .put(automerge::ROOT, "text", "Title\n\nBody copy")
            .unwrap();

        let root = match edited.get(automerge::ROOT, "doc").unwrap() {
            Some((automerge::Value::Object(ObjType::Map), root)) => root,
            _ => panic!("expected graph root"),
        };
        let content = match edited.get(&root, "content").unwrap() {
            Some((automerge::Value::Object(ObjType::List), content)) => content,
            _ => panic!("expected graph content list"),
        };
        edited.delete(&content, 0).unwrap();
        let heading = edited.insert_object(&content, 0, ObjType::Map).unwrap();
        edited.put(&heading, "id", "heading-1").unwrap();
        edited.put(&heading, "type", "heading").unwrap();
        edited.put(&heading, "nodeVersion", 1_u64).unwrap();
        let attrs = edited.put_object(&heading, "attrs", ObjType::Map).unwrap();
        edited.put(&attrs, "level", 1_u64).unwrap();
        let heading_content = edited
            .put_object(&heading, "content", ObjType::List)
            .unwrap();
        let heading_text = edited
            .insert_object(&heading_content, 0, ObjType::Map)
            .unwrap();
        edited.put(&heading_text, "id", "heading-text-1").unwrap();
        edited.put(&heading_text, "type", "text").unwrap();
        edited.put(&heading_text, "nodeVersion", 1_u64).unwrap();
        edited.put(&heading_text, "text", "Title").unwrap();

        let paragraph = edited.insert_object(&content, 1, ObjType::Map).unwrap();
        edited.put(&paragraph, "id", "paragraph-1").unwrap();
        edited.put(&paragraph, "type", "paragraph").unwrap();
        edited.put(&paragraph, "nodeVersion", 1_u64).unwrap();
        let paragraph_content = edited
            .put_object(&paragraph, "content", ObjType::List)
            .unwrap();
        let paragraph_text = edited
            .insert_object(&paragraph_content, 0, ObjType::Map)
            .unwrap();
        edited
            .put(&paragraph_text, "id", "paragraph-text-1")
            .unwrap();
        edited.put(&paragraph_text, "type", "text").unwrap();
        edited.put(&paragraph_text, "nodeVersion", 1_u64).unwrap();
        edited.put(&paragraph_text, "text", "Body copy").unwrap();

        let incremental = edited.save_incremental();

        let applied = store
            .apply_incremental_and_collect(&id, &incremental)
            .await
            .unwrap();

        assert!(!applied.current_heads.is_empty());
        assert!(!applied.new_changes.is_empty());
        assert_eq!(store.get_text(&id).await.unwrap(), "Title\n\nBody copy");
    }

    #[tokio::test]
    async fn test_invalid_structured_incremental_does_not_replace_loaded_doc() {
        let store = DocStore::new();
        let id = Uuid::new_v4();
        store.create_doc_with_id(id).unwrap();

        let initial = store.save_doc(&id).await.unwrap();
        let mut edited = AutoCommit::load(&initial).unwrap();
        edited.put(automerge::ROOT, "schemaVersion", 2_u64).unwrap();
        edited.delete(automerge::ROOT, "doc").unwrap();
        let incremental = edited.save_incremental();

        assert!(store.apply_incremental(&id, &incremental).await.is_err());
        assert_eq!(store.get_text(&id).await.unwrap(), "");
    }

    #[tokio::test]
    async fn test_get_editor_document_materializes_legacy_doc_as_v2() {
        let store = DocStore::new();
        let id = Uuid::new_v4();
        let mut doc = AutoCommit::new();
        doc.put(automerge::ROOT, "schemaVersion", 1_u64).unwrap();
        let text = doc
            .put_object(automerge::ROOT, "text", ObjType::Text)
            .unwrap();
        doc.splice_text(&text, 0, 0, "legacy body").unwrap();
        let data = doc.save();

        store.load_doc(id, &data).unwrap();

        let editor_doc = store.get_editor_document(&id).await.unwrap();
        assert_eq!(editor_doc.schema_version, 2);
        assert_eq!(editor_doc.doc.node_type, KnownNodeKind::Doc.as_str());
        assert_eq!(store.get_visible_text(&id).await.unwrap(), "legacy body");
    }

    #[tokio::test]
    async fn test_get_editor_document_reads_structured_v2_doc() {
        let store = DocStore::new();
        let id = Uuid::new_v4();
        store.create_doc_with_id(id).unwrap();

        let initial = store.save_doc(&id).await.unwrap();
        let mut edited = AutoCommit::load(&initial).unwrap();
        edited
            .put(automerge::ROOT, "text", "Title\n\nBody copy")
            .unwrap();

        let root = match edited.get(automerge::ROOT, "doc").unwrap() {
            Some((automerge::Value::Object(ObjType::Map), root)) => root,
            _ => panic!("expected graph root"),
        };
        edited.put(&root, "id", "root").unwrap();
        edited.put(&root, "nodeVersion", 1_u64).unwrap();
        let content = match edited.get(&root, "content").unwrap() {
            Some((automerge::Value::Object(ObjType::List), content)) => content,
            _ => panic!("expected graph content list"),
        };
        edited.delete(&content, 0).unwrap();
        let heading = edited.insert_object(&content, 0, ObjType::Map).unwrap();
        edited.put(&heading, "id", "heading-1").unwrap();
        edited.put(&heading, "type", "heading").unwrap();
        edited.put(&heading, "nodeVersion", 1_u64).unwrap();
        let attrs = edited.put_object(&heading, "attrs", ObjType::Map).unwrap();
        edited.put(&attrs, "level", 1_u64).unwrap();
        let heading_content = edited
            .put_object(&heading, "content", ObjType::List)
            .unwrap();
        let heading_text = edited
            .insert_object(&heading_content, 0, ObjType::Map)
            .unwrap();
        edited.put(&heading_text, "id", "heading-text-1").unwrap();
        edited.put(&heading_text, "type", "text").unwrap();
        edited.put(&heading_text, "nodeVersion", 1_u64).unwrap();
        edited.put(&heading_text, "text", "Title").unwrap();

        let paragraph = edited.insert_object(&content, 1, ObjType::Map).unwrap();
        edited.put(&paragraph, "id", "paragraph-1").unwrap();
        edited.put(&paragraph, "type", "paragraph").unwrap();
        edited.put(&paragraph, "nodeVersion", 1_u64).unwrap();
        let paragraph_content = edited
            .put_object(&paragraph, "content", ObjType::List)
            .unwrap();
        let paragraph_text = edited
            .insert_object(&paragraph_content, 0, ObjType::Map)
            .unwrap();
        edited
            .put(&paragraph_text, "id", "paragraph-text-1")
            .unwrap();
        edited.put(&paragraph_text, "type", "text").unwrap();
        edited.put(&paragraph_text, "nodeVersion", 1_u64).unwrap();
        edited.put(&paragraph_text, "text", "Body copy").unwrap();

        let data = edited.save();
        let store = DocStore::new();
        store.load_doc(id, &data).unwrap();

        let editor_doc = store.get_editor_document(&id).await.unwrap();
        assert_eq!(editor_doc.schema_version, 2);
        assert_eq!(editor_doc.doc.content.len(), 2);
        assert_eq!(
            store.get_visible_text(&id).await.unwrap(),
            "Title\n\nBody copy"
        );
    }

    #[tokio::test]
    async fn test_load_doc_rejects_malformed_v2_document() {
        let store = DocStore::new();
        let id = Uuid::new_v4();
        let mut doc = AutoCommit::new();
        doc.put(automerge::ROOT, "schemaVersion", 2_u64).unwrap();
        doc.put(automerge::ROOT, "text", "mirror").unwrap();
        let root = doc
            .put_object(automerge::ROOT, "doc", ObjType::Map)
            .unwrap();
        doc.put(&root, "id", "root").unwrap();
        doc.put(&root, "type", "doc").unwrap();
        doc.put(&root, "nodeVersion", 1_u64).unwrap();
        let content = doc.put_object(&root, "content", ObjType::List).unwrap();
        let bad = doc.insert_object(&content, 0, ObjType::Map).unwrap();
        doc.put(&bad, "id", "bad-1").unwrap();
        doc.put(&bad, "type", "paragraph").unwrap();
        doc.put(&bad, "nodeVersion", 1_u64).unwrap();
        doc.put(&bad, "text", "not allowed").unwrap();

        let data = doc.save();
        assert!(store.load_doc(id, &data).is_err());
        assert!(!store.contains(&id));
    }

    #[tokio::test]
    async fn test_get_read_snapshot_reports_legacy_source_schema() {
        let store = DocStore::new();
        let id = Uuid::new_v4();
        let mut doc = AutoCommit::new();
        doc.put(automerge::ROOT, "schemaVersion", 1_u64).unwrap();
        let text = doc
            .put_object(automerge::ROOT, "text", ObjType::Text)
            .unwrap();
        doc.splice_text(&text, 0, 0, "legacy body").unwrap();
        let data = doc.save();

        store.load_doc(id, &data).unwrap();

        let snapshot = store.get_read_snapshot(&id).await.unwrap();
        assert_eq!(snapshot.source_schema, DocumentSourceSchema::LegacyText);
        assert!(snapshot.needs_migration);
        assert_eq!(snapshot.visible_text, "legacy body");
        assert!(snapshot.can_edit_plain_text);
        assert!(!snapshot.can_edit_rich_text);
    }

    #[tokio::test]
    async fn test_get_read_snapshot_reports_unsupported_nodes() {
        let mut doc = AutoCommit::new();
        doc.put(automerge::ROOT, "schemaVersion", 2_u64).unwrap();
        doc.put(automerge::ROOT, "text", "").unwrap();
        let root = doc
            .put_object(automerge::ROOT, "doc", ObjType::Map)
            .unwrap();
        doc.put(&root, "id", "root").unwrap();
        doc.put(&root, "type", "doc").unwrap();
        doc.put(&root, "nodeVersion", 1_u64).unwrap();
        let content = doc.put_object(&root, "content", ObjType::List).unwrap();
        let callout = doc.insert_object(&content, 0, ObjType::Map).unwrap();
        doc.put(&callout, "id", "callout-1").unwrap();
        doc.put(&callout, "type", "callout").unwrap();
        doc.put(&callout, "nodeVersion", 1_u64).unwrap();
        let callout_content = doc.put_object(&callout, "content", ObjType::List).unwrap();
        let paragraph = doc
            .insert_object(&callout_content, 0, ObjType::Map)
            .unwrap();
        doc.put(&paragraph, "id", "p-1").unwrap();
        doc.put(&paragraph, "type", "paragraph").unwrap();
        doc.put(&paragraph, "nodeVersion", 1_u64).unwrap();
        let paragraph_content = doc
            .put_object(&paragraph, "content", ObjType::List)
            .unwrap();
        let text = doc
            .insert_object(&paragraph_content, 0, ObjType::Map)
            .unwrap();
        doc.put(&text, "id", "t-1").unwrap();
        doc.put(&text, "type", "text").unwrap();
        doc.put(&text, "nodeVersion", 1_u64).unwrap();
        doc.put(&text, "text", "hello").unwrap();

        let snapshot = read_snapshot_from_bytes(&doc.save()).unwrap();
        assert_eq!(snapshot.source_schema, DocumentSourceSchema::GraphV2);
        assert_eq!(snapshot.unsupported_nodes.count, 1);
        assert_eq!(
            snapshot.unsupported_nodes.node_types,
            vec!["callout".to_string()]
        );
        assert!(!snapshot.can_edit_rich_text);
        assert!(!snapshot.can_edit_plain_text);
    }
}
