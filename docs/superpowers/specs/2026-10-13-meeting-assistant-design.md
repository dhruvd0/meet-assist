# Meet Assist: Meeting Assistant Design

Date: 2026-10-13
Status: approved in conversation, pending written review

## 1. Intent

Turn this Handy fork into **Meet Assist**: a private meeting copilot. It listens to a live call, keeps a transcript, and speaks up (to the user only) when the conversation loops, someone is stuck, a quick fact is missing, or an obvious idea is untried.

Agreed decisions:

- **Audience:** only the user hears/sees interventions. No audio into the call.
- **Audio:** microphone plus system audio (remote participants). Mic-only fallback when system capture is unavailable.
- **Brain:** existing LLM provider client (`llm_client.rs`); user picks Anthropic, OpenAI, OpenRouter, Ollama, LM Studio, or any OpenAI/Anthropic-format endpoint.
- **Knowledge:** model knowledge plus user-pasted meeting context. No web search, no integrations.
- **Approach:** meeting mode is a new subsystem beside dictation. Dictation stays intact.
- **Product model:** meetings are first-class persisted threads of sessions. Start, pause, resume, end, and continue later.
- **Visuals:** keep Handy's design language and components. Minor differences only.

Non-goals for v1: speaking into the call, web search, speaker diarization beyond me/them, calendar or chat integrations, user-editable observer prompt, Windows/Linux system-audio parity if blocked (mic-only there is acceptable).

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

## 3. Backend architecture

All new code under `src-tauri/src/meeting/` unless noted.

| Module | Responsibility |
|---|---|
| `mod.rs` | `MeetingManager` (Tauri state). Session state machine `Idle → Live ⇄ Paused → Ended`. Public: `start(meeting_id?)`, `pause`, `resume`, `end`, `update_context`, `dismiss_intervention`. |
| `capture.rs` | `MeetingCapture`. Two sources: mic via handy-recorder (reuse `open_microphone` in `managers/audio.rs`), system audio via `audio_toolkit/audio/system_capture/`. Both resampled to 16 kHz mono. One `CaptureVad` per source. Emits `Utterance { speaker, samples, started_at }` on speech end. |
| `transcriber.rs` | Worker thread. Consumes utterances in order, calls `TranscriptionManager::transcribe()`, appends `TranscriptLine`, emits `meeting:transcript-line`. Keeps model loaded for session via existing unload-timeout. |
| `observer.rs` | Timer loop, default 20 s. Runs only when new lines exist. Builds prompt, calls `llm_client::send_chat_completion_with_schema`. Cooldown 60 s, dedupe against recent interventions. On intervene: beep via `audio_feedback`, store, emit `meeting:intervention`. |
| `summary.rs` | On session end: session summary, then rolling summary merge. |
| `store.rs` | SQLite via rusqlite, same pattern as `managers/history.rs`. Tables `meetings`, `sessions`, `transcript_lines`, `interventions`. Schema version column for migrations. Audio not stored unless setting enabled. |
| `commands/meeting.rs` | Tauri commands with specta bindings: `list_meetings`, `get_meeting`, `start_meeting`, `pause_meeting`, `resume_meeting`, `end_meeting`, `update_meeting_context`, `update_meeting_title`, `delete_meeting`, `dismiss_intervention`. |

`Judge` trait wraps the LLM call so tests inject a fake.

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

`meeting:state`, `meeting:transcript-line`, `meeting:intervention`, `meeting:capture-warning`, `meeting:observer-error`.

### Failure handling

- System capture unavailable: session runs mic-only, `meeting:capture-warning` banner.
- LLM call fails: log, skip tick, retry next tick, banner after 3 consecutive failures.
- Transcription error: drop utterance, log.
- Crash mid-session: on startup any `live` session is marked `paused`; user can resume or end.
- Start while another session live: rejected with error.

## 4. Frontend

- `stores/meetingStore.ts`: Zustand. Meeting list, active meeting, live transcript buffer, interventions. Subscribes to events above.
- `components/meetings/MeetingList.tsx`, `MeetingDetail.tsx` (with `ContextEditor`, `TranscriptThread`, `InterventionTimeline`, `SessionSummary`), `MeetingControls.tsx` (shared with tray actions).
- `components/settings/meeting/`: `MeetingProviderSelector`, `InterventionSensitivity`, `ObserverInterval`, `SystemAudioSource`, `MeetingBeepPicker`, `KeepMeetingAudio`.
- `src/intervention/`: new window entry following `src/overlay/` pattern. Separate Vite entry.
- Sidebar: "Meetings" first. Existing tabs shift down.
- i18n: all strings under `meetings.*` in `en/translation.json`; other locales fall back to English.
- Bindings regenerated via tauri-specta.

## 5. Prompting

Observer system prompt (fixed in v1):

- Role: silent meeting copilot for one participant. Intervene only when clearly useful. Default `intervene=false`.
- Signals: same point restated three or more times without decision (`loop`); long pause or explicit "not sure how" (`stuck`); factual question nobody answers (`info`); discussion converging with an obvious untried alternative (`idea`).
- Sensitivity: low adds "only loop and explicit stuck"; high adds "offer ideas freely".
- Message order: rolling summary, user context, recent interventions (do not repeat), last ~3 minutes of transcript.

Session summary prompt: decisions, open questions, action items, three-line recap. Rolling summary prompt: merge previous rolling summary with new session summary, cap ~300 words.

## 6. Testing

- Rust unit: per-source VAD segmentation on synthetic audio; observer cooldown and dedupe; store CRUD and migration; prompt builder output from fixtures.
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
2. Store, commands, meeting list/detail UI with manual "Start" that captures mic only and transcribes. End-to-end thread visible.
3. Observer plus intervention window plus beep.
4. System audio capture, macOS first, then Windows, Linux.
5. Summaries, continue flow polish, onboarding step.
