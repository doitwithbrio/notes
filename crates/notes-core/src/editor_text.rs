use crate::editor_schema::{EditorDocument, EditorNode, KnownNodeKind};

pub fn visible_text(document: &EditorDocument) -> String {
    visible_text_for_node(&document.doc)
}

pub fn visible_text_for_node(node: &EditorNode) -> String {
    match node.node_type.as_str() {
        "doc" => join_doc_blocks(&node.content),
        "paragraph" | "heading" | "blockquote" | "list_item" | "task_item" | "code_block" => node
            .content
            .iter()
            .map(visible_text_for_node)
            .collect::<Vec<_>>()
            .join(""),
        "bullet_list" | "ordered_list" | "task_list" => join_blocks(&node.content, "\n"),
        "text" => node.text.clone().unwrap_or_default(),
        "hard_break" => "\n".into(),
        "horizontal_rule" => "---".into(),
        "image" => node
            .attrs
            .get("alt")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        other if is_unknown_node(other) => String::new(),
        _ => node
            .content
            .iter()
            .map(visible_text_for_node)
            .collect::<Vec<_>>()
            .join(""),
    }
}

fn join_blocks(nodes: &[EditorNode], separator: &str) -> String {
    nodes
        .iter()
        .map(visible_text_for_node)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(separator)
}

fn join_doc_blocks(nodes: &[EditorNode]) -> String {
    let mut rendered = Vec::new();

    for node in nodes {
        let value = visible_text_for_node(node);
        if value.is_empty() && is_unknown_node(node.node_type.as_str()) {
            continue;
        }
        rendered.push(value);
    }

    rendered.join("\n\n")
}

fn is_unknown_node(kind: &str) -> bool {
    ![
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
    .iter()
    .any(|known| known.as_str() == kind)
}

#[cfg(test)]
mod tests {
    use serde_json::Map;

    use crate::editor_doc::{new_paragraph_node, new_text_node};
    use crate::editor_schema::{EditorDocument, EditorNode, KnownNodeKind, EDITOR_SCHEMA_VERSION};

    use super::visible_text;

    #[test]
    fn extracts_visible_text_from_paragraph() {
        let document = EditorDocument {
            schema_version: EDITOR_SCHEMA_VERSION,
            doc: EditorNode {
                id: "root".into(),
                node_type: KnownNodeKind::Doc.as_str().into(),
                node_version: 1,
                attrs: Map::new(),
                content: vec![new_paragraph_node("hello")],
                text: None,
                marks: Vec::new(),
            },
        };

        assert_eq!(visible_text(&document), "hello");
    }

    #[test]
    fn extracts_visible_text_from_heading_and_paragraph() {
        let document = EditorDocument {
            schema_version: EDITOR_SCHEMA_VERSION,
            doc: EditorNode {
                id: "root".into(),
                node_type: KnownNodeKind::Doc.as_str().into(),
                node_version: 1,
                attrs: Map::new(),
                content: vec![
                    EditorNode {
                        id: "h1".into(),
                        node_type: KnownNodeKind::Heading.as_str().into(),
                        node_version: 1,
                        attrs: Map::new(),
                        content: vec![new_text_node("Title")],
                        text: None,
                        marks: Vec::new(),
                    },
                    new_paragraph_node("Body"),
                ],
                text: None,
                marks: Vec::new(),
            },
        };

        assert_eq!(visible_text(&document), "Title\n\nBody");
    }

    #[test]
    fn extracts_visible_text_from_task_list() {
        let document = EditorDocument {
            schema_version: EDITOR_SCHEMA_VERSION,
            doc: EditorNode {
                id: "root".into(),
                node_type: KnownNodeKind::Doc.as_str().into(),
                node_version: 1,
                attrs: Map::new(),
                content: vec![EditorNode {
                    id: "tasks".into(),
                    node_type: KnownNodeKind::TaskList.as_str().into(),
                    node_version: 1,
                    attrs: Map::new(),
                    content: vec![
                        EditorNode {
                            id: "task-1".into(),
                            node_type: KnownNodeKind::TaskItem.as_str().into(),
                            node_version: 1,
                            attrs: Map::new(),
                            content: vec![new_text_node("first")],
                            text: None,
                            marks: Vec::new(),
                        },
                        EditorNode {
                            id: "task-2".into(),
                            node_type: KnownNodeKind::TaskItem.as_str().into(),
                            node_version: 1,
                            attrs: Map::new(),
                            content: vec![new_text_node("second")],
                            text: None,
                            marks: Vec::new(),
                        },
                    ],
                    text: None,
                    marks: Vec::new(),
                }],
                text: None,
                marks: Vec::new(),
            },
        };

        assert_eq!(visible_text(&document), "first\nsecond");
    }

    #[test]
    fn ignores_unknown_nodes_for_visible_text() {
        let document = EditorDocument {
            schema_version: EDITOR_SCHEMA_VERSION,
            doc: EditorNode {
                id: "root".into(),
                node_type: KnownNodeKind::Doc.as_str().into(),
                node_version: 1,
                attrs: Map::new(),
                content: vec![EditorNode {
                    id: "future".into(),
                    node_type: "callout".into(),
                    node_version: 1,
                    attrs: Map::new(),
                    content: vec![new_paragraph_node("hidden")],
                    text: None,
                    marks: Vec::new(),
                }],
                text: None,
                marks: Vec::new(),
            },
        };

        assert_eq!(visible_text(&document), "");
    }
}
