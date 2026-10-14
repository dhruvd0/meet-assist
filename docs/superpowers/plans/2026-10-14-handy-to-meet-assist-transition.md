# Handy to Meet Assist Transition Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove Handy's dictation product (paste output, post-processing, dictation recorder, history, overlay, accessibility) and most tests, repurpose the shortcut to toggle meetings, and give the AI provider an always-visible Assistant page.

**Architecture:** Mostly deletion, guided by the compiler. Meetings already use their own capture (`meeting/capture.rs`, `meeting/system_capture.rs`) and only borrow two helpers from `managers/audio.rs` (`create_capture_vad`, `microphone_config`). The dictation recorder, coordinator, actions, paste, and history go entirely. One new module, `meeting/toggle.rs`, owns the shortcut, tray, CLI, and signal entry point for start/end. Settings keep their store with serde aliases and a schema-3 migration.

**Tech Stack:** Rust (Tauri 2, tauri-specta, serde), React + TypeScript, Zustand, i18next, Bun.

**Spec:** `docs/superpowers/specs/2026-10-14-handy-to-meet-assist-transition-design.md`

## Global Constraints

- Branch: `feat/meet-assist-transition`. Commit after each task with conventional prefixes (`feat:`, `fix:`, `refactor:`, `chore:`, `docs:`), message about *why*, ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Test policy: no new tests except `handy_era_settings_keep_provider`. Exactly 7 Rust tests remain at the end (listed in Task 1).
- Every task ends green on: `cd src-tauri && cargo clippy --all-targets -- -D warnings && cargo test`, plus `bun run build && bun run lint` once frontend files are touched.
- Do not rename crates or internal identifiers like `handy-recorder` or `handy_keys`.
- No new UI primitives. Reuse `components/ui` (`SettingsGroup`, `SettingContainer`, `ToggleSwitch`, `Dropdown`, `Input`, `Button`).
- All user-facing strings go through i18next. New keys go in `src/i18n/locales/en/translation.json` only.
- Settings store compatibility: an old store must load without losing the provider, API keys, models, selected model, microphone, language, custom words, or meeting settings.
- Do not regenerate `src/bindings.ts` by hand. It is written by a debug build (`bun run tauri dev` or `cargo run`) via tauri-specta.
- When unsure whether a setting is read by meeting code, grep `src-tauri/src/meeting/` and `managers/transcription.rs`. If either reads it, keep it.

## Review Focus

1. **Stored Handy bindings.** An old store has `transcribe`, `transcribe_with_post_process`, `cancel` in `bindings`. The load path merges defaults but never prunes, so stale ids would be registered as global shortcuts with no handler, and `escape` would stay grabbed. Expected: only `toggle_meeting` exists after load, carrying the user's old `transcribe` key combo. Pinned by `handy_era_settings_keep_provider` (Task 4).
2. **Assistant on with no usable provider.** Expected: the toggle stays on and the page shows the backend readiness message (missing key, model, or provider). It doesn't re-derive the rule in TypeScript. Checked in Task 5's manual step.
3. **Shortcut pressed while a meeting is paused.** `MeetingManager::state().meeting_id` is `Some` for live *and* paused meetings. Expected: the shortcut ends it. Pinned by the `toggle` match in Task 2.
4. **Shortcut with no projects and no meetings.** Expected: the main window opens on Projects. No meeting with a dangling project is created. Task 2.
5. **Locales missing new tray keys.** `build.rs` currently writes `""` for a missing key, which gives a blank tray item in non-English locales. Expected: English fallback. Fixed in Task 2.

---

## File map

**Delete (Rust, `src-tauri/src/`):** `actions.rs`, `transcription_coordinator.rs`, `clipboard.rs`, `input.rs`, `paste_tx/`, `secure_input.rs`, `apple_intelligence.rs`, `overlay.rs`, `managers/history.rs`, `commands/history.rs`, `helpers/clamshell.rs` (if only the recorder and the clamshell commands use it).

**Shrink:** `managers/audio.rs` (keep `create_capture_vad`, `microphone_config`, and anything they call; delete `AudioRecordingManager` and the recorder), `shortcut/mod.rs`, `shortcut/handler.rs`, `signal_handle.rs`, `tray.rs`, `utils.rs`, `lib.rs`, `cli.rs`, `commands/mod.rs`, `commands/audio.rs`, `settings.rs`, `managers/transcription.rs`, `meeting/session.rs`, `intervention.rs`, `build.rs`.

**Create:** `src-tauri/src/meeting/toggle.rs`, `src/components/settings/assistant/` (moved files plus `AssistantSettings.tsx`), `src/components/settings/audio/AudioSettings.tsx`.

**Delete (frontend):** `src/overlay/`, `src/components/settings/history/`, `post-processing/`, the dictation-only setting components (Task 5 list), `AccessibilityPermissions.tsx`, `SecureInputWarning.tsx`, `onboarding/AccessibilityOnboarding.tsx`, `tests/`, `playwright.config.*`, three `*.test.ts` files.

---

### Task 1: Cut tests and CI to the core

**Files:**
- Delete: `tests/`, `playwright.config.ts` (or `.js`), `src/lib/utils/keyboard.test.ts`, `src/components/update-checker/portableInstaller.test.ts`, `src/components/settings/history/clipboard.test.ts`, `.github/workflows/playwright.yml`
- Modify: `package.json`, `.github/workflows/code-quality.yml`, every `src-tauri/src/**/*.rs` containing `#[cfg(test)]`

**Interfaces:**
- Produces: a Rust test suite of exactly these 6 existing tests (the 7th arrives in Task 4):
  - `meeting::store::tests::open_enables_wal`
  - `meeting::store::tests::startup_recovery_closes_open_sessions_and_pauses_live`
  - `meeting::store::tests::delete_project_cascades_sources_and_chunks`
  - `meeting::store::tests::delete_meeting_and_delete_project_cascade_lines`
  - `meeting::summary::tests::job_stores_session_and_rolling_summary`
  - `meeting::summary::tests::merge_failure_stores_nothing`

- [ ] **Step 1: Delete frontend tests and Playwright**

```bash
git rm -r tests playwright.config.* src/lib/utils/keyboard.test.ts \
  src/components/update-checker/portableInstaller.test.ts \
  src/components/settings/history/clipboard.test.ts \
  .github/workflows/playwright.yml
```

In `package.json`, remove the scripts `test:keyboard`, `test:playwright`, `test:playwright:ui` and the devDependency `@playwright/test`. Then run `bun install` to update `bun.lock`.

- [ ] **Step 2: Update code-quality CI**

In `.github/workflows/code-quality.yml`, delete the step that runs `bun run test:keyboard`. After the `bun run lint` step, add:

```yaml
      - name: Typecheck frontend
        run: bunx tsc --noEmit
```

If the workflow already has a Rust job running `cargo clippy`, make its command `cargo clippy --all-targets -- -D warnings`. If there is no Rust job, leave Rust linting to `test.yml` (next step).

In `.github/workflows/test.yml`, change the line `run: cargo test` to:

```yaml
        run: |
          cargo clippy --all-targets -- -D warnings
          cargo test
```

Grep the remaining workflows for `playwright` and `test:keyboard` and delete those steps: `grep -n "playwright\|test:keyboard" .github/workflows/*.yml` must print nothing.

- [ ] **Step 3: Delete Rust tests outside the core**

For each file listed by `grep -rl '#\[cfg(test)\]' src-tauri/src`:
- Not `meeting/store.rs` or `meeting/summary.rs`: delete the whole `#[cfg(test)] mod … { … }` block. Also delete standalone `#[cfg(test)]` items in that file (helpers like `MeetingManager::new_for_test`, `with_summarizer`, fake structs, `#[cfg(test)] fn` helpers) **unless** `meeting/store.rs` or `meeting/summary.rs` tests use them. Check with `grep -n "<name>" src-tauri/src/meeting/store.rs src-tauri/src/meeting/summary.rs`.
- `meeting/store.rs`: inside its `mod tests`, keep the 4 listed functions and any helper they call (e.g. a `fn store()` constructor). Delete every other `#[test]` fn.
- `meeting/summary.rs`: keep the 2 listed functions and the fakes they use. Delete the rest.

Also delete `src-tauri/tests/` if it exists. Delete `lib.rs`'s `mod headless_guard_tests` block (around line 442).

- [ ] **Step 4: Verify**

Run: `cd src-tauri && cargo test 2>&1 | tail -5`
Expected: `test result: ok. 6 passed`

Run: `cargo clippy --all-targets -- -D warnings`
Expected: no `dead_code` warnings from removed test helpers. If any appear, delete the item it names.

Run: `bun run build && bun run lint`
Expected: success.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "chore: cut tests to a data-safety core

UI, end-to-end and most unit tests guarded Handy's dictation behaviour
and slowed changes; typecheck, clippy and six store/summary tests stay.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Add the meeting toggle (shortcut, tray, CLI, signal)

Additive: dictation still compiles after this task. The new `toggle_meeting` binding works alongside the old ones.

**Files:**
- Create: `src-tauri/src/meeting/toggle.rs`
- Modify: `src-tauri/src/meeting/mod.rs`, `src-tauri/src/settings.rs` (default bindings), `src-tauri/src/shortcut/handler.rs`, `src-tauri/src/meeting/session.rs` (`emit_state`), `src-tauri/src/tray.rs`, `src-tauri/src/lib.rs` (tray menu handler, single-instance args), `src-tauri/src/cli.rs`, `src-tauri/src/signal_handle.rs`, `src-tauri/build.rs`, `src/i18n/locales/en/translation.json`

**Interfaces:**
- Produces: `pub fn crate::meeting::toggle::toggle_meeting(app: &tauri::AppHandle)` ends the running meeting (live or paused) or starts one. Never panics. Logs and emits `show-main-window` style focus on failure.
- Produces: binding id `"toggle_meeting"` in `get_default_settings().bindings`.
- Produces: tray menu ids `"start_meeting"`, `"end_meeting"`; tray strings `start_meeting`, `end_meeting` (from en keys `startMeeting`, `endMeeting`).
- Produces: CLI flag `--toggle-meeting` (`CliArgs::toggle_meeting: bool`).
- Consumes: `MeetingManager::{state, start, end}` (`meeting/session.rs`), `ProjectStore::{list_meetings, list_projects}` (`meeting/store.rs`), `crate::show_main_window` (lib.rs).

- [ ] **Step 1: Write `meeting/toggle.rs`**

```rust
//! One entry point for every "start or end a meeting" trigger: the global
//! shortcut, the tray, `--toggle-meeting`, and SIGUSR2.

use std::sync::Arc;

use log::{error, info};
use tauri::{AppHandle, Emitter, Manager};

use crate::meeting::session::MeetingManager;

/// Ends the running meeting (live or paused), otherwise starts a new one in
/// the most recently used project. With no projects, opens the window on
/// Projects instead of creating an orphan meeting.
pub fn toggle_meeting(app: &AppHandle) {
    let Some(mm) = app.try_state::<Arc<MeetingManager>>() else {
        error!("toggle_meeting: MeetingManager not initialised");
        return;
    };
    let mm = mm.inner().clone();
    let app = app.clone();
    // Start/end do blocking capture and store work; keep it off the shortcut
    // and main threads.
    std::thread::spawn(move || {
        if mm.state().meeting_id.is_some() {
            if let Err(e) = mm.end() {
                error!("toggle_meeting: end failed: {e:#}");
            }
            return;
        }
        match last_used_project(&mm) {
            Some(project_id) => match mm.start(Some(project_id), None) {
                Ok(m) => info!("toggle_meeting: started meeting {}", m.id),
                Err(e) => {
                    error!("toggle_meeting: start failed: {e:#}");
                    crate::show_main_window(&app);
                }
            },
            None => {
                crate::show_main_window(&app);
                let _ = app.emit("navigate-section", "projects");
            }
        }
    });
}

/// Project of the most recently updated meeting, else the most recently
/// updated project. Both lists are already ordered `updated_at DESC`.
fn last_used_project(mm: &MeetingManager) -> Option<i64> {
    let store = mm.store();
    if let Ok(meetings) = store.list_meetings(None) {
        if let Some(pid) = meetings.iter().find_map(|m| m.project_id) {
            return Some(pid);
        }
    }
    store.list_projects().ok()?.first().map(|p| p.id)
}
```

Facts this relies on (checked): `MeetingManager::store()` returns `&ProjectStore` (`session.rs:350`); `list_meetings(None)` returns all meetings, `updated_at DESC` (`store.rs:727`); `Meeting.project_id` is `Option<i64>`. `show_main_window` in `lib.rs:101` is private: change it to `pub(crate) fn show_main_window`.

Add `pub mod toggle;` to `src-tauri/src/meeting/mod.rs`.

- [ ] **Step 2: Handle `navigate-section` in the frontend**

In `src/App.tsx`, next to the existing `listen(...)` effects, add:

```tsx
  useEffect(() => {
    const unlisten = listen<string>("navigate-section", (e) => {
      if (e.payload in SECTIONS_CONFIG) {
        setCurrentSection(e.payload as SidebarSection);
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);
```

- [ ] **Step 3: Add the default binding**

In `settings.rs` `get_default_settings()`, after the existing `bindings.insert(...)` calls, add:

```rust
    #[cfg(target_os = "macos")]
    let default_meeting_shortcut = "option+shift+m";
    #[cfg(not(target_os = "macos"))]
    let default_meeting_shortcut = "ctrl+shift+m";
    bindings.insert(
        "toggle_meeting".to_string(),
        ShortcutBinding {
            id: "toggle_meeting".to_string(),
            name: "Start or end meeting".to_string(),
            description: "Starts a meeting, or ends the running one.".to_string(),
            default_binding: default_meeting_shortcut.to_string(),
            current_binding: default_meeting_shortcut.to_string(),
        },
    );
```

Task 4 moves the user's old `transcribe` combo onto this binding and makes `option+space`/`ctrl+space` the default. Until then the two must not collide.

- [ ] **Step 4: Route the binding in `shortcut/handler.rs`**

At the top of `handle_shortcut_event`, before `let settings = get_settings(app);`, add:

```rust
    if binding_id == "toggle_meeting" {
        if is_pressed {
            crate::meeting::toggle::toggle_meeting(app);
        }
        return;
    }
```

- [ ] **Step 5: Tray follows meeting state**

In `meeting/session.rs` `emit_state`, after the emit, add:

```rust
            let busy = self.state().meeting_id.is_some();
            crate::tray::set_tray_state(
                app,
                if busy {
                    crate::tray::TrayIconState::Recording
                } else {
                    crate::tray::TrayIconState::Idle
                },
            );
```

In `src/i18n/locales/en/translation.json` under `"tray"`, add `"startMeeting": "Start Meeting"` and `"endMeeting": "End Meeting"`.

In `build.rs` `generate_tray_translations`, replace

```rust
            let val = tray.get(json_key).and_then(|v| v.as_str()).unwrap_or("");
```

with

```rust
            let val = tray
                .get(json_key)
                .and_then(|v| v.as_str())
                .or_else(|| english.get(json_key).and_then(|v| v.as_str()))
                .unwrap_or("");
```

In `tray.rs` menu construction (around line 512-520, where `cancel_i` is built when busy), build the toggle item in both states:

```rust
    let meeting_i = if inputs.busy {
        MenuItem::with_id(app, "end_meeting", &strings.end_meeting, true, None::<&str>)?
    } else {
        MenuItem::with_id(app, "start_meeting", &strings.start_meeting, true, None::<&str>)?
    };
```

Insert `&meeting_i` as the first item after the version line in both menu layouts. Use the actual local names in `tray.rs` (`inputs`/`menu` may be called differently there). Read the function first.

In `lib.rs` `on_menu_event`, add arms:

```rust
            "start_meeting" | "end_meeting" => {
                crate::meeting::toggle::toggle_meeting(app);
            }
```

- [ ] **Step 6: CLI flag and signal**

In `cli.rs`, add next to `toggle_transcription`:

```rust
    /// Start a meeting, or end the running one, on a running instance
    #[arg(long)]
    pub toggle_meeting: bool,
```

In `lib.rs` single-instance handler, add as the first branch:

```rust
            if args.iter().any(|a| a == "--toggle-meeting") {
                crate::meeting::toggle::toggle_meeting(app);
            } else if args.iter().any(|a| a == "--toggle-transcription") {
```

Leave `signal_handle.rs` alone in this task. Task 3 rewrites it.

- [ ] **Step 7: Verify**

Run: `cd src-tauri && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: clean, 6 passed.

Manual: `bun run tauri dev` (on macOS add `CMAKE_POLICY_VERSION_MINIMUM=3.5` if cmake errors). Create a project. Press `option+shift+m`: a meeting starts, the tray shows the recording icon and "End Meeting". Press again: it ends. Delete all projects and meetings, press it: the window opens on Projects.

- [ ] **Step 8: Commit**

```bash
git add -A && git commit -m "feat: toggle meetings from the shortcut, tray, CLI

Meet Assist's main action is starting a meeting; give it the global
shortcut and tray entry that dictation used to own.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Delete the dictation pipeline (backend)

**Files:**
- Delete: `actions.rs`, `transcription_coordinator.rs`, `clipboard.rs`, `input.rs`, `paste_tx/`, `secure_input.rs`, `apple_intelligence.rs`, `overlay.rs`, `managers/history.rs`, `commands/history.rs`
- Modify: `lib.rs`, `managers/audio.rs`, `managers/mod.rs`, `managers/transcription.rs`, `commands/mod.rs`, `commands/audio.rs`, `shortcut/mod.rs`, `shortcut/handler.rs`, `signal_handle.rs`, `tray.rs`, `utils.rs`, `cli.rs`, `intervention.rs`, `meeting/session.rs`, `capabilities/default.json`, `tauri.conf.json` (if the overlay window is declared there)

**Interfaces:**
- Consumes: `toggle_meeting` (Task 2).
- Produces: `managers::audio` exports only `create_capture_vad`, `microphone_config` and their private dependencies. `MeetingManager::new(app, store, tm)` drops its `rm` parameter.

- [ ] **Step 1: Move the monitor helper out of overlay**

Copy `get_monitor_with_cursor` (and only the private helpers it calls) from `overlay.rs` into `intervention.rs` as a private `fn`. Change the call at `intervention.rs:102` from `crate::overlay::get_monitor_with_cursor(app)` to `get_monitor_with_cursor(app)`.

- [ ] **Step 2: Delete files and module declarations**

```bash
cd src-tauri/src
git rm actions.rs transcription_coordinator.rs clipboard.rs input.rs secure_input.rs \
  apple_intelligence.rs overlay.rs managers/history.rs commands/history.rs
git rm -r paste_tx
```

In `lib.rs`, delete the `mod` lines for `actions`, `apple_intelligence`, `clipboard`, `input`, `overlay`, `paste_tx`, `secure_input`, `transcription_coordinator`, and the `use`/`pub use` lines for `HistoryManager` and `TranscriptionCoordinator`. In `managers/mod.rs`, delete `pub mod history;`. In `commands/mod.rs`, delete `pub mod history;`, `cancel_operation`, `check_apple_intelligence_available`, `initialize_enigo`, and `open_recordings_folder` (recordings were dictation history audio).

- [ ] **Step 3: Remove the dictation recorder**

In `managers/audio.rs`, delete `AudioRecordingManager`, `MicrophoneMode`, `RecordingState`, `RecordingReadiness`, `OpenMicrophone`, `open_microphone*`, mute helpers (`set_mute`, `get_mute`, `restore_mute`), `recording_error_type`, `DesiredMicrophone`/clamshell resolution, and the lazy-close logic. Keep `create_capture_vad` and `microphone_config`, then delete whatever clippy reports unused.

In `lib.rs` setup, delete construction and `manage` of `recording_manager` and `history_manager`, the `TranscriptionCoordinator` manage (around line 1086), `secure_input::init`, the overlay cache init (around line 1095), and `utils::create_recording_overlay(app_handle)`. Change the `MeetingManager::new` call to drop `recording_manager.clone()`.

In `meeting/session.rs`: remove the `rm` parameter and the `dictation_recording` field, and the check at the top of `start()` ("Dictation is recording…"). Remove the `use crate::managers::audio::AudioRecordingManager;` import.

In `managers/transcription.rs` around line 332, replace

```rust
                    let is_recording = app_handle_cloned
                        .try_state::<Arc<AudioRecordingManager>>()
                        .is_some_and(|a| a.is_recording());
                    if is_recording || meeting_is_live(&app_handle_cloned) {
```

with

```rust
                    if meeting_is_live(&app_handle_cloned) {
```

and delete the import. The streaming router (`stream_router`, `cancel_stream`, `StreamTextEvent`, `StreamPhaseEvent`) only served the dictation overlay. If clippy reports any of it unused, delete it and remove the two events from `collect_events!`.

In `commands/audio.rs`, delete `update_microphone_mode`, `get_microphone_mode`, `set_clamshell_microphone`, `get_clamshell_microphone`, `is_recording`. In `set_selected_microphone` and `set_selected_channel`, delete the `rm.update_selected_device()` / `rm.update_selected_channel(..)` calls. Meeting capture reads the setting at start, so writing the setting is enough. Delete `helpers/clamshell.rs` and `helpers::clamshell::is_laptop` if nothing else uses them.

- [ ] **Step 4: Shortcut, signal, utils, tray, CLI**

`shortcut/handler.rs` becomes:

```rust
//! Shared shortcut event handling for the Tauri and handy-keys backends.

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
```

In `shortcut/mod.rs`:
- Delete every `"cancel"` special case (dynamic register/unregister, `unregister_cancel_shortcut`, `register_cancel_shortcut`) and the `transcribe_with_post_process` checks.
- Delete the setting commands for removed settings: `change_shortcut_activation_setting`, `change_hold_threshold_ms_setting`, `change_overlay_position_setting`, `change_overlay_style_setting`, `change_extra_recording_buffer_setting`, `change_paste_delay_ms_setting`, `change_paste_delay_after_ms_setting`, `change_reliable_paste_setting`, `change_paste_method_setting`, `get_available_typing_tools`, `change_typing_tool_setting`, `change_external_script_path_setting`, `change_clipboard_handling_setting`, `change_auto_submit_setting`, `change_auto_submit_key_setting`, `change_post_process_enabled_setting`, `add_post_process_prompt`, `update_post_process_prompt`, `delete_post_process_prompt`, `set_post_process_selected_prompt`, `change_mute_while_recording_setting`, `change_append_trailing_space_setting`, `change_lazy_stream_close_setting`.
- Delete the matching lines in `lib.rs` `collect_commands!`, plus `secure_input::*`, `commands::cancel_operation`, `commands::check_apple_intelligence_available`, `commands::initialize_enigo`, `commands::open_recordings_folder`, all `commands::history::*`, the deleted `commands::audio::*`, `helpers::clamshell::is_laptop` (if deleted), and `managers::history::HistoryUpdatePayload` from `collect_events!`.
- Around line 1439, delete the `AudioRecordingManager` use and the logic around it (it guarded rebinding during recording).

`signal_handle.rs` becomes:

```rust
use log::debug;
use tauri::AppHandle;

#[cfg(unix)]
use signal_hook::{consts::SIGUSR2, iterator::Signals};

/// SIGUSR2 toggles a meeting. SIGUSR1 is not handled: on Linux it belongs to
/// WebKitGTK's garbage collector (#1660).
#[cfg(unix)]
pub fn setup_signal_handler(app_handle: AppHandle) {
    let mut signals = Signals::new([SIGUSR2]).expect("failed to register SIGUSR2 handler");
    debug!("Signal handler registered (SIGUSR2)");
    std::thread::spawn(move || {
        for _ in signals.forever() {
            debug!("Received SIGUSR2");
            crate::meeting::toggle::toggle_meeting(&app_handle);
        }
    });
}
```

`utils.rs`: delete `cancel_current_operation` and its imports.

`cli.rs`: delete `toggle_transcription`, `toggle_post_process`, `cancel`. `lib.rs` single-instance handler keeps only the `--toggle-meeting` branch and the `else` show-window branch. Update `main.rs` if it references the removed fields.

`tray.rs`:
- Delete `TrayIconState::Transcribing` and its icon arms. Delete `copy_last_transcript`, `last_transcript_text`, and the secure-input `warning` input (pass `false` or remove the field and `secureInputWarning` handling).
- Delete the `cancel` menu item.
- In `lib.rs` `on_menu_event`, delete the `"secure_input_warning"`, `"copy_last_transcript"`, `"cancel"` arms.
- In `en/translation.json` `"tray"`, delete `copyLastTranscript`, `cancel`, `secureInputWarning`.
- Delete the tray resources `resources/tray_transcribing*.png` and `resources/transcribing.png` only if `grep -rn transcribing src-tauri/src src-tauri/tauri.conf.json` shows no remaining use.

`capabilities/default.json`: change `"windows": ["main", "recording_overlay", "intervention"]` to `["main", "intervention"]`. Grep `tauri.conf.json` for `recording_overlay` and remove any window entry.

- [ ] **Step 5: Drop Enigo and the clipboard plugin if unused**

`grep -rn "enigo\|clipboard_manager\|tauri_plugin_clipboard" src-tauri/src`. If the only hits are in `lib.rs` plugin registration, remove `.plugin(tauri_plugin_clipboard_manager::init())` and the `enigo`, `tauri-plugin-clipboard-manager` entries from `Cargo.toml`. Do the same for any other dependency `cargo machete` or a grep shows unused (`rdev` stays if `handy_keys` uses it). Remove the matching JS packages from `package.json` only if `grep -rn "@tauri-apps/plugin-clipboard-manager" src` is empty.

- [ ] **Step 6: Verify**

Run: `cd src-tauri && cargo clippy --all-targets -- -D warnings`
Expected: clean. For each `dead_code` warning, delete the item. It was dictation-only.

Run: `cargo test`
Expected: 6 passed.

The frontend will not typecheck until Task 5 (bindings changed). Do not run `bun run build` here.

- [ ] **Step 7: Commit**

```bash
git add -A && git commit -m "refactor: remove the dictation pipeline

Meetings never used the dictation recorder, coordinator, paste or
history; keeping them cost a second capture path and accessibility
permissions for a feature this product no longer has.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Settings: single LLM provider, removed fields, schema 3 migration

**Files:**
- Modify: `src-tauri/src/settings.rs`, `src-tauri/src/shortcut/mod.rs`, `src-tauri/src/llm_client.rs`, `src-tauri/src/meeting/{manager,observer,summary}.rs`, `src-tauri/src/lib.rs`
- Test: `src-tauri/src/settings.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Produces (Rust, exported to TS via specta):
  - `pub struct LlmProvider` (was `PostProcessProvider`), `pub enum LlmApiFormat` (was `PostProcessApiFormat`). Same fields and serde shape.
  - `AppSettings` fields `llm_provider_id: String`, `llm_providers: Vec<LlmProvider>`, `llm_api_keys: SecretMap`, `llm_models: HashMap<String, String>`, each with `#[serde(alias = "post_process_…")]`.
  - `AppSettings::{llm_provider() -> Option<&LlmProvider>, llm_model() -> Option<String>, llm_api_key() -> String, llm_provider_by_id(&str) -> Option<&LlmProvider>, llm_provider_mut(&str) -> Option<&mut LlmProvider>}`.
  - Commands renamed: `change_llm_base_url_setting`, `change_llm_api_format_setting`, `change_llm_api_key_setting`, `change_llm_model_setting`, `set_llm_provider`, `fetch_llm_models`. Same parameters as the `post_process` versions.
  - `fn apply_settings_migrations(settings: &mut AppSettings, settings_value: &serde_json::Value) -> bool` (existing, extended).
- Consumers updated in Task 5: frontend `commands.*` names.

- [ ] **Step 1: Write the failing migration test**

Add to `settings.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// A Handy-era store must keep the user's provider, key, model, and
    /// shortcut combo, and must not keep dictation bindings registered.
    #[test]
    fn handy_era_settings_keep_provider() {
        let stored = serde_json::json!({
            "settings_schema_version": 2,
            "bindings": {
                "transcribe": { "id": "transcribe", "name": "Transcribe", "description": "",
                    "default_binding": "option+space", "current_binding": "f13" },
                "transcribe_with_post_process": { "id": "transcribe_with_post_process",
                    "name": "", "description": "", "default_binding": "option+shift+space",
                    "current_binding": "option+shift+space" },
                "cancel": { "id": "cancel", "name": "", "description": "",
                    "default_binding": "escape", "current_binding": "escape" }
            },
            "post_process_enabled": true,
            "post_process_provider_id": "openai",
            "post_process_api_keys": { "openai": "sk-old", "anthropic": "sk-ant" },
            "post_process_models": { "openai": "gpt-4o", "anthropic": "claude-x" },
            "meeting_provider_id": "anthropic",
            "paste_method": "ctrl_v",
            "selected_model": "parakeet",
            "meeting_assistant_enabled": true
        });

        let mut settings: AppSettings =
            serde_json::from_value(stored.clone()).expect("old store deserializes");
        assert!(apply_settings_migrations(&mut settings, &stored));
        ensure_llm_defaults(&mut settings);

        assert_eq!(settings.llm_provider_id, "anthropic");
        assert_eq!(settings.llm_api_key(), "sk-ant");
        assert_eq!(settings.llm_model().as_deref(), Some("claude-x"));
        assert_eq!(settings.llm_api_keys.get("openai").map(String::as_str), Some("sk-old"));
        assert_eq!(settings.selected_model, "parakeet");
        assert!(settings.meeting_assistant_enabled);

        let ids: Vec<_> = settings.bindings.keys().cloned().collect();
        assert_eq!(ids, vec!["toggle_meeting".to_string()]);
        assert_eq!(settings.bindings["toggle_meeting"].current_binding, "f13");
        assert_eq!(settings.settings_schema_version, 3);

        // Idempotent: a second pass changes nothing.
        let again = serde_json::to_value(&settings).unwrap();
        let mut reloaded: AppSettings = serde_json::from_value(again.clone()).unwrap();
        assert!(!apply_settings_migrations(&mut reloaded, &again));
    }
}
```

Run: `cd src-tauri && cargo test handy_era_settings_keep_provider`
Expected: FAIL to compile (`llm_provider_id` unknown).

- [ ] **Step 2: Rename provider types and fields**

In `settings.rs`:
- Rename `PostProcessApiFormat` to `LlmApiFormat` and `PostProcessProvider` to `LlmProvider`. Update `llm_client.rs` imports and signatures.
- Rename the four fields with aliases:

```rust
    #[serde(default = "default_llm_provider_id", alias = "post_process_provider_id")]
    pub llm_provider_id: String,
    #[serde(default = "default_llm_providers", alias = "post_process_providers")]
    pub llm_providers: Vec<LlmProvider>,
    #[serde(default = "default_llm_api_keys", alias = "post_process_api_keys")]
    pub llm_api_keys: SecretMap,
    #[serde(default = "default_llm_models", alias = "post_process_models")]
    pub llm_models: HashMap<String, String>,
```

- Rename the `default_post_process_*` fns to `default_llm_*` and `ensure_post_process_defaults` to `ensure_llm_defaults` (update its caller in `get_settings`).
- Replace the `impl AppSettings` block (`active_post_process_provider`, `meeting_provider`, `meeting_model`, `meeting_api_key`, `post_process_provider`, `post_process_provider_mut`) with:

```rust
impl AppSettings {
    /// The single provider used by the observer, summaries, and digests.
    pub fn llm_provider(&self) -> Option<&LlmProvider> {
        self.llm_provider_by_id(&self.llm_provider_id)
    }

    pub fn llm_model(&self) -> Option<String> {
        let provider = self.llm_provider()?;
        self.llm_models.get(&provider.id).cloned()
    }

    pub fn llm_api_key(&self) -> String {
        self.llm_provider()
            .and_then(|p| self.llm_api_keys.get(&p.id).cloned())
            .unwrap_or_default()
    }

    pub fn llm_provider_by_id(&self, provider_id: &str) -> Option<&LlmProvider> {
        self.llm_providers.iter().find(|p| p.id == provider_id)
    }

    pub fn llm_provider_mut(&mut self, provider_id: &str) -> Option<&mut LlmProvider> {
        self.llm_providers.iter_mut().find(|p| p.id == provider_id)
    }
}
```

- Update callers: `meeting/manager.rs:38-44`, `meeting/observer.rs:128-141,218-224`, `meeting/summary.rs:138-144` change `meeting_provider()` → `llm_provider()`, `meeting_model()` → `llm_model()`, `meeting_api_key()` → `llm_api_key()`.
- In `shortcut/mod.rs`, rename the six provider commands (Interfaces above) and their field accesses. Delete `change_meeting_provider_setting`. Update `lib.rs` `collect_commands!`.

- [ ] **Step 3: Delete removed fields**

Delete from `AppSettings`, from `get_default_settings()`, and their `default_*` fns and enum types (if the type becomes unused):
`shortcut_activation`, `hold_threshold_ms`, `always_on_microphone`, `clamshell_microphone`, `overlay_position`, `overlay_style`, `history_limit`, `recording_retention_period`, `paste_method`, `clipboard_handling`, `auto_submit`, `auto_submit_key`, `post_process_enabled`, `meeting_provider_id`, `post_process_prompts`, `post_process_selected_prompt_id`, `mute_while_recording`, `append_trailing_space`, `lazy_stream_close`, `paste_delay_ms`, `paste_delay_after_ms`, `reliable_paste`, `typing_tool`, `external_script_path`, `extra_recording_buffer_ms`.

Delete `get_history_limit`, `get_recording_retention_period`. In `apply_settings_migrations`, delete the `shortcut_activation` and `overlay_style` migration blocks (their fields are gone).

In `get_default_settings()`, delete the `transcribe`, `transcribe_with_post_process`, `cancel` binding inserts. Change the Task 2 `toggle_meeting` default to the old transcribe default (`option+space` on macOS, `ctrl+space` on Windows/Linux, `alt+space` otherwise) and delete `default_meeting_shortcut`.

Before deleting each field, run `grep -rn "<field>" src-tauri/src/meeting src-tauri/src/managers/transcription.rs`. If anything matches, keep the field and note it in the commit message.

- [ ] **Step 4: Schema 3 migration**

Set `const CURRENT_SETTINGS_SCHEMA_VERSION: u32 = 3;`. In `apply_settings_migrations`, change the `< 2` block so it no longer sets the version itself (remove its `settings.settings_schema_version = …` line, keep `updated = true`), then add after it:

```rust
    if stored_schema_version < 3 {
        // Meet Assist has one provider. A dedicated meeting provider, if the
        // user picked one, was the one meetings actually used.
        if let Some(id) = settings_value
            .get("meeting_provider_id")
            .and_then(|v| v.as_str())
            .filter(|id| !id.is_empty())
        {
            settings.llm_provider_id = id.to_string();
        }
        // Dictation bindings are gone. Keep the user's dictation combo for the
        // meeting toggle so muscle memory carries over.
        let old_combo = settings
            .bindings
            .get("transcribe")
            .map(|b| b.current_binding.clone());
        let defaults = get_default_settings().bindings;
        settings.bindings.retain(|id, _| defaults.contains_key(id));
        if let Some(combo) = old_combo {
            let mut binding = defaults["toggle_meeting"].clone();
            binding.current_binding = combo;
            settings.bindings.insert("toggle_meeting".to_string(), binding);
        }
        settings.settings_schema_version = CURRENT_SETTINGS_SCHEMA_VERSION;
        updated = true;
    }
```

Also, in `get_settings`, right after the existing "Merge in any bindings added since this store was written" loop, add a prune so a binding id removed in a future version never lingers:

```rust
        let defaults = get_default_settings().bindings;
        let before = settings.bindings.len();
        settings.bindings.retain(|id, _| defaults.contains_key(id));
        if settings.bindings.len() != before {
            updated = true;
        }
```

- [ ] **Step 5: Run the test**

Run: `cd src-tauri && cargo test`
Expected: `7 passed`.

Run: `cargo clippy --all-targets -- -D warnings`
Expected: clean. Delete any now-unused enum (`PasteMethod`, `ClipboardHandling`, `AutoSubmitKey`, `TypingTool`, `OverlayPosition`, `OverlayStyle`, `ShortcutActivation`, `RecordingRetentionPeriod`, `LLMPrompt`) that clippy or the compiler flags.

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "refactor: one LLM provider and a schema 3 settings migration

Provider setup lived under post-processing names and a separate meeting
override; one provider removes the confusion. Aliases and the migration
keep existing keys, models and the user's shortcut combo.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Frontend: Assistant page, new sidebar, removed UI

**Files:**
- Create: `src/components/settings/assistant/AssistantSettings.tsx`, `src/components/settings/audio/AudioSettings.tsx`
- Move: `src/components/settings/PostProcessingSettingsApi/*` → `src/components/settings/assistant/provider/*`; `src/components/settings/meeting/MeetingAssistantSettings.tsx` → `src/components/settings/assistant/AssistantBehaviour.tsx`
- Modify: `src/components/Sidebar.tsx`, `src/App.tsx`, `src/components/settings/index.ts`, `src/components/settings/general/GeneralSettings.tsx`, `src/components/settings/advanced/AdvancedSettings.tsx`, `src/components/settings/debug/DebugSettings.tsx`, `src/components/settings/models/ModelsSettings.tsx`, `src/components/onboarding/{AssistantOnboarding,index}.tsx`, `src/components/settings/debug/OnboardingPreview.tsx`, `src/stores/settingsStore.ts`, `src/lib/types/events.ts`, `vite.config.ts`, `src/i18n/locales/en/translation.json`
- Delete: `src/overlay/`, `src/components/settings/history/`, `src/components/settings/post-processing/`, `src/components/AccessibilityPermissions.tsx`, `src/components/SecureInputWarning.tsx`, `src/components/onboarding/AccessibilityOnboarding.tsx`, and these settings components: `PasteMethod`, `TypingTool`, `ClipboardHandling`, `AutoSubmit`, `AppendTrailingSpace`, `PostProcessingToggle`, `PostProcessingSettingsPrompts`, `ShortcutActivation`, `ShowOverlay`, `HistoryLimit`, `RecordingRetentionPeriod`, `MuteWhileRecording`, `AlwaysOnMicrophone`, `ClamshellMicrophoneSelector`, `LazyStreamClose`, and debug `PasteDelay`, `ReliablePaste`, `HoldThreshold`, `RecordingBuffer`, `KeyboardDiagnostic`

**Interfaces:**
- Consumes: regenerated `src/bindings.ts` with the Task 4 command and type names (`LlmProvider`, `fetchLlmModels`, `setLlmProvider`, `changeLlmApiKeySetting`, `changeLlmModelSetting`, `changeLlmBaseUrlSetting`, `changeLlmApiFormatSetting`; settings fields `llm_provider_id`, `llm_providers`, `llm_api_keys`, `llm_models`).
- Produces: sidebar section ids `projects`, `meetings`, `assistant`, `audio`, `models`, `general`, `advanced`, `debug`, `about`.

- [ ] **Step 1: Regenerate bindings**

Run: `CMAKE_POLICY_VERSION_MINIMUM=3.5 bun run tauri dev`, wait for the window, then quit.
Expected: `git diff --stat src/bindings.ts` shows changes. `grep -c "post_process\|PostProcess" src/bindings.ts` prints `0`.

- [ ] **Step 2: Delete removed UI**

```bash
git rm -r src/overlay src/components/settings/history src/components/settings/post-processing
git rm src/components/AccessibilityPermissions.tsx src/components/SecureInputWarning.tsx \
  src/components/onboarding/AccessibilityOnboarding.tsx
cd src/components/settings
git rm PasteMethod.tsx TypingTool.tsx ClipboardHandling.tsx AutoSubmit.tsx AppendTrailingSpace.tsx \
  PostProcessingToggle.tsx PostProcessingSettingsPrompts.tsx ShortcutActivation.tsx ShowOverlay.tsx \
  HistoryLimit.tsx RecordingRetentionPeriod.tsx MuteWhileRecording.tsx AlwaysOnMicrophone.tsx \
  ClamshellMicrophoneSelector.tsx LazyStreamClose.tsx \
  debug/PasteDelay.tsx debug/ReliablePaste.tsx debug/HoldThreshold.tsx debug/RecordingBuffer.tsx \
  debug/KeyboardDiagnostic.tsx
```

Remove their exports from `settings/index.ts` and `settings/debug/index.ts`. In `vite.config.ts`, delete the `overlay: resolve(__dirname, "src/overlay/index.html"),` entry.

- [ ] **Step 3: Move provider components and rename calls**

```bash
mkdir -p src/components/settings/assistant
git mv src/components/settings/PostProcessingSettingsApi src/components/settings/assistant/provider
git mv src/components/settings/meeting/MeetingAssistantSettings.tsx src/components/settings/assistant/AssistantBehaviour.tsx
```

In `assistant/provider/*` and `src/stores/settingsStore.ts`, rename every `post_process_*` settings field to `llm_*`, `PostProcessProvider` to `LlmProvider`, and `commands.*PostProcess*` to the `Llm` names from Interfaces. Rename the hook `usePostProcessProviderState` to `useLlmProviderState` (file `useLlmProviderState.ts`). Rename the component export `PostProcessingSettingsApi` to `ProviderSettings`. Fix the imports in `AssistantOnboarding.tsx` to `../settings/assistant/provider/...`.

In `AssistantBehaviour.tsx`: rename the component to `AssistantBehaviour`, and delete the provider `Dropdown` block (`providerOptions` and the `settings.meeting.provider` `SettingContainer`). The single provider now comes from the card. Delete `settings/meeting/` once empty.

- [ ] **Step 4: Write `AssistantSettings.tsx`**

Readiness is already computed in Rust (`meeting/observer.rs` `readiness()`, exposed as `commands.getObserverReadiness()` returning `{ readiness: string; detail: string | null }`), and its messages already exist under `meetings.observer.readiness.*`. Reuse both. Do not re-derive "is the key missing" in TypeScript.

```tsx
import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type ObserverReadiness } from "@/bindings";
import { useSettings } from "../../../hooks/useSettings";
import { ProviderSettings } from "./provider";
import { AssistantBehaviour } from "./AssistantBehaviour";

/** Provider first, behaviour second: one page, one pass. */
export const AssistantSettings: React.FC = () => {
  const { t } = useTranslation();
  const { settings } = useSettings();
  const [ready, setReady] = useState<ObserverReadiness | null>(null);

  // Re-check whenever any setting changes (provider, key, model, opt-in).
  useEffect(() => {
    commands.getObserverReadiness().then(setReady);
  }, [settings]);

  const blocked =
    ready && ready.readiness !== "ready" && ready.readiness !== "disabled";

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      {blocked && (
        <p role="alert" className="px-4 py-2 text-sm text-red-500">
          {t(`meetings.observer.readiness.${ready.readiness}`, {
            detail: ready.detail ?? "",
          })}
        </p>
      )}
      <ProviderSettings />
      <AssistantBehaviour />
    </div>
  );
};
```

Match the wrapper classes to `GeneralSettings.tsx`. `ProviderSettings` and `AssistantBehaviour` each render their own `SettingsGroup`. If `ProviderSettings` doesn't, wrap it in `<SettingsGroup title={t("settings.assistant.provider.title")} description={t("settings.assistant.provider.description")}>`.

Rewrite the readiness strings in `en/translation.json` (`meetings.observer.readiness`) so they point at the new page:

```json
"readiness": {
  "disabled": "The meeting assistant is off. Turn it on in Assistant.",
  "no_provider": "No AI provider is set. Choose one in Assistant.",
  "unsupported_provider": "{{detail}} cannot run the meeting assistant. Choose another provider in Assistant.",
  "missing_api_key": "No API key for {{detail}}. Add one in Assistant.",
  "no_model": "No model is selected for {{detail}}. Pick one in Assistant."
}
```

"Test connection": if `provider/` has no such button, add one next to the model select in `provider/ModelSelect.tsx`, using the existing `Button` and calling `commands.fetchLlmModels(providerId)`. On `status === "ok"`, show `toast.success(t("settings.assistant.provider.test.ok", { count: result.data.length }))`. Otherwise show `toast.error(result.error)` (`toast` from `sonner`, already used in `App.tsx`). Check the result shape in `bindings.ts` first.

Add i18n keys under `settings` in `en/translation.json`:

```json
"assistant": {
  "provider": {
    "title": "AI provider",
    "description": "The model Meet Assist uses for nudges, summaries, and image digests.",
    "test": {
      "button": "Test connection",
      "ok": "Connected. {{count}} models available."
    }
  }
}
```

- [ ] **Step 5: Audio page and sidebar**

`src/components/settings/audio/AudioSettings.tsx`: move the shortcut, microphone, channel, audio feedback, output device and volume group out of `GeneralSettings.tsx`. Use `<ShortcutInput shortcutId="toggle_meeting" grouped={true} />` and delete the `transcribe`/`cancel` shortcut inputs and `ShortcutActivationSetting`. Add the system audio toggle by moving that `ToggleSwitch` out of `AssistantBehaviour.tsx` (it's capture, not assistant).

`GeneralSettings.tsx` keeps: theme, app language, autostart, tray icon, start hidden (move those three from `AdvancedSettings.tsx`). `ModelsSettings.tsx` gains `ModelSettingsCard` (from General), `CustomWords`, `FillerWordRemoval`, `ChineseScriptSetting`, `VoiceActivityDetection`, `TranslateToEnglish`/language if General had them. `AdvancedSettings.tsx` keeps `ModelUnloadTimeoutSetting`, `ExperimentalToggle`, `KeyboardImplementationSelector`, `AccelerationSelector`, `VadBackendSelector`, `AppDataDirectory`, and drops everything deleted plus `<MeetingAssistantSettings />`. `DebugSettings.tsx` drops the deleted debug panels.

`Sidebar.tsx` `SECTIONS_CONFIG` becomes, in this order:

```tsx
  projects: { labelKey: "sidebar.projects", icon: FolderKanban, component: ProjectsSection, enabled: () => true },
  meetings: { labelKey: "sidebar.meetings", icon: Mic, component: MeetingsSection, enabled: () => true },
  assistant: { labelKey: "sidebar.assistant", icon: Sparkles, component: AssistantSettings, enabled: () => true },
  audio: { labelKey: "sidebar.audio", icon: AudioLines, component: AudioSettings, enabled: () => true },
  models: { labelKey: "sidebar.models", icon: Cpu, component: ModelsSettings, enabled: () => true },
  general: { labelKey: "sidebar.general", icon: HandyHand, component: GeneralSettings, enabled: () => true },
  advanced: { labelKey: "sidebar.advanced", icon: Cog, component: AdvancedSettings, enabled: () => true },
  debug: { labelKey: "sidebar.debug", icon: FlaskConical, component: DebugSettings, enabled: (settings) => settings?.debug_mode ?? false },
  about: { labelKey: "sidebar.about", icon: Info, component: AboutSettings, enabled: () => true },
```

Format it multi-line as in the existing file (Prettier will do it). Import `AudioLines` from `lucide-react`, and drop the `History` import. Add `"assistant": "Assistant"` and `"audio": "Audio"` under `sidebar` in `en/translation.json`. Delete `sidebar.history` and `sidebar.postProcessing`.

- [ ] **Step 6: App shell and onboarding**

In `App.tsx`:
- `type OnboardingStep = "model" | "assistant" | "done";`
- Delete the accessibility step, `isReturningUser`, `checkAccessibilityPermission` and its import, `AccessibilityPermissions`, `SecureInputWarning`, `commands.initializeEnigo()` (keep `commands.initializeShortcuts()`), and the `RecordingErrorEvent` listener plus its type in `lib/types/events.ts`.
- `checkOnboardingStatus` routes a new user to `"model"` and a returning user to `"done"`. On macOS, keep the `checkMicrophonePermission` call only if `AssistantOnboarding` doesn't already request the microphone. Grep before deleting.

In `components/onboarding/index.ts`, drop the `AccessibilityOnboarding` export. In `debug/OnboardingPreview.tsx`, drop the `"accessibility"` preview step.

- [ ] **Step 7: Translations cleanup**

Delete keys from `en/translation.json` whose components are gone. Run `bun run check:translations` and `grep -rn "t(\"" src | …`, or rely on the script output. Delete `settings.general.shortcut.bindings.transcribe`, `.cancel`, `.transcribe_with_post_process`, and add:

```json
"toggle_meeting": {
  "name": "Meeting shortcut",
  "description": "Starts a meeting in your most recent project, or ends the running one."
}
```

Change `settings.general.shortcut.title` to `"Shortcut"` and its description to `"Start or end a meeting from anywhere."`

- [ ] **Step 8: Verify**

Run: `bun run build && bun run lint && bun run format:check && bun run check:translations`
Expected: all pass. `bunx tsc --noEmit` reports no errors.

Run: `grep -rn "post_process\|PostProcess\|history\|overlay" src --include=*.ts --include=*.tsx | grep -v bindings.ts`
Expected: no hits for removed features. Hits for the intervention card are fine.

Manual (`bun run tauri dev`):
1. The sidebar shows Projects, Meetings, Assistant, Audio, Models, General, Advanced, About.
2. On the Assistant page, turn the assistant on with a cloud provider and no key: the red message names the provider and says the key is missing. Pick a provider, paste a key, choose a model: the message disappears. "Test connection" shows a model count or the provider's error.
3. Start a meeting. A nudge arrives within the interval when the conversation warrants one.

- [ ] **Step 9: Commit**

```bash
git add -A && git commit -m "feat: assistant page with provider setup, audio page, no dictation UI

Setting up the assistant took three pages behind a post-processing
toggle; the provider now sits on an always-visible Assistant page.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Docs and final verification

**Files:**
- Modify: `docs/VISION.md`, `docs/ARCHITECTURE.md`, `AGENTS.md`, `README.md` (CLI and features mentions only)

- [ ] **Step 1: VISION.md**

Replace principle 4 with:

```markdown
4. **Reuse Handy's engine.** Transcription, models, and audio capture come from Handy. Dictation output (paste, post-processing, history) was removed; the global shortcut starts and ends meetings.
```

Delete any other "Dictation keeps working" sentence.

- [ ] **Step 2: ARCHITECTURE.md**

Delete the dictation pipeline section and diagram. Add a short "Entry points" paragraph: shortcut, tray, `--toggle-meeting`, and SIGUSR2 all call `meeting::toggle::toggle_meeting`. Settings: one LLM provider (`llm_*`), schema 3.

- [ ] **Step 3: AGENTS.md**

- Replace "Backend Structure" with the remaining modules. Remove `history.rs`, `signal_handle.rs`'s `send_transcription_input()` reference, and the overlay.
- Replace "Application Flow" with: start minimized, onboarding (model, assistant), shortcut toggles a meeting, transcript plus observer nudges, summary on end.
- CLI table: replace `--toggle-transcription`, `--toggle-post-process`, `--cancel` with `--toggle-meeting`.
- Under "GitHub workflow", change the upstream-sync bullet to "Engine, audio, and model fixes from Handy are welcome; dictation-output changes no longer apply."
- Add under "Code Style": "**Tests:** typecheck, clippy, lint, and the 7 data-safety tests (settings migration, store, summary). Do not add UI, end-to-end, or unit tests outside that core."

- [ ] **Step 4: Full verification**

```bash
cd src-tauri && cargo clippy --all-targets -- -D warnings && cargo test && cd ..
bun run build && bun run lint && bun run format:check && bun run check:translations
```

Expected: clippy clean, `7 passed`, frontend green.

Manual checklist from the spec's Verification section:
1. Copy a pre-transition `settings_store.json` into the app data dir (or keep the dev one from before Task 4) and launch. The provider, key, and model show on Assistant. The meeting shortcut is your old dictation combo.
2. Fresh install (empty app data dir): onboarding is model, then assistant, then done. No accessibility prompt.
3. Assistant setup is one page.
4. Shortcut and tray start and end meetings (with a project) and open Projects (with none).
5. A meeting transcribes both speakers, the observer nudges, and ending writes a summary.
6. No History or Post Processing page. Nothing pastes.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "docs: describe Meet Assist as a meeting-only product

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
