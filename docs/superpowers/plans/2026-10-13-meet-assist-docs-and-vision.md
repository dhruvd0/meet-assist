# Meet Assist Docs and Vision Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebrand the Handy fork as Meet Assist and rewrite every top-level doc so a newcomer understands the product (private meeting copilot), the architecture (dictation core plus planned meeting subsystem), and this fork's contribution rules.

**Architecture:** Docs-only plan, plus three config renames. New `docs/VISION.md` and `docs/ARCHITECTURE.md` carry the product and technical story. `README.md`, `AGENTS.md`, `CONTRIBUTING.md` are rewritten in place. Handy's dictation content is kept but demoted under clearly labelled "Dictation" sections, since dictation stays a working feature.

**Tech Stack:** Markdown, Prettier (`bun run format:frontend`), `jq` for JSON validity, `bun run build` for frontend sanity.

**Spec:** `docs/superpowers/specs/2026-10-13-meeting-assistant-design.md` (section 7 is the source for this plan; sections 1 to 5 supply the content for VISION and ARCHITECTURE).

## Global Constraints

- Product name is exactly `Meet Assist`. Bundle identifier is exactly `com.meetassist.app`. npm package name is exactly `meet-assist`.
- Cargo crate name stays `handy` in this plan (binary name, nix, CI untouched). Only `description` changes.
- Keep Handy's design language and components; docs must not promise a redesign.
- Credit upstream: every rewritten top-level doc links `https://github.com/cjpais/Handy` once.
- Non-goals from the spec must appear verbatim in VISION: speaking into the call, web search, diarization beyond me/them, calendar or chat integrations, user-editable observer prompt.
- All relative links in docs must resolve to files that exist in the repo.
- Prettier must pass: `bun run format:check` exits 0 for the markdown and JSON touched.

## Review Focus

1. A user upgrading from a Handy install expects settings and downloaded models to carry over; the identifier change moves the app-data directory, so README must state this plainly. Pinned in Task 6.
2. A reader following README troubleshooting expects the app-data paths to match the real identifier; stale `com.pais.handy` paths would mislead them. Pinned in Task 3.
3. A contributor expects `AGENTS.md` instructions to be true of this repo; the upstream PR-template mandate references `.github/PULL_REQUEST_TEMPLATE.md` whose Community Feedback section no longer applies. Pinned in Task 4.
4. A reader of ARCHITECTURE expects every named module to either exist or be marked planned; mixing them silently misleads. Pinned in Task 2.
5. A reader on Windows or Linux expects to know system-audio capture is not yet verified there. Pinned in Task 1.

---

### Task 1: Write `docs/VISION.md`

**Files:**

- Create: `docs/VISION.md`

**Interfaces:**

- Consumes: spec sections 1, 2, 5.
- Produces: `docs/VISION.md`, linked from README (Task 3) and AGENTS (Task 4).

- [ ] **Step 1: Create the file with this exact content**

```markdown
# Meet Assist Vision

Meet Assist is a private meeting copilot. It sits on your machine, listens to the call you are on, and speaks up only to you when it has something worth saying.

It is a fork of [Handy](https://github.com/cjpais/Handy), the open source offline dictation app. Handy supplies the audio capture, voice activity detection, and local speech-to-text pipeline. Meet Assist builds the meeting layer on top.

## The problem

Calls drift. The same point gets restated three times with no decision. Someone goes quiet because they do not know the next step. A simple fact is missing and nobody looks it up. An obvious alternative never gets mentioned. Everyone notices afterwards.

Meeting bots that join the call as a participant are intrusive, record everyone, and ship audio to someone else's server. Note takers summarize after the fact when it is too late to help.

## What Meet Assist does

- **Listens** to both sides of a call: your microphone and the system audio of remote participants.
- **Transcribes** locally with the same models Handy uses. Each line is tagged as you or them.
- **Watches** the recent conversation on a short timer and asks a language model one question: is there a reason to speak up right now?
- **Beeps and shows a card**, only to you, when the answer is yes. Four kinds of nudge: the conversation is looping, someone is stuck, a quick fact is needed, or an idea is worth raising.
- **Remembers** meetings as threads. End a meeting today, continue it tomorrow, and the assistant carries a rolling summary forward.

## Principles

1. **Private by default.** Nothing leaves the machine unless you point the assistant at a cloud model. Local models through Ollama or LM Studio are first-class.
2. **Speak only when useful.** Silence is the default answer. A nudge that interrupts flow is worse than none.
3. **Only you hear it.** The assistant never speaks into the call. You decide whether to say it out loud.
4. **Reuse Handy.** Dictation keeps working. Models, settings, providers, and the overlay pattern are shared, not duplicated.
5. **Same look.** Meet Assist keeps Handy's design language and components. New screens use the existing primitives.

## Non-goals for v1

- Speaking into the call.
- Web search.
- Diarization beyond me/them.
- Calendar or chat integrations.
- User-editable observer prompt.

## Platform status

| Platform | Microphone capture | System audio capture                                                                    |
| -------- | ------------------ | --------------------------------------------------------------------------------------- |
| macOS    | Working (Handy)    | Planned, first target. CoreAudio process tap on 14.2+, ScreenCaptureKit fallback on 13. |
| Windows  | Working (Handy)    | Planned, WASAPI loopback. Not yet verified.                                             |
| Linux    | Working (Handy)    | Planned, PulseAudio/PipeWire monitor source. Not yet verified.                          |

Where system audio is unavailable, a meeting runs microphone-only and the UI says so.

## Roadmap

1. Docs and vision (this document).
2. Meetings as persisted threads: list, detail, start, pause, resume, end. Microphone-only capture, local transcript.
3. Observer loop, intervention card, beep.
4. System audio capture, macOS first, then Windows and Linux.
5. Session and rolling summaries, continue-meeting polish, onboarding step for permissions and provider.

See [ARCHITECTURE.md](ARCHITECTURE.md) for how the pieces fit, and [superpowers/specs/2026-10-13-meeting-assistant-design.md](superpowers/specs/2026-10-13-meeting-assistant-design.md) for the full design.
```

- [ ] **Step 2: Verify links resolve**

Run: `cd /Users/dhruv/meet-assist && test -f docs/superpowers/specs/2026-10-13-meeting-assistant-design.md && echo ok`
Expected: `ok` (ARCHITECTURE.md link is satisfied by Task 2; do not commit until Task 2 is done if you want link checks green, otherwise commit now and let Task 2 close it)

- [ ] **Step 3: Format check**

Run: `cd /Users/dhruv/meet-assist && bunx prettier --check docs/VISION.md`
Expected: `All matched files use Prettier code style!` If not, run `bunx prettier --write docs/VISION.md` and re-check.

- [ ] **Step 4: Commit**

```bash
git add docs/VISION.md
git commit -m "docs: add Meet Assist vision

States what the fork is for, its principles, v1 non-goals, and platform
status so contributors stop reading the repo as a dictation app."
```

---

### Task 2: Write `docs/ARCHITECTURE.md`

**Files:**

- Create: `docs/ARCHITECTURE.md`

**Interfaces:**

- Consumes: spec sections 3, 4, 5. Existing module list from `src-tauri/src/` and `src/`.
- Produces: `docs/ARCHITECTURE.md`, linked from README and AGENTS.

- [ ] **Step 1: Create the file with this exact content**

````markdown
# Meet Assist Architecture

Meet Assist is a Tauri 2 desktop app: Rust backend, React/TypeScript frontend. It inherits Handy's dictation pipeline unchanged and adds a meeting subsystem beside it.

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
````

- [ ] **Step 2: Verify every "existing" path really exists**

Run:

```bash
cd /Users/dhruv/meet-assist && for f in src-tauri/src/lib.rs src-tauri/src/managers/audio.rs src-tauri/src/managers/transcription.rs src-tauri/src/managers/model.rs src-tauri/src/managers/history.rs src-tauri/src/audio_toolkit/audio/sink.rs src-tauri/src/audio_toolkit/vad src-tauri/src/llm_client.rs src-tauri/src/audio_feedback.rs src-tauri/src/overlay.rs src-tauri/src/settings.rs src-tauri/src/commands src/App.tsx src/components/settings src/components/ui src/stores/settingsStore.ts src/overlay src/bindings.ts; do test -e "$f" && echo "ok $f" || echo "MISSING $f"; done
```

Expected: every line starts with `ok`. If any is MISSING, fix the table row rather than the filesystem.

- [ ] **Step 3: Format check**

Run: `cd /Users/dhruv/meet-assist && bunx prettier --check docs/ARCHITECTURE.md || bunx prettier --write docs/ARCHITECTURE.md`
Expected: passes after write.

- [ ] **Step 4: Commit**

```bash
git add docs/ARCHITECTURE.md
git commit -m "docs: add architecture overview with existing vs planned modules

Separates what Handy ships today from the planned meeting subsystem so
readers do not look for code that is not there yet."
```

---

### Task 3: Rewrite `README.md`

**Files:**

- Modify: `README.md` (lines 1-124 replaced; lines 125 to end kept with edits noted below)

**Interfaces:**

- Consumes: `docs/VISION.md`, `docs/ARCHITECTURE.md`.
- Produces: README with sections `Why`, `How it works`, `Status`, `Quick start`, `Dictation (inherited from Handy)`, then the kept troubleshooting material.

- [ ] **Step 1: Replace lines 1 through 124 (everything before `## Known Issues & Current Limitations`) with this exact content**

````markdown
# Meet Assist

**A private meeting copilot that listens to your call and nudges only you when the conversation needs it.**

Meet Assist runs on your desktop. It transcribes both sides of a call locally, watches for loops, stuck moments, missing facts, and untried ideas, and shows you a quiet card with a beep. It never joins the call and never speaks into it.

It is a fork of [Handy](https://github.com/cjpais/Handy), the open source offline dictation app. Handy's dictation still works here. Read the [vision](docs/VISION.md) for why this exists and the [architecture](docs/ARCHITECTURE.md) for how it is built.

## Status

Early. The meeting subsystem is specified and being built in order:

1. Docs and vision (done)
2. Meetings as persisted threads with microphone-only transcript
3. Observer, intervention card, beep
4. System audio capture (macOS first)
5. Summaries, continue flow, onboarding

Until step 2 lands, this app behaves like Handy with a new name.

## How it works

1. **Start a meeting.** Button or global shortcut. A meeting is a thread; you can pause, resume, end, and continue it on another day.
2. **It listens.** Your microphone and, where supported, system audio for the remote side. Each utterance is transcribed locally and tagged as you or them.
3. **It watches.** Every ~20 seconds the recent transcript, your pasted context (agenda, notes), and a rolling summary go to the language model you chose with one question: should I speak up?
4. **It nudges.** If yes, you hear a beep and see a floating card: looping, stuck, quick fact, or idea. Dismiss it or open the meeting.
5. **It remembers.** Ending a session writes a summary. Continuing a meeting carries the summary forward.

Everything local stays local. The language model is your choice: Anthropic, OpenAI, OpenRouter, or a local Ollama / LM Studio endpoint through the same provider settings Handy uses for post-processing.

## Quick start

### Install

Releases are not yet published for Meet Assist. Build from source (see below).

### Build from source

Prerequisites: [Rust](https://rustup.rs/) stable, [Bun](https://bun.sh/).

```bash
bun install
mkdir -p src-tauri/resources/models
curl -L -o src-tauri/resources/models/silero-vad-v6.2-F32.gguf https://huggingface.co/handy-computer/silero-vad-v6.2-gguf/resolve/main/silero-vad-v6.2-F32.gguf
bun run tauri dev
# macOS cmake error? prefix with CMAKE_POLICY_VERSION_MINIMUM=3.5
```

Platform-specific details are in [BUILD.md](BUILD.md).

### Migrating from Handy

Meet Assist uses its own bundle identifier (`com.meetassist.app`), so it gets a fresh app-data directory. Handy's settings, history, and downloaded models are not picked up automatically. To reuse downloaded models, copy the `models` folder from Handy's app-data directory into Meet Assist's (paths in [Manual Model Installation](#manual-model-installation-for-proxy-users-or-network-restrictions)).

## Dictation (inherited from Handy)

Press a shortcut, speak, release, and your words are pasted into the active app. Silence is trimmed with Silero VAD; transcription uses Whisper-family or Parakeet models with GPU acceleration where available. Works on macOS, Windows, and Linux.

### Debug mode

- **macOS**: `Cmd+Shift+D`
- **Windows/Linux**: `Ctrl+Shift+D`

### CLI parameters

Flags work on all platforms. Remote-control flags are sent to the running instance; startup flags change how this launch behaves.

```bash
handy --toggle-transcription    # Toggle recording on/off
handy --toggle-post-process     # Toggle recording with post-processing on/off
handy --cancel                  # Cancel the current operation
handy --start-hidden            # Start without showing the main window
handy --no-tray                 # Start without the system tray icon
handy --debug                   # Enable debug mode with verbose logging
handy --help                    # Show all available flags
```

> **macOS tip:** When installed as an app bundle, invoke the binary directly:
>
> ```bash
> "/Applications/Meet Assist.app/Contents/MacOS/<binary>" --toggle-transcription   # list the folder to find the binary name
> ```
````

- [ ] **Step 2: In the kept remainder, replace every `com.pais.handy` with `com.meetassist.app`**

Run: `cd /Users/dhruv/meet-assist && sed -i '' 's/com\.pais\.handy/com.meetassist.app/g' README.md && grep -c "com.meetassist.app" README.md`
Expected: a count of at least 5 (three path lines plus two mkdir lines).

- [ ] **Step 3: In the kept remainder, replace the `### How to Contribute` section body and the `## Related Projects` section**

Find the `### How to Contribute` heading. Replace its body (up to `## Related Projects`) with:

```markdown
See [CONTRIBUTING.md](CONTRIBUTING.md). Bug reports and fixes against the inherited dictation pipeline are welcome; meeting-subsystem work follows the [design spec](docs/superpowers/specs/2026-10-13-meeting-assistant-design.md).
```

Replace the `## Related Projects` section body with:

```markdown
- [Handy](https://github.com/cjpais/Handy), the upstream dictation app this fork is built on.
```

- [ ] **Step 4: Update `## Acknowledgments`**

Insert as the first bullet of the Acknowledgments list:

```markdown
- [Handy](https://github.com/cjpais/Handy) by cjpais and contributors, the dictation app Meet Assist is forked from
```

- [ ] **Step 5: Remove the `## Sponsors` and `## Integrations` sections if they survived Step 1**

Run: `cd /Users/dhruv/meet-assist && grep -n "^## Sponsors\|^## Integrations\|raycast" README.md`
Expected: no output. If any, delete those sections; they describe upstream, not this fork.

- [ ] **Step 6: Link check**

Run:

```bash
cd /Users/dhruv/meet-assist && grep -o '](\([^)#]*\.md\)' README.md | sed 's/](//' | sort -u | while read f; do test -f "$f" && echo "ok $f" || echo "MISSING $f"; done
```

Expected: all `ok`.

- [ ] **Step 7: Format and commit**

Run: `cd /Users/dhruv/meet-assist && bunx prettier --write README.md && bunx prettier --check README.md`
Expected: passes.

```bash
git add README.md
git commit -m "docs: rewrite README for Meet Assist

Leads with the meeting copilot story, keeps Handy dictation and
troubleshooting under their own section, and documents the app-data
directory change for people migrating from Handy."
```

---

### Task 4: Update `AGENTS.md`

**Files:**

- Modify: `AGENTS.md`

**Interfaces:**

- Consumes: `docs/ARCHITECTURE.md`, `docs/VISION.md`.
- Produces: AGENTS.md that describes the fork accurately and no longer mandates upstream's freeze or template.

- [ ] **Step 1: Replace the first two lines (title and intro) with**

```markdown
# AGENTS.md

This file provides guidance to AI coding assistants working with code in this repository.

**This is Meet Assist**, a private meeting copilot forked from [Handy](https://github.com/cjpais/Handy). Read [docs/VISION.md](docs/VISION.md) first, then [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). The meeting subsystem is specified in [docs/superpowers/specs/2026-10-13-meeting-assistant-design.md](docs/superpowers/specs/2026-10-13-meeting-assistant-design.md); implementation plans live in `docs/superpowers/plans/`.
```

- [ ] **Step 2: Replace the `## Architecture Overview` opening sentence**

Old: `Handy is a cross-platform desktop speech-to-text application built with Tauri 2.x (Rust backend + React/TypeScript frontend).`

New:

```markdown
Meet Assist is a cross-platform desktop app built with Tauri 2.x (Rust backend + React/TypeScript frontend). It has two pipelines sharing one transcription engine: the inherited dictation pipeline (shipping) and the meeting pipeline (planned, see `docs/ARCHITECTURE.md`). The module lists below describe the dictation pipeline that exists today.
```

- [ ] **Step 3: Add a `### Meeting Subsystem (planned)` subsection after `### Key Architecture Patterns`**

```markdown
### Meeting Subsystem (planned)

New backend code goes under `src-tauri/src/meeting/` (`mod.rs`, `capture.rs`, `transcriber.rs`, `observer.rs`, `summary.rs`, `store.rs`) plus `audio_toolkit/audio/system_capture/` and `commands/meeting.rs`. New frontend code goes under `src/components/meetings/`, `src/stores/meetingStore.ts`, `src/components/settings/meeting/`, and a separate window entry `src/intervention/`. Reuse `TranscriptionManager`, `CaptureVad`, `llm_client`, `audio_feedback`, and `components/ui`. Do not add a second LLM client, a second VAD, or new UI primitives. Keep the existing design language.
```

- [ ] **Step 4: Replace the entire `## GitHub workflow for AI coding assistants` section with**

```markdown
## GitHub workflow for AI coding assistants

This fork is not under Handy's feature freeze. Meeting-subsystem features are in scope when they follow the design spec. Keep these rules:

- **Opening a PR:** Read [`.github/PULL_REQUEST_TEMPLATE.md`](.github/PULL_REQUEST_TEMPLATE.md) and fill every section. The "Community Feedback" section may be answered with a link to the relevant spec or plan instead of a Discussion. Leave "Human Written Description" as a TODO for the human contributor.
- **Opening an issue:** Use [`.github/ISSUE_TEMPLATE/bug_report.md`](.github/ISSUE_TEMPLATE/bug_report.md) for bugs. Feature ideas for the meeting subsystem go in a spec change under `docs/superpowers/specs/`, not an issue.
- **Upstream sync:** Dictation fixes from Handy are welcome; cherry-pick with the upstream commit referenced in the message.
- **Translations:** Follow [CONTRIBUTING_TRANSLATIONS.md](CONTRIBUTING_TRANSLATIONS.md). New `meetings.*` keys land in `en/translation.json` first; other locales fall back to English.

**Commits:** Use conventional commit prefixes (`feat:`, `fix:`, `docs:`, `refactor:`, `chore:`). Focus the message on _why_, not _what_.
```

- [ ] **Step 5: Verify no stale claims**

Run: `cd /Users/dhruv/meet-assist && grep -n "feature freeze\|Discussions) before any PR\|Handy is a cross-platform" AGENTS.md`
Expected: no output.

- [ ] **Step 6: Format and commit**

Run: `cd /Users/dhruv/meet-assist && bunx prettier --write AGENTS.md && bunx prettier --check AGENTS.md`

```bash
git add AGENTS.md
git commit -m "docs: point AGENTS.md at the Meet Assist vision and fork rules

Drops the upstream feature-freeze mandate, names the planned meeting
module layout, and tells assistants which existing seams to reuse."
```

---

### Task 5: Trim `CONTRIBUTING.md`

**Files:**

- Modify: `CONTRIBUTING.md` (full replacement)

**Interfaces:**

- Consumes: nothing new.
- Produces: contributing guide scoped to this fork.

- [ ] **Step 1: Replace the whole file with this exact content**

````markdown
# Contributing to Meet Assist

Meet Assist is a private meeting copilot forked from [Handy](https://github.com/cjpais/Handy). Thanks for helping.

## Where things stand

Read [docs/VISION.md](docs/VISION.md) and [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). The meeting subsystem is built from the spec in `docs/superpowers/specs/` through plans in `docs/superpowers/plans/`. If you want to change the direction, propose a spec edit first.

Dictation is inherited from Handy and kept working. Bug fixes there are welcome; consider sending them upstream too.

## Setup

- [Rust](https://rustup.rs/) stable
- [Bun](https://bun.sh/)
- Platform build tools, see [BUILD.md](BUILD.md)

```bash
git clone git@github.com:dhruvd0/meet-assist.git
cd meet-assist
bun install
mkdir -p src-tauri/resources/models
curl -L -o src-tauri/resources/models/silero-vad-v6.2-F32.gguf https://huggingface.co/handy-computer/silero-vad-v6.2-gguf/resolve/main/silero-vad-v6.2-F32.gguf
bun run tauri dev
# macOS cmake error? prefix with CMAKE_POLICY_VERSION_MINIMUM=3.5
```

## Workflow

1. Branch: `feature/<name>` or `fix/<name>`.
2. Follow existing patterns. Reuse `TranscriptionManager`, `CaptureVad`, `llm_client`, `audio_feedback`, and `components/ui`. Keep the current design language.
3. Before committing: `bun run lint`, `bun run format`, `cargo clippy` in `src-tauri`.
4. Conventional commits: `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`. Say why.
5. Open a PR using [`.github/PULL_REQUEST_TEMPLATE.md`](.github/PULL_REQUEST_TEMPLATE.md). Link the spec or plan task in place of community feedback. Disclose AI assistance: whether, which tools, how much.

## Code style

**Rust:** `cargo fmt`, `cargo clippy`, explicit error handling, doc comments on public APIs, no `unwrap` in production paths.

**TypeScript/React:** strict types, no `any`, functional components, Tailwind, all user-facing strings through i18next (`src/i18n/locales/en/translation.json`).

## Bugs

Open an issue with the [bug report template](.github/ISSUE_TEMPLATE/bug_report.md): app version, OS, CPU/GPU, steps, expected, actual, debug-mode output (`Cmd/Ctrl+Shift+D`).

## Translations

See [CONTRIBUTING_TRANSLATIONS.md](CONTRIBUTING_TRANSLATIONS.md).

## License

MIT, see [LICENSE](LICENSE). Contributions are licensed the same way.
````

- [ ] **Step 2: Link check**

Run:

```bash
cd /Users/dhruv/meet-assist && grep -o '](\([^)#]*\.md\|LICENSE\)' CONTRIBUTING.md | sed 's/](//' | sort -u | while read f; do test -f "$f" && echo "ok $f" || echo "MISSING $f"; done
```

Expected: all `ok`.

- [ ] **Step 3: Format and commit**

Run: `cd /Users/dhruv/meet-assist && bunx prettier --write CONTRIBUTING.md && bunx prettier --check CONTRIBUTING.md`

```bash
git add CONTRIBUTING.md
git commit -m "docs: scope CONTRIBUTING to the Meet Assist fork

Removes upstream's freeze, discussion, and sponsor process; keeps setup,
style, and PR rules that still apply here."
```

---

### Task 6: Rename product in config

**Files:**

- Modify: `src-tauri/tauri.conf.json:3` (`productName`), `src-tauri/tauri.conf.json:5` (`identifier`)
- Modify: `package.json:2` (`name`)
- Modify: `src-tauri/Cargo.toml:4` (`description`)

**Interfaces:**

- Produces: product name `Meet Assist`, identifier `com.meetassist.app`, npm name `meet-assist`. Cargo crate name remains `handy`.

- [ ] **Step 1: Apply the renames**

Run:

```bash
cd /Users/dhruv/meet-assist \
&& sed -i '' 's/"productName": "Handy"/"productName": "Meet Assist"/' src-tauri/tauri.conf.json \
&& sed -i '' 's/"identifier": "com.pais.handy"/"identifier": "com.meetassist.app"/' src-tauri/tauri.conf.json \
&& sed -i '' 's/"name": "handy-app"/"name": "meet-assist"/' package.json \
&& sed -i '' 's/^description = "Handy"/description = "Meet Assist, a private meeting copilot"/' src-tauri/Cargo.toml
```

- [ ] **Step 2: Verify JSON validity and values**

Run: `cd /Users/dhruv/meet-assist && jq -r '.productName, .identifier' src-tauri/tauri.conf.json && jq -r '.name' package.json && grep '^description' src-tauri/Cargo.toml`
Expected:

```
Meet Assist
com.meetassist.app
meet-assist
description = "Meet Assist, a private meeting copilot"
```

- [ ] **Step 3: Check for other hard-coded identifier or product-name uses that would now diverge**

Run: `cd /Users/dhruv/meet-assist && grep -rn "com\.pais\.handy\|handy-app" --include='*.rs' --include='*.ts' --include='*.tsx' --include='*.json' --include='*.plist' --include='*.nix' --include='*.yml' --include='*.md' . | grep -v node_modules | grep -v '/target/' | grep -v 'docs/superpowers'`
Expected: no output. If a hit appears in a file other than those listed in this task, record it in the commit message body and fix it only if it is a plain string substitution; otherwise leave it and note it in the final report.

- [ ] **Step 4: Frontend build sanity**

Run: `cd /Users/dhruv/meet-assist && bun run build`
Expected: exits 0 (TypeScript plus Vite). Cargo build is not required for this change; `tauri.conf.json` is read at dev/build time and validated then.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/tauri.conf.json package.json src-tauri/Cargo.toml
git commit -m "chore: rename product to Meet Assist

New bundle identifier gives the fork its own app-data directory so it
cannot clobber a Handy install. Cargo crate name stays handy for now to
keep nix, CI, and bundle scripts untouched."
```

---

### Task 7: Final consistency pass

**Files:**

- Read-only check across `README.md`, `AGENTS.md`, `CONTRIBUTING.md`, `docs/VISION.md`, `docs/ARCHITECTURE.md`.

- [ ] **Step 1: Every rewritten doc credits upstream exactly once or more**

Run: `cd /Users/dhruv/meet-assist && for f in README.md AGENTS.md CONTRIBUTING.md docs/VISION.md docs/ARCHITECTURE.md; do printf "%s %s\n" "$(grep -c 'github.com/cjpais/Handy' $f)" "$f"; done`
Expected: every count at least 1. (ARCHITECTURE.md references Handy by name; if its count is 0, add the link to its first paragraph: `inherits [Handy](https://github.com/cjpais/Handy)'s dictation pipeline`.)

- [ ] **Step 2: Non-goals appear verbatim in VISION**

Run: `cd /Users/dhruv/meet-assist && for s in "Speaking into the call" "Web search" "Diarization beyond me/them" "Calendar or chat integrations" "User-editable observer prompt"; do grep -q "$s" docs/VISION.md && echo "ok $s" || echo "MISSING $s"; done`
Expected: all `ok`.

- [ ] **Step 3: Full format check**

Run: `cd /Users/dhruv/meet-assist && bun run format:check`
Expected: exits 0. If cargo fmt complains about unrelated Rust, that is pre-existing; report it but do not fix in this plan.

- [ ] **Step 4: No commit needed unless Step 1 required an edit; if so**

```bash
git add docs/ARCHITECTURE.md
git commit -m "docs: credit upstream in architecture overview"
```
