use automerge::{transaction::Transactable, AutoCommit, ObjType, ReadDoc};
use chrono::Utc;
use uuid::Uuid;

use crate::error::CoreError;
use crate::types::*;

/// Helper to extract a string from an Automerge Value.
pub fn value_to_string(val: automerge::Value<'_>) -> Option<String> {
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
    pub fn new(name: &str) -> Result<Self, CoreError> {
        let mut doc = AutoCommit::new();
        let project_id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        doc.put(automerge::ROOT, "schemaVersion", 1_u64)?;
        doc.put(automerge::ROOT, "projectId", project_id.as_str())?;
        doc.put(automerge::ROOT, "name", name)?;
        doc.put(automerge::ROOT, "created", now.as_str())?;
        doc.put_object(automerge::ROOT, "files", ObjType::Map)?;

        // _ownerControlled section (for shared projects)
        let owner_section = doc.put_object(automerge::ROOT, "_ownerControlled", ObjType::Map)?;
        doc.put(&owner_section, "owner", "")?; // Set when sharing is enabled
        doc.put_object(&owner_section, "peers", ObjType::Map)?;
        doc.put(&owner_section, "keyEpoch", 0_u64)?;
        doc.put_object(&owner_section, "epochKeys", ObjType::Map)?;
        let sharing = doc.put_object(&owner_section, "sharing", ObjType::Map)?;
        doc.put(&sharing, "group", "single")?;

        Ok(Self { doc })
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

    /// Get the project creation timestamp.
    pub fn created(&self) -> Option<String> {
        self.doc
            .get(automerge::ROOT, "created")
            .ok()
            .flatten()
            .and_then(|(v, _)| value_to_string(v))
    }

    // ── Project metadata ──────────────────────────────────────────────

    /// Set the project name.
    pub fn set_name(&mut self, name: &str) -> Result<(), CoreError> {
        self.doc.put(automerge::ROOT, "name", name)?;
        Ok(())
    }

    /// Get the project emoji (optional).
    pub fn emoji(&self) -> Option<String> {
        self.doc
            .get(automerge::ROOT, "emoji")
            .ok()
            .flatten()
            .and_then(|(v, _)| value_to_string(v))
    }

    /// Set the project emoji.
    pub fn set_emoji(&mut self, emoji: &str) -> Result<(), CoreError> {
        self.doc.put(automerge::ROOT, "emoji", emoji)?;
        Ok(())
    }

    /// Get the project description (optional).
    pub fn description(&self) -> Option<String> {
        self.doc
            .get(automerge::ROOT, "description")
            .ok()
            .flatten()
            .and_then(|(v, _)| value_to_string(v))
    }

    /// Set the project description.
    pub fn set_description(&mut self, desc: &str) -> Result<(), CoreError> {
        self.doc.put(automerge::ROOT, "description", desc)?;
        Ok(())
    }

    /// Get the project color (from preset palette).
    pub fn color(&self) -> Option<String> {
        self.doc
            .get(automerge::ROOT, "color")
            .ok()
            .flatten()
            .and_then(|(v, _)| value_to_string(v))
    }

    /// Set the project color (must be one of the preset palette names).
    pub fn set_color(&mut self, color: &str) -> Result<(), CoreError> {
        const PALETTE: &[&str] = &[
            "blue", "red", "green", "purple", "orange", "pink", "teal", "yellow", "gray", "indigo",
            "amber", "rose",
        ];
        if !PALETTE.contains(&color) && !color.is_empty() {
            return Err(CoreError::InvalidInput(format!(
                "invalid color '{color}', must be one of: {}",
                PALETTE.join(", ")
            )));
        }
        self.doc.put(automerge::ROOT, "color", color)?;
        Ok(())
    }

    /// Get whether the project is archived.
    pub fn is_archived(&self) -> bool {
        self.doc
            .get(automerge::ROOT, "archived")
            .ok()
            .flatten()
            .and_then(|(v, _)| v.to_bool())
            .unwrap_or(false)
    }

    /// Set the archived state.
    pub fn set_archived(&mut self, archived: bool) -> Result<(), CoreError> {
        self.doc.put(automerge::ROOT, "archived", archived)?;
        Ok(())
    }

    // ── Todo management ──────────────────────────────────────────────

    /// Get or create the `todos` map in the manifest.
    fn todos_map_id(&mut self) -> Result<automerge::ObjId, CoreError> {
        if let Some((automerge::Value::Object(ObjType::Map), id)) =
            self.doc.get(automerge::ROOT, "todos")?
        {
            return Ok(id);
        }
        // Create the todos map if it doesn't exist
        let id = self
            .doc
            .put_object(automerge::ROOT, "todos", ObjType::Map)?;
        Ok(id)
    }

    /// Get the `todos` map ID for reading (returns None if it doesn't exist).
    fn todos_map_id_read(&self) -> Option<automerge::ObjId> {
        self.doc
            .get(automerge::ROOT, "todos")
            .ok()
            .flatten()
            .and_then(|(v, id)| match v {
                automerge::Value::Object(ObjType::Map) => Some(id),
                _ => None,
            })
    }

    /// Add a todo item. Returns its UUID.
    pub fn add_todo(
        &mut self,
        text: &str,
        created_by: &str,
        linked_doc_id: Option<&str>,
    ) -> Result<uuid::Uuid, CoreError> {
        let todos_id = self.todos_map_id()?;
        let todo_id = uuid::Uuid::new_v4();
        let now = Utc::now().to_rfc3339();

        let entry_id =
            self.doc
                .put_object(&todos_id, todo_id.to_string().as_str(), ObjType::Map)?;
        self.doc.put(&entry_id, "text", text)?;
        self.doc.put(&entry_id, "done", false)?;
        self.doc.put(&entry_id, "createdBy", created_by)?;
        self.doc.put(&entry_id, "createdAt", now.as_str())?;
        if let Some(doc_id) = linked_doc_id {
            self.doc.put(&entry_id, "linkedDocId", doc_id)?;
        }

        Ok(todo_id)
    }

    /// Toggle a todo's done state.
    pub fn toggle_todo(&mut self, todo_id: &str) -> Result<bool, CoreError> {
        let todos_id = self.todos_map_id()?;
        let entry = self
            .doc
            .get(&todos_id, todo_id)?
            .ok_or_else(|| CoreError::InvalidData(format!("todo not found: {todo_id}")))?;
        let (_, entry_id) = entry;

        let current_done = self
            .doc
            .get(&entry_id, "done")?
            .and_then(|(v, _)| v.to_bool())
            .unwrap_or(false);

        let new_done = !current_done;
        self.doc.put(&entry_id, "done", new_done)?;
        Ok(new_done)
    }

    /// Remove a todo.
    pub fn remove_todo(&mut self, todo_id: &str) -> Result<(), CoreError> {
        let todos_id = self.todos_map_id()?;
        self.doc.delete(&todos_id, todo_id)?;
        Ok(())
    }

    /// Update a todo's text.
    pub fn update_todo_text(&mut self, todo_id: &str, text: &str) -> Result<(), CoreError> {
        let todos_id = self.todos_map_id()?;
        let entry = self
            .doc
            .get(&todos_id, todo_id)?
            .ok_or_else(|| CoreError::InvalidData(format!("todo not found: {todo_id}")))?;
        let (_, entry_id) = entry;
        self.doc.put(&entry_id, "text", text)?;
        Ok(())
    }

    /// List all todos in the project.
    pub fn list_todos(&self) -> Result<Vec<TodoItem>, CoreError> {
        let todos_id = match self.todos_map_id_read() {
            Some(id) => id,
            None => return Ok(vec![]),
        };

        let mut todos = Vec::new();
        for key in self.doc.keys(&todos_id) {
            if let Some((automerge::Value::Object(ObjType::Map), entry_id)) =
                self.doc.get(&todos_id, key.as_str())?
            {
                let text = self
                    .doc
                    .get(&entry_id, "text")?
                    .and_then(|(v, _)| value_to_string(v))
                    .unwrap_or_default();
                let done = self
                    .doc
                    .get(&entry_id, "done")?
                    .and_then(|(v, _)| v.to_bool())
                    .unwrap_or(false);
                let created_by = self
                    .doc
                    .get(&entry_id, "createdBy")?
                    .and_then(|(v, _)| value_to_string(v))
                    .unwrap_or_default();
                let created_at = self
                    .doc
                    .get(&entry_id, "createdAt")?
                    .and_then(|(v, _)| value_to_string(v))
                    .unwrap_or_default();
                let linked_doc_id = self
                    .doc
                    .get(&entry_id, "linkedDocId")?
                    .and_then(|(v, _)| value_to_string(v));

                todos.push(TodoItem {
                    id: key.to_string(),
                    text,
                    done,
                    created_by,
                    created_at,
                    linked_doc_id,
                });
            }
        }

        // Sort: undone first, then by created_at
        todos.sort_by(|a, b| a.done.cmp(&b.done).then(a.created_at.cmp(&b.created_at)));

        Ok(todos)
    }

    // ── File management ──────────────────────────────────────────────

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

    // ── Owner-controlled section ──────────────────────────────────────

    /// Helper to get the `_ownerControlled` map ID.
    fn owner_section_id(&self) -> Result<automerge::ObjId, CoreError> {
        self.doc
            .get(automerge::ROOT, "_ownerControlled")?
            .map(|(_, id)| id)
            .ok_or_else(|| CoreError::InvalidData("manifest missing _ownerControlled".into()))
    }

    /// Set the project owner (peer ID string).
    pub fn set_owner(&mut self, owner_peer_id: &str) -> Result<(), CoreError> {
        let section = self.owner_section_id()?;
        self.doc.put(&section, "owner", owner_peer_id)?;
        Ok(())
    }

    /// Get the project owner peer ID.
    pub fn get_owner(&self) -> Result<String, CoreError> {
        let section = self.owner_section_id()?;
        self.doc
            .get(&section, "owner")?
            .and_then(|(v, _)| value_to_string(v))
            .ok_or_else(|| CoreError::InvalidData("manifest missing owner".into()))
    }

    /// Add a peer to the project.
    pub fn add_peer(&mut self, peer_id: &str, role: &str, alias: &str) -> Result<(), CoreError> {
        let section = self.owner_section_id()?;
        let (_, peers_id) = self
            .doc
            .get(&section, "peers")?
            .ok_or_else(|| CoreError::InvalidData("manifest missing peers map".into()))?;

        let peer_obj = self.doc.put_object(&peers_id, peer_id, ObjType::Map)?;
        let now = Utc::now().to_rfc3339();
        self.doc.put(&peer_obj, "role", role)?;
        self.doc.put(&peer_obj, "alias", alias)?;
        self.doc.put(&peer_obj, "since", now.as_str())?;
        Ok(())
    }

    /// Set the Automerge actor ID for a peer (called on first sync).
    pub fn set_peer_actor_id(&mut self, peer_id: &str, actor_id: &str) -> Result<(), CoreError> {
        let section = self.owner_section_id()?;
        let (_, peers_id) = self
            .doc
            .get(&section, "peers")?
            .ok_or_else(|| CoreError::InvalidData("manifest missing peers map".into()))?;

        let entry = self
            .doc
            .get(&peers_id, peer_id)?
            .ok_or_else(|| CoreError::InvalidInput(format!("peer {peer_id} not found")))?;
        let (_, entry_id) = entry;

        self.doc.put(&entry_id, "actorId", actor_id)?;
        Ok(())
    }

    /// Get a mapping of Automerge actor hex -> display alias for all peers.
    /// Used by the version history UI to show human-readable author names.
    pub fn get_actor_aliases(
        &self,
    ) -> Result<std::collections::HashMap<String, String>, CoreError> {
        let mut aliases = std::collections::HashMap::new();

        let section = self.owner_section_id()?;
        let peers_result = self.doc.get(&section, "peers")?;
        let (_, peers_id) = match peers_result {
            Some(v) => v,
            None => return Ok(aliases),
        };

        for key in self.doc.keys(&peers_id) {
            if let Some((automerge::Value::Object(ObjType::Map), entry_id)) =
                self.doc.get(&peers_id, key.as_str())?
            {
                let actor_id = self
                    .doc
                    .get(&entry_id, "actorId")?
                    .and_then(|(v, _)| value_to_string(v));

                let alias = self
                    .doc
                    .get(&entry_id, "alias")?
                    .and_then(|(v, _)| value_to_string(v))
                    .unwrap_or_default();

                if let Some(actor) = actor_id {
                    if !actor.is_empty() && !alias.is_empty() {
                        aliases.insert(actor, alias);
                    }
                }
            }
        }

        Ok(aliases)
    }

    /// Remove a peer from the project.
    pub fn remove_peer(&mut self, peer_id: &str) -> Result<(), CoreError> {
        let section = self.owner_section_id()?;
        let (_, peers_id) = self
            .doc
            .get(&section, "peers")?
            .ok_or_else(|| CoreError::InvalidData("manifest missing peers map".into()))?;

        self.doc.delete(&peers_id, peer_id)?;
        Ok(())
    }

    /// List all peers in the project.
    pub fn list_peers(&self) -> Result<Vec<PeerInfo>, CoreError> {
        let section = self.owner_section_id()?;
        let (_, peers_id) = self
            .doc
            .get(&section, "peers")?
            .ok_or_else(|| CoreError::InvalidData("manifest missing peers map".into()))?;

        let mut result = Vec::new();
        for key in self.doc.keys(&peers_id) {
            if let Some((automerge::Value::Object(ObjType::Map), entry_id)) =
                self.doc.get(&peers_id, key.as_str())?
            {
                let role_str = self
                    .doc
                    .get(&entry_id, "role")?
                    .and_then(|(v, _)| value_to_string(v))
                    .unwrap_or_else(|| "editor".to_string());

                let role = match role_str.as_str() {
                    "owner" => PeerRole::Owner,
                    "viewer" => PeerRole::Viewer,
                    _ => PeerRole::Editor,
                };

                let alias = self
                    .doc
                    .get(&entry_id, "alias")?
                    .and_then(|(v, _)| value_to_string(v))
                    .unwrap_or_default();

                let since_str = self
                    .doc
                    .get(&entry_id, "since")?
                    .and_then(|(v, _)| value_to_string(v))
                    .unwrap_or_default();

                let since = chrono::DateTime::parse_from_rfc3339(&since_str)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| Utc::now());

                result.push(PeerInfo {
                    peer_id: key,
                    role,
                    alias,
                    since,
                });
            }
        }

        Ok(result)
    }

    /// Set the current key epoch number.
    pub fn set_key_epoch(&mut self, epoch: u64) -> Result<(), CoreError> {
        let section = self.owner_section_id()?;
        self.doc.put(&section, "keyEpoch", epoch)?;
        Ok(())
    }

    /// Get the current key epoch number.
    pub fn get_key_epoch(&self) -> Result<u64, CoreError> {
        let section = self.owner_section_id()?;
        self.doc
            .get(&section, "keyEpoch")?
            .and_then(|(v, _)| v.to_u64())
            .ok_or_else(|| CoreError::InvalidData("manifest missing keyEpoch".into()))
    }

    // ── Accessors ────────────────────────────────────────────────────

    /// Get a reference to the underlying Automerge document.
    pub fn doc(&self) -> &AutoCommit {
        &self.doc
    }

    /// Get a mutable reference to the underlying Automerge document.
    pub fn doc_mut(&mut self) -> &mut AutoCommit {
        &mut self.doc
    }

    // ── ACL Validation ───────────────────────────────────────────────

    /// Validate that `_ownerControlled` fields were only modified by the owner.
    ///
    /// Call this after syncing the manifest. Checks all changes applied since
    /// `before_heads`. If any change that modifies `_ownerControlled` was made
    /// by an actor that is NOT the owner, returns an error.
    ///
    /// `owner_actor_hex` is the Automerge actor ID (hex string) of the owner.
    /// This must be set in the manifest (via `set_peer_actor_id`) during the
    /// invite flow so we know which actor corresponds to the owner.
    pub fn validate_owner_controlled_changes(
        &mut self,
        before_heads: &[automerge::ChangeHash],
        owner_actor_hex: &str,
    ) -> Result<(), CoreError> {
        let changes = self.doc.get_changes(before_heads);

        if changes.is_empty() {
            return Ok(());
        }

        // Get the _ownerControlled object ID
        let owner_section_id = self.owner_section_id()?;

        // For each new change, check if it touches the _ownerControlled subtree.
        // We do this by checking if any operation in the change targets the
        // _ownerControlled object or any of its descendants.
        //
        // A simpler heuristic: compare _ownerControlled state at before_heads
        // vs current heads. If it changed, verify ALL new changes are from the owner.
        let owner_before = self
            .doc
            .get_at(&owner_section_id, "owner", before_heads)?
            .and_then(|(v, _)| value_to_string(v));
        let owner_after = self
            .doc
            .get(&owner_section_id, "owner")?
            .and_then(|(v, _)| value_to_string(v));

        let epoch_before = self
            .doc
            .get_at(&owner_section_id, "keyEpoch", before_heads)?
            .and_then(|(v, _)| v.to_u64());
        let epoch_after = self
            .doc
            .get(&owner_section_id, "keyEpoch")?
            .and_then(|(v, _)| v.to_u64());

        // Check if _ownerControlled fields changed
        let owner_changed = owner_before != owner_after;
        let epoch_changed = epoch_before != epoch_after;
        // Note: peers map changes are harder to detect generically.
        // For now, check the two most critical fields.

        if owner_changed || epoch_changed {
            // Verify all new changes are from the owner actor
            for change in &changes {
                let actor = change.actor_id().to_hex_string();
                if actor != owner_actor_hex {
                    log::error!(
                        "Unauthorized _ownerControlled modification by actor {}, expected {}",
                        &actor[..8.min(actor.len())],
                        &owner_actor_hex[..8.min(owner_actor_hex.len())],
                    );
                    return Err(CoreError::InvalidInput(
                        "unauthorized modification of owner-controlled fields".into(),
                    ));
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_manifest() {
        let manifest = ProjectManifest::new("test-project").unwrap();
        assert_eq!(manifest.name().unwrap(), "test-project");
        assert!(manifest.list_files().unwrap().is_empty());
    }

    #[test]
    fn test_add_and_list_files() {
        let mut manifest = ProjectManifest::new("test-project").unwrap();
        let id1 = manifest.add_file("notes/hello.md", FileType::Note).unwrap();
        let id2 = manifest.add_file("notes/world.md", FileType::Note).unwrap();

        let files = manifest.list_files().unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|f| f.id == id1));
        assert!(files.iter().any(|f| f.id == id2));
    }

    #[test]
    fn test_rename_file() {
        let mut manifest = ProjectManifest::new("test-project").unwrap();
        let id = manifest.add_file("old-name.md", FileType::Note).unwrap();
        manifest.rename_file(&id, "new-name.md").unwrap();
        let path = manifest.get_file_path(&id).unwrap();
        assert_eq!(path, "new-name.md");
    }

    #[test]
    fn test_remove_file() {
        let mut manifest = ProjectManifest::new("test-project").unwrap();
        let id = manifest.add_file("to-delete.md", FileType::Note).unwrap();
        assert_eq!(manifest.list_files().unwrap().len(), 1);
        manifest.remove_file(&id).unwrap();
        assert_eq!(manifest.list_files().unwrap().len(), 0);
    }

    #[test]
    fn test_save_and_load() {
        let mut manifest = ProjectManifest::new("test-project").unwrap();
        manifest.add_file("hello.md", FileType::Note).unwrap();
        let data = manifest.save();
        let loaded = ProjectManifest::load(&data).unwrap();
        assert_eq!(loaded.name().unwrap(), "test-project");
        assert_eq!(loaded.list_files().unwrap().len(), 1);
    }

    #[test]
    fn test_duplicate_path_rejected() {
        let mut manifest = ProjectManifest::new("test-project").unwrap();
        manifest.add_file("hello.md", FileType::Note).unwrap();
        let result = manifest.add_file("hello.md", FileType::Note);
        assert!(matches!(result, Err(CoreError::FileAlreadyExists(_))));
    }

    #[test]
    fn test_validate_owner_controlled_no_changes() {
        let mut manifest = ProjectManifest::new("test").unwrap();
        manifest.set_owner("owner-node-id").unwrap();
        let heads = manifest.doc.get_heads().to_vec();
        // No new changes — should pass
        assert!(manifest
            .validate_owner_controlled_changes(&heads, "owner-actor-hex")
            .is_ok());
    }
}
