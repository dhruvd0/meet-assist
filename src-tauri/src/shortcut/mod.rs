//! Keyboard shortcut management module
//!
//! This module provides a unified interface for keyboard shortcuts with
//! multiple backend implementations:
//!
//! - `tauri`: Uses Tauri's built-in global-shortcut plugin
//! - `native_keys`: Uses the native-keys library for more control
//!
//! The active implementation is determined by the `keyboard_implementation`
//! setting and can be changed at runtime.

mod handler;
pub mod native_keys;
pub mod tauri_impl;

use log::{debug, error, info, warn};
use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Emitter, Manager};

use crate::settings::{
    self, get_settings, ChineseScript, KeyboardImplementation, ShortcutBinding, SoundTheme, Theme,
    VadBackend,
};
use crate::tray;

// Note: Commands are accessed via shortcut::native_keys:: in lib.rs

/// Initialize shortcuts using the configured implementation
pub fn init_shortcuts(app: &AppHandle) {
    let user_settings = settings::load_or_create_app_settings(app);

    // Check which implementation to use
    match user_settings.keyboard_implementation {
        KeyboardImplementation::Tauri => {
            tauri_impl::init_shortcuts(app);
        }
        KeyboardImplementation::NativeKeys => {
            if let Err(e) = native_keys::init_shortcuts(app) {
                error!("Failed to initialize native-keys shortcuts: {}", e);
                // Fall back to Tauri implementation and persist this fallback
                warn!("Falling back to Tauri global shortcut implementation and saving fallback to settings");

                // Update settings to persist the fallback so we don't retry NativeKeys on next launch
                let mut settings = settings::get_settings(app);
                settings.keyboard_implementation = KeyboardImplementation::Tauri;
                settings::write_settings(app, settings);

                // Validate as well as register: a combo only native-keys can
                // express (e.g. `fn`) is reset to the default instead of
                // leaving the meeting shortcut unregistered.
                register_all_shortcuts_for_implementation(app, KeyboardImplementation::Tauri);
            }
        }
    }
}

/// Register a shortcut using the appropriate implementation
pub fn register_shortcut(app: &AppHandle, binding: ShortcutBinding) -> Result<(), String> {
    let settings = get_settings(app);
    match settings.keyboard_implementation {
        KeyboardImplementation::Tauri => tauri_impl::register_shortcut(app, binding),
        KeyboardImplementation::NativeKeys => native_keys::register_shortcut(app, binding),
    }
}

/// Unregister a shortcut using the appropriate implementation
pub fn unregister_shortcut(app: &AppHandle, binding: ShortcutBinding) -> Result<(), String> {
    let settings = get_settings(app);
    match settings.keyboard_implementation {
        KeyboardImplementation::Tauri => tauri_impl::unregister_shortcut(app, binding),
        KeyboardImplementation::NativeKeys => native_keys::unregister_shortcut(app, binding),
    }
}

// ============================================================================
// Binding Management Commands
// ============================================================================

#[derive(Serialize, Type)]
pub struct BindingResponse {
    success: bool,
    binding: Option<ShortcutBinding>,
    error: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub fn change_binding(
    app: AppHandle,
    id: String,
    binding: String,
) -> Result<BindingResponse, String> {
    // Reject empty bindings — every shortcut should have a value
    if binding.trim().is_empty() {
        return Err("Binding cannot be empty".to_string());
    }

    let mut settings = settings::get_settings(&app);

    // Get the binding to modify, or create it from defaults if it doesn't exist
    let binding_to_modify = match settings.bindings.get(&id) {
        Some(binding) => binding.clone(),
        None => {
            // Try to get the default binding for this id
            let default_settings = settings::get_default_settings();
            match default_settings.bindings.get(&id) {
                Some(default_binding) => {
                    warn!(
                        "Binding '{}' not found in settings, creating from defaults",
                        id
                    );
                    default_binding.clone()
                }
                None => {
                    let error_msg = format!("Binding with id '{}' not found in defaults", id);
                    warn!("change_binding error: {}", error_msg);
                    return Ok(BindingResponse {
                        success: false,
                        binding: None,
                        error: Some(error_msg),
                    });
                }
            }
        }
    };

    // Unregister the existing binding
    if let Err(e) = unregister_shortcut(&app, binding_to_modify.clone()) {
        let error_msg = format!("Failed to unregister shortcut: {}", e);
        error!("change_binding error: {}", error_msg);
    }

    // Validate the new shortcut for the current keyboard implementation
    if let Err(e) = validate_shortcut_for_implementation(&binding, settings.keyboard_implementation)
    {
        warn!("change_binding validation error: {}", e);
        restore_registration(&app, &binding_to_modify);
        return Err(e);
    }

    // Create an updated binding
    let mut updated_binding = binding_to_modify.clone();
    updated_binding.current_binding = binding;

    // Register the new binding
    if let Err(e) = register_shortcut(&app, updated_binding.clone()) {
        let error_msg = format!("Failed to register shortcut: {}", e);
        error!("change_binding error: {}", error_msg);
        restore_registration(&app, &binding_to_modify);
        return Ok(BindingResponse {
            success: false,
            binding: None,
            error: Some(error_msg),
        });
    }

    // Update the binding in the settings
    settings.bindings.insert(id, updated_binding.clone());

    settings::write_settings(&app, settings);

    // Return the updated binding
    Ok(BindingResponse {
        success: true,
        binding: Some(updated_binding),
        error: None,
    })
}

/// Best-effort re-register of the previous binding after a failed change,
/// so a failure leaves the user's shortcut working exactly as before.
fn restore_registration(app: &AppHandle, binding: &ShortcutBinding) {
    if let Err(e) = register_shortcut(app, binding.clone()) {
        error!(
            "Failed to restore previous binding '{}' ({}): {}",
            binding.id, binding.current_binding, e
        );
    }
}

#[tauri::command]
#[specta::specta]
pub fn reset_binding(app: AppHandle, id: String) -> Result<BindingResponse, String> {
    let binding = settings::get_stored_binding(&settings::get_settings(&app), &id)?;
    change_binding(app, id, binding.default_binding)
}

/// Unregister every binding while the user is recording a new shortcut in
/// the UI, so no existing shortcut can fire — or swallow the keystrokes —
/// mid-capture.
pub fn suspend_all_shortcuts(app: &AppHandle) {
    for (id, binding) in settings::get_bindings(app) {
        if let Err(e) = unregister_shortcut(app, binding) {
            debug!(
                "suspend_all_shortcuts: could not unregister '{}': {}",
                id, e
            );
        }
    }
}

/// Re-register every binding from settings after shortcut recording ends.
/// Registering an already-registered shortcut fails cleanly in both
/// implementations, so this is idempotent and safe on every exit path.
pub fn resume_all_shortcuts(app: &AppHandle) {
    let settings = get_settings(app);
    for (id, binding) in &settings.bindings {
        if let Err(e) = register_shortcut(app, binding.clone()) {
            debug!("resume_all_shortcuts: could not register '{}': {}", id, e);
        }
    }
}

/// Temporarily unregister all bindings while the user is recording a
/// shortcut in the UI. This avoids firing actions while keys are recorded.
#[tauri::command]
#[specta::specta]
pub fn suspend_all_bindings(app: AppHandle) -> Result<(), String> {
    suspend_all_shortcuts(&app);
    Ok(())
}

/// Re-register all bindings after the user has finished recording.
#[tauri::command]
#[specta::specta]
pub fn resume_all_bindings(app: AppHandle) -> Result<(), String> {
    resume_all_shortcuts(&app);
    Ok(())
}

// ============================================================================
// Keyboard Implementation Switching
// ============================================================================

/// Result of changing keyboard implementation
#[derive(Serialize, Type)]
pub struct ImplementationChangeResult {
    pub success: bool,
    /// List of binding IDs that were reset to defaults due to incompatibility
    pub reset_bindings: Vec<String>,
}

/// Change the keyboard implementation with runtime switching.
/// This will unregister all shortcuts from the old implementation,
/// validate shortcuts for the new implementation (resetting invalid ones to defaults),
/// and register them with the new implementation.
#[tauri::command]
#[specta::specta]
pub fn change_keyboard_implementation_setting(
    app: AppHandle,
    implementation: String,
) -> Result<ImplementationChangeResult, String> {
    let current_settings = settings::get_settings(&app);
    let current_impl = current_settings.keyboard_implementation;
    let new_impl = parse_keyboard_implementation(&implementation);

    // If same implementation, nothing to do
    if current_impl == new_impl {
        return Ok(ImplementationChangeResult {
            success: true,
            reset_bindings: vec![],
        });
    }

    info!(
        "Switching keyboard implementation from {:?} to {:?}",
        current_impl, new_impl
    );

    // Unregister all shortcuts from the current implementation
    unregister_all_shortcuts(&app, current_impl);

    // Update the setting
    let mut settings = settings::get_settings(&app);
    settings.keyboard_implementation = new_impl;
    settings::write_settings(&app, settings);

    // Initialize new implementation if needed (NativeKeys needs state)
    if new_impl == KeyboardImplementation::NativeKeys && initialize_native_keys_with_rollback(&app)?
    {
        // Shortcuts already registered during init.
        return Ok(ImplementationChangeResult {
            success: true,
            reset_bindings: vec![],
        });
    }

    // Register all shortcuts with new implementation, resetting invalid ones
    let reset_bindings = register_all_shortcuts_for_implementation(&app, new_impl);

    // Emit event to notify frontend of the change
    let _ = app.emit(
        "settings-changed",
        serde_json::json!({
            "setting": "keyboard_implementation",
            "value": implementation,
            "reset_bindings": reset_bindings
        }),
    );

    info!("Keyboard implementation switched to {:?}", new_impl);

    Ok(ImplementationChangeResult {
        success: true,
        reset_bindings,
    })
}

/// Get the current keyboard implementation
#[tauri::command]
#[specta::specta]
pub fn get_keyboard_implementation(app: AppHandle) -> String {
    let settings = settings::get_settings(&app);
    match settings.keyboard_implementation {
        KeyboardImplementation::Tauri => "tauri".to_string(),
        KeyboardImplementation::NativeKeys => "native_keys".to_string(),
    }
}

// ============================================================================
// Validation Helpers
// ============================================================================

/// Validate a shortcut for a specific implementation
fn validate_shortcut_for_implementation(
    raw: &str,
    implementation: KeyboardImplementation,
) -> Result<(), String> {
    match implementation {
        KeyboardImplementation::Tauri => tauri_impl::validate_shortcut(raw),
        KeyboardImplementation::NativeKeys => native_keys::validate_shortcut(raw),
    }
}

/// Parse a keyboard implementation string into the enum
fn parse_keyboard_implementation(s: &str) -> KeyboardImplementation {
    match s {
        "tauri" => KeyboardImplementation::Tauri,
        "native_keys" => KeyboardImplementation::NativeKeys,
        other => {
            warn!(
                "Invalid keyboard implementation '{}', defaulting to tauri",
                other
            );
            KeyboardImplementation::Tauri
        }
    }
}

/// Unregister all shortcuts for the current implementation
fn unregister_all_shortcuts(app: &AppHandle, implementation: KeyboardImplementation) {
    let bindings = settings::get_bindings(app);

    for (id, binding) in bindings {
        let result = match implementation {
            KeyboardImplementation::Tauri => tauri_impl::unregister_shortcut(app, binding),
            KeyboardImplementation::NativeKeys => native_keys::unregister_shortcut(app, binding),
        };

        if let Err(e) = result {
            warn!(
                "Failed to unregister shortcut '{}' during switch: {}",
                id, e
            );
        }
    }
}

/// Register all shortcuts for a specific implementation, validating and resetting invalid ones
fn register_all_shortcuts_for_implementation(
    app: &AppHandle,
    implementation: KeyboardImplementation,
) -> Vec<String> {
    let mut reset_bindings = Vec::new();
    let default_bindings = settings::get_default_settings().bindings;
    let mut current_settings = settings::get_settings(app);

    for (id, default_binding) in &default_bindings {
        let mut binding = current_settings
            .bindings
            .get(id)
            .cloned()
            .unwrap_or_else(|| default_binding.clone());

        // Validate the shortcut for the target implementation
        if let Err(e) =
            validate_shortcut_for_implementation(&binding.current_binding, implementation)
        {
            info!(
                "Shortcut '{}' ({}) is invalid for {:?}: {}. Resetting to default.",
                id, binding.current_binding, implementation, e
            );

            // Reset to default
            binding.current_binding = default_binding.current_binding.clone();
            current_settings
                .bindings
                .insert(id.clone(), binding.clone());
            reset_bindings.push(id.clone());
        }

        // Register with the appropriate implementation
        let result = match implementation {
            KeyboardImplementation::Tauri => tauri_impl::register_shortcut(app, binding),
            KeyboardImplementation::NativeKeys => native_keys::register_shortcut(app, binding),
        };

        if let Err(e) = result {
            error!(
                "Failed to register shortcut '{}' for {:?}: {}",
                id, implementation, e
            );
        }
    }

    // Save settings if any bindings were reset
    if !reset_bindings.is_empty() {
        settings::write_settings(app, current_settings);
    }

    reset_bindings
}

/// Initialize NativeKeys if not already initialized, with rollback on failure
fn initialize_native_keys_with_rollback(app: &AppHandle) -> Result<bool, String> {
    if app.try_state::<native_keys::NativeKeysState>().is_some() {
        return Ok(false); // Already initialized, caller should continue
    }

    if let Err(e) = native_keys::init_shortcuts(app) {
        error!("Failed to initialize NativeKeys: {}", e);
        // Rollback to Tauri
        let mut settings = settings::get_settings(app);
        settings.keyboard_implementation = KeyboardImplementation::Tauri;
        settings::write_settings(app, settings);
        tauri_impl::init_shortcuts(app);
        return Err(format!(
            "Failed to initialize NativeKeys: {}. Reverted to Tauri.",
            e
        ));
    }

    // init_shortcuts already registered shortcuts
    Ok(true)
}

// ============================================================================
// General Settings Commands
// ============================================================================

#[tauri::command]
#[specta::specta]
pub fn change_audio_feedback_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.audio_feedback = enabled;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_audio_feedback_volume_setting(app: AppHandle, volume: f32) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.audio_feedback_volume = volume;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_sound_theme_setting(app: AppHandle, theme: String) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    let parsed = match theme.as_str() {
        "marimba" => SoundTheme::Marimba,
        "pop" => SoundTheme::Pop,
        "custom" => SoundTheme::Custom,
        other => {
            warn!("Invalid sound theme '{}', defaulting to marimba", other);
            SoundTheme::Marimba
        }
    };
    settings.sound_theme = parsed;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_theme_setting(app: AppHandle, theme: String) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    let parsed = match theme.as_str() {
        "system" => Theme::System,
        "light" => Theme::Light,
        "dark" => Theme::Dark,
        other => {
            warn!("Invalid theme '{}', defaulting to system", other);
            Theme::System
        }
    };
    settings.theme = parsed;
    settings::write_settings(&app, settings);
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    apply_window_theme(&app, parsed);
    // Notify other webviews (the recording overlay) so they re-apply the palette
    // live — they set `data-theme` on their own document and can't see this one.
    let _ = app.emit("theme-changed", parsed);
    Ok(())
}

/// Applies the appearance setting to the native window chrome (title bar), which
/// CSS `data-theme` cannot reach. `System` clears the override so the window
/// follows the OS. Call this on startup and whenever the setting changes to keep
/// the title bar in sync with the in-app palette.
///
/// On Windows this themes the title bar only. On macOS `set_theme` sets
/// `NSApp.appearance` app-wide, which is what we want here: it darkens the title
/// bar and keeps the overlay in step. Linux is left to `data-theme` alone, since
/// its window theming is backend-dependent and unreliable.
#[cfg(any(target_os = "windows", target_os = "macos"))]
pub fn apply_window_theme(app: &AppHandle, theme: Theme) {
    let window_theme = match theme {
        Theme::System => None,
        Theme::Light => Some(tauri::Theme::Light),
        Theme::Dark => Some(tauri::Theme::Dark),
    };
    if let Some(window) = app.get_webview_window("main") {
        if let Err(e) = window.set_theme(window_theme) {
            warn!("Failed to apply window theme: {}", e);
        }
    }
}

#[tauri::command]
#[specta::specta]
pub fn change_translate_to_english_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.translate_to_english = enabled;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_selected_language_setting(app: AppHandle, language: String) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.selected_language = language;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_debug_mode_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.debug_mode = enabled;
    settings::write_settings(&app, settings);

    // Keep webview log streaming in sync: the live log viewer only exists in
    // debug mode, so logs are forwarded to the frontend only while it is on.
    crate::WEBVIEW_LOG_STREAMING.store(enabled, std::sync::atomic::Ordering::Relaxed);

    // Emit event to notify frontend of debug mode change
    let _ = app.emit(
        "settings-changed",
        serde_json::json!({
            "setting": "debug_mode",
            "value": enabled
        }),
    );

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_start_hidden_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.start_hidden = enabled;
    settings::write_settings(&app, settings);

    // Notify frontend
    let _ = app.emit(
        "settings-changed",
        serde_json::json!({
            "setting": "start_hidden",
            "value": enabled
        }),
    );

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_autostart_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    // Apply first, so a refusal leaves the saved setting as it was.
    crate::autostart::apply_autostart(&app, enabled)?;
    let mut settings = settings::get_settings(&app);
    settings.autostart_enabled = enabled;
    settings::write_settings(&app, settings);

    // Notify frontend
    let _ = app.emit(
        "settings-changed",
        serde_json::json!({
            "setting": "autostart_enabled",
            "value": enabled
        }),
    );

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_update_checks_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    if settings::update_checks_forced_disabled() {
        return Err(
            "Update checks are disabled by system configuration (CASE_DISABLE_UPDATER)".into(),
        );
    }

    let mut settings = settings::get_settings(&app);
    settings.update_checks_enabled = enabled;
    settings::write_settings(&app, settings);

    let _ = app.emit(
        "settings-changed",
        serde_json::json!({
            "setting": "update_checks_enabled",
            "value": enabled
        }),
    );

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_show_whats_new_on_update_setting(
    app: AppHandle,
    enabled: bool,
) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.show_whats_new_on_update = enabled;
    settings::write_settings(&app, settings);

    let _ = app.emit(
        "settings-changed",
        serde_json::json!({
            "setting": "show_whats_new_on_update",
            "value": enabled
        }),
    );

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_whats_new_last_seen_version_setting(
    app: AppHandle,
    version: String,
) -> Result<(), String> {
    let version = version.trim().to_string();
    let mut settings = settings::get_settings(&app);
    settings.whats_new_last_seen_version = version.clone();
    settings::write_settings(&app, settings);

    let _ = app.emit(
        "settings-changed",
        serde_json::json!({
            "setting": "whats_new_last_seen_version",
            "value": version
        }),
    );

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn update_custom_words(app: AppHandle, words: Vec<String>) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.custom_words = words;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_word_correction_threshold_setting(
    app: AppHandle,
    threshold: f64,
) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.word_correction_threshold = threshold;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_meeting_sensitivity_setting(
    app: AppHandle,
    sensitivity: settings::MeetingSensitivity,
) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.meeting_sensitivity = sensitivity;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_meeting_observer_interval_setting(app: AppHandle, secs: u32) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.meeting_observer_interval_secs = secs.clamp(10, 120);
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_meeting_max_line_setting(app: AppHandle, secs: u32) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.meeting_max_line_secs = secs.clamp(5, 60);
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_meeting_assistant_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.meeting_assistant_enabled = enabled;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_meeting_system_audio_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.meeting_system_audio_enabled = enabled;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_meeting_vision_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.meeting_vision_enabled = enabled;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_meeting_beep_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.meeting_beep_enabled = enabled;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_experimental_enabled_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.experimental_enabled = enabled;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_llm_base_url_setting(
    app: AppHandle,
    provider_id: String,
    base_url: String,
) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    let label = settings
        .llm_provider_by_id(&provider_id)
        .map(|provider| provider.label.clone())
        .ok_or_else(|| format!("Provider '{}' not found", provider_id))?;

    let provider = settings
        .llm_provider_mut(&provider_id)
        .expect("Provider looked up above must exist");

    if provider.id != "custom" {
        return Err(format!(
            "Provider '{}' does not allow editing the base URL",
            label
        ));
    }

    provider.base_url = base_url;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_llm_api_format_setting(
    app: AppHandle,
    provider_id: String,
    api_format: settings::LlmApiFormat,
) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    let provider = settings
        .llm_provider_mut(&provider_id)
        .ok_or_else(|| format!("Provider '{}' not found", provider_id))?;

    if provider.id != "custom" {
        return Err(format!(
            "Provider '{}' does not allow changing the API format",
            provider.label
        ));
    }

    provider.api_format = api_format;
    settings::write_settings(&app, settings);
    Ok(())
}

/// Generic helper to validate provider exists
fn validate_provider_exists(
    settings: &settings::AppSettings,
    provider_id: &str,
) -> Result<(), String> {
    if !settings
        .llm_providers
        .iter()
        .any(|provider| provider.id == provider_id)
    {
        return Err(format!("Provider '{}' not found", provider_id));
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_llm_api_key_setting(
    app: AppHandle,
    provider_id: String,
    api_key: String,
) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    validate_provider_exists(&settings, &provider_id)?;
    settings.llm_api_keys.insert(provider_id, api_key);
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_llm_model_setting(
    app: AppHandle,
    provider_id: String,
    model: String,
) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    validate_provider_exists(&settings, &provider_id)?;
    settings.llm_models.insert(provider_id, model);
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn set_llm_provider(app: AppHandle, provider_id: String) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    validate_provider_exists(&settings, &provider_id)?;
    settings.llm_provider_id = provider_id;
    settings::write_settings(&app, settings);
    Ok(())
}

/// Whether this build ships a default OpenRouter key.
#[tauri::command]
#[specta::specta]
pub fn has_bundled_llm_key() -> bool {
    !settings::BUNDLED_OPENROUTER_API_KEY.is_empty()
}

/// Reply to `ping_llm_model`: what the model said and how long it took.
#[derive(Serialize, Type)]
pub struct LlmPing {
    pub model: String,
    pub reply: String,
    pub ms: u32,
}

/// Send "ping" to the saved provider and model through the same JSON path the
/// assistant uses, so a pass means nudges and summaries can work.
#[tauri::command]
#[specta::specta]
pub async fn ping_llm_model(app: AppHandle) -> Result<LlmPing, String> {
    let settings = settings::get_settings(&app);
    let provider = settings
        .llm_provider()
        .cloned()
        .ok_or("No provider is selected.")?;
    let model = settings
        .llm_model()
        .filter(|m| !m.trim().is_empty())
        .ok_or("No model is selected.")?;
    let schema = serde_json::json!({
        "type": "object",
        "properties": { "reply": { "type": "string" } },
        "required": ["reply"],
        "additionalProperties": false
    });
    let started = std::time::Instant::now();
    let call = crate::llm_client::complete_json(
        &provider,
        settings.llm_api_key(),
        &model,
        "You are a connection test. Answer with JSON only.".to_string(),
        r#"Reply with exactly {"reply": "pong"}."#.to_string(),
        schema,
    );
    let value = tokio::time::timeout(std::time::Duration::from_secs(30), call)
        .await
        .map_err(|_| "No reply within 30 seconds.".to_string())??;
    let reply = value["reply"]
        .as_str()
        .unwrap_or_default()
        .trim()
        .to_string();
    if reply.is_empty() {
        return Err(format!("Unexpected reply: {value}"));
    }
    Ok(LlmPing {
        model,
        reply,
        ms: started.elapsed().as_millis().min(u32::MAX as u128) as u32,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn fetch_llm_models(app: AppHandle, provider_id: String) -> Result<Vec<String>, String> {
    let settings = settings::get_settings(&app);

    // Find the provider
    let provider = settings
        .llm_providers
        .iter()
        .find(|p| p.id == provider_id)
        .ok_or_else(|| format!("Provider '{}' not found", provider_id))?;

    // Get API key
    let api_key = settings.llm_provider_api_key(&provider_id);

    // Skip fetching if no API key for providers that typically need one
    if api_key.trim().is_empty() && provider.id != "custom" {
        return Err(format!(
            "API key is required for {}. Please add an API key to list available models.",
            provider.label
        ));
    }

    crate::llm_client::fetch_models(provider, api_key).await
}

#[tauri::command]
#[specta::specta]
pub fn change_vad_enabled_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.vad_enabled = enabled;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_vad_backend_setting(app: AppHandle, backend: VadBackend) -> Result<(), String> {
    // Meeting capture builds its VAD from the setting when it starts.
    let mut current_settings = settings::get_settings(&app);
    current_settings.vad_backend = backend;
    settings::write_settings(&app, current_settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_filler_word_removal_enabled_setting(
    app: AppHandle,
    enabled: bool,
) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.filler_word_removal_enabled = enabled;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_chinese_script_setting(app: AppHandle, script: ChineseScript) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.chinese_script = script;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_app_language_setting(app: AppHandle, language: String) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.app_language = language.clone();
    settings::write_settings(&app, settings);

    // Refresh the tray menu with the new language
    tray::update_tray_menu(&app);

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_show_tray_icon_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.show_tray_icon = enabled;
    settings::write_settings(&app, settings);

    // Apply change immediately
    tray::set_tray_visibility(&app, enabled);

    Ok(())
}

/// Save accelerator settings and make the next model use reload with them.
/// The currently running transcription, if any, keeps its existing engine.
fn save_accelerator_and_reload_next_use(app: &AppHandle, s: settings::AppSettings) {
    settings::write_settings(app, s);

    let tm = app.state::<std::sync::Arc<crate::managers::transcription::TranscriptionManager>>();
    tm.reload_model_on_next_use();
}

#[tauri::command]
#[specta::specta]
pub fn change_transcribe_accelerator_setting(
    app: AppHandle,
    accelerator: settings::TranscribeAcceleratorSetting,
) -> Result<(), String> {
    let mut s = settings::get_settings(&app);
    s.transcribe_accelerator = accelerator;
    save_accelerator_and_reload_next_use(&app, s);
    app.state::<std::sync::Arc<crate::managers::transcription::TranscriptionManager>>()
        .retry_transcribe_gpu("the transcribe.cpp accelerator setting changed");
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_ort_accelerator_setting(
    app: AppHandle,
    accelerator: settings::OrtAcceleratorSetting,
) -> Result<(), String> {
    let mut s = settings::get_settings(&app);
    s.ort_accelerator = accelerator;
    save_accelerator_and_reload_next_use(&app, s);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_transcribe_gpu_device(app: AppHandle, device: Option<String>) -> Result<(), String> {
    let mut s = settings::get_settings(&app);
    s.transcribe_gpu_device = device;
    save_accelerator_and_reload_next_use(&app, s);
    app.state::<std::sync::Arc<crate::managers::transcription::TranscriptionManager>>()
        .retry_transcribe_gpu("the transcribe.cpp GPU device setting changed");
    Ok(())
}

/// Return which accelerators and GPU devices are available for this build.
///
/// First-call cost is dominated by enumerating GPU devices through the
/// transcribe.cpp Metal/Vulkan backend, which loads dynamic libraries and
/// probes hardware. Run it on the blocking pool so the webview thread
/// stays responsive — see also the startup pre-warm in `lib.rs`.
#[tauri::command]
#[specta::specta]
pub async fn get_available_accelerators(
    app: AppHandle,
) -> crate::managers::transcription::AvailableAccelerators {
    tauri::async_runtime::spawn_blocking(move || {
        let tm =
            app.state::<std::sync::Arc<crate::managers::transcription::TranscriptionManager>>();
        crate::managers::transcription::get_available_accelerators(&tm)
    })
    .await
    .expect("get_available_accelerators panicked")
}
