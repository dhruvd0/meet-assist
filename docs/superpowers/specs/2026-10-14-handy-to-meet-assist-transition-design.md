# Handy to Meet Assist Transition Design

Date: 2026-10-14
Status: Draft, awaiting review

## Goal

Meet Assist still carries Handy's dictation product: paste into the active app, post-processing, push-to-talk, a dictation history, Handy's test and CI surface. A user who only wants meetings must pass through those concepts. The clearest symptom: to give the meeting assistant an API key, the user enables "post processing", opens the Post Processing page that then appears, and adds the key there.

This change makes Meet Assist a meeting product. Dictation output is removed. The transcription engine, audio capture, and models stay. Provider setup becomes a first-class, always-visible Assistant page. The test suite is cut to a small core plus typecheck and lint.

## Decisions

These were settled during brainstorming:

1. **Dictation output is removed.** No paste into the active app, no post-processing of dictated text, no dictation history. Transcription itself keeps working as it does now inside meetings.
2. **The global shortcut is kept and repurposed** to start and end a meeting.
3. **Important settings are not buried.** The assistant and its provider live on their own sidebar page, always visible.
4. **One provider.** A single active provider (key, base URL, model) serves the observer, summaries, and image digests. The per-feature `meeting_provider_id` override goes away.
5. **Tests are cut to a tiny core.** UI, end-to-end, and almost all unit tests are deleted. The gate is typecheck, clippy, lint, format, and seven data-safety tests (listed below).

## Sub-project context

This is sub-project 1 of 3:

1. **This spec:** scope cut, settings rework, test cut.
2. **Quick-start meeting:** the shortcut starts an unfiled meeting; after enough transcript, the AI files it into a project using app-data tools (list projects, read briefs, assign the meeting, create a project if none fits, rename the meeting). Separate spec.
3. **Journey UX:** onboarding, banners, intervention card, logo and icons. Separate spec.

This spec gives the shortcut an interim start behaviour (below) that sub-project 2 replaces.

## Non-goals

- Renaming crates or internal identifiers such as `handy-recorder` or `handy_keys`. No user benefit, high churn.
- Visual rebrand (logo, icons, colours). Sub-project 3.
- AI project filing. Sub-project 2.
- Changing meeting capture, the observer, summaries, or the intervention card behaviour.

## Sidebar and settings layout

| Page                            | Contents                                                                                                                                                  |
| ------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Projects                        | Unchanged.                                                                                                                                                |
| Meetings                        | Unchanged.                                                                                                                                                |
| Assistant (new, always visible) | Provider card first: provider, base URL, API key, model, "Test connection". Then assistant behaviour: on/off, sensitivity, interval, beep, image digests. |
| Audio (replaces General)        | Meeting shortcut, microphone, system audio toggle, output device, audio feedback and volume.                                                              |
| Models                          | Transcription model, language, custom words, filler-word removal, VAD.                                                                                    |
| General                         | Theme, app language, autostart, tray icon, start hidden.                                                                                                  |
| Advanced                        | Only settings that still apply after the cut.                                                                                                             |
| Debug                           | Hidden unless debug mode, as today. Dictation-only diagnostics removed.                                                                                   |
| About                           | Unchanged.                                                                                                                                                |

Removed pages: History, Post Processing.

### Assistant page behaviour

- The assistant on/off toggle stays. It is the privacy opt-in that gates every cloud call (observer, summaries, digests). Its meaning does not change.
- The provider card sits above the toggle on the same page and is always editable.
- Turning the assistant on with no usable provider (no model chosen, or a non-local provider with an empty key) keeps the toggle on and shows an inline message above the provider card saying what is missing. Nothing navigates away.
- The missing-piece message reuses the backend's `get_observer_readiness` command and the existing `meetings.observer.readiness.*` strings, reworded to point at the Assistant page. The rule is not duplicated in TypeScript.
- "Test connection" calls the existing model-listing command (`fetch_post_process_models`, renamed below) and reports success or the provider's error text.
- `MeetingAssistantSettings` and the components in `PostProcessingSettingsApi/` move under a new `src/components/settings/assistant/` folder. `AssistantOnboarding` imports them from there. No new UI primitives.

## Backend removal

Meetings depend on `AudioRecordingManager` and the transcription manager. They do not use the transcription coordinator, paste, clipboard, history, or the recording overlay. The one cross-dependency is `intervention.rs` calling `overlay::get_monitor_with_cursor`.

### Deleted

- `transcription_coordinator.rs`
- `clipboard.rs`, `input.rs`, `paste_tx/`
- `secure_input.rs`
- `managers/history.rs`, `commands/history.rs`
- `apple_intelligence.rs` (only a post-processing provider; the meeting LLM never calls it)
- `overlay.rs`, after moving `get_monitor_with_cursor` into `intervention.rs`
- `actions.rs` entirely. The one remaining binding is routed directly in `shortcut/handler.rs` to `meeting::toggle::toggle_meeting`, which also serves the tray, `--toggle-meeting`, and SIGUSR2.
- `signal_handle.rs` logic tied to dictation; it now forwards only the meeting toggle.
- Enigo initialisation (`initialize_enigo` command) and the accessibility permission check.

### Kept and trimmed

- `managers/audio.rs` keeps only `create_capture_vad` and `microphone_config`. Planning found that meeting capture (`meeting/capture.rs`, `meeting/system_capture.rs`) uses only those two helpers plus the `selected_microphone`, `selected_channel`, and `vad_backend` settings. `AudioRecordingManager` is the dictation recorder, so it is deleted along with always-on microphone, mute while recording, clamshell microphone, lazy stream close, and extra recording buffer. The model idle-unload check keeps only the "meeting is live" guard.
- `chinese_script.rs`, translate-to-English, custom words, and filler-word removal stay: `TranscriptionManager` applies them to meeting transcripts.
- Dev CLI: `--transcribe-file`, `--model`, `--device-index`, `--list-devices`, `--list-models`, `--repeat`, `--json`.

### Shortcut

- Shortcut infrastructure (`shortcut/tauri_impl.rs`, `shortcut/handy_keys.rs`, `shortcut/mod.rs`) stays.
- Bindings shrink to one: `toggle_meeting`. Push-to-talk and activation modes are removed, and so are the transcribe, post-process, and cancel bindings.
- Interim behaviour: if a meeting is live, end it. Otherwise start a new meeting in the most recently used project: the project of the meeting with the latest session start, or the newest project if no meeting exists. If there are no projects, show and focus the main window on Projects.
- The action calls `MeetingManager` through the same paths the meeting commands use. No new meeting logic.

### CLI

| Before                                   | After              |
| ---------------------------------------- | ------------------ |
| `--toggle-transcription`                 | `--toggle-meeting` |
| `--toggle-post-process`                  | removed            |
| `--cancel`                               | removed            |
| `--start-hidden`, `--no-tray`, `--debug` | unchanged          |

No compatibility alias for the old flag. This fork has no external script users yet.

### Tray

Dictation entries are removed. The tray gets "Start meeting" or "End meeting", depending on state, using the same action as the shortcut.

## Settings migration

Settings deserialization already ignores unknown fields, so removed fields need no migration code.

Provider fields are renamed, keeping the old names as serde aliases so existing stores load:

| Old                        | New               |
| -------------------------- | ----------------- |
| `post_process_provider_id` | `llm_provider_id` |
| `post_process_providers`   | `llm_providers`   |
| `post_process_api_keys`    | `llm_api_keys`    |
| `post_process_models`      | `llm_models`      |

Type and command renames follow: `PostProcessProvider` becomes `LlmProvider`, `fetch_post_process_models` becomes `fetch_llm_models`. `ensure_post_process_defaults` becomes `ensure_llm_defaults`.

One explicit migration runs on load: if `meeting_provider_id` is non-empty, its value replaces `llm_provider_id`. The field is then dropped from the struct. A user who had picked a dedicated meeting provider keeps it as the single provider.

Deleted fields include `post_process_enabled`, `post_process_prompts`, `post_process_selected_prompt_id`, `paste_method`, `clipboard_handling`, `auto_submit`, `auto_submit_key`, `append_trailing_space`, `paste_delay_ms`, `paste_delay_after_ms`, `typing_tool`, `history_limit`, recording retention, overlay position and style, and any recording settings the grep above clears for removal.

## Frontend removal

Deleted components: `PasteMethod`, `TypingTool`, `ClipboardHandling`, `AutoSubmit`, `AppendTrailingSpace`, `PostProcessingToggle`, `PostProcessingSettingsPrompts`, `post-processing/`, `history/`, `ShortcutActivation`, `ShowOverlay`, `HistoryLimit`, `RecordingRetentionPeriod`, `AccessibilityPermissions`, `SecureInputWarning`, `onboarding/AccessibilityOnboarding`, the overlay window entry under `src/overlay/`, and debug panels for paste delay, reliable paste, and keyboard diagnostics tied to dictation. `MuteWhileRecording`, `AlwaysOnMicrophone`, `ClamshellMicrophoneSelector`, and `LazyStreamClose` are deleted with the recorder.

Onboarding drops the accessibility step: model, then assistant, then done.

`bindings.ts` is regenerated. Translation keys for removed UI are deleted from `en/translation.json` only; other locales fall back to English and are cleaned when next touched. `bun run check:translations` must pass.

## Test cut

### Deleted

- Playwright: `tests/app.spec.ts`, `playwright.config.*`, the `@playwright/test` dependency, the `test:playwright*` scripts, and `.github/workflows/playwright.yml`.
- Bun unit tests: `src/lib/utils/keyboard.test.ts`, `src/components/update-checker/portableInstaller.test.ts`, `src/components/settings/history/clipboard.test.ts`, and the `test:keyboard` script and its CI step.
- Every Rust `#[test]` and `#[tokio::test]` except the core below. Test-only helpers and fakes (`new_for_test`, fake describers and summarizers, slicer test helpers) go too unless a core test needs them.

### Kept core (7 tests)

Each guards against losing or corrupting user data:

| Test                                                    | File                 | Guards                                                                                                                            |
| ------------------------------------------------------- | -------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `handy_era_settings_keep_provider` (new)                | `settings.rs`        | Old settings JSON with `post_process_*` and `meeting_provider_id` loads into `llm_*` with key, model, and chosen provider intact. |
| `open_enables_wal`                                      | `meeting/store.rs`   | Database opens and initialises its schema.                                                                                        |
| `startup_recovery_closes_open_sessions_and_pauses_live` | `meeting/store.rs`   | Crash recovery does not leave sessions stuck open.                                                                                |
| `delete_project_cascades_sources_and_chunks`            | `meeting/store.rs`   | No orphaned rows after deleting a project.                                                                                        |
| `delete_meeting_and_delete_project_cascade_lines`       | `meeting/store.rs`   | No orphaned transcript lines.                                                                                                     |
| `job_stores_session_and_rolling_summary`                | `meeting/summary.rs` | Summaries are written.                                                                                                            |
| `merge_failure_stores_nothing`                          | `meeting/summary.rs` | A failed merge does not half-write a summary.                                                                                     |

### CI after the cut

- Delete `playwright.yml`.
- `test.yml` keeps `cargo test`, which now runs only the core.
- `code-quality.yml` keeps translations, model-language coverage, lint, and format checks. It gains `bunx tsc --noEmit` and `cargo clippy --all-targets -- -D warnings`.
- Other workflows (`build-test.yml`, `pr-test-build.yml`, `build.yml`, `main-build.yml`, `release.yml`, `nix-check.yml`) lose any Playwright or bun-test steps and are otherwise unchanged.

## Documentation

- `docs/VISION.md`: principle 4 becomes "Reuse Handy's engine. Transcription, models, and audio capture come from Handy; dictation output was removed." Remove "Dictation keeps working."
- `docs/ARCHITECTURE.md`: drop the dictation pipeline section. Describe the shortcut and tray as meeting toggles.
- `AGENTS.md`: replace the dictation module list and application flow with the meeting flow. Update the CLI table. Upstream cherry-picks are limited to engine, audio, and model fixes. Note the test policy: typecheck, clippy, lint, and the core tests only, with no new UI or end-to-end tests.

## Verification

Automated:

- `bun run build` (includes `tsc`)
- `bun run lint`, `bun run format:check`, `bun run check:translations`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test` (the 7 core tests)

Manual, on macOS with `bun run tauri dev`:

1. An existing Handy-era settings store loads. The provider, key, and model show on the Assistant page.
2. Fresh install: onboarding is model, then assistant, then done. No accessibility prompt.
3. On the Assistant page, turn the assistant on, pick a provider, paste a key, run "Test connection". All on one page.
4. The shortcut starts a meeting in the last-used project and ends it on the second press. The tray entry matches.
5. A meeting transcribes both speakers, the observer nudges, and ending writes a summary.
6. History and Post Processing pages are gone. No paste happens anywhere.

## Risks

- **Dead shared code.** Removing dictation may leave helpers that only the deleted code called. Clippy's dead-code warnings, enforced with `-D warnings`, catch these.
- **Stale shortcut bindings.** Old stores contain `transcribe`, `transcribe_with_post_process`, and `cancel` bindings, and the load path only adds bindings, never removes them. The schema 3 migration prunes them and carries the `transcribe` combo over to `toggle_meeting`.
- **Fewer tests.** Regressions in observer, session, and capture logic are now caught only by the manual pass. This trade-off was chosen deliberately.
