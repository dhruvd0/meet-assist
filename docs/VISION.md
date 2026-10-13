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
- **Knows your project.** Meetings live in projects. A project holds a short brief plus sources: pasted notes, local files and folders (checked-out repos included), links, images. The assistant starts each check with only the brief and the last meeting's summary, then searches and reads sources on its own when the conversation calls for it.

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
- Folder watching, embeddings or vector search, remote GitHub API access, a standalone MCP server process, cloud sync.

## Platform status

| Platform | Microphone capture | System audio capture                                                                    |
| -------- | ------------------ | --------------------------------------------------------------------------------------- |
| macOS    | Working (Handy)    | Planned, first target. CoreAudio process tap on 14.2+, ScreenCaptureKit fallback on 13. |
| Windows  | Working (Handy)    | Planned, WASAPI loopback. Not yet verified.                                             |
| Linux    | Working (Handy)    | Planned, PulseAudio/PipeWire monitor source. Not yet verified.                          |

Where system audio is unavailable, a meeting runs microphone-only and the UI says so.

## Roadmap

1. Docs and vision (this document).
2. Projects and sources: brief, local files and folders, links, images, full-text search.
3. Meetings as persisted threads under projects: list, detail, start, pause, resume, end. Microphone-only capture, local transcript.
4. Observer loop with context tools, intervention card, beep.
5. System audio capture, macOS first, then Windows and Linux.
6. Session and rolling summaries, image digests, continue-meeting polish, onboarding step for permissions and provider.

See [ARCHITECTURE.md](ARCHITECTURE.md) for how the pieces fit, and [superpowers/specs/2026-10-13-meeting-assistant-design.md](superpowers/specs/2026-10-13-meeting-assistant-design.md) for the full design.
