# Meet Assist: Meeting Assistant Design

Date: 2026-10-13
Status: approved. Amended 2026-10-13 with projects, sources, and agentic context retrieval (sections 2a, 3a, 5).

## 1. Intent

Turn this Handy fork into **Meet Assist**: a private meeting copilot. It listens to a live call, keeps a transcript, and speaks up (to the user only) when the conversation loops, someone is stuck, a quick fact is missing, or an obvious idea is untried.

Agreed decisions:

- **Audience:** only the user hears/sees interventions. No audio into the call.
- **Audio:** microphone plus system audio (remote participants). Mic-only fallback when system capture is unavailable.
- **Brain:** existing LLM provider client (`llm_client.rs`); user picks Anthropic, OpenAI, OpenRouter, Ollama, LM Studio, or any OpenAI/Anthropic-format endpoint.
- **Knowledge:** model knowledge, project sources, meeting context. No web search, no remote integrations.
- **Approach:** meeting mode is a new subsystem beside dictation. Dictation stays intact.
- **Product model:** projects contain meetings. Meetings are persisted threads of sessions. Start, pause, resume, end, and continue later.
- **Context:** projects hold sources (pasted text, local files and folders including checked-out repos, links, images). The observer loads a small base prompt and retrieves the rest through MCP-shaped internal tools.
- **Visuals:** keep Handy's design language and components. Minor differences only.

Non-goals for v1: speaking into the call, web search, speaker diarization beyond me/them, calendar or chat integrations, user-editable observer prompt, Windows/Linux system-audio parity if blocked (mic-only there is acceptable), folder watching, embeddings or vector search, remote GitHub API access, a standalone MCP server process, cloud sync.

## 2. Product model and UX

### Entities

```
Meeting
  id, title (auto "Meeting Oct 13 14:02", editable)
  created_at, updated_at
  status: live | paused | ended
  context: text            (agenda, notes, docs pasted by user; editable any time)
  rolling_summary: text?   (merged across sessions, ~300 words cap)

Session (belongs to Meeting)
  id, meeting_id, started_at, ended_at?, summary?

TranscriptLine (belongs to Session)
  id, session_id, ts, speaker: me | them, text

Intervention (belongs to Session)
  id, session_id, ts, kind: loop | stuck | info | idea, text, reason, dismissed
```

### Flows

- **Start:** button or global shortcut. New meeting plus first session. Capture begins. Tray shows live state.
- **Pause / Resume:** capture stops, session stays open. Resume appends to same session.
- **End:** capture stops, session closed, session summary generated, rolling summary updated, meeting `ended`.
- **Continue:** from list, any `ended` or `paused` meeting. New session under same meeting. Observer gets rolling summary of past sessions plus live window.
- **Intervention:** beep plus floating card (always-on-top window, top-right). Kind icon plus one to three sentences. Dismiss or open meeting. Auto-hide after 30 s unless hovered. Also appears in meeting timeline.

### Screens

- **Meetings list:** new first sidebar entry and default landing when any meeting exists. Cards: title, last session date, status badge, line count. New, Continue, Delete.
- **Meeting detail:** header (title edit, status, Start/Pause/Resume/End). Context editor. Transcript thread as collapsible dated session segments, me/them colouring, autoscroll when live. Intervention timeline. Session summaries.
- **Settings, new group "Meeting":** provider (reuses post-process provider components), intervention sensitivity (low/medium/high), observer interval, system-audio source, beep sound, keep audio recordings (off by default).
- **Onboarding:** one skippable step for system-audio permission and provider check.

Design language unchanged: reuse `components/ui`, Tailwind tokens, sidebar layout, overlay window conventions. New items: a meetings icon, me/them colours, kind icons.

## 2a. Projects and sources

### Entities

```
Project
  id, title, created_at, updated_at
  brief: text            always in the prompt; user-written; ~300 words cap

Source (belongs to Project)
  id, project_id, kind: text | file | folder | link | image
  title, locator (path or URL), added_at, indexed_at
  status: pending | ready | failed, error: text?
  digest: text?          ~200 word LLM summary written at add time

SourceChunk (FTS5 virtual table)
  source_id, path (file within a folder, or section label), ord, content

Meeting gains project_id (nullable; quick meetings without a project remain possible)
```

### Flows

- **Projects list** is the first sidebar entry and default landing. Cards: title, meeting count, source count, last activity. New, Open, Delete.
- **Project detail:** brief editor, sources list with status, meetings list with Continue, New meeting button.
- **Add source:** file or folder picker, link or pasted-text box, image drop zone. Indexing runs in the background; per-source status shown.
- **Folder:** walked with `.gitignore` respected, text files only, 512 KB per-file cap, chunked at ~1,500 characters. Re-index button. No watching.
- **Link:** fetched once, HTML stripped to text, chunked. Re-fetch button.
- **Image:** copied into app data. If the configured provider is vision-capable, the digest is model-written at add time; otherwise the user types a caption which becomes the digest.
- **Text:** chunked directly.
- **Quick meeting:** from the Meetings tab without a project; behaves as before with no sources.

## 3. Backend architecture

All new code under `src-tauri/src/meeting/` unless noted.

| Module                | Responsibility                                                                                                                                                                                                                                                                                            |
| --------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `mod.rs`              | `MeetingManager` (Tauri state). Session state machine `Idle → Live ⇄ Paused → Ended`. Public: `start(meeting_id?)`, `pause`, `resume`, `end`, `update_context`, `dismiss_intervention`.                                                                                                                   |
| `capture.rs`          | `MeetingCapture`. Two sources: mic via handy-recorder (reuse `open_microphone` in `managers/audio.rs`), system audio via `audio_toolkit/audio/system_capture/`. Both resampled to 16 kHz mono. One `CaptureVad` per source. Emits `Utterance { speaker, samples, started_at }` on speech end.             |
| `transcriber.rs`      | Worker thread. Consumes utterances in order, calls `TranscriptionManager::transcribe()`, appends `TranscriptLine`, emits `meeting:transcript-line`. Keeps model loaded for session via existing unload-timeout.                                                                                           |
| `observer.rs`         | Timer loop, default 20 s. Runs only when new lines exist. Builds the base prompt and runs the tool loop (section 3a) via `llm_client::send_with_tools`. Cooldown 60 s, dedupe against recent interventions. On intervene: beep via `audio_feedback`, store, emit `meeting:intervention`.                  |
| `tools.rs`            | MCP-shaped tool definitions and handlers: `list_sources`, `search_context`, `read_source`, `read_transcript`, `list_meetings`. One implementation, reusable by a future MCP server.                                                                                                                       |
| `indexer.rs`          | Background source indexing: folder walk (`ignore` crate), link fetch and HTML-to-text, image copy and digest, chunking, FTS5 writes.                                                                                                                                                                      |
| `summary.rs`          | On session end: session summary, then rolling summary merge.                                                                                                                                                                                                                                              |
| `manager.rs`          | `ProjectManager` (Tauri state). Owns the store, runs source indexing on a worker thread, resumes `pending` sources at startup, emits `source-status-payload`.                                                                                                                                             |
| `store.rs`            | SQLite via rusqlite (WAL journal), same pattern as `managers/history.rs`. Tables `projects`, `sources`, `source_chunks` (FTS5, `porter unicode61` tokenizer), `meetings`, `sessions`, `transcript_lines`, `interventions`. Schema version column for migrations. Audio not stored unless setting enabled. |
| `commands/meeting.rs` | Tauri commands with specta bindings: `list_meetings`, `get_meeting`, `start_meeting(project_id?)`, `pause_meeting`, `resume_meeting`, `end_meeting`, `update_meeting_context`, `update_meeting_title`, `delete_meeting`, `dismiss_intervention`.                                                          |
| `commands/project.rs` | `list_projects`, `get_project`, `create_project`, `update_project_brief`, `update_project_title`, `delete_project`, `add_source`, `remove_source`, `reindex_source`, `set_source_caption`.                                                                                                                |

`Judge` trait wraps the LLM call so tests inject a fake.

### 3a. Observer tool loop

Base prompt per tick, in order: system prompt, project brief, rolling summary of the most recent other meeting in the same project, current meeting context, recent interventions, last ~3 minutes of transcript. Everything else is retrieved on demand.

Tools (JSON schema in MCP tool format, defined once in `meeting/tools.rs`):

| Tool              | Input                                     | Output                                                       |
| ----------------- | ----------------------------------------- | ------------------------------------------------------------ |
| `list_sources`    | none                                      | id, kind, title, digest per source                           |
| `search_context`  | `query`, `limit` (default 8)              | snippets with source id, path, score                         |
| `read_source`     | `source_id`, `path?`, `offset?`, `limit?` | raw chunk text                                               |
| `read_transcript` | `meeting_id?`, `from_ts?`, `to_ts?`       | transcript lines from any session in the project             |
| `list_meetings`   | none                                      | id, title, dates, rolling summary per meeting in the project |

Loop limits: 5 tool calls per tick, 15 s wall budget, tool results capped at 4 KB each. When limits hit, the model is asked for its final answer with the same `{intervene, kind, text, reason}` schema.

`llm_client` gains `send_with_tools(messages, tools, handler)` supporting tool calls in both OpenAI and Anthropic formats. If the provider rejects tools, the observer runs the no-tools path with only the base prompt and shows a banner. Session and rolling summaries use the same loop so they can verify against sources.

### Observer output schema

```json
{
  "intervene": false,
  "kind": "loop | stuck | info | idea | null",
  "text": "string | null",
  "reason": "string"
}
```

### System audio capture

`audio_toolkit/audio/system_capture/{mod,macos,windows,linux}.rs`, trait `SystemCapture { start(cb), stop() }` yielding f32 frames at source rate; resampling shared.

- **macOS:** CoreAudio process tap (`AudioHardwareCreateProcessTap`, macOS 14.2+). Fallback on 13.x: ScreenCaptureKit audio-only. Requires `NSAudioCaptureUsageDescription` and audio-input entitlement.
- **Windows:** WASAPI loopback via `windows` crate (already a dependency).
- **Linux:** PulseAudio/PipeWire monitor source via cpal (monitor sources appear as inputs). None found means mic-only with warning.

### Events to frontend

`meeting:state`, `meeting:transcript-line`, `meeting:intervention`, `meeting:capture-warning`, `meeting:observer-error`, `source-status-payload` (tauri-specta derives the name from `SourceStatusPayload`; frontend `events.sourceStatusPayload`).

### Failure handling

- System capture unavailable: session runs mic-only, `meeting:capture-warning` banner.
- LLM call fails: log, skip tick, retry next tick, banner after 3 consecutive failures.
- Transcription error: drop utterance, log.
- Crash mid-session: on startup any `live` session is marked `paused`; user can resume or end.
- Start while another session live: rejected with error.

## 4. Frontend

- `stores/projectStore.ts`: Zustand. Project list, active project, sources with status. Subscribes to `source-status-payload` (`events.sourceStatusPayload`).
- `components/projects/ProjectList.tsx`, `ProjectDetail.tsx` (with `BriefEditor`, `SourceList`, `AddSourceDialog`, `ProjectMeetings`).
- `stores/meetingStore.ts`: Zustand. Meeting list, active meeting, live transcript buffer, interventions. Subscribes to events above.
- `components/meetings/MeetingList.tsx`, `MeetingDetail.tsx` (with `ContextEditor`, `TranscriptThread`, `InterventionTimeline`, `SessionSummary`), `MeetingControls.tsx` (shared with tray actions).
- `components/settings/meeting/`: `MeetingProviderSelector`, `InterventionSensitivity`, `ObserverInterval`, `SystemAudioSource`, `MeetingBeepPicker`, `KeepMeetingAudio`.
- `src/intervention/`: new window entry following `src/overlay/` pattern. Separate Vite entry.
- Sidebar: "Projects" first, "Meetings" second (all meetings, including quick ones). Existing tabs shift down.
- i18n: all strings under `meetings.*` in `en/translation.json`; other locales fall back to English.
- Bindings regenerated via tauri-specta.

## 5. Prompting

Observer system prompt (fixed in v1):

- Role: silent meeting copilot for one participant. Intervene only when clearly useful. Default `intervene=false`.
- Signals: same point restated three or more times without decision (`loop`); long pause or explicit "not sure how" (`stuck`); factual question nobody answers (`info`); discussion converging with an obvious untried alternative (`idea`).
- Sensitivity: low adds "only loop and explicit stuck"; high adds "offer ideas freely".
- Message order: project brief, previous meeting's rolling summary, meeting context, recent interventions (do not repeat), last ~3 minutes of transcript. Instruction: use tools to check sources before asserting project facts; prefer silence over an unverified claim.

Session summary prompt: decisions, open questions, action items, three-line recap. Rolling summary prompt: merge previous rolling summary with new session summary, cap ~300 words.

## 6. Testing

- Rust unit: per-source VAD segmentation on synthetic audio; observer cooldown and dedupe; store CRUD and migration; prompt builder output from fixtures; indexer chunking and `.gitignore` handling on a fixture folder; FTS search ranking on fixture chunks; tool loop limits with a fake provider that emits scripted tool calls.
- Rust integration: `MeetingManager` state machine with fake capture and fake `Judge`.
- Frontend: Playwright, meeting list and start/pause/end flow against mocked commands.
- Manual: macOS Zoom/Meet call, both sides transcribed, loop detection with scripted conversation.

## 7. Docs and vision (first deliverable)

- `README.md`: new name, pitch, how it works, privacy stance, early status, credit Handy upstream.
- `AGENTS.md`: add meeting subsystem, keep dictation sections, replace upstream feature-freeze and PR-template mandates with fork rules.
- `docs/VISION.md`: why, principles (private copilot, local-first, speak only when useful), roadmap.
- `docs/ARCHITECTURE.md`: meeting pipeline, data model, capture per OS.
- `CONTRIBUTING.md`: trimmed to fork reality.
- `tauri.conf.json` productName and identifier, `package.json` name.

## 8. Delivery order

1. Docs and vision rewrite.
2. Projects and sources: store, indexer, FTS5, project list and detail UI. No LLM yet.
3. Meetings under projects: store, commands, list and detail UI, mic-only capture and transcript.
4. Tool calling in `llm_client`, observer loop, intervention window, beep.
5. System audio capture, macOS first, then Windows, Linux.
6. Summaries, image digests, continue flow polish, onboarding step.
