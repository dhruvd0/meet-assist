//! Shared shortcut event handling for the Tauri and native-keys backends.

use log::warn;
use tauri::AppHandle;

/// The only binding is `toggle_meeting`; it fires on press.
pub fn handle_shortcut_event(
    app: &AppHandle,
    binding_id: &str,
    hotkey_string: &str,
    is_pressed: bool,
) {
    if binding_id == "toggle_meeting" {
        if is_pressed {
            crate::meeting::toggle::toggle_meeting(app);
        }
        return;
    }
    warn!("No handler for shortcut '{binding_id}' ({hotkey_string})");
}
