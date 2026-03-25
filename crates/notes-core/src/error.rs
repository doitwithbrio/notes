use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum CoreError {
    #[error("Document not found: {0}")]
    DocNotFound(Uuid),

    #[error("Document already exists: {0}")]
    DocAlreadyExists(Uuid),

    #[error("Project not found: {0}")]
    ProjectNotFound(String),

    #[error("Project already exists: {0}")]
    ProjectAlreadyExists(String),

    #[error("File already exists at path: {0}")]
    FileAlreadyExists(String),

    #[error("Automerge error: {0}")]
    Automerge(#[from] automerge::AutomergeError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Invalid document data: {0}")]
    InvalidData(String),

    #[error("Resource exhausted: {0}")]
    ResourceExhausted(String),
}

/// Structured error serialization for Tauri IPC.
/// Frontend receives `{ code: "DOC_NOT_FOUND", message: "Document not found: ..." }`
/// instead of a flat string.
impl serde::Serialize for CoreError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(2))?;

        let code = match self {
            CoreError::DocNotFound(_) => "DOC_NOT_FOUND",
            CoreError::DocAlreadyExists(_) => "DOC_ALREADY_EXISTS",
            CoreError::ProjectNotFound(_) => "PROJECT_NOT_FOUND",
            CoreError::ProjectAlreadyExists(_) => "PROJECT_ALREADY_EXISTS",
            CoreError::FileAlreadyExists(_) => "FILE_ALREADY_EXISTS",
            CoreError::Automerge(_) => "AUTOMERGE_ERROR",
            CoreError::Io(_) => "IO_ERROR",
            CoreError::Serde(_) => "SERDE_ERROR",
            CoreError::InvalidInput(_) => "INVALID_INPUT",
            CoreError::InvalidData(_) => "INVALID_DATA",
            CoreError::ResourceExhausted(_) => "RESOURCE_EXHAUSTED",
        };

        map.serialize_entry("code", code)?;

        // Sanitize error messages before sending to frontend.
        // Raw IO/Automerge/Serde errors can contain filesystem paths, internal state,
        // or peer addresses that should not be exposed to the webview.
        let message = match self {
            CoreError::Io(e) => {
                log::error!("IO error (sanitized for frontend): {e}");
                "A file system error occurred".to_string()
            }
            CoreError::Automerge(e) => {
                log::error!("Automerge error (sanitized for frontend): {e}");
                "A document processing error occurred".to_string()
            }
            CoreError::Serde(e) => {
                log::error!("Serialization error (sanitized for frontend): {e}");
                "A data format error occurred".to_string()
            }
            other => other.to_string(),
        };
        map.serialize_entry("message", &message)?;
        map.end()
    }
}
