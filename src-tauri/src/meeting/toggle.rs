//! One entry point for every "start or end a meeting" trigger: the global
//! shortcut, the tray, `--toggle-meeting`, and SIGUSR2.

use std::sync::Arc;

use log::{error, info};
use tauri::{AppHandle, Emitter, Manager};

use crate::meeting::session::MeetingManager;

/// Ends the running meeting (live or paused), otherwise starts a new,
/// unfiled one. The assistant files it into a project once it has enough
/// transcript (see `meeting::filing`).
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
        match mm.start(None, None) {
            Ok(m) => info!("toggle_meeting: started meeting {}", m.id),
            Err(e) => {
                // A double press can race another start; the meeting the
                // user wanted is running, so there is nothing to report.
                if mm.state().meeting_id.is_some() {
                    info!("toggle_meeting: start skipped, a meeting is already running");
                    return;
                }
                error!("toggle_meeting: start failed: {e:#}");
                crate::show_main_window(&app);
                let _ = app.emit("meeting-toggle-error", format!("{e:#}"));
            }
        }
    });
}
