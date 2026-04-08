use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::CoreError;

pub const EDITOR_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationMode {
    Strict,
    Permissive,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditorDocument {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    pub doc: EditorNode,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditorNode {
    pub id: String,
    #[serde(rename = "type")]
    pub node_type: String,
    #[serde(rename = "nodeVersion")]
    pub node_version: u32,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub attrs: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<EditorNode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub marks: Vec<EditorMark>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditorMark {
    #[serde(rename = "type")]
    pub mark_type: String,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub attrs: Map<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KnownNodeKind {
    Doc,
    Paragraph,
    Text,
    Heading,
    Blockquote,
    BulletList,
    OrderedList,
    ListItem,
    TaskList,
    TaskItem,
    CodeBlock,
    HorizontalRule,
    Image,
    HardBreak,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KnownMarkKind {
    Bold,
    Italic,
    Strike,
    Code,
    Link,
}

impl KnownNodeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            KnownNodeKind::Doc => "doc",
            KnownNodeKind::Paragraph => "paragraph",
            KnownNodeKind::Text => "text",
            KnownNodeKind::Heading => "heading",
            KnownNodeKind::Blockquote => "blockquote",
            KnownNodeKind::BulletList => "bullet_list",
            KnownNodeKind::OrderedList => "ordered_list",
            KnownNodeKind::ListItem => "list_item",
            KnownNodeKind::TaskList => "task_list",
            KnownNodeKind::TaskItem => "task_item",
            KnownNodeKind::CodeBlock => "code_block",
            KnownNodeKind::HorizontalRule => "horizontal_rule",
            KnownNodeKind::Image => "image",
            KnownNodeKind::HardBreak => "hard_break",
        }
    }

    pub fn is_container(self) -> bool {
        !matches!(
            self,
            KnownNodeKind::Text
                | KnownNodeKind::HorizontalRule
                | KnownNodeKind::Image
                | KnownNodeKind::HardBreak
        )
    }
}

impl KnownMarkKind {
    pub fn as_str(self) -> &'static str {
        match self {
            KnownMarkKind::Bold => "bold",
            KnownMarkKind::Italic => "italic",
            KnownMarkKind::Strike => "strike",
            KnownMarkKind::Code => "code",
            KnownMarkKind::Link => "link",
        }
    }
}

fn known_node_kinds() -> HashMap<&'static str, KnownNodeKind> {
    [
        KnownNodeKind::Doc,
        KnownNodeKind::Paragraph,
        KnownNodeKind::Text,
        KnownNodeKind::Heading,
        KnownNodeKind::Blockquote,
        KnownNodeKind::BulletList,
        KnownNodeKind::OrderedList,
        KnownNodeKind::ListItem,
        KnownNodeKind::TaskList,
        KnownNodeKind::TaskItem,
        KnownNodeKind::CodeBlock,
        KnownNodeKind::HorizontalRule,
        KnownNodeKind::Image,
        KnownNodeKind::HardBreak,
    ]
    .into_iter()
    .map(|kind| (kind.as_str(), kind))
    .collect()
}

fn known_mark_kinds() -> HashMap<&'static str, KnownMarkKind> {
    [
        KnownMarkKind::Bold,
        KnownMarkKind::Italic,
        KnownMarkKind::Strike,
        KnownMarkKind::Code,
        KnownMarkKind::Link,
    ]
    .into_iter()
    .map(|kind| (kind.as_str(), kind))
    .collect()
}

pub fn validate_document(document: &EditorDocument, mode: ValidationMode) -> Result<(), CoreError> {
    if document.schema_version != EDITOR_SCHEMA_VERSION {
        return Err(CoreError::InvalidData(format!(
            "unsupported editor schema version: {}",
            document.schema_version
        )));
    }

    if document.doc.node_type != KnownNodeKind::Doc.as_str() {
        return Err(CoreError::InvalidData(
            "editor document root must be a doc node".into(),
        ));
    }

    validate_node(&document.doc, mode, true)
}

pub fn validate_node(
    node: &EditorNode,
    mode: ValidationMode,
    is_root: bool,
) -> Result<(), CoreError> {
    let known_nodes = known_node_kinds();
    let known_marks = known_mark_kinds();
    let known_kind = known_nodes.get(node.node_type.as_str()).copied();

    if known_kind.is_none() && mode == ValidationMode::Strict {
        return Err(CoreError::InvalidData(format!(
            "unknown node type in strict mode: {}",
            node.node_type
        )));
    }

    if is_root && known_kind != Some(KnownNodeKind::Doc) {
        return Err(CoreError::InvalidData(
            "editor document root must be a doc node".into(),
        ));
    }

    if node.id.trim().is_empty() {
        return Err(CoreError::InvalidData(
            "editor nodes must have a stable id".into(),
        ));
    }

    if let Some(kind) = known_kind {
        if kind != KnownNodeKind::Text && node.text.is_some() {
            return Err(CoreError::InvalidData(format!(
                "{} nodes cannot contain direct text payloads",
                node.node_type
            )));
        }

        match kind {
            KnownNodeKind::Text => {
                if node.text.is_none() {
                    return Err(CoreError::InvalidData(
                        "text nodes must contain text".into(),
                    ));
                }
                if !node.content.is_empty() {
                    return Err(CoreError::InvalidData(
                        "text nodes cannot contain child nodes".into(),
                    ));
                }
            }
            KnownNodeKind::HorizontalRule | KnownNodeKind::Image | KnownNodeKind::HardBreak => {
                if node.text.is_some() {
                    return Err(CoreError::InvalidData(format!(
                        "{} nodes cannot contain text payloads",
                        node.node_type
                    )));
                }
                if !node.content.is_empty() {
                    return Err(CoreError::InvalidData(format!(
                        "{} nodes cannot contain child nodes",
                        node.node_type
                    )));
                }
            }
            _ => {
                if !kind.is_container() && !node.content.is_empty() {
                    return Err(CoreError::InvalidData(format!(
                        "{} nodes cannot contain child nodes",
                        node.node_type
                    )));
                }
            }
        }
    }

    for mark in &node.marks {
        if !known_marks.contains_key(mark.mark_type.as_str()) && mode == ValidationMode::Strict {
            return Err(CoreError::InvalidData(format!(
                "unknown mark type in strict mode: {}",
                mark.mark_type
            )));
        }
    }

    for child in &node.content {
        validate_node(child, mode, false)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn text_node(id: &str, text: &str) -> EditorNode {
        EditorNode {
            id: id.into(),
            node_type: KnownNodeKind::Text.as_str().into(),
            node_version: 1,
            attrs: Map::new(),
            content: Vec::new(),
            text: Some(text.into()),
            marks: Vec::new(),
        }
    }

    fn paragraph_node(id: &str, text: &str) -> EditorNode {
        EditorNode {
            id: id.into(),
            node_type: KnownNodeKind::Paragraph.as_str().into(),
            node_version: 1,
            attrs: Map::new(),
            content: vec![text_node(&format!("{id}-text"), text)],
            text: None,
            marks: Vec::new(),
        }
    }

    #[test]
    fn creates_minimal_v2_doc() {
        let document = EditorDocument {
            schema_version: EDITOR_SCHEMA_VERSION,
            doc: EditorNode {
                id: "root".into(),
                node_type: KnownNodeKind::Doc.as_str().into(),
                node_version: 1,
                attrs: Map::new(),
                content: vec![paragraph_node("p1", "")],
                text: None,
                marks: Vec::new(),
            },
        };

        assert!(validate_document(&document, ValidationMode::Strict).is_ok());
    }

    #[test]
    fn rejects_invalid_root_in_strict_mode() {
        let document = EditorDocument {
            schema_version: EDITOR_SCHEMA_VERSION,
            doc: paragraph_node("p1", "hello"),
        };

        assert!(validate_document(&document, ValidationMode::Strict).is_err());
    }

    #[test]
    fn rejects_text_node_as_root_in_strict_mode() {
        let document = EditorDocument {
            schema_version: EDITOR_SCHEMA_VERSION,
            doc: text_node("t1", "hello"),
        };

        assert!(validate_document(&document, ValidationMode::Strict).is_err());
    }

    #[test]
    fn loads_unknown_node_in_permissive_mode() {
        let document = EditorDocument {
            schema_version: EDITOR_SCHEMA_VERSION,
            doc: EditorNode {
                id: "root".into(),
                node_type: KnownNodeKind::Doc.as_str().into(),
                node_version: 1,
                attrs: Map::new(),
                content: vec![EditorNode {
                    id: "future-1".into(),
                    node_type: "callout".into(),
                    node_version: 3,
                    attrs: Map::from_iter([(String::from("tone"), json!("info"))]),
                    content: vec![paragraph_node("p1", "hello")],
                    text: None,
                    marks: Vec::new(),
                }],
                text: None,
                marks: Vec::new(),
            },
        };

        assert!(validate_document(&document, ValidationMode::Permissive).is_ok());
    }

    #[test]
    fn rejects_unknown_node_in_strict_mode() {
        let document = EditorDocument {
            schema_version: EDITOR_SCHEMA_VERSION,
            doc: EditorNode {
                id: "root".into(),
                node_type: KnownNodeKind::Doc.as_str().into(),
                node_version: 1,
                attrs: Map::new(),
                content: vec![EditorNode {
                    id: "future-1".into(),
                    node_type: "callout".into(),
                    node_version: 3,
                    attrs: Map::new(),
                    content: Vec::new(),
                    text: None,
                    marks: Vec::new(),
                }],
                text: None,
                marks: Vec::new(),
            },
        };

        assert!(validate_document(&document, ValidationMode::Strict).is_err());
    }

    #[test]
    fn preserves_unknown_node_payload_in_permissive_mode() {
        let document = EditorDocument {
            schema_version: EDITOR_SCHEMA_VERSION,
            doc: EditorNode {
                id: "root".into(),
                node_type: KnownNodeKind::Doc.as_str().into(),
                node_version: 1,
                attrs: Map::new(),
                content: vec![EditorNode {
                    id: "future-1".into(),
                    node_type: "callout".into(),
                    node_version: 7,
                    attrs: Map::from_iter([(String::from("tone"), json!("warning"))]),
                    content: vec![paragraph_node("p1", "hi")],
                    text: None,
                    marks: Vec::new(),
                }],
                text: None,
                marks: Vec::new(),
            },
        };

        validate_document(&document, ValidationMode::Permissive).unwrap();
        assert_eq!(document.doc.content[0].node_type, "callout");
        assert_eq!(document.doc.content[0].node_version, 7);
        assert_eq!(
            document.doc.content[0].attrs.get("tone"),
            Some(&json!("warning"))
        );
    }

    #[test]
    fn rejects_leaf_nodes_with_children_in_strict_mode() {
        let document = EditorDocument {
            schema_version: EDITOR_SCHEMA_VERSION,
            doc: EditorNode {
                id: "root".into(),
                node_type: KnownNodeKind::Doc.as_str().into(),
                node_version: 1,
                attrs: Map::new(),
                content: vec![EditorNode {
                    id: "rule-1".into(),
                    node_type: KnownNodeKind::HorizontalRule.as_str().into(),
                    node_version: 1,
                    attrs: Map::new(),
                    content: vec![paragraph_node("p1", "bad")],
                    text: None,
                    marks: Vec::new(),
                }],
                text: None,
                marks: Vec::new(),
            },
        };

        assert!(validate_document(&document, ValidationMode::Strict).is_err());
    }

    #[test]
    fn rejects_non_text_nodes_with_direct_text_payloads_in_strict_mode() {
        let document = EditorDocument {
            schema_version: EDITOR_SCHEMA_VERSION,
            doc: EditorNode {
                id: "root".into(),
                node_type: KnownNodeKind::Doc.as_str().into(),
                node_version: 1,
                attrs: Map::new(),
                content: vec![EditorNode {
                    id: "p1".into(),
                    node_type: KnownNodeKind::Paragraph.as_str().into(),
                    node_version: 1,
                    attrs: Map::new(),
                    content: Vec::new(),
                    text: Some("bad".into()),
                    marks: Vec::new(),
                }],
                text: None,
                marks: Vec::new(),
            },
        };

        assert!(validate_document(&document, ValidationMode::Strict).is_err());
    }
}
