# Meet Assist Architecture

Meet Assist is a Tauri 2 desktop app: Rust backend, React/TypeScript frontend. It inherits [Handy](https://github.com/cjpais/Handy)'s dictation pipeline unchanged and adds a meeting subsystem beside it.

Status legend: **existing** means it ships today from Handy. **planned** means it is specified in [the design spec](superpowers/specs/2026-10-13-meeting-assistant-design.md) and not yet built.

## Two pipelines, one engine

```
Dictation (existing)
  shortcut -> mic capture -> VAD -> transcribe -> paste into active app

Meeting (planned)
  project sources -> indexer -> FTS5 chunks  <-- tools <-- observer

  start -> mic capture  ----\
                             +-> VAD per source -> utterances -> transcribe -> transcript lines
           system capture --/                                                      |
                                                                                   v
                                                             observer timer -> LLM judge -> intervention
                                                                                   |
                                                                                   v
                                                                         beep + floating card (user only)
```

Both pipelines share `TranscriptionManager`, the model store, settings, the LLM provider client, and the feedback-sound player.

## Backend (`src-tauri/src/`)

### Existing (from Handy)

| Path                          | Role                                                                                                               |
| ----------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| `lib.rs`                      | Tauri setup, manager initialization, tray, shortcuts                                                               |
| `managers/audio.rs`           | Microphone selection and recording via `handy-recorder`                                                            |
| `managers/transcription.rs`   | Model lifecycle, batch `transcribe(Vec<f32>)`, streaming preview                                                   |
| `managers/model.rs`           | Model download and discovery                                                                                       |
| `managers/history.rs`         | SQLite transcription history (pattern reused by the meeting store)                                                 |
| `audio_toolkit/audio/sink.rs` | `CaptureVad`: frames in, speech segments out                                                                       |
| `audio_toolkit/vad/`          | Silero VAD                                                                                                         |
| `llm_client.rs`               | Chat completion client, OpenAI and Anthropic formats, JSON schema responses, tool-calling loop (`send_with_tools`) |
| `audio_feedback.rs`           | Plays feedback sounds via rodio                                                                                    |
| `overlay.rs`                  | Always-on-top recording overlay window                                                                             |
| `settings.rs`                 | Persisted settings and migrations                                                                                  |
| `commands/`                   | Tauri commands exposed to the frontend via tauri-specta                                                            |

### Existing (Meet Assist)

| Path                  | Role                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| --------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `meeting/store.rs`    | SQLite (WAL journal) tables `projects`, `sources`, `meetings`, `sessions`, `transcript_lines`, and FTS5 `source_chunks` with the `porter unicode61` tokenizer. Search ANDs tokens, falls back to OR when that finds nothing, and returns the raw `bm25` score (lower is better). Write transactions are `IMMEDIATE` so concurrent writers wait on the busy timeout instead of failing; `transcript_lines` has a `(session_id, ts)` index. The `interventions` table holds observer nudges per session with a dismissed flag.                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `meeting/chunk.rs`    | Paragraph-aware chunking                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `meeting/indexer.rs`  | Source indexing: text, file, folder, link, and image caption to FTS5 chunks. Folder walks are capped by files, entries, and total bytes.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `meeting/manager.rs`  | `ProjectManager`: Tauri state, background indexing thread (resumes `pending` sources at startup, survives indexer panics), `source-status-payload` event                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `commands/project.rs` | Project and source CRUD, `search_sources`, `pick_source_path`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| `meeting/segment.rs`  | Utterance segmentation over VAD frames                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `meeting/capture.rs`  | `MeetingCapture`: mic-only capture, 16 kHz mono, `CaptureVad`, emits `Utterance`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| `meeting/session.rs`  | `MeetingManager` (Tauri state). Session state machine `Idle -> Live <-> Paused -> Ended`, plus the transcriber thread that feeds utterances to `TranscriptionManager::transcribe()` and appends `TranscriptLine`. Emits `meeting-state-payload`, `transcript-line-payload`, and `meeting-capture-warning-payload`. Pause and end drain queued utterances before updating the database.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `meeting/observer.rs` | Timer loop (default 20 s) that runs only when the opt-in `meeting_assistant_enabled` setting is on (off by default), a meeting is live, readiness is `ready`, and new lines exist since the last successful judgement (failed ticks retry the same window; after 5 consecutive failures the interval is multiplied by 4). Readiness is one of `ready`, `disabled`, `no_provider`, `unsupported_provider` (Apple Intelligence or a non-HTTP base URL), `missing_api_key` (non-local providers), `no_model`. Builds the judge prompt (context capped at 4096 chars), runs the tool loop through `llm_client::send_with_tools_with_client` behind the `Judge` trait, applies 60 s cooldown and dedupe, caps card text at 280 chars, then stores the intervention, plays the beep, and shows the intervention window. Falls back to a no-tools prompt when the first tooled request fails with anything but 401/403. Emits `observer-status-payload` (with readiness) on change. |
| `meeting/tools.rs`    | Observer tool definitions and handlers (`list_sources`, `search_context`, `read_source`, `read_transcript`, `list_meetings`), scoped to the meeting's project, results capped at 4 KB                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| `intervention.rs`     | Always-on-top, non-focus-stealing intervention card window, auto-hides after 30 s                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| `commands/meeting.rs` | Meeting list, detail, start, pause, resume, end, update, delete commands, plus `list_interventions`, `dismiss_intervention`, `hide_intervention_window`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |

### Planned (meeting subsystem)

| Path                                                          | Role                                                                                                                                                                                  |
| ------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `meeting/summary.rs`                                          | Session summary on end, then rolling meeting summary merge.                                                                                                                           |
| `audio_toolkit/audio/system_capture/{macos,windows,linux}.rs` | Trait `SystemCapture { start(cb), stop() }`. macOS CoreAudio process tap (14.2+) with ScreenCaptureKit fallback; Windows WASAPI loopback; Linux PulseAudio/PipeWire monitor via cpal. |

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

Existing: `meeting-state-payload`, `transcript-line-payload`, and `meeting-capture-warning-payload` (from `MeetingStatePayload`, `TranscriptLinePayload`, and `MeetingCaptureWarningPayload`; the frontend uses `events.meetingStatePayload`, `events.transcriptLinePayload`, and `events.meetingCaptureWarningPayload`), `source-status-payload` (tauri-specta derives the name from `SourceStatusPayload`; the frontend listens via `events.sourceStatusPayload`), `intervention-payload` and `observer-status-payload` (observer output, health and readiness; the frontend uses `events.interventionPayload` and `events.observerStatusPayload`), and `open-meeting-payload` (sent by the `focus_meeting` command when the card's "Open meeting" is clicked; `events.openMeetingPayload`).

## Frontend (`src/`)

### Existing (from Handy)

| Path                      | Role                                                                            |
| ------------------------- | ------------------------------------------------------------------------------- |
| `App.tsx`                 | Shell, onboarding, sidebar routing                                              |
| `components/settings/`    | Dictation and provider settings                                                 |
| `components/ui/`          | Shared primitives (buttons, inputs, selects). Meeting screens must reuse these. |
| `stores/settingsStore.ts` | Zustand settings store                                                          |
| `overlay/`                | Separate window entry for the recording overlay                                 |
| `bindings.ts`             | Generated by tauri-specta, do not edit by hand                                  |

### Existing (Meet Assist)

| Path                                           | Role                                                                                                                                                                                                                                                                                                      |
| ---------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `stores/projectStore.ts`                       | Zustand store for projects and sources, subscribes to `source-status-payload` (`events.sourceStatusPayload`)                                                                                                                                                                                              |
| `components/projects/ProjectsSection.tsx`      | Section root: loads projects, subscribes to status events, reloads open project's sources on mount                                                                                                                                                                                                        |
| `components/projects/ProjectList.tsx`          | Landing screen, first sidebar entry                                                                                                                                                                                                                                                                       |
| `components/projects/ProjectDetail.tsx`        | Brief editor and sources list                                                                                                                                                                                                                                                                             |
| `stores/meetingStore.ts`                       | Zustand store for the meeting list, active meeting, and live transcript buffer; subscribes to `meeting-state-payload`, `transcript-line-payload`, `meeting-capture-warning-payload` (toast), `intervention-payload`, `observer-status-payload` (readiness and health banners), and `open-meeting-payload` |
| `components/meetings/*`                        | Meetings section: list, detail with live transcript, controls                                                                                                                                                                                                                                             |
| `components/meetings/InterventionTimeline.tsx` | Assistant nudges in the meeting detail timeline, with dismiss                                                                                                                                                                                                                                             |
| `components/settings/meeting/`                 | `MeetingAssistantSettings`: opt-in toggle, observer provider, sensitivity, interval, beep (Advanced settings)                                                                                                                                                                                             |
| `intervention/`                                | Separate window entry for the floating intervention card, same pattern as `overlay/`                                                                                                                                                                                                                      |

## Failure handling (planned)

- System capture unavailable: run mic-only, emit `meeting:capture-warning`.
- LLM call fails: skip tick, retry next tick, banner after three consecutive failures.
- Transcription error: drop the utterance, log.
- Crash mid-session: on startup any `live` session becomes `paused`.
- Start while a session is live: rejected.

## Delivery order

1. Docs and vision.
2. Projects and sources: store, indexer, FTS5, project UI. (done)
3. Meetings under projects: store, commands, list and detail UI, mic-only capture and transcript. (done)
4. Tool calling in `llm_client`, observer loop, intervention window, beep. (done)
5. System audio capture, macOS first.
6. Summaries, image digests, continue polish, onboarding step.
