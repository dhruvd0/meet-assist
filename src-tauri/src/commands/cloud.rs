use crate::cloud::service::{CloudService, CloudStatus};
use std::sync::Arc;
use tauri::State;

type Cloud<'a> = State<'a, Arc<CloudService>>;

#[tauri::command]
#[specta::specta]
pub fn cloud_status(cloud: Cloud<'_>) -> CloudStatus {
    cloud.status()
}

#[tauri::command]
#[specta::specta]
pub async fn cloud_sign_in(
    cloud: Cloud<'_>,
    email: String,
    password: String,
) -> Result<CloudStatus, String> {
    cloud.sign_in(&email, &password, false).await
}

#[tauri::command]
#[specta::specta]
pub async fn cloud_sign_up(
    cloud: Cloud<'_>,
    email: String,
    password: String,
) -> Result<CloudStatus, String> {
    cloud.sign_in(&email, &password, true).await
}

#[tauri::command]
#[specta::specta]
pub async fn cloud_sign_out(cloud: Cloud<'_>) -> Result<CloudStatus, String> {
    Ok(cloud.sign_out().await)
}

#[tauri::command]
#[specta::specta]
pub fn cloud_sync_now(cloud: Cloud<'_>) {
    cloud.sync_now();
}

/// Turn recording backup on or off for this device.
#[tauri::command]
#[specta::specta]
pub fn change_cloud_audio_backup_setting(
    app: tauri::AppHandle,
    cloud: Cloud<'_>,
    enabled: bool,
) -> Result<(), String> {
    let mut settings = crate::settings::get_settings(&app);
    settings.cloud_audio_backup_enabled = enabled;
    crate::settings::write_settings(&app, settings);
    if enabled {
        cloud.resume_audio_backup();
    }
    Ok(())
}
