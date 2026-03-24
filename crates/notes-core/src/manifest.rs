use automerge::{transaction::Transactable, AutoCommit, ObjType, ReadDoc};
use chrono::Utc;
use uuid::Uuid;

use crate::error::CoreError;
use crate::types::*;

/// Helper to extract a string from an Automerge Value.
fn value_to_string(val: automerge::Value<'_>) -> Option<String> {
    val.into_string().ok()
}

/// An Automerge-backed project manifest.
///
/// Structure:
/// {
///   schemaVersion: u64,
///   projectId: String,
///   name: String,
///   created: String (ISO 8601),
///   files: { <uuid>: { path, created, type } },
/// }
pub struct ProjectManifest {
    doc: AutoCommit,
}

impl ProjectManifest {
    /// Create a new project manifest.
    pub fn new(name: &str) -> Self {
        let mut doc = AutoCommit::new();
        let project_id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        doc.put(automerge::ROOT, "schemaVersion", 1_u64)
            .expect("put schemaVersion");
        doc.put(automerge::ROOT, "projectId", project_id.as_str())
            .expect("put projectId");
        doc.put(automerge::ROOT, "name", name).expect("put name");
        doc.put(automerge::ROOT, "created", now.as_str())
            .expect("put created");
        doc.put_object(automerge::ROOT, "files", ObjType::Map)
            .expect("put files");

        Self { doc }
    }

    /// Load a manifest from saved binary data.
    pub fn load(data: &[u8]) -> Result<Self, CoreError> {
        let doc = AutoCommit::load(data)?;
        Ok(Self { doc })
    }

    /// Save the manifest to binary.
    pub fn save(&mut self) -> Vec<u8> {
        self.doc.save()
    }

    /// Get the project name.
    pub fn name(&self) -> Result<String, CoreError> {
        self.doc
            .get(automerge::ROOT, "name")?
            .and_then(|(v, _)| value_to_string(v))
            .ok_or_else(|| CoreError::InvalidData("manifest missing name".into()))
    }

    /// Get the project ID.
    pub fn project_id(&self) -> Result<String, CoreError> {
        self.doc
            .get(automerge::ROOT, "projectId")?
            .and_then(|(v, _)| value_to_string(v))
            .ok_or_else(|| CoreError::InvalidData("manifest missing projectId".into()))
    }

    /// Register a new file in the manifest. Returns the file's UUID.
    pub fn add_file(&mut self, path: &str, file_type: FileType) -> Result<DocId, CoreError> {
        let files_obj = self
            .doc
            .get(automerge::ROOT, "files")?
            .ok_or_else(|| CoreError::InvalidData("manifest missing files map".into()))?;
        let (_, files_id) = files_obj;

        // Check for path collision
        for key in self.doc.keys(&files_id) {
            if let Some((automerge::Value::Object(ObjType::Map), entry_id)) =
                self.doc.get(&files_id, key.as_str())?
            {
                if let Some((val, _)) = self.doc.get(&entry_id, "path")? {
                    if let Ok(existing_path) = val.into_string() {
                        if existing_path == path {
                            return Err(CoreError::FileAlreadyExists(path.to_string()));
                        }
                    }
                }
            }
        }

        let doc_id = Uuid::new_v4();
        let now = Utc::now().to_rfc3339();
        let type_str = match file_type {
            FileType::Note => "note",
            FileType::Asset => "asset",
        };

        let entry_id = self
            .doc
            .put_object(&files_id, doc_id.to_string().as_str(), ObjType::Map)?;
        self.doc.put(&entry_id, "path", path)?;
        self.doc.put(&entry_id, "created", now.as_str())?;
        self.doc.put(&entry_id, "type", type_str)?;

        Ok(doc_id)
    }

    /// Remove a file from the manifest.
    pub fn remove_file(&mut self, doc_id: &DocId) -> Result<(), CoreError> {
        let files_obj = self
            .doc
            .get(automerge::ROOT, "files")?
            .ok_or_else(|| CoreError::InvalidData("manifest missing files map".into()))?;
        let (_, files_id) = files_obj;

        self.doc.delete(&files_id, doc_id.to_string().as_str())?;
        Ok(())
    }

    /// Rename a file in the manifest (only changes the path mapping).
    pub fn rename_file(&mut self, doc_id: &DocId, new_path: &str) -> Result<(), CoreError> {
        let files_obj = self
            .doc
            .get(automerge::ROOT, "files")?
            .ok_or_else(|| CoreError::InvalidData("manifest missing files map".into()))?;
        let (_, files_id) = files_obj;

        let entry = self
            .doc
            .get(&files_id, doc_id.to_string().as_str())?
            .ok_or(CoreError::DocNotFound(*doc_id))?;
        let (_, entry_id) = entry;

        self.doc.put(&entry_id, "path", new_path)?;
        Ok(())
    }

    /// List all files in the manifest.
    pub fn list_files(&self) -> Result<Vec<DocInfo>, CoreError> {
        let files_obj = self
            .doc
            .get(automerge::ROOT, "files")?
            .ok_or_else(|| CoreError::InvalidData("manifest missing files map".into()))?;
        let (_, files_id) = files_obj;

        let mut result = Vec::new();

        for key in self.doc.keys(&files_id) {
            let id: DocId = match key.parse() {
                Ok(id) => id,
                Err(_) => continue,
            };

            if let Some((automerge::Value::Object(ObjType::Map), entry_id)) =
                self.doc.get(&files_id, key.as_str())?
            {
                let path = self
                    .doc
                    .get(&entry_id, "path")?
                    .and_then(|(v, _)| value_to_string(v))
                    .unwrap_or_default();

                let created_str = self
                    .doc
                    .get(&entry_id, "created")?
                    .and_then(|(v, _)| value_to_string(v))
                    .unwrap_or_default();

                let created = chrono::DateTime::parse_from_rfc3339(&created_str)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| Utc::now());

                let type_str = self
                    .doc
                    .get(&entry_id, "type")?
                    .and_then(|(v, _)| value_to_string(v))
                    .unwrap_or_else(|| "note".to_string());

                let file_type = match type_str.as_str() {
                    "asset" => FileType::Asset,
                    _ => FileType::Note,
                };

                result.push(DocInfo {
                    id,
                    path,
                    file_type,
                    created,
                });
            }
        }

        result.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(result)
    }

    /// Get the file path for a document ID.
    pub fn get_file_path(&self, doc_id: &DocId) -> Result<String, CoreError> {
        let files_obj = self
            .doc
            .get(automerge::ROOT, "files")?
            .ok_or_else(|| CoreError::InvalidData("manifest missing files map".into()))?;
        let (_, files_id) = files_obj;

        let entry = self
            .doc
            .get(&files_id, doc_id.to_string().as_str())?
            .ok_or(CoreError::DocNotFound(*doc_id))?;
        let (_, entry_id) = entry;

        self.doc
            .get(&entry_id, "path")?
            .and_then(|(v, _)| value_to_string(v))
            .ok_or_else(|| CoreError::InvalidData("file entry missing path".into()))
    }

    /// Get a reference to the underlying Automerge document.
    pub fn doc(&self) -> &AutoCommit {
        &self.doc
    }

    /// Get a mutable reference to the underlying Automerge document.
    pub fn doc_mut(&mut self) -> &mut AutoCommit {
        &mut self.doc
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_manifest() {
        let manifest = ProjectManifest::new("test-project");
        assert_eq!(manifest.name().unwrap(), "test-project");
        assert!(manifest.list_files().unwrap().is_empty());
    }

    #[test]
    fn test_add_and_list_files() {
        let mut manifest = ProjectManifest::new("test-project");
        let id1 = manifest.add_file("notes/hello.md", FileType::Note).unwrap();
        let id2 = manifest.add_file("notes/world.md", FileType::Note).unwrap();

        let files = manifest.list_files().unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|f| f.id == id1));
        assert!(files.iter().any(|f| f.id == id2));
    }

    #[test]
    fn test_rename_file() {
        let mut manifest = ProjectManifest::new("test-project");
        let id = manifest.add_file("old-name.md", FileType::Note).unwrap();
        manifest.rename_file(&id, "new-name.md").unwrap();
        let path = manifest.get_file_path(&id).unwrap();
        assert_eq!(path, "new-name.md");
    }

    #[test]
    fn test_remove_file() {
        let mut manifest = ProjectManifest::new("test-project");
        let id = manifest.add_file("to-delete.md", FileType::Note).unwrap();
        assert_eq!(manifest.list_files().unwrap().len(), 1);
        manifest.remove_file(&id).unwrap();
        assert_eq!(manifest.list_files().unwrap().len(), 0);
    }

    #[test]
    fn test_save_and_load() {
        let mut manifest = ProjectManifest::new("test-project");
        manifest.add_file("hello.md", FileType::Note).unwrap();
        let data = manifest.save();
        let loaded = ProjectManifest::load(&data).unwrap();
        assert_eq!(loaded.name().unwrap(), "test-project");
        assert_eq!(loaded.list_files().unwrap().len(), 1);
    }

    #[test]
    fn test_duplicate_path_rejected() {
        let mut manifest = ProjectManifest::new("test-project");
        manifest.add_file("hello.md", FileType::Note).unwrap();
        let result = manifest.add_file("hello.md", FileType::Note);
        assert!(result.is_err());
    }
}
