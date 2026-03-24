use std::sync::Arc;

use notes_core::{CoreError, DocId, DocInfo, ProjectManager};
use tauri::{Manager, State};

/// Shared app state accessible from all Tauri commands.
struct AppState {
    project_manager: Arc<ProjectManager>,
}

// ── Project Commands ─────────────────────────────────────────────────

#[tauri::command]
async fn list_projects(state: State<'_, AppState>) -> Result<Vec<String>, CoreError> {
    state.project_manager.list_projects().await
}

#[tauri::command]
async fn create_project(
    state: State<'_, AppState>,
    name: String,
) -> Result<(), CoreError> {
    state.project_manager.create_project(&name).await
}

#[tauri::command]
async fn open_project(
    state: State<'_, AppState>,
    name: String,
) -> Result<(), CoreError> {
    state.project_manager.open_project(&name).await
}

// ── Document Commands ────────────────────────────────────────────────

#[tauri::command]
async fn list_files(
    state: State<'_, AppState>,
    project: String,
) -> Result<Vec<DocInfo>, CoreError> {
    state.project_manager.list_files(&project).await
}

#[tauri::command]
async fn create_note(
    state: State<'_, AppState>,
    project: String,
    path: String,
) -> Result<DocId, CoreError> {
    state.project_manager.create_note(&project, &path).await
}

#[tauri::command]
async fn open_doc(
    state: State<'_, AppState>,
    project: String,
    doc_id: DocId,
) -> Result<(), CoreError> {
    state.project_manager.open_doc(&project, &doc_id).await
}

#[tauri::command]
async fn close_doc(
    state: State<'_, AppState>,
    project: String,
    doc_id: DocId,
) -> Result<(), CoreError> {
    state.project_manager.close_doc(&project, &doc_id).await
}

#[tauri::command]
async fn delete_note(
    state: State<'_, AppState>,
    project: String,
    doc_id: DocId,
) -> Result<(), CoreError> {
    state.project_manager.delete_note(&project, &doc_id).await
}

#[tauri::command]
async fn rename_note(
    state: State<'_, AppState>,
    project: String,
    doc_id: DocId,
    new_path: String,
) -> Result<(), CoreError> {
    state
        .project_manager
        .rename_note(&project, &doc_id, &new_path)
        .await
}

/// Get the full Automerge binary for a document.
/// The frontend uses this to initialize its WASM Automerge instance.
#[tauri::command]
async fn get_doc_binary(
    state: State<'_, AppState>,
    project: String,
    doc_id: DocId,
) -> Result<Vec<u8>, CoreError> {
    // Ensure the doc is loaded
    state.project_manager.open_doc(&project, &doc_id).await?;
    state.project_manager.get_doc_binary(&doc_id).await
}

/// Get plain text content of a document (for preview/search).
#[tauri::command]
async fn get_doc_text(
    state: State<'_, AppState>,
    project: String,
    doc_id: DocId,
) -> Result<String, CoreError> {
    state.project_manager.open_doc(&project, &doc_id).await?;
    state.project_manager.get_doc_text(&doc_id).await
}

/// Apply incremental Automerge changes from the frontend WASM instance.
/// This is called in batches (every 100-500ms) during editing.
#[tauri::command]
async fn apply_changes(
    state: State<'_, AppState>,
    project: String,
    doc_id: DocId,
    data: Vec<u8>,
) -> Result<(), CoreError> {
    state
        .project_manager
        .apply_changes(&project, &doc_id, &data)
        .await
}

/// Compact a document to reduce memory/storage usage.
#[tauri::command]
async fn compact_doc(
    state: State<'_, AppState>,
    project: String,
    doc_id: DocId,
) -> Result<(), CoreError> {
    state
        .project_manager
        .compact_doc(&project, &doc_id)
        .await
}

// ── App Setup ────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // Set up logging
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Debug)
                        .build(),
                )?;
            }

            // Determine the notes directory.
            // Default: ~/Notes (can be made configurable later).
            let notes_dir = dirs::home_dir()
                .expect("could not determine home directory")
                .join("Notes");

            // Ensure the base directory exists
            std::fs::create_dir_all(&notes_dir)
                .expect("could not create Notes directory");

            log::info!("Notes directory: {}", notes_dir.display());

            // Create the project manager
            let project_manager = Arc::new(ProjectManager::new(notes_dir));

            // Store in Tauri state
            app.manage(AppState { project_manager });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_projects,
            create_project,
            open_project,
            list_files,
            create_note,
            open_doc,
            close_doc,
            delete_note,
            rename_note,
            get_doc_binary,
            get_doc_text,
            apply_changes,
            compact_doc,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
