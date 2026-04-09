use crate::editor_doc::{new_empty_document, new_paragraph_node};
use crate::editor_schema::{EditorDocument, EditorNode, EDITOR_SCHEMA_VERSION};
use crate::error::CoreError;

pub fn migrate_legacy_text_to_v2(text: &str) -> EditorDocument {
    let mut document = new_empty_document();
    document.doc.content = legacy_text_to_paragraphs(text);
    document
}

pub fn ensure_v2_document(document: &EditorDocument) -> Result<EditorDocument, CoreError> {
    if document.schema_version == EDITOR_SCHEMA_VERSION {
        return Ok(document.clone());
    }

    Err(CoreError::InvalidInput(
        "ensure_v2_document only accepts already-canonical v2 documents".into(),
    ))
}

pub fn legacy_text_to_paragraphs(text: &str) -> Vec<EditorNode> {
    if text.is_empty() {
        return vec![new_paragraph_node("")];
    }

    text.split("\n\n").map(new_paragraph_node).collect()
}

#[cfg(test)]
mod tests {
    use crate::editor_doc::new_empty_document;
    use crate::editor_schema::EditorDocument;
    use crate::editor_text::visible_text;

    use super::{ensure_v2_document, migrate_legacy_text_to_v2};

    #[test]
    fn migrates_legacy_text_doc_to_v2_paragraph_tree() {
        let migrated = migrate_legacy_text_to_v2("hello\n\nworld");

        assert_eq!(migrated.doc.content.len(), 2);
        assert_eq!(visible_text(&migrated), "hello\n\nworld");
    }

    #[test]
    fn migration_is_idempotent_for_v2_doc() {
        let document = new_empty_document();
        let result = ensure_v2_document(&document).unwrap();

        assert_eq!(result, document);
    }

    #[test]
    fn ensure_v2_document_rejects_non_v2_inputs() {
        let document = EditorDocument {
            schema_version: 1,
            doc: new_empty_document().doc,
        };

        assert!(ensure_v2_document(&document).is_err());
    }

    #[test]
    fn migration_preserves_blank_paragraph_structure_literally() {
        let migrated = migrate_legacy_text_to_v2("alpha\n\n\n\nbeta");

        assert_eq!(migrated.doc.content.len(), 3);
        assert_eq!(visible_text(&migrated), "alpha\n\n\n\nbeta");
    }

    #[test]
    fn migration_does_not_interpret_markdown_syntax() {
        let migrated = migrate_legacy_text_to_v2("# title\n\n- [ ] task\n\n**bold**");

        assert_eq!(visible_text(&migrated), "# title\n\n- [ ] task\n\n**bold**");
        assert_eq!(migrated.doc.content.len(), 3);
    }
}
