# Meet Assist Architecture

Meet Assist is a Tauri 2 desktop app: Rust backend, React/TypeScript frontend. It reuses [Handy](https://github.com/cjpais/Handy)'s transcription engine, model store, and audio capture. The dictation output path (paste, post-processing, history, overlay) was removed; the app is meeting-only.

Everything below ships today. The meeting subsystem was built in six plans from [the design spec](superpowers/specs/2026-10-13-meeting-assistant-design.md); all six are merged as of 2026-10-14.

## Pipeline

```
Meeting
  project sources -> indexer -> FTS5 chunks  <-- tools <-- observer

  start -> mic capture ("me") ----\
                                   +-> VAD per source -> utterances -> transcribe -> transcript lines
           system capture ("them") /      (callback -> resample/slice -> meet-sysaudio worker)   |
                                                                                   v
                                                             observer timer -> LLM judge -> intervention
                                                                                   |
                                                                                   v
                                                                         beep + floating card (user only)

  end -> Summarizer (session summary) -> rolling meeting summary      image source -> Describer -> digest
```

## Entry points

The global shortcut (the only binding, `toggle_meeting`), the tray Start/End Meeting item, the `--toggle-meeting` CLI flag, and SIGUSR2 all call `meeting::toggle::toggle_meeting`. It ends a running (live or paused) meeting; otherwise it starts a new, unfiled one. Once the meeting has 20 transcript lines, the observer loop runs `meeting::filing`: a tool loop (`list_projects`, `read_project`, `assign_meeting`, `create_project`, `rename_meeting`) that files the meeting into a project and renames it. It retries up to 3 times as the transcript grows and makes one last attempt when the meeting ends. Writes are conditional on `meetings.filed_by IS NULL`, so a user move (`move_meeting`, `filed_by = 'user'`) always wins. A filing shows on the floating card with Change and Undo.

## Settings

One LLM provider serves the observer, summaries, and vision digests: `llm_provider_id`, `llm_providers`, `llm_api_keys`, `llm_models` (serde aliases accept the old `post_process_*` names). Settings schema version 3 migrates `meeting_provider_id` into `llm_provider_id`, moves the old transcribe shortcut to `toggle_meeting`, prunes stale bindings, and drops the `apple_intelligence` provider.

## Backend (`src-tauri/src/`)

### Inherited from Handy

| Path                          | Role                                                                                                               |
| ----------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| `lib.rs`                      | Tauri setup, manager initialization, tray, shortcut registration                                                   |
| `managers/audio.rs`           | `create_capture_vad` and `microphone_config` helpers used by meeting capture                                       |
| `managers/transcription.rs`   | Model lifecycle, batch `transcribe(Vec<f32>)`, streaming preview                                                   |
| `managers/model.rs`           | Model download and discovery                                                                                       |
| `audio_toolkit/audio/sink.rs` | `CaptureVad`: frames in, speech segments out                                                                       |
| `audio_toolkit/vad/`          | Silero VAD                                                                                                         |
| `llm_client.rs`               | Chat completion client, OpenAI and Anthropic formats, JSON schema responses, tool-calling loop (`send_with_tools`) |
| `audio_feedback.rs`           | Plays feedback sounds via rodio                                                                                    |
| `settings.rs`                 | Persisted settings and migrations                                                                                  |
| `commands/`                   | Tauri commands exposed to the frontend via tauri-specta                                                            |

### Meet Assist

| Path                        | Role                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| --------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `meeting/store.rs`          | SQLite (WAL journal) tables `projects`, `sources`, `meetings`, `sessions`, `transcript_lines`, and FTS5 `source_chunks` with the `porter unicode61` tokenizer. Search ANDs tokens, falls back to OR when that finds nothing, and returns the raw `bm25` score (lower is better). Write transactions are `IMMEDIATE` so concurrent writers wait on the busy timeout instead of failing; `transcript_lines` has a `(session_id, ts)` index. The `interventions` table holds observer nudges per session with a dismissed flag.                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `meeting/chunk.rs`          | Paragraph-aware chunking                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `meeting/indexer.rs`        | Source indexing: text, file, folder, link, and image caption to FTS5 chunks. Folder walks are capped by files, entries, and total bytes.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `meeting/manager.rs`        | `ProjectManager`: Tauri state, vision digests (`Describer` trait, `LlmDescriber` using the LLM provider; images over 3.7 MB are skipped; gated by `meeting_vision_enabled` and the assistant opt-in `meeting_assistant_enabled`, which also gates summaries; digest capped at 1200 chars), background indexing thread (resumes `pending` sources at startup, survives indexer panics), `source-status-payload` event                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `commands/project.rs`       | Project and source CRUD, `search_sources`, `pick_source_path`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| `meeting/toggle.rs`         | `toggle_meeting`: the single entry point for shortcut, tray, CLI, and SIGUSR2; starts an unfiled meeting                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `meeting/filing.rs`         | Assistant filing of unfiled meetings: prompt, filing tools, attempt tracker, `LlmFiler`, `MeetingFiledPayload`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `meeting/segment.rs`        | Utterance segmentation over VAD frames                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `meeting/capture.rs`        | `MeetingCapture`: microphone capture (speaker `me`), 16 kHz mono, `CaptureVad`, emits `Utterance`. `MeetingSink` is the per-source VAD sink and takes a speaker tag (`me` or `them`).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| `meeting/session.rs`        | `MeetingManager` (Tauri state). `CombinedCapture` starts the mic capture and, when `meeting_system_audio_enabled` is on, `SystemCapture`; if system capture fails the meeting continues mic-only and a `meeting-capture-warning-payload` (source `system_audio`) is emitted. Session state machine `Idle -> Live <-> Paused -> Ended`, plus the transcriber thread that feeds utterances to `TranscriptionManager::transcribe()` and appends `TranscriptLine`. Emits `meeting-state-payload`, `transcript-line-payload`, and `meeting-capture-warning-payload`. Pause and end drain queued utterances before updating the database. After end, a summary job runs under `summary_lock` (jobs are serialised) and emits `session-summary-payload`.                                                                                                                                                                                                                            |
| `meeting/system_capture.rs` | `SystemCapture`: OS audio callback (macOS CoreAudio process tap on the default output, 14.2+; Windows WASAPI loopback; Linux PulseAudio monitor) pushes into a bounded queue, never doing VAD on the real-time thread. Resample and slice happen off-thread, then the `meet-sysaudio` worker owns a `MeetingSink("them")` and emits utterances. Input-capable default outputs are refused. Stop lets the worker exit even if the stream callback leaks.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `meeting/resample.rs`       | `MonoResampler` (any rate and channel count to 16 kHz mono) and `FrameSlicer` (fixed-size VAD frames)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| `meeting/summary.rs`        | `Summarizer` trait with `LlmSummarizer`; `run_summary_job` writes the session summary and the merged rolling meeting summary atomically (JSON-schema completion). Emits `SessionSummaryPayload` (`session-summary-payload`).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `meeting/observer.rs`       | Timer loop (default 20 s) that runs only when the opt-in `meeting_assistant_enabled` setting is on (off by default), a meeting is live, readiness is `ready`, and new lines exist since the last successful judgement (failed ticks retry the same window; after 5 consecutive failures the interval is multiplied by 4). Readiness is one of `ready`, `disabled`, `no_provider`, `unsupported_provider` (Apple Intelligence or a non-HTTP base URL), `missing_api_key` (non-local providers), `no_model`. Builds the judge prompt (context capped at 4096 chars), runs the tool loop through `llm_client::send_with_tools_with_client` behind the `Judge` trait, applies 60 s cooldown and dedupe, caps card text at 280 chars, then stores the intervention, plays the beep, and shows the intervention window. Falls back to a no-tools prompt when the first tooled request fails with anything but 401/403. Emits `observer-status-payload` (with readiness) on change. |
| `meeting/tools.rs`          | Observer tool definitions and handlers (`list_sources`, `search_context`, `read_source`, `read_transcript`, `list_meetings`), scoped to the meeting's project, results capped at 4 KB                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| `intervention.rs`           | Always-on-top, non-focus-stealing intervention card window, auto-hides after 30 s                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| `commands/meeting.rs`       | Meeting list, detail, start, pause, resume, end, update, delete commands, plus `list_interventions`, `dismiss_intervention`, `hide_intervention_window`, `focus_meeting`, `get_observer_readiness`, and `summarize_session` (re-run a summary)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |

### Settings added for the meeting subsystem

`meeting_assistant_enabled`, `meeting_system_audio_enabled` (default on), and `meeting_vision_enabled` (default on), alongside the observer sensitivity, interval, and beep keys. The provider and model come from the shared `llm_*` settings.

## Data model

```
Project        id, title, created_at, updated_at, brief
Source         id, project_id, kind (text|file|folder|link|image), title, locator, added_at, indexed_at, status, error, digest
SourceChunk    source_id, path, ord, content            (FTS5 virtual table)
Meeting        id, project_id?, title, created_at, updated_at, status (live|paused|ended), context, rolling_summary
Session        id, meeting_id, started_at, ended_at, summary
TranscriptLine id, session_id, ts, speaker (me|them), text
Intervention   id, session_id, ts, kind (loop|stuck|info|idea), text, reason, dismissed
```

Project, Source, SourceChunk, Meeting, Session, TranscriptLine, and Intervention exist today (interventions live in the `interventions` table).

A project holds sources and meetings. A meeting is a thread of sessions. "Continue" opens a new session under an existing meeting. The observer receives the rolling summary of past sessions plus the live window, never the full raw history.

## Observer contract

Base prompt, in order: project brief, rolling summary of the most recent other meeting in the project, meeting context, recent interventions, last ~3 minutes of transcript. Anything else the model fetches through tools:

| Tool              | Input                                     | Output                                           |
| ----------------- | ----------------------------------------- | ------------------------------------------------ |
| `list_sources`    | none                                      | id, kind, title, digest                          |
| `search_context`  | `query`, `limit`                          | snippets with source id, path, score             |
| `read_source`     | `source_id`, `path?`, `offset?`, `limit?` | raw chunk text                                   |
| `read_transcript` | `meeting_id?`, `from_ts?`, `to_ts?`       | transcript lines from any session in the project |
| `list_meetings`   | none                                      | id, title, dates, rolling summary                |

Limits: 5 tool calls per tick (then one final request with `tool_choice: none`), 15 s wall budget, 4 KB per tool result, 4096 chars of meeting context in the prompt. Providers without tool support fall back to the base prompt alone with a banner. The prompt explains speakers: `me` is the user, `them` everyone else.

Response schema:

```json
{
  "intervene": false,
  "kind": "loop | stuck | info | idea | null",
  "text": "string | null",
  "reason": "string"
}
```

Sensitivity setting adds one instruction: low restricts to loop and explicit stuck, high permits ideas freely.

## Events backend to frontend

Existing: `meeting-state-payload`, `transcript-line-payload`, and `meeting-capture-warning-payload` (from `MeetingStatePayload`, `TranscriptLinePayload`, and `MeetingCaptureWarningPayload`; the frontend uses `events.meetingStatePayload`, `events.transcriptLinePayload`, and `events.meetingCaptureWarningPayload`), `session-summary-payload` (from `SessionSummaryPayload`; `events.sessionSummaryPayload`), `source-status-payload` (tauri-specta derives the name from `SourceStatusPayload`; the frontend listens via `events.sourceStatusPayload`), `intervention-payload` and `observer-status-payload` (observer output, health and readiness; the frontend uses `events.interventionPayload` and `events.observerStatusPayload`), and `open-meeting-payload` (sent by the `focus_meeting` command when the card's "Open meeting" is clicked; `events.openMeetingPayload`).

## Frontend (`src/`)

### Inherited from Handy

| Path                      | Role                                                                            |
| ------------------------- | ------------------------------------------------------------------------------- |
| `App.tsx`                 | Shell, onboarding, sidebar routing                                              |
| `components/settings/`    | Audio, model, and general settings                                              |
| `components/ui/`          | Shared primitives (buttons, inputs, selects). Meeting screens must reuse these. |
| `stores/settingsStore.ts` | Zustand settings store                                                          |
| `bindings.ts`             | Generated by tauri-specta, do not edit by hand                                  |

### Meet Assist

| Path                                            | Role                                                                                                                                                                                                                                                                                                      |
| ----------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `stores/projectStore.ts`                        | Zustand store for projects and sources, subscribes to `source-status-payload` (`events.sourceStatusPayload`)                                                                                                                                                                                              |
| `components/projects/ProjectsSection.tsx`       | Section root: loads projects, subscribes to status events, reloads open project's sources on mount                                                                                                                                                                                                        |
| `components/projects/ProjectList.tsx`           | Landing screen, first sidebar entry                                                                                                                                                                                                                                                                       |
| `components/projects/ProjectDetail.tsx`         | Brief editor and sources list                                                                                                                                                                                                                                                                             |
| `stores/meetingStore.ts`                        | Zustand store for the meeting list, active meeting, and live transcript buffer; subscribes to `meeting-state-payload`, `transcript-line-payload`, `meeting-capture-warning-payload` (toast), `intervention-payload`, `observer-status-payload` (readiness and health banners), and `open-meeting-payload` |
| `components/meetings/*`                         | Meetings section: list, detail with live transcript, controls                                                                                                                                                                                                                                             |
| `components/meetings/SessionSummary.tsx`        | Session and rolling summary in the meeting detail                                                                                                                                                                                                                                                         |
| `components/onboarding/AssistantOnboarding.tsx` | Onboarding step after the model step: provider, base URL, API key, model, with a connection test                                                                                                                                                                                                          |
| `components/meetings/InterventionTimeline.tsx`  | Assistant nudges in the meeting detail timeline, with dismiss                                                                                                                                                                                                                                             |
| `components/settings/meeting/`                  | `MeetingAssistantSettings`: opt-in toggle, observer provider, sensitivity, interval, beep, system audio toggle, vision digest toggle (Advanced settings)                                                                                                                                                  |
| `intervention/`                                 | Separate window entry for the floating intervention card, a separate Tauri window                                                                                                                                                                                                                         |

## Failure handling

- System capture unavailable: run mic-only, emit `meeting-capture-warning-payload`; the UI shows a banner.
- LLM call fails: skip tick, retry next tick, banner after three consecutive failures.
- Transcription error: drop the utterance, log.
- Crash mid-session: on startup any `live` session becomes `paused`.
- Start while a session is live: rejected.

## Delivery order

1. Docs and vision.
2. Projects and sources: store, indexer, FTS5, project UI. (done)
3. Meetings under projects: store, commands, list and detail UI, mic-only capture and transcript. (done)
4. Tool calling in `llm_client`, observer loop, intervention window, beep. (done)
5. System audio capture, macOS first. (done; Windows and Linux built, unverified)
6. Summaries, image digests, continue polish, onboarding step. (done)
