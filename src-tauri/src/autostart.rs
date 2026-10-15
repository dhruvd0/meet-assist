//! Launch-at-login (autostart) handling.
//!
//! All platforms apply the setting through tauri-plugin-autostart, except
//! macOS 13+ where the app registers itself as a login item via
//! `SMAppService`. The plugin's launch agent plist carries no app
//! association, so the System Settings Login Items pane attributes it to the
//! code-signing certificate's developer name instead of the app (#337).
//! `SMAppService` login items are attributed to the app bundle itself and
//! appear under "Open at Login" with the app's name and icon.

use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;

/// Apply the user's autostart preference using the best mechanism for the
/// current platform.
///
/// Errors are logged rather than returned: the preference is re-applied on
/// every launch, so a transient failure self-heals and must not block
/// startup. This mirrors the pre-existing behavior of ignoring
/// enable()/disable() results.
pub fn apply_autostart(app: &AppHandle, enabled: bool) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    if macos::login_item_api_available() {
        macos::remove_plugin_launch_agent(app);
        return macos::set_login_item(enabled);
    }

    let manager = app.autolaunch();
    let result = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    result.map_err(|e| {
        log::warn!(
            "Failed to apply autostart setting (enabled={}): {}",
            enabled,
            e
        );
        e.to_string()
    })
}

#[cfg(target_os = "macos")]
mod macos {
    use std::path::{Path, PathBuf};

    use objc2::runtime::AnyClass;
    use objc2_service_management::{SMAppService, SMAppServiceStatus};
    use tauri::{AppHandle, Manager};

    /// `SMAppService` requires macOS 13. The ServiceManagement framework is
    /// linked unconditionally (it has existed since 10.6), so looking up the
    /// class doubles as the OS version check: present exactly when the API is
    /// usable.
    pub fn login_item_api_available() -> bool {
        AnyClass::get(c"SMAppService").is_some()
    }

    /// Register or unregister the app as a login item, skipping the call when
    /// the service is already in the requested state (unregistering a
    /// never-registered service returns an error on every launch otherwise).
    pub fn set_login_item(enabled: bool) -> Result<(), String> {
        let service = unsafe { SMAppService::mainAppService() };
        let status = unsafe { service.status() };

        if enabled {
            if status == SMAppServiceStatus::Enabled {
                return Ok(());
            }
            match unsafe { service.registerAndReturnError() } {
                Ok(()) => {
                    log::info!("Registered login item via SMAppService");
                    Ok(())
                }
                // Fails in dev (no signed app bundle) and when the user has
                // switched the item off in System Settings, which apps are
                // not allowed to override.
                Err(e) => {
                    log::warn!("Failed to register login item: {}", e);
                    Err(format!(
                        "macOS refused to add the login item ({e}). Allow it in System Settings → General → Login Items."
                    ))
                }
            }
        } else {
            if status == SMAppServiceStatus::NotRegistered || status == SMAppServiceStatus::NotFound
            {
                return Ok(());
            }
            match unsafe { service.unregisterAndReturnError() } {
                Ok(()) => {
                    log::info!("Unregistered login item via SMAppService");
                    Ok(())
                }
                Err(e) => {
                    log::warn!("Failed to unregister login item: {}", e);
                    Err(format!("macOS refused to remove the login item ({e})."))
                }
            }
        }
    }

    /// Remove the launch agent plist that tauri-plugin-autostart (via the
    /// auto-launch crate) wrote on older versions, so login doesn't start the
    /// app twice after migrating to `SMAppService`. Runs on every launch;
    /// missing file is the normal case.
    pub fn remove_plugin_launch_agent(app: &AppHandle) {
        let Ok(home) = app.path().home_dir() else {
            return;
        };
        remove_launch_agent_file(&plugin_launch_agent_path(&home, &app.package_info().name));
    }

    /// Path of the plist the auto-launch crate writes:
    /// `~/Library/LaunchAgents/{app name}.plist`.
    fn plugin_launch_agent_path(home: &Path, app_name: &str) -> PathBuf {
        home.join("Library")
            .join("LaunchAgents")
            .join(format!("{}.plist", app_name))
    }

    fn remove_launch_agent_file(path: &Path) {
        match std::fs::remove_file(path) {
            Ok(()) => log::info!("Removed legacy autostart launch agent {:?}", path),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log::warn!("Failed to remove legacy launch agent {:?}: {}", path, e),
        }
    }
}
