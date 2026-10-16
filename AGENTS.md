# AGENTS.md

This file provides guidance to AI coding assistants working with code in this repository.

**This is CASE**, a private meeting companion. Read [docs/VISION.md](docs/VISION.md) first, then [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). The meeting subsystem is specified in [docs/superpowers/specs/2026-10-13-meeting-assistant-design.md](docs/superpowers/specs/2026-10-13-meeting-assistant-design.md); implementation plans live in `docs/superpowers/plans/`.

## Development Commands

**Prerequisites:**

- [Rust](https://rustup.rs/) (latest stable)
- [Bun](https://bun.sh/) package manager

**Core Development:**

```bash
# Install dependencies
bun install

# Run in development mode
bun run tauri dev
# If cmake error on macOS:
CMAKE_POLICY_VERSION_MINIMUM=3.5 bun run tauri dev

# Build for production
bun run tauri build

# Frontend only development
bun run dev        # Start Vite dev server
bun run build      # Build frontend (TypeScript + Vite)
bun run preview    # Preview built frontend
```

**Linting and Formatting (run before committing):**

```bash
bun run lint              # ESLint for frontend
bun run lint:fix          # ESLint with auto-fix
bun run format            # Prettier + cargo fmt
bun run format:check      # Check formatting without changes
bun run format:frontend   # Prettier only
bun run format:backend    # cargo fmt only
```

**Model Setup (Required for Development):**

```bash
mkdir -p src-tauri/resources/models
curl -L -o src-tauri/resources/models/silero-vad-v6.2-F32.gguf https://huggingface.co/handy-computer/silero-vad-v6.2-gguf/resolve/main/silero-vad-v6.2-F32.gguf
```

For detailed platform-specific build setup, see [BUILD.md](BUILD.md).

## Architecture Overview

CASE is a cross-platform, meeting-only desktop app built with Tauri 2.x (Rust backend + React/TypeScript frontend). It reuses the shared transcription engine; the dictation output path (paste, post-processing, history, overlay) was removed. See `docs/ARCHITECTURE.md`. The meeting modules are listed in the Meeting Subsystem section.

### Backend Structure (src-tauri/src/)

- `lib.rs` - Main entry point, Tauri setup, manager initialization
- `managers/` - Shared engine:
  - `audio.rs` - `create_capture_vad` and `microphone_config` helpers for meeting capture
  - `model.rs` - Model downloading and management
  - `transcription.rs` - Speech-to-text processing
- `audio_toolkit/` - Low-level audio processing:
  - `audio/` - Recording sink (VAD, levels, live streaming feed), output devices, WAV I/O
  - `vad/` - Voice Activity Detection (Silero VAD)
- `commands/` - Tauri command handlers for frontend communication
- `cli.rs` - CLI argument definitions (clap derive)
- `shortcut/` - Global shortcut handling (single binding, `toggle_meeting`)
- `settings.rs` - Application settings management and migrations
- `signal_handle.rs` - Unix signal handlers (SIGUSR2 toggles a meeting)
- `tray.rs` - Tray menu (Start/End Meeting)
- `llm_client.rs`, `audio_feedback.rs`, `utils.rs` - LLM client, feedback sounds, platform helpers

### Frontend Structure (src/)

- `App.tsx` - App shell: onboarding, sidebar, page routing (home, space, meeting, settings), live dock
- `components/` - React UI components:
  - `settings/` - Settings UI (`SettingsView.tsx` tabs)
  - `model-selector/` - Model management interface
  - `onboarding/` - First-run experience
  - `update-checker/` - App update notifications
  - `shared/` (`DocSection`), `ui/`, `icons/` (`Wordmark`) - Shared components
- `hooks/useSettings.ts` - Settings state management hook
- `stores/settingsStore.ts` - Zustand store for settings
- `bindings.ts` - Auto-generated Tauri type bindings (via tauri-specta)
- `intervention/` - Floating intervention card window entry point
- `lib/types.ts` - Shared TypeScript type definitions

### Key Architecture Patterns

**Manager Pattern:** Core functionality organized into managers (Audio, Model, Transcription) initialized at startup and managed via Tauri state.

**Command-Event Architecture:** Frontend → Backend via Tauri commands; Backend → Frontend via events.

**Pipeline Processing:** Audio (mic + system) → VAD → Whisper/Parakeet → transcript lines → observer → nudge card

**State Flow:** Zustand → Tauri Command → Rust State → Persistence (tauri-plugin-store)

### Meeting Subsystem

Built; see `docs/ARCHITECTURE.md` for the full table. Backend code lives under `src-tauri/src/meeting/`: `mod.rs`, `store.rs`, `chunk.rs`, `indexer.rs`, `manager.rs` (projects, sources, vision digests via `Describer`), `capture.rs` (mic capture, `MeetingSink`), `system_capture.rs` and `resample.rs` (system audio loopback tagged `them`), `segment.rs`, `session.rs` (`MeetingManager`, `CombinedCapture`), `toggle.rs` (`toggle_meeting`, the one entry point for shortcut, tray, CLI, and SIGUSR2), `filing.rs` (assistant files unfiled meetings into projects), `observer.rs`, `tools.rs`, and `summary.rs` (session and rolling summaries). Commands are in `commands/project.rs` and `commands/meeting.rs`; the floating card is `intervention.rs`. Frontend code lives under `src/components/projects/`, `src/components/meetings/`, `src/components/settings/meeting/`, `src/components/onboarding/AssistantOnboarding.tsx`, `src/stores/projectStore.ts`, `src/stores/meetingStore.ts`, and the separate window entry `src/intervention/`. Reuse `TranscriptionManager`, `CaptureVad`, `llm_client`, `audio_feedback`, and `components/ui`. Do not add a second LLM client, a second VAD, or new UI primitives. Follow DESIGN.md (t3.codes tokens in `src/styles/theme.css`, Granola-style shell).

### Technology Stack

**Core Libraries:**

- `transcribe-cpp` - Local Whisper-family inference (GGML/GGUF) with GPU acceleration
- `transcribe-rs` - ONNX speech recognition (Parakeet, Moonshine, SenseVoice, etc.)
- `handy-recorder` - Microphone capture: device selection, resampling to 16 kHz, failure detection
- `transcribe-cpp` (Silero VAD v6, CPU-only, in-process) / `earshot` - Voice Activity Detection
- `native-keys` - Global keyboard shortcuts
- `rodio` - Audio playback for feedback sounds

### Application Flow

1. **Start:** App starts minimized to tray, loads settings, initializes managers
2. **Onboarding:** Model, then assistant (LLM provider), then done
3. **Meeting:** The shortcut toggles a meeting (starts an unfiled one, or ends the running one); the assistant files it into a project once it knows what it is about
4. **Live:** Mic and system audio are transcribed into the transcript; the observer nudges with a beep and card
5. **End:** A session summary is written and merged into the rolling summary

### Settings System

Settings are stored using Tauri's store plugin with reactive updates:

- Keyboard shortcut (one binding, `toggle_meeting`)
- Audio devices (microphone/output selection)
- Model preferences (Small/Medium/Turbo/Large Whisper variants)
- One LLM provider (`llm_*` keys) for observer, summaries, and vision; settings schema version 3
- Audio feedback

### Single Instance Architecture

The app enforces single instance behavior — launching when already running brings the settings window to front rather than creating a new process. Remote control flag (`--toggle-meeting`) works by launching a second instance that sends args to the running instance via `tauri_plugin_single_instance`, then exits.

## Internationalization (i18n)

All user-facing strings must use i18next translations. ESLint enforces this (no hardcoded strings in JSX).

**Adding new text:**

1. Add key to `src/i18n/locales/en/translation.json`
2. Use in component: `const { t } = useTranslation(); t('key.path')`

**File structure:**

```
src/i18n/
├── index.ts           # i18n setup
├── languages.ts       # Language metadata
└── locales/
    ├── en/translation.json  # English (source)
    ├── de/, es/, fr/, ja/, ru/, zh/, ...
    └── ...
```

For translation contribution guidelines, see [CONTRIBUTING_TRANSLATIONS.md](CONTRIBUTING_TRANSLATIONS.md).

## Code Style

**Rust:**

- Run `cargo fmt` and `cargo clippy` before committing
- Handle errors explicitly (avoid unwrap in production)
- Use descriptive names, add doc comments for public APIs

**TypeScript/React:**

- Strict TypeScript, avoid `any` types
- Functional components with hooks
- Tailwind CSS for styling
- Path aliases: `@/` → `./src/`

**Tests:** typecheck, clippy, lint, and the 10 data-safety tests (settings migration, store, summary). Do not add UI, end-to-end, or unit tests outside that core.

## CLI Parameters

CASE supports command-line parameters on all platforms for integration with scripts, window managers, and autostart configurations.

**Implementation:** `cli.rs` (definitions), `main.rs` (parsing), `lib.rs` (applying), `meeting/toggle.rs` (shared logic)

| Flag               | Description                                                    |
| ------------------ | -------------------------------------------------------------- |
| `--toggle-meeting` | Start a meeting, or end the running one, on a running instance |
| `--start-hidden`   | Launch without showing the main window (tray icon visible)     |
| `--no-tray`        | Launch without system tray (closing window quits the app)      |
| `--debug`          | Enable debug mode with verbose (Trace) logging                 |

Dev-only flags: `--transcribe-file/-f`, `--model`, `--device-index`, `--list-devices`, `--list-models`, `--repeat`, `--json` (headless transcription benchmarking).

**Key design decisions:**

- CLI flags are runtime-only overrides — they do NOT modify persisted settings
- Remote control flags work via `tauri_plugin_single_instance`: second instance sends args, then exits
- `meeting::toggle::toggle_meeting` is shared by the shortcut, tray, signal handler, and CLI

## Debug Mode

Access debug features: `Cmd+Shift+D` (macOS) or `Ctrl+Shift+D` (Windows/Linux)

## Platform Notes

- **macOS**: Metal acceleration, microphone and audio capture permissions required
- **Windows**: Vulkan acceleration, code signing. Implicit Vulkan layers (overlays, capture hooks) are disabled for the CASE process via `VK_LOADER_LAYERS_DISABLE=~implicit~` set in `main.rs`; opt out with `CASE_KEEP_VULKAN_IMPLICIT_LAYERS=1` or by setting `VK_LOADER_LAYERS_DISABLE` yourself
- **Linux**: OpenBLAS + Vulkan, limited Wayland support
- **Nix/NixOS**: the Nix package sets `CASE_DISABLE_UPDATER=1` to force-disable the self-updater at runtime without touching the persisted setting (self-update can't work against an immutable `/nix/store`)

## Troubleshooting

See the [Troubleshooting](README.md#troubleshooting) section in README.md.

## GitHub workflow for AI coding assistants

Meeting-subsystem features are in scope when they follow the design spec. Keep these rules:

- **Opening a PR:** Read [`.github/PULL_REQUEST_TEMPLATE.md`](.github/PULL_REQUEST_TEMPLATE.md) and fill every section. The "Spec or Plan" section must link the spec section or plan task the PR implements. Leave "Human Written Description" as a TODO for the human contributor.
- **Opening an issue:** Use [`.github/ISSUE_TEMPLATE/bug_report.md`](.github/ISSUE_TEMPLATE/bug_report.md) for bugs. Feature ideas for the meeting subsystem go in a spec change under `docs/superpowers/specs/`, not an issue.
- **Upstream sync:** Engine, audio, and model fixes from shared dependencies are welcome; dictation-output changes no longer apply. Cherry-pick with the upstream commit referenced in the message.
- **Translations:** Follow [CONTRIBUTING_TRANSLATIONS.md](CONTRIBUTING_TRANSLATIONS.md). New `meetings.*` keys land in `en/translation.json` first; other locales fall back to English.

**Commits:** Use conventional commit prefixes (`feat:`, `fix:`, `docs:`, `refactor:`, `chore:`). Focus the message on _why_, not _what_.
