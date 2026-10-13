//! Intervention window. Task 6 replaces these stubs.

pub fn show(_app: &tauri::AppHandle, iv: &crate::meeting::store::Intervention) {
    log::info!("intervention: [{}] {}", iv.kind, iv.text);
}

pub fn hide(_app: &tauri::AppHandle) {}
