use crate::meeting::session::{MeetingManager, MeetingStatePayload};
use crate::meeting::store::{Meeting, MeetingDetail};
use std::sync::Arc;
use tauri::State;

type Mm<'a> = State<'a, Arc<MeetingManager>>;

fn err(e: anyhow::Error) -> String {
    format!("{e:#}")
}

#[tauri::command]
#[specta::specta]
pub fn list_meetings(mm: Mm<'_>, project_id: Option<i64>) -> Result<Vec<Meeting>, String> {
    mm.store().list_meetings(project_id).map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn get_meeting_detail(mm: Mm<'_>, id: i64) -> Result<Option<MeetingDetail>, String> {
    mm.store().get_meeting_detail(id).map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn start_meeting(
    mm: Mm<'_>,
    project_id: Option<i64>,
    meeting_id: Option<i64>,
) -> Result<Meeting, String> {
    mm.start(project_id, meeting_id).map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn pause_meeting(mm: Mm<'_>) -> Result<(), String> {
    mm.pause().map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn resume_meeting(mm: Mm<'_>) -> Result<(), String> {
    mm.resume().map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn end_meeting(mm: Mm<'_>) -> Result<(), String> {
    mm.end().map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn meeting_state(mm: Mm<'_>) -> MeetingStatePayload {
    mm.state()
}

#[tauri::command]
#[specta::specta]
pub fn update_meeting_title(mm: Mm<'_>, id: i64, title: String) -> Result<(), String> {
    let t = title.trim();
    if t.is_empty() {
        return Err("Title cannot be empty".into());
    }
    mm.store().update_meeting_title(id, t).map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn update_meeting_context(mm: Mm<'_>, id: i64, context: String) -> Result<(), String> {
    mm.store().update_meeting_context(id, &context).map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn delete_meeting(mm: Mm<'_>, id: i64) -> Result<(), String> {
    if mm.state().meeting_id == Some(id) {
        return Err("End the meeting before deleting it".into());
    }
    mm.store().delete_meeting(id).map_err(err)
}
