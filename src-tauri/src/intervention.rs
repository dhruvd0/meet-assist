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
const HEIGHT: f64 = 175.0;
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
pub fn create_intervention_window(app: &AppHandle) {
    create_floating(app, LABEL, "src/intervention/index.html", WIDTH, HEIGHT);
}

/// Creates a small always-on-top window that never takes focus, hidden by
/// default. Shared by the intervention card and the floating meeting bar.
#[cfg(not(target_os = "macos"))]
pub(crate) fn create_floating(app: &AppHandle, label: &str, url: &str, width: f64, height: f64) {
    let mut builder = WebviewWindowBuilder::new(app, label, tauri::WebviewUrl::App(url.into()))
        .title("Meet Assist")
        .resizable(false)
        .inner_size(width, height)
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
        Ok(_) => log::debug!("Floating window {label} created (hidden)"),
        Err(e) => log::error!("Failed to create floating window {label}: {e}"),
    }
}

/// macOS: a non-activating floating NSPanel, so showing or clicking it never
/// activates the app.
#[cfg(target_os = "macos")]
pub(crate) fn create_floating(app: &AppHandle, label: &str, url: &str, width: f64, height: f64) {
    match PanelBuilder::<_, InterventionPanel>::new(app, label)
        .url(WebviewUrl::App(url.into()))
        .title("Meet Assist")
        .level(PanelLevel::Floating)
        .size(tauri::Size::Logical(tauri::LogicalSize { width, height }))
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
        Err(e) => log::error!("Failed to create floating panel {label}: {e}"),
    }
}

/// Shows a floating window without taking focus.
pub(crate) fn show_floating(app: &AppHandle, window: &tauri::webview::WebviewWindow) {
    #[cfg(target_os = "macos")]
    match app.get_webview_panel(window.label()) {
        Ok(panel) => panel.show(),
        Err(_) => {
            let _ = window.show();
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        let _ = window.show();
    }
}

/// Moves the window to the top-right of the monitor under the cursor.
fn position_top_right(app: &AppHandle, window: &tauri::webview::WebviewWindow) {
    let Some(monitor) = get_monitor_with_cursor(app) else {
        return;
    };
    let scale = monitor.scale_factor();
    let x = (monitor.position().x as f64 + monitor.size().width as f64) / scale - WIDTH - MARGIN;
    let y = monitor.position().y as f64 / scale + MARGIN;
    let _ = window.set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }));
}

/// The monitor under the mouse cursor, falling back to the primary monitor.
pub(crate) fn get_monitor_with_cursor(app: &AppHandle) -> Option<tauri::Monitor> {
    let primary = app.primary_monitor().ok().flatten();
    if let Ok(cursor) = app.cursor_position() {
        // `cursor_position` and `monitor_from_point` use different spaces off
        // Windows. tao reads the cursor in logical points and scales it by the
        // primary monitor's factor (macos/util `cursor_position`; linux/util
        // uses the GDK scale), but `monitor_from_point` compares against
        // logical bounds (CGDisplayBounds / GDK geometry). Undo that scaling.
        // Windows is physical on both sides.
        #[cfg(not(target_os = "windows"))]
        let (x, y) = {
            let scale = primary.as_ref().map_or(1.0, |m| m.scale_factor());
            (cursor.x / scale, cursor.y / scale)
        };
        #[cfg(target_os = "windows")]
        let (x, y) = (cursor.x, cursor.y);
        if let Ok(Some(monitor)) = app.monitor_from_point(x, y) {
            return Some(monitor);
        }
    }
    primary
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
    present(app, "intervention:show", card_payload(meeting_id, iv));
}

/// Shows the "filed by assistant" card without taking focus.
pub fn show_filed(app: &AppHandle, payload: &crate::meeting::filing::MeetingFiledPayload) {
    let v = serde_json::to_value(payload).unwrap_or_else(|_| serde_json::json!({}));
    present(app, "intervention:filed", v);
}

/// Positions the card, sends it `event` with `payload`, and shows it.
fn present(app: &AppHandle, event: &'static str, payload: serde_json::Value) {
    let handle = app.clone();
    INTERVENTION_GENERATION.fetch_add(1, Ordering::SeqCst);
    let _ = app.run_on_main_thread(move || {
        let Some(window) = handle.get_webview_window(LABEL) else {
            log::warn!("Intervention window missing; cannot show card");
            return;
        };
        position_top_right(&handle, &window);
        let _ = window.emit(event, &payload);
        show_floating(&handle, &window);
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
