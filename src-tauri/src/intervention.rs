//! Floating intervention card shown during a live meeting. A small always-on-top,
//! non-focusable window (an NSPanel on macOS) so it never steals focus from the call.

use crate::meeting::store::Intervention;
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, Emitter, Manager};

#[cfg(not(target_os = "macos"))]
use tauri::WebviewWindowBuilder;

#[cfg(target_os = "macos")]
use tauri::WebviewUrl;

#[cfg(target_os = "macos")]
use tauri_nspanel::{
    tauri_panel, CollectionBehavior, ManagerExt, PanelBuilder, PanelLevel, StyleMask,
};

const LABEL: &str = "intervention";
const WIDTH: f64 = 380.0;
const HEIGHT: f64 = 150.0;
const MARGIN: f64 = 16.0;

/// Bumped on every show so a hide scheduled for an earlier card cannot hide a newer one.
static INTERVENTION_GENERATION: AtomicU64 = AtomicU64::new(0);

#[cfg(target_os = "macos")]
tauri_panel! {
    panel!(InterventionPanel {
        config: {
            can_become_key_window: false,
            is_floating_panel: true
        }
    })
}

/// Creates the intervention window, hidden by default.
#[cfg(not(target_os = "macos"))]
pub fn create_intervention_window(app: &AppHandle) {
    let mut builder = WebviewWindowBuilder::new(
        app,
        LABEL,
        tauri::WebviewUrl::App("src/intervention/index.html".into()),
    )
    .title("Meet Assist")
    .resizable(false)
    .inner_size(WIDTH, HEIGHT)
    .shadow(true)
    .maximizable(false)
    .minimizable(false)
    .accept_first_mouse(true)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .transparent(true)
    .focusable(false)
    .focused(false)
    .visible(false);

    if let Some(data_dir) = crate::portable::data_dir() {
        builder = builder.data_directory(data_dir.join("webview"));
    }

    match builder.build() {
        Ok(_) => log::debug!("Intervention window created (hidden)"),
        Err(e) => log::error!("Failed to create intervention window: {e}"),
    }
}

/// Creates the intervention panel, hidden by default (macOS). A non-activating
/// floating NSPanel so showing the card never activates the app.
#[cfg(target_os = "macos")]
pub fn create_intervention_window(app: &AppHandle) {
    match PanelBuilder::<_, InterventionPanel>::new(app, LABEL)
        .url(WebviewUrl::App("src/intervention/index.html".into()))
        .title("Meet Assist")
        .level(PanelLevel::Floating)
        .size(tauri::Size::Logical(tauri::LogicalSize {
            width: WIDTH,
            height: HEIGHT,
        }))
        .has_shadow(true)
        .transparent(true)
        .no_activate(true)
        .corner_radius(0.0)
        .style_mask(StyleMask::empty().borderless().nonactivating_panel())
        .with_window(|w| w.decorations(false).transparent(true).focusable(false))
        .collection_behavior(
            CollectionBehavior::new()
                .can_join_all_spaces()
                .full_screen_auxiliary(),
        )
        .build()
    {
        Ok(panel) => panel.hide(),
        Err(e) => log::error!("Failed to create intervention panel: {e}"),
    }
}

/// Moves the window to the top-right of the monitor under the cursor.
fn position_top_right(app: &AppHandle, window: &tauri::webview::WebviewWindow) {
    let Some(monitor) = crate::overlay::get_monitor_with_cursor(app) else {
        return;
    };
    let scale = monitor.scale_factor();
    let x = (monitor.position().x as f64 + monitor.size().width as f64) / scale - WIDTH - MARGIN;
    let y = monitor.position().y as f64 / scale + MARGIN;
    let _ = window.set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }));
}

/// Card payload: the intervention's fields plus `meeting_id`, so the card can
/// open its meeting.
fn card_payload(meeting_id: i64, iv: &Intervention) -> serde_json::Value {
    let mut v = serde_json::to_value(iv).unwrap_or_else(|_| serde_json::json!({}));
    if let Some(obj) = v.as_object_mut() {
        obj.insert("meeting_id".into(), serde_json::json!(meeting_id));
    }
    v
}

/// Shows the card for `iv` (from meeting `meeting_id`) without taking focus.
pub fn show(app: &AppHandle, meeting_id: i64, iv: &Intervention) {
    let handle = app.clone();
    let iv = card_payload(meeting_id, iv);
    INTERVENTION_GENERATION.fetch_add(1, Ordering::SeqCst);
    let _ = app.run_on_main_thread(move || {
        let Some(window) = handle.get_webview_window(LABEL) else {
            log::warn!("Intervention window missing; cannot show card");
            return;
        };
        position_top_right(&handle, &window);
        let _ = window.emit("intervention:show", &iv);
        #[cfg(target_os = "macos")]
        match handle.get_webview_panel(LABEL) {
            Ok(panel) => panel.show(),
            Err(_) => {
                let _ = window.show();
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = window.show();
    });
}

/// Hides the card. Ignored if a newer card was shown while the hide was queued.
pub fn hide(app: &AppHandle) {
    let handle = app.clone();
    let scheduled_at = INTERVENTION_GENERATION.load(Ordering::SeqCst);
    let _ = app.run_on_main_thread(move || {
        if INTERVENTION_GENERATION.load(Ordering::SeqCst) != scheduled_at {
            return;
        }
        if let Some(window) = handle.get_webview_window(LABEL) {
            let _ = window.hide();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_payload_merges_meeting_id_into_intervention_fields() {
        let iv = Intervention {
            id: 7,
            session_id: 3,
            ts: 1000,
            kind: "loop".into(),
            text: "t".into(),
            reason: "r".into(),
            dismissed: false,
        };
        let v = card_payload(42, &iv);
        assert_eq!(v["meeting_id"], 42);
        assert_eq!(v["id"], 7);
        assert_eq!(v["kind"], "loop");
        assert_eq!(v["text"], "t");
    }
}
