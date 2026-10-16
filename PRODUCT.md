# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

(Tauri 2 desktop app; the UI is React in a system webview on macOS, Windows, and Linux.)

## Users

One person on calls all day: an engineer, lead, or founder who runs many back-to-back meetings across a few ongoing projects. They work in the meeting itself, with the app open beside it or hidden in the tray, and come back between calls to read what happened.

## Product Purpose

CASE is a private meeting copilot. It listens to both sides of a call (microphone and system audio), transcribes locally, and nudges only the user with a beep and a small card when the conversation is looping, someone is stuck, a quick fact is missing, or an idea is worth raising. Meetings are threads that continue across sessions; each session ends with a summary merged into the meeting's rolling summary. Success: the user says the useful thing during the call, and afterwards finds every meeting's notes without hunting.

## Positioning

Local-first and silent by default. It never joins the call as a participant, never speaks into the call, and keeps audio and transcripts on the machine unless the user points it at a cloud model. It knows the user's projects (brief plus sources: notes, files, folders, repos, links, images) and reads them on its own when the conversation calls for it.

## Operating Context

- Started and ended with one global shortcut, the tray menu, the CLI (`--toggle-meeting`), or SIGUSR2. A meeting starts unfiled; the assistant files it into a project once it knows what it is about.
- During a call the main window is usually hidden; the floating intervention card is the live surface.
- Between calls the user scans recent meetings by day, opens one, and reads its notes (rolling summary, session summaries, nudges, transcript).
- Projects hold a brief and sources.
- Settings: assistant (LLM provider), audio devices, transcription models, shortcut, general, advanced, about.

## Capabilities and Constraints

- Local transcription, VAD, and model management reuse the shared speech engine. Required copyright and license notices are retained in license files; product identity is CASE.
- One LLM provider for observer, summaries, filing, and vision. No chat-over-meetings feature yet (an "Ask anything" bar is deliberately not built).
- No calendar integration, no sharing, no cloud sync.
- All UI strings go through i18next; English is the source locale.
- Offline app: fonts and assets are bundled, never loaded from a CDN.

## Brand Commitments

- Name: CASE. Uppercase, without an acronym.
- Mark: two curved conversational strokes and an offset dot; blue app icon, monochrome native tray variants.
- All product surfaces, executable names, logs, and installers use CASE.
- Visual tokens follow t3.codes (zinc neutrals, one blue accent, DM Sans and JetBrains Mono, 8/12/16 radii), as the user requested on 2026-10-15.
- Experience modeled on Granola: quiet sidebar with spaces, home feed grouped by day, each meeting read as a centered document with a serif title. User-confirmed 2026-10-15, with serif titles.

## Evidence on Hand

No marketing claims, customers, or benchmarks exist. Do not invent any.

## Product Principles

1. Private by default.
2. Speak only when useful; silence is the default.
3. Only the user hears it.
4. Notes first: after a call, what was decided matters more than the raw transcript.
5. One shortcut runs the whole loop.
