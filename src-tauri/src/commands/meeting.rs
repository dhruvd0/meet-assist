use crate::meeting::session::{MeetingManager, MeetingStatePayload};
use crate::meeting::store::{Intervention, Meeting, MeetingDetail};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::Arc;
use tauri::{Manager, State};
use tauri_specta::Event;

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
pub async fn start_meeting(
    mm: Mm<'_>,
    project_id: Option<i64>,
    meeting_id: Option<i64>,
) -> Result<Meeting, String> {
    mm.start(project_id, meeting_id).map_err(err)
}

#[tauri::command]
#[specta::specta]
pub async fn pause_meeting(mm: Mm<'_>) -> Result<(), String> {
    mm.pause().map_err(err)
}

#[tauri::command]
#[specta::specta]
pub async fn resume_meeting(mm: Mm<'_>) -> Result<(), String> {
    mm.resume().map_err(err)
}

#[tauri::command]
#[specta::specta]
/// `None` (or the running meeting's id) ends the running meeting; any other
/// id ends that idle meeting, e.g. one left paused by a restart.
pub async fn end_meeting(mm: Mm<'_>, meeting_id: Option<i64>) -> Result<(), String> {
    match meeting_id {
        Some(id) if mm.state().meeting_id != Some(id) => mm.end_idle(id).map_err(err),
        _ => mm.end().map_err(err),
    }
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

#[tauri::command]
#[specta::specta]
pub fn list_interventions(mm: Mm<'_>, meeting_id: i64) -> Result<Vec<Intervention>, String> {
    mm.store().list_interventions(meeting_id).map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn dismiss_intervention(mm: Mm<'_>, id: i64) -> Result<(), String> {
    mm.store().dismiss_intervention(id).map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn hide_intervention_window(app: tauri::AppHandle) {
    crate::intervention::hide(&app);
}

/// Asks the main window to open a meeting (sent when the user clicks
/// "Open meeting" on the intervention card).
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct OpenMeetingPayload {
    pub meeting_id: i64,
}

#[tauri::command]
#[specta::specta]
pub fn focus_meeting(app: tauri::AppHandle, meeting_id: i64) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    OpenMeetingPayload { meeting_id }
        .emit(&app)
        .map_err(|e| e.to_string())?;
    crate::intervention::hide(&app);
    Ok(())
}

/// Current observer readiness, so a view opened after the last
/// `ObserverStatusPayload` (emitted only on change) still knows it.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct ObserverReadiness {
    pub readiness: String,
    pub detail: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub fn get_observer_readiness(app: tauri::AppHandle) -> ObserverReadiness {
    let r = crate::meeting::observer::readiness(&crate::settings::get_settings(&app));
    ObserverReadiness {
        readiness: r.code().to_string(),
        detail: r.detail(),
    }
}

/// Summarize a session in the background (also the UI's Retry). The result
/// arrives as a `SessionSummaryPayload` event.
#[tauri::command]
#[specta::specta]
pub fn summarize_session(mm: Mm<'_>, session_id: i64) -> Result<(), String> {
    mm.spawn_summary(session_id);
    Ok(())
}
