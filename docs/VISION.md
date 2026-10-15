# Meet Assist Vision

Meet Assist is a private meeting copilot. It sits on your machine, listens to the call you are on, and speaks up only to you when it has something worth saying.

It is a fork of [Handy](https://github.com/cjpais/Handy), the open source offline dictation app. Handy supplies the audio capture, voice activity detection, and local speech-to-text engine. Meet Assist is meeting-only: the dictation features were removed.

## The problem

Calls drift. The same point gets restated three times with no decision. Someone goes quiet because they do not know the next step. A simple fact is missing and nobody looks it up. An obvious alternative never gets mentioned. Everyone notices afterwards.

Meeting bots that join the call as a participant are intrusive, record everyone, and ship audio to someone else's server. Note takers summarize after the fact when it is too late to help.

## What Meet Assist does

- **Listens** to both sides of a call: your microphone and the system audio of remote participants.
- **Transcribes** locally with the same models Handy uses. Each line is tagged as you or them.
- **Watches** the recent conversation on a short timer and asks a language model one question: is there a reason to speak up right now?
- **Beeps and shows a card**, only to you, when the answer is yes. Four kinds of nudge: the conversation is looping, someone is stuck, a quick fact is needed, or an idea is worth raising.
- **Remembers** meetings as threads. End a meeting today, continue it tomorrow, and the assistant carries a rolling summary forward. Ending a session writes a session summary, then merges it into the meeting's rolling summary.
- **Knows your project.** Meetings live in projects. A project holds a short brief plus sources: pasted notes, local files and folders (checked-out repos included), links, images. The assistant starts each check with only the brief and the last meeting's summary, then searches and reads sources on its own when the conversation calls for it.

## Principles

1. **Private by default.** Nothing leaves the machine unless you point the assistant at a cloud model. Local models through Ollama or LM Studio are first-class.
2. **Speak only when useful.** Silence is the default answer. A nudge that interrupts flow is worse than none.
3. **Only you hear it.** The assistant never speaks into the call. You decide whether to say it out loud.
4. **Reuse Handy's engine.** Transcription, models, and audio capture come from Handy. Dictation output (paste, post-processing, history) was removed; the global shortcut starts and ends meetings.
5. **Notes-first look.** t3.codes tokens (zinc neutrals, one blue accent, DM Sans, Newsreader titles, JetBrains Mono) in a Granola-style shell: spaces in the sidebar, a home feed by day, each meeting as a document. See [DESIGN.md](../DESIGN.md).

## Non-goals for v1

- Speaking into the call.
- Web search.
- Diarization beyond me/them.
- Calendar or chat integrations.
- User-editable observer prompt.
- Folder watching, embeddings or vector search, remote GitHub API access, a standalone MCP server process, cloud sync.

## Platform status

| Platform | Microphone capture | System audio capture                                                                                        |
| -------- | ------------------ | ----------------------------------------------------------------------------------------------------------- |
| macOS    | Working (Handy)    | Working. CoreAudio process tap on the default output (14.2+); asks for the "Audio capture" permission once. |
| Windows  | Working (Handy)    | Built, WASAPI loopback. Not yet verified.                                                                   |
| Linux    | Working (Handy)    | Built, PulseAudio monitor source. Not yet verified.                                                         |

Where system audio is unavailable (older macOS, permission denied, or a default output that is also an input, such as a headset in call mode, which is refused to avoid capturing your own voice as "them"), a meeting runs microphone-only and a banner says so. System audio can also be switched off in settings.

## Roadmap

1. Docs and vision (this document).
2. Projects and sources: brief, local files and folders, links, images, full-text search.
3. Meetings as persisted threads under projects: list, detail, start, pause, resume, end. Microphone-only capture, local transcript.
4. Observer loop with context tools, intervention card, beep.
5. System audio capture, macOS first, then Windows and Linux. (macOS verified; Windows and Linux built, unverified.)
6. Session and rolling summaries, image digests, continue-meeting polish, onboarding step for permissions and provider (the "assistant" step, skippable). Steps 1 to 6 are done.

See [ARCHITECTURE.md](ARCHITECTURE.md) for how the pieces fit, and [superpowers/specs/2026-10-13-meeting-assistant-design.md](superpowers/specs/2026-10-13-meeting-assistant-design.md) for the full design.
