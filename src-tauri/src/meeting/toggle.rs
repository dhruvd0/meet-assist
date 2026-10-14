//! One entry point for every "start or end a meeting" trigger: the global
//! shortcut, the tray, `--toggle-meeting`, and SIGUSR2.

use std::sync::Arc;

use log::{error, info};
use tauri::{AppHandle, Emitter, Manager};

use crate::meeting::session::MeetingManager;

/// Ends the running meeting (live or paused), otherwise starts a new one in
/// the most recently used project. With no projects, opens the window on
/// Projects instead of creating an orphan meeting.
pub fn toggle_meeting(app: &AppHandle) {
    let Some(mm) = app.try_state::<Arc<MeetingManager>>() else {
        error!("toggle_meeting: MeetingManager not initialised");
        return;
    };
    let mm = mm.inner().clone();
    let app = app.clone();
    // Start/end do blocking capture and store work; keep it off the shortcut
    // and main threads.
    std::thread::spawn(move || {
        if mm.state().meeting_id.is_some() {
            if let Err(e) = mm.end() {
                error!("toggle_meeting: end failed: {e:#}");
            }
            return;
        }
        match last_used_project(&mm) {
            Some(project_id) => match mm.start(Some(project_id), None) {
                Ok(m) => info!("toggle_meeting: started meeting {}", m.id),
                Err(e) => {
                    error!("toggle_meeting: start failed: {e:#}");
                    crate::show_main_window(&app);
                }
            },
            None => {
                crate::show_main_window(&app);
                let _ = app.emit("navigate-section", "projects");
            }
        }
    });
}

/// Project of the most recently updated meeting, else the most recently
/// updated project. Both lists are already ordered `updated_at DESC`.
fn last_used_project(mm: &MeetingManager) -> Option<i64> {
    let store = mm.store();
    if let Ok(meetings) = store.list_meetings(None) {
        if let Some(pid) = meetings.iter().find_map(|m| m.project_id) {
            return Some(pid);
        }
    }
    store.list_projects().ok()?.first().map(|p| p.id)
}
