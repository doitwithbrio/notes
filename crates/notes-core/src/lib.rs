pub mod blame;
pub mod doc_store;
pub mod editor_doc;
pub mod editor_migration;
pub mod editor_schema;
pub mod editor_text;
pub mod error;
pub mod invite_state;
pub mod manifest;
pub mod persistence;
pub mod project;
pub mod search;
pub mod seen_state;
pub mod settings;
pub mod types;
pub mod validation;
pub mod version;
pub mod version_store;

pub use blame::get_document_blame;
pub use doc_store::DocStore;
pub use editor_doc::{new_empty_document, new_paragraph_node, new_text_node};
pub use editor_migration::{ensure_v2_document, migrate_legacy_text_to_v2};
pub use editor_schema::{
    validate_document, EditorDocument, EditorMark, EditorNode, KnownMarkKind, KnownNodeKind,
    ValidationMode, EDITOR_SCHEMA_VERSION,
};
pub use editor_text::visible_text;
pub use error::CoreError;
pub use invite_state::{
    JoinSessionStore, OwnerInviteStateStore, PersistedJoinSecret, PersistedJoinSession,
    PersistedJoinStage, PersistedOwnerInvitePhase, PersistedOwnerInviteRecord,
};
pub use manifest::ProjectManifest;
pub use project::ProjectManager;
pub use search::{SearchIndex, SearchResult};
pub use seen_state::{ProjectSeenState, SeenStateManager, UnseenDocInfo};
pub use settings::{AppSettings, DegradationLevel};
pub use types::*;
pub use validation::{validate_note_path, validate_project_name, validate_relative_path};
pub use version::{Version, VersionSignificance, VersionType};
pub use version_store::VersionStore;
