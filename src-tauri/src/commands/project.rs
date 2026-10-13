use crate::meeting::manager::ProjectManager;
use crate::meeting::store::{Project, SearchHit, Source, SourceKind};
use std::sync::Arc;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

type Pm<'a> = State<'a, Arc<ProjectManager>>;

fn err(e: anyhow::Error) -> String {
    e.to_string()
}

#[tauri::command]
#[specta::specta]
pub fn list_projects(pm: Pm<'_>) -> Result<Vec<Project>, String> {
    pm.store().list_projects().map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn get_project(pm: Pm<'_>, id: i64) -> Result<Option<Project>, String> {
    pm.store().get_project(id).map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn create_project(pm: Pm<'_>, title: String) -> Result<Project, String> {
    pm.store().create_project(title.trim()).map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn update_project_title(pm: Pm<'_>, id: i64, title: String) -> Result<(), String> {
    pm.store()
        .update_project_title(id, title.trim())
        .map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn update_project_brief(pm: Pm<'_>, id: i64, brief: String) -> Result<(), String> {
    pm.store().update_project_brief(id, &brief).map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn delete_project(pm: Pm<'_>, id: i64) -> Result<(), String> {
    pm.delete_project(id).map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn list_sources(pm: Pm<'_>, project_id: i64) -> Result<Vec<Source>, String> {
    pm.store().list_sources(project_id).map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn add_source(
    pm: Pm<'_>,
    project_id: i64,
    kind: SourceKind,
    title: String,
    locator: String,
) -> Result<Source, String> {
    pm.add_and_index(project_id, kind, title.trim(), &locator)
        .map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn remove_source(pm: Pm<'_>, id: i64) -> Result<(), String> {
    pm.remove_source(id).map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn reindex_source(pm: Pm<'_>, id: i64) -> Result<(), String> {
    pm.enqueue_index(id);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn set_source_caption(pm: Pm<'_>, id: i64, caption: String) -> Result<(), String> {
    pm.store()
        .set_source_digest(id, caption.trim())
        .map_err(err)?;
    pm.enqueue_index(id);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn search_sources(
    pm: Pm<'_>,
    project_id: i64,
    query: String,
    limit: Option<u32>,
) -> Result<Vec<SearchHit>, String> {
    pm.store()
        .search(project_id, &query, limit.unwrap_or(8) as usize)
        .map_err(err)
}

/// Native picker. `kind` is `file`, `folder`, or `image`.
#[tauri::command]
#[specta::specta]
pub async fn pick_source_path(app: AppHandle, kind: SourceKind) -> Result<Option<String>, String> {
    let dialog = app.dialog().file();
    let picked = match kind {
        SourceKind::Folder => dialog.blocking_pick_folder(),
        SourceKind::Image => dialog
            .add_filter("Images", &["png", "jpg", "jpeg", "gif", "webp", "bmp"])
            .blocking_pick_file(),
        _ => dialog.blocking_pick_file(),
    };
    Ok(picked.and_then(|p| p.as_path().map(|p| p.to_string_lossy().to_string())))
}
