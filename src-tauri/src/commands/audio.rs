use crate::audio_feedback;
use crate::audio_toolkit::audio::list_output_devices;
use crate::settings::{get_settings, write_settings};
use log::warn;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::AppHandle;

#[cfg(target_os = "windows")]
use winreg::{
    enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE},
    RegKey, HKEY,
};

#[derive(Serialize, Type)]
pub struct CustomSounds {
    start: bool,
    stop: bool,
}

fn custom_sound_exists(app: &AppHandle, sound_type: &str) -> bool {
    crate::portable::resolve_app_data(app, &format!("custom_{}.wav", sound_type))
        .is_ok_and(|path| path.exists())
}

#[tauri::command]
#[specta::specta]
pub fn check_custom_sounds(app: AppHandle) -> CustomSounds {
    CustomSounds {
        start: custom_sound_exists(&app, "start"),
        stop: custom_sound_exists(&app, "stop"),
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct AudioDevice {
    pub index: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum PermissionAccess {
    Allowed,
    Denied,
    Unknown,
}

#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct WindowsMicrophonePermissionStatus {
    pub supported: bool,
    pub overall_access: PermissionAccess,
    pub device_access: PermissionAccess,
    pub app_access: PermissionAccess,
    pub desktop_app_access: PermissionAccess,
}

#[cfg(target_os = "windows")]
fn read_registry_permission_access(root_hkey: HKEY, path: &str) -> PermissionAccess {
    let root = RegKey::predef(root_hkey);
    let Ok(key) = root.open_subkey(path) else {
        return PermissionAccess::Unknown;
    };

    let Ok(value) = key.get_value::<String, _>("Value") else {
        return PermissionAccess::Unknown;
    };

    match value.to_ascii_lowercase().as_str() {
        "allow" => PermissionAccess::Allowed,
        "deny" => PermissionAccess::Denied,
        _ => PermissionAccess::Unknown,
    }
}

#[cfg(target_os = "windows")]
fn get_windows_microphone_permission_status_impl() -> WindowsMicrophonePermissionStatus {
    const MICROPHONE_PATH: &str =
        "Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\microphone";
    const DESKTOP_APPS_PATH: &str =
        "Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\microphone\\NonPackaged";

    let device_access = read_registry_permission_access(HKEY_LOCAL_MACHINE, MICROPHONE_PATH);
    let app_access = read_registry_permission_access(HKEY_CURRENT_USER, MICROPHONE_PATH);
    let desktop_app_access = read_registry_permission_access(HKEY_CURRENT_USER, DESKTOP_APPS_PATH);

    // Handy is a desktop app, so the NonPackaged key (desktop_app_access) is
    // the relevant permission scope. The UWP master key (app_access) can be
    // "deny" on systems with debloaters (e.g. O&O ShutUp10) without actually
    // blocking desktop app microphone access.
    let overall_access = if device_access == PermissionAccess::Denied {
        PermissionAccess::Denied
    } else if desktop_app_access == PermissionAccess::Denied {
        PermissionAccess::Denied
    } else if desktop_app_access == PermissionAccess::Allowed {
        PermissionAccess::Allowed
    } else if app_access == PermissionAccess::Denied {
        PermissionAccess::Denied
    } else if device_access == PermissionAccess::Allowed && app_access == PermissionAccess::Allowed
    {
        PermissionAccess::Allowed
    } else {
        PermissionAccess::Unknown
    };

    WindowsMicrophonePermissionStatus {
        supported: true,
        overall_access,
        device_access,
        app_access,
        desktop_app_access,
    }
}

#[tauri::command]
#[specta::specta]
pub fn get_windows_microphone_permission_status() -> WindowsMicrophonePermissionStatus {
    #[cfg(target_os = "windows")]
    {
        get_windows_microphone_permission_status_impl()
    }

    #[cfg(not(target_os = "windows"))]
    {
        WindowsMicrophonePermissionStatus {
            supported: false,
            overall_access: PermissionAccess::Unknown,
            device_access: PermissionAccess::Unknown,
            app_access: PermissionAccess::Unknown,
            desktop_app_access: PermissionAccess::Unknown,
        }
    }
}

#[tauri::command]
#[specta::specta]
pub fn open_microphone_privacy_settings() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        Command::new("cmd")
            .args(["/C", "start", "", "ms-settings:privacy-microphone"])
            .spawn()
            .map_err(|e| format!("Failed to open Windows microphone privacy settings: {}", e))?;
        return Ok(());
    }

    #[cfg(not(target_os = "windows"))]
    {
        Err("Opening microphone privacy settings is only supported on Windows".to_string())
    }
}

#[tauri::command]
#[specta::specta]
pub async fn get_available_microphones() -> Result<Vec<AudioDevice>, String> {
    // cpal device enumeration can stall — run it off the webview/main run loop.
    tokio::task::spawn_blocking(|| {
        let devices = handy_recorder::list_input_devices()
            .map_err(|e| format!("Failed to list audio devices: {}", e))?;

        let mut result = vec![AudioDevice {
            index: "default".to_string(),
            name: "Default".to_string(),
            is_default: true,
        }];

        // PulseAudio monitor sources record a speaker's output, not a microphone.
        result.extend(
            devices
                .into_iter()
                .filter(|d| !d.is_monitor)
                .map(|d| AudioDevice {
                    index: d.id,
                    name: d.name,
                    is_default: false, // The explicit default is handled separately
                }),
        );

        Ok::<_, String>(result)
    })
    .await
    .map_err(|e| format!("audio task join failed: {}", e))?
}

#[tauri::command]
#[specta::specta]
pub fn set_selected_microphone(app: AppHandle, device_name: String) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.selected_microphone = if device_name == "default" {
        None
    } else {
        Some(device_name)
    };
    // Meeting capture reads the setting when it starts.
    write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn get_selected_microphone(app: AppHandle) -> Result<String, String> {
    let settings = get_settings(&app);
    Ok(settings
        .selected_microphone
        .unwrap_or_else(|| "default".to_string()))
}

#[tauri::command]
#[specta::specta]
pub async fn get_available_output_devices() -> Result<Vec<AudioDevice>, String> {
    // cpal device enumeration can stall — run it off the webview/main run loop.
    tokio::task::spawn_blocking(|| {
        let devices =
            list_output_devices().map_err(|e| format!("Failed to list output devices: {}", e))?;

        let mut result = vec![AudioDevice {
            index: "default".to_string(),
            name: "Default".to_string(),
            is_default: true,
        }];

        result.extend(devices.into_iter().map(|d| AudioDevice {
            index: d.index,
            name: d.name,
            is_default: false, // The explicit default is handled separately
        }));

        Ok::<_, String>(result)
    })
    .await
    .map_err(|e| format!("audio task join failed: {}", e))?
}

#[tauri::command]
#[specta::specta]
pub fn set_selected_output_device(app: AppHandle, device_name: String) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.selected_output_device = if device_name == "default" {
        None
    } else {
        Some(device_name)
    };
    write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn get_selected_output_device(app: AppHandle) -> Result<String, String> {
    let settings = get_settings(&app);
    Ok(settings
        .selected_output_device
        .unwrap_or_else(|| "default".to_string()))
}

#[tauri::command]
#[specta::specta]
pub async fn play_test_sound(app: AppHandle, sound_type: String) {
    let sound = match sound_type.as_str() {
        "start" => audio_feedback::SoundType::Start,
        "stop" => audio_feedback::SoundType::Stop,
        _ => {
            warn!("Unknown sound type: {}", sound_type);
            return;
        }
    };
    audio_feedback::play_test_sound(&app, sound);
}

#[tauri::command]
#[specta::specta]
pub async fn get_microphone_channels(device_name: String) -> Result<u16, String> {
    // Device enumeration can stall, so keep it off the webview/main run loop.
    tokio::task::spawn_blocking(move || {
        let devices = handy_recorder::list_input_devices()
            .map_err(|e| format!("Failed to list audio devices: {e}"))?;
        let device = if device_name.eq_ignore_ascii_case("default") {
            devices.into_iter().find(|device| device.is_default)
        } else {
            devices
                .into_iter()
                .find(|device| device.name == device_name)
        };
        // `channels` is unknown only on ALSA, where reading it opens the device.
        Ok(device.and_then(|device| device.channels).unwrap_or(1))
    })
    .await
    .map_err(|e| format!("audio task join failed: {e}"))?
}

#[tauri::command]
#[specta::specta]
pub fn set_selected_channel(app: AppHandle, channel: Option<u16>) -> Result<(), String> {
    // Meeting capture reads the setting when it starts.
    let mut settings = get_settings(&app);
    settings.selected_channel = channel;
    write_settings(&app, settings);
    Ok(())
}
