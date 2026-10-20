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
