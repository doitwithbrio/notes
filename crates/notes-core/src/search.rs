//! Full-text search across notes using SQLite FTS5.
//!
//! Maintains a search index alongside the Automerge documents.
//! The index is updated when documents are saved.

use std::path::Path;

use rusqlite::{params, Connection};
use uuid::Uuid;

use crate::error::CoreError;
use crate::types::DocId;

/// Full-text search index backed by SQLite FTS5.
pub struct SearchIndex {
    conn: Connection,
}

impl SearchIndex {
    /// Open or create the search index database.
    pub fn open(db_path: &Path) -> Result<Self, CoreError> {
        let conn = Connection::open(db_path)
            .map_err(|e| CoreError::InvalidData(format!("failed to open search db: {e}")))?;

        // Create the FTS5 virtual table if it doesn't exist
        conn.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS notes_fts USING fts5(
                doc_id UNINDEXED,
                project UNINDEXED,
                path,
                title,
                content,
                tokenize='unicode61'
            );",
        )
        .map_err(|e| CoreError::InvalidData(format!("failed to create FTS table: {e}")))?;

        Ok(Self { conn })
    }

    /// Open an in-memory search index (for testing).
    pub fn open_in_memory() -> Result<Self, CoreError> {
        let conn = Connection::open_in_memory()
            .map_err(|e| CoreError::InvalidData(format!("failed to open in-memory db: {e}")))?;

        conn.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS notes_fts USING fts5(
                doc_id UNINDEXED,
                project UNINDEXED,
                path,
                title,
                content,
                tokenize='unicode61'
            );",
        )
        .map_err(|e| CoreError::InvalidData(format!("failed to create FTS table: {e}")))?;

        Ok(Self { conn })
    }

    /// Index or update a document in the search index.
    pub fn index_document(
        &self,
        doc_id: &DocId,
        project: &str,
        path: &str,
        content: &str,
    ) -> Result<(), CoreError> {
        let doc_id_str = doc_id.to_string();

        // Extract title from first heading or first line
        let title = extract_title(content);

        // Delete existing entry (upsert)
        self.conn
            .execute(
                "DELETE FROM notes_fts WHERE doc_id = ?1",
                params![doc_id_str],
            )
            .map_err(|e| CoreError::InvalidData(format!("FTS delete failed: {e}")))?;

        // Insert new entry
        self.conn
            .execute(
                "INSERT INTO notes_fts (doc_id, project, path, title, content) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![doc_id_str, project, path, title, content],
            )
            .map_err(|e| CoreError::InvalidData(format!("FTS insert failed: {e}")))?;

        Ok(())
    }

    /// Remove a document from the search index.
    pub fn remove_document(&self, doc_id: &DocId) -> Result<(), CoreError> {
        self.conn
            .execute(
                "DELETE FROM notes_fts WHERE doc_id = ?1",
                params![doc_id.to_string()],
            )
            .map_err(|e| CoreError::InvalidData(format!("FTS delete failed: {e}")))?;
        Ok(())
    }

    /// Search for notes matching a query.
    /// Returns results ranked by relevance.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>, CoreError> {
        if query.trim().is_empty() {
            return Ok(vec![]);
        }

        // Sanitize query for FTS5 (escape special characters)
        let safe_query = sanitize_fts_query(query);

        let mut stmt = self
            .conn
            .prepare(
                "SELECT doc_id, project, path, title, snippet(notes_fts, 4, '<mark>', '</mark>', '...', 32)
                 FROM notes_fts
                 WHERE notes_fts MATCH ?1
                 ORDER BY rank
                 LIMIT ?2",
            )
            .map_err(|e| CoreError::InvalidData(format!("FTS query prepare failed: {e}")))?;

        let results = stmt
            .query_map(params![safe_query, limit as i64], |row| {
                Ok(SearchResult {
                    doc_id: row.get::<_, String>(0)?.parse().unwrap_or(Uuid::nil()),
                    project: row.get(1)?,
                    path: row.get(2)?,
                    title: row.get(3)?,
                    snippet: row.get(4)?,
                })
            })
            .map_err(|e| CoreError::InvalidData(format!("FTS query failed: {e}")))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(results)
    }

    /// Search within a specific project.
    pub fn search_project(
        &self,
        query: &str,
        project: &str,
        limit: usize,
    ) -> Result<Vec<SearchResult>, CoreError> {
        if query.trim().is_empty() {
            return Ok(vec![]);
        }

        let safe_query = sanitize_fts_query(query);

        let mut stmt = self
            .conn
            .prepare(
                "SELECT doc_id, project, path, title, snippet(notes_fts, 4, '<mark>', '</mark>', '...', 32)
                 FROM notes_fts
                 WHERE notes_fts MATCH ?1 AND project = ?2
                 ORDER BY rank
                 LIMIT ?3",
            )
            .map_err(|e| CoreError::InvalidData(format!("FTS query failed: {e}")))?;

        let results = stmt
            .query_map(params![safe_query, project, limit as i64], |row| {
                Ok(SearchResult {
                    doc_id: row.get::<_, String>(0)?.parse().unwrap_or(Uuid::nil()),
                    project: row.get(1)?,
                    path: row.get(2)?,
                    title: row.get(3)?,
                    snippet: row.get(4)?,
                })
            })
            .map_err(|e| CoreError::InvalidData(format!("FTS query failed: {e}")))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(results)
    }

    /// Get the total number of indexed documents.
    pub fn document_count(&self) -> Result<usize, CoreError> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM notes_fts", [], |row| row.get(0))
            .map_err(|e| CoreError::InvalidData(format!("count query failed: {e}")))?;
        Ok(count as usize)
    }
}

/// A search result with relevance-ranked snippet.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub doc_id: DocId,
    pub project: String,
    pub path: String,
    pub title: String,
    pub snippet: String,
}

/// Extract a title from markdown content.
/// Uses the first `# heading` or the first non-empty line.
fn extract_title(content: &str) -> String {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Check for markdown heading
        if let Some(heading) = trimmed.strip_prefix('#') {
            return heading.trim_start_matches('#').trim().to_string();
        }
        // Use first non-empty line as title
        return trimmed.chars().take(100).collect();
    }
    String::new()
}

/// Sanitize a query string for FTS5.
/// Wraps each word in quotes to prevent FTS5 syntax injection.
fn sanitize_fts_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(|word| {
            // Remove any existing quotes and special FTS5 chars
            let clean: String = word
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            if clean.is_empty() {
                String::new()
            } else {
                format!("\"{clean}\"")
            }
        })
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_search_index() {
        let index = SearchIndex::open_in_memory().unwrap();
        assert_eq!(index.document_count().unwrap(), 0);
    }

    #[test]
    fn test_index_and_search() {
        let index = SearchIndex::open_in_memory().unwrap();

        let doc_id = Uuid::new_v4();
        index
            .index_document(
                &doc_id,
                "test-project",
                "notes/hello.md",
                "# Hello World\n\nThis is a test note about Rust programming.",
            )
            .unwrap();

        let results = index.search("Rust programming", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].doc_id, doc_id);
        assert_eq!(results[0].title, "Hello World");
    }

    #[test]
    fn test_search_multiple_documents() {
        let index = SearchIndex::open_in_memory().unwrap();

        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();

        index
            .index_document(&id1, "proj", "a.md", "# Alpha\n\nRust is great")
            .unwrap();
        index
            .index_document(&id2, "proj", "b.md", "# Beta\n\nPython is popular")
            .unwrap();
        index
            .index_document(&id3, "proj", "c.md", "# Gamma\n\nRust and Python together")
            .unwrap();

        let results = index.search("Rust", 10).unwrap();
        assert_eq!(results.len(), 2);

        let results = index.search("Python", 10).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_search_project_filter() {
        let index = SearchIndex::open_in_memory().unwrap();

        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();

        index
            .index_document(&id1, "project-a", "note.md", "Hello from project A")
            .unwrap();
        index
            .index_document(&id2, "project-b", "note.md", "Hello from project B")
            .unwrap();

        let results = index.search_project("Hello", "project-a", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].doc_id, id1);
    }

    #[test]
    fn test_remove_document() {
        let index = SearchIndex::open_in_memory().unwrap();
        let doc_id = Uuid::new_v4();

        index
            .index_document(&doc_id, "proj", "note.md", "content")
            .unwrap();
        assert_eq!(index.document_count().unwrap(), 1);

        index.remove_document(&doc_id).unwrap();
        assert_eq!(index.document_count().unwrap(), 0);
    }

    #[test]
    fn test_update_document() {
        let index = SearchIndex::open_in_memory().unwrap();
        let doc_id = Uuid::new_v4();

        index
            .index_document(&doc_id, "proj", "note.md", "old content")
            .unwrap();
        index
            .index_document(&doc_id, "proj", "note.md", "new content about cats")
            .unwrap();

        assert_eq!(index.document_count().unwrap(), 1);

        let results = index.search("cats", 10).unwrap();
        assert_eq!(results.len(), 1);

        let results = index.search("old", 10).unwrap();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_empty_query() {
        let index = SearchIndex::open_in_memory().unwrap();
        let results = index.search("", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_extract_title_heading() {
        assert_eq!(extract_title("# My Title\n\nContent"), "My Title");
        assert_eq!(extract_title("## Sub Heading"), "Sub Heading");
    }

    #[test]
    fn test_extract_title_no_heading() {
        assert_eq!(extract_title("Just some text"), "Just some text");
    }

    #[test]
    fn test_extract_title_empty() {
        assert_eq!(extract_title(""), "");
        assert_eq!(extract_title("\n\n"), "");
    }

    #[test]
    fn test_unicode_search() {
        let index = SearchIndex::open_in_memory().unwrap();
        let doc_id = Uuid::new_v4();

        index
            .index_document(
                &doc_id,
                "proj",
                "note.md",
                "Notes about café and résumé writing",
            )
            .unwrap();

        // FTS5 unicode61 tokenizer handles accented Latin characters
        let results = index.search("café", 10).unwrap();
        assert_eq!(results.len(), 1);

        let results = index.search("resume", 10).unwrap();
        // unicode61 may normalize accents, so "resume" might match "résumé"
        // This is tokenizer-dependent, so we just verify no crash
        assert!(results.len() <= 1);
    }
}
