//! Floating meeting bar: while a meeting runs and the main window is not in
//! front, a small always-on-top copy of the in-app live dock (title, time,
//! assistant status, pause/resume, end) so the meeting stays in reach from
//! the call. It never takes focus, and hides when the main window comes back.

use crate::meeting::session::MeetingManager;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

const LABEL: &str = "pill";
const WIDTH: f64 = 460.0;
const HEIGHT: f64 = 72.0;
/// Gap below the menu bar / top of the work area.
const MARGIN: f64 = 8.0;

/// Whether the main window has focus; set from its focus events. Starts
/// false: the app can start hidden in the tray, with no focus event.
static MAIN_FOCUSED: AtomicBool = AtomicBool::new(false);

pub fn create_window(app: &AppHandle) {
    crate::intervention::create_floating(app, LABEL, "src/pill/index.html", WIDTH, HEIGHT);
}

/// Record the main window's focus and update the bar.
pub fn main_focus_changed(app: &AppHandle, focused: bool) {
    MAIN_FOCUSED.store(focused, Ordering::SeqCst);
    sync(app);
}

/// Show the bar while a meeting is running or paused and the main window is
/// not in front; hide it otherwise.
pub fn sync(app: &AppHandle) {
    let running = app
        .try_state::<Arc<MeetingManager>>()
        .is_some_and(|mm| mm.state().meeting_id.is_some());
    let show = running && !MAIN_FOCUSED.load(Ordering::SeqCst);
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Some(window) = handle.get_webview_window(LABEL) else {
            return;
        };
        if !show {
            let _ = window.hide();
            return;
        }
        if window.is_visible().unwrap_or(false) {
            return;
        }
        position_top_center(&handle, &window);
        crate::intervention::show_floating(&handle, &window);
    });
}

/// Centre the bar at the top of the work area of the monitor under the cursor.
fn position_top_center(app: &AppHandle, window: &tauri::webview::WebviewWindow) {
    let Some(monitor) = crate::intervention::get_monitor_with_cursor(app) else {
        return;
    };
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let x = (area.position.x as f64 + area.size.width as f64 / 2.0) / scale - WIDTH / 2.0;
    let y = area.position.y as f64 / scale + MARGIN;
    let _ = window.set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }));
}
