# Meet Assist Architecture

Meet Assist is a Tauri 2 desktop app: Rust backend, React/TypeScript frontend. It inherits [Handy](https://github.com/cjpais/Handy)'s dictation pipeline unchanged and adds a meeting subsystem beside it.

Status legend: **existing** means it ships today from Handy. **planned** means it is specified in [the design spec](superpowers/specs/2026-10-13-meeting-assistant-design.md) and not yet built.

## Two pipelines, one engine

```
Dictation (existing)
  shortcut -> mic capture -> VAD -> transcribe -> paste into active app

Meeting (planned)
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

| Path                          | Role                                                                        |
| ----------------------------- | --------------------------------------------------------------------------- |
| `lib.rs`                      | Tauri setup, manager initialization, tray, shortcuts                        |
| `managers/audio.rs`           | Microphone selection and recording via `handy-recorder`                     |
| `managers/transcription.rs`   | Model lifecycle, batch `transcribe(Vec<f32>)`, streaming preview            |
| `managers/model.rs`           | Model download and discovery                                                |
| `managers/history.rs`         | SQLite transcription history (pattern reused by the meeting store)          |
| `audio_toolkit/audio/sink.rs` | `CaptureVad`: frames in, speech segments out                                |
| `audio_toolkit/vad/`          | Silero VAD                                                                  |
| `llm_client.rs`               | Chat completion client, OpenAI and Anthropic formats, JSON schema responses |
| `audio_feedback.rs`           | Plays feedback sounds via rodio                                             |
| `overlay.rs`                  | Always-on-top recording overlay window                                      |
| `settings.rs`                 | Persisted settings and migrations                                           |
| `commands/`                   | Tauri commands exposed to the frontend via tauri-specta                     |

### Planned (meeting subsystem)

| Path                                                          | Role                                                                                                                                                                                           |
| ------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `meeting/mod.rs`                                              | `MeetingManager`, Tauri state. Session state machine `Idle -> Live <-> Paused -> Ended`.                                                                                                       |
| `meeting/capture.rs`                                          | `MeetingCapture`: mic plus system audio, both resampled to 16 kHz mono, one `CaptureVad` per source, emits `Utterance { speaker, samples, started_at }`.                                       |
| `meeting/transcriber.rs`                                      | Worker thread. Utterances in order to `TranscriptionManager::transcribe()`. Appends `TranscriptLine`, emits `meeting:transcript-line`.                                                         |
| `meeting/observer.rs`                                         | Timer loop, default 20 s. Builds the judge prompt, calls `llm_client` with a JSON schema, applies 60 s cooldown and dedupe, fires beep and `meeting:intervention`.                             |
| `meeting/summary.rs`                                          | Session summary on end, then rolling meeting summary merge.                                                                                                                                    |
| `meeting/store.rs`                                            | SQLite tables `meetings`, `sessions`, `transcript_lines`, `interventions`.                                                                                                                     |
| `audio_toolkit/audio/system_capture/{macos,windows,linux}.rs` | Trait `SystemCapture { start(cb), stop() }`. macOS CoreAudio process tap (14.2+) with ScreenCaptureKit fallback; Windows WASAPI loopback; Linux PulseAudio/PipeWire monitor via cpal.          |
| `commands/meeting.rs`                                         | `list_meetings`, `get_meeting`, `start_meeting`, `pause_meeting`, `resume_meeting`, `end_meeting`, `update_meeting_context`, `update_meeting_title`, `delete_meeting`, `dismiss_intervention`. |

## Data model (planned)

```
Meeting        id, title, created_at, updated_at, status (live|paused|ended), context, rolling_summary
Session        id, meeting_id, started_at, ended_at, summary
TranscriptLine id, session_id, ts, speaker (me|them), text
Intervention   id, session_id, ts, kind (loop|stuck|info|idea), text, reason, dismissed
```

A meeting is a thread of sessions. "Continue" opens a new session under an existing meeting. The observer receives the rolling summary of past sessions plus the live window, never the full raw history.

## Observer contract (planned)

Request, in order: rolling summary, user context, recent interventions, last ~3 minutes of transcript.

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

## Events backend to frontend (planned)

`meeting:state`, `meeting:transcript-line`, `meeting:intervention`, `meeting:capture-warning`, `meeting:observer-error`.

## Frontend (`src/`)

### Existing

| Path                      | Role                                                                            |
| ------------------------- | ------------------------------------------------------------------------------- |
| `App.tsx`                 | Shell, onboarding, sidebar routing                                              |
| `components/settings/`    | Dictation and provider settings                                                 |
| `components/ui/`          | Shared primitives (buttons, inputs, selects). Meeting screens must reuse these. |
| `stores/settingsStore.ts` | Zustand settings store                                                          |
| `overlay/`                | Separate window entry for the recording overlay                                 |
| `bindings.ts`             | Generated by tauri-specta, do not edit by hand                                  |

### Planned

| Path                                      | Role                                                                                                 |
| ----------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `stores/meetingStore.ts`                  | Meeting list, active meeting, live transcript buffer, interventions. Subscribes to the events above. |
| `components/meetings/MeetingList.tsx`     | Landing screen, first sidebar entry                                                                  |
| `components/meetings/MeetingDetail.tsx`   | Title, controls, context editor, transcript thread by session, intervention timeline, summaries      |
| `components/meetings/MeetingControls.tsx` | Start / Pause / Resume / End, shared with tray actions                                               |
| `components/settings/meeting/`            | Provider, sensitivity, observer interval, system-audio source, beep, keep-audio toggle               |
| `intervention/`                           | Separate window entry for the floating intervention card, same pattern as `overlay/`                 |

## Failure handling (planned)

- System capture unavailable: run mic-only, emit `meeting:capture-warning`.
- LLM call fails: skip tick, retry next tick, banner after three consecutive failures.
- Transcription error: drop the utterance, log.
- Crash mid-session: on startup any `live` session becomes `paused`.
- Start while a session is live: rejected.

## Delivery order

1. Docs and vision.
2. Store, commands, list and detail UI, mic-only capture and transcript.
3. Observer, intervention window, beep.
4. System audio capture, macOS first.
5. Summaries, continue polish, onboarding step.
