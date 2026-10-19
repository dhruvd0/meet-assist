use crate::meeting::filing::MeetingFiledPayload;
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

/// File path of a transcript line's recorded audio, if one was saved.
#[tauri::command]
#[specta::specta]
pub fn get_line_audio_path(mm: Mm<'_>, line_id: i64) -> Result<Option<String>, String> {
    mm.store()
        .line_audio_path(line_id)
        .map(|p| p.map(|p| p.to_string_lossy().into_owned()))
        .map_err(err)
}

/// The user's choice of project for a meeting (`None` = Unfiled).
#[tauri::command]
#[specta::specta]
pub fn move_meeting(
    app: tauri::AppHandle,
    mm: Mm<'_>,
    meeting_id: i64,
    project_id: Option<i64>,
) -> Result<(), String> {
    mm.store()
        .move_meeting(meeting_id, project_id)
        .map_err(err)?;
    mm.set_active_project(meeting_id, project_id);
    emit_user_filed(&app, &mm, meeting_id, project_id);
    Ok(())
}

/// Undo from the filed card: back to Unfiled, and delete the project the
/// assistant created if nobody has used it yet.
#[tauri::command]
#[specta::specta]
pub fn undo_filing(
    app: tauri::AppHandle,
    mm: Mm<'_>,
    meeting_id: i64,
    created_project_id: Option<i64>,
    created_brief: Option<String>,
) -> Result<(), String> {
    mm.store()
        .undo_filing(meeting_id, created_project_id, created_brief.as_deref())
        .map_err(err)?;
    mm.set_active_project(meeting_id, None);
    emit_user_filed(&app, &mm, meeting_id, None);
    crate::intervention::hide(&app);
    Ok(())
}

fn emit_user_filed(
    app: &tauri::AppHandle,
    mm: &MeetingManager,
    meeting_id: i64,
    project_id: Option<i64>,
) {
    let project_title = project_id
        .and_then(|p| mm.store().get_project(p).ok().flatten())
        .map(|p| p.title);
    let meeting_title = mm
        .store()
        .get_meeting(meeting_id)
        .ok()
        .flatten()
        .map(|m| m.title)
        .unwrap_or_default();
    let payload = MeetingFiledPayload {
        meeting_id,
        project_id,
        project_title,
        meeting_title,
        by: crate::meeting::store::FILED_BY_USER.into(),
        created_project: false,
        created_brief: None,
    };
    if let Err(e) = payload.emit(app) {
        log::warn!("emit meeting filed: {e}");
    }
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

/// Summarize a session (the UI's Retry) and resolve when it is done, so the
/// UI can show progress. The summary itself arrives as a
/// `SessionSummaryPayload` event.
#[tauri::command]
#[specta::specta]
pub async fn summarize_session(mm: Mm<'_>, session_id: i64) -> Result<(), String> {
    let mm = Arc::clone(&mm);
    tauri::async_runtime::spawn_blocking(move || mm.summarize_session_now(session_id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
#[specta::specta]
pub async fn start_text_tester(mm: Mm<'_>) -> Result<Meeting, String> {
    mm.start_text_tester().map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn append_tester_turn(
    mm: Mm<'_>,
    meeting_id: i64,
    session_id: i64,
    speaker: crate::meeting::tester::TesterSpeaker,
    text: String,
) -> Result<crate::meeting::store::TranscriptLine, String> {
    mm.append_tester_turn(meeting_id, session_id, speaker, text)
        .map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn request_tester_check(mm: Mm<'_>, meeting_id: i64, session_id: i64) -> Result<(), String> {
    mm.request_tester_check(meeting_id, session_id).map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn get_tester_trace(
    mm: Mm<'_>,
    meeting_id: i64,
    session_id: i64,
) -> Vec<crate::meeting::tester::TesterTrace> {
    mm.tester_trace(meeting_id, session_id)
}
