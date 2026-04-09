use serde_json::Map;
use uuid::Uuid;

use crate::editor_schema::{
    validate_document, EditorDocument, EditorNode, KnownNodeKind, ValidationMode,
    EDITOR_SCHEMA_VERSION,
};
use crate::error::CoreError;

pub fn new_empty_document() -> EditorDocument {
    EditorDocument {
        schema_version: EDITOR_SCHEMA_VERSION,
        doc: EditorNode {
            id: new_node_id(),
            node_type: KnownNodeKind::Doc.as_str().into(),
            node_version: 1,
            attrs: Map::new(),
            content: vec![new_paragraph_node("")],
            text: None,
            marks: Vec::new(),
        },
    }
}

pub fn new_paragraph_node(text: &str) -> EditorNode {
    EditorNode {
        id: new_node_id(),
        node_type: KnownNodeKind::Paragraph.as_str().into(),
        node_version: 1,
        attrs: Map::new(),
        content: if text.is_empty() {
            Vec::new()
        } else {
            vec![new_text_node(text)]
        },
        text: None,
        marks: Vec::new(),
    }
}

pub fn new_text_node(text: &str) -> EditorNode {
    EditorNode {
        id: new_node_id(),
        node_type: KnownNodeKind::Text.as_str().into(),
        node_version: 1,
        attrs: Map::new(),
        content: Vec::new(),
        text: Some(text.into()),
        marks: Vec::new(),
    }
}

pub fn validate_strict(document: &EditorDocument) -> Result<(), CoreError> {
    validate_document(document, ValidationMode::Strict)
}

pub fn validate_permissive(document: &EditorDocument) -> Result<(), CoreError> {
    validate_document(document, ValidationMode::Permissive)
}

fn new_node_id() -> String {
    Uuid::new_v4().to_string()
}
