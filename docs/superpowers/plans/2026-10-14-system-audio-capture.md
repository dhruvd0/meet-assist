# System Audio Capture Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** During a live meeting the remote side of the call (what the user hears) is captured, segmented, transcribed, and appended to the transcript as speaker `them`, alongside the existing microphone path (`me`). macOS first; Windows and Linux through the same seam where the platform already supports it. When system capture is unavailable the meeting runs microphone-only and the UI says so.

**Architecture:** The cpal fork that handy-recorder already depends on (`handy-cpal` 0.18.2) implements loopback when an input stream is built on an output device: CoreAudio process tap on macOS (14.2+, aggregate device, all processes), WASAPI `AUDCLNT_STREAMFLAGS_LOOPBACK` on Windows. handy-recorder itself cannot open output devices, so a new `meeting/system_capture.rs` uses `handy_cpal` directly for macOS and Windows: open `default_output_device`, build an f32 input stream with `default_output_config`, mix to mono, resample to 16 kHz with `rubato`, slice into VAD frames, and feed a `MeetingSink` tagged `them`. On Linux, PulseAudio monitor sources are ordinary input devices (`InputDevice::is_monitor`), so handy-recorder opens them directly. `MeetingManager`'s `CaptureSource` becomes a combined source that starts both streams on one channel; the transcriber path needs no change (`speaker` already flows through).

**Tech Stack:** Rust (`handy-cpal` =0.18.2 as a direct dependency, `rubato` =5.0.0, handy-recorder), React, i18next.

**Spec:** `docs/superpowers/specs/2026-10-13-meeting-assistant-design.md` sections 1 (mic + system audio, mic-only fallback), 2 (settings: system-audio device), 3 (`capture.rs`, `system_capture/`, failure handling), 8 step 5.

## Parallel lanes

| Lane                | Tasks     | Files                                                                                                                                                  |
| ------------------- | --------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| A                   | 1         | `src-tauri/Cargo.toml`, `Cargo.lock`, `src-tauri/src/meeting/resample.rs` (new), `meeting/mod.rs`                                                      |
| B                   | 2         | `src-tauri/src/meeting/capture.rs` (speaker parameter)                                                                                                 |
| C                   | 3         | `src-tauri/src/settings.rs`, `src-tauri/src/shortcut/mod.rs`, `src-tauri/src/lib.rs` (commands), `src/stores/settingsStore.ts`, `src-tauri/Info.plist` |
| D (after A, B, C)   | 4 then 5  | `src-tauri/src/meeting/system_capture.rs` (new), `meeting/mod.rs`, `meeting/session.rs`, `lib.rs` (events); then bindings regen                        |
| E (parallel with D) | 6a        | `src/i18n/locales/**`, `src/components/meetings/TranscriptThread.tsx`, `src/components/settings/meeting/MeetingAssistantSettings.tsx`                  |
| F (after D, E)      | 6b then 7 | `src/stores/meetingStore.ts`; then docs                                                                                                                |

Rules as in the previous plans: disjoint files per lane, per-file formatting, path-scoped commits, retry on `index.lock`, wait and retry when another lane's file breaks the crate build.

## Global Constraints

- Toolchain exports as before; cargo from `src-tauri/`, bun from repo root; `cargo run -- --list-models` only in Task 5; never hand-edit `src/bindings.ts`; no `bun run build`.
- No `unwrap` in non-test production paths. Rust tests use `tempfile::tempdir()`.
- Speaker strings exactly `me` and `them`.
- System capture output is 16 kHz mono f32 frames exactly `vad.frame_samples()` long; the same `Segmenter` defaults apply.
- `meeting_system_audio_enabled: bool` default `true`. When enabled and the platform start fails, the meeting still starts mic-only and a `MeetingCaptureWarningPayload` is emitted with the `system_audio` source so the UI shows a banner (not a toast storm; one per session).
- macOS: `NSAudioCaptureUsageDescription` added to `src-tauri/Info.plist`; the process tap is created with all processes; the app's own alert beep may be captured, the VAD and the 300 ms minimum utterance drop it in practice.
- Linux: the monitor source of the default sink is chosen (first `InputDevice` with `is_monitor == true` whose name contains the default output device name, else the first monitor); if none, system capture is unavailable.
- Strings under `meetings.transcript.them`, `meetings.capture.*`, `settings.meeting.systemAudio.*` in en, copied into 25 locales.
- Commit each task with a conventional prefix and the trailer `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.

## Review Focus

1. A user without the macOS audio-capture permission (first run) must get a clear banner and a working mic-only meeting, not a crash or a silent `them` channel. Pinned in Task 4 (start error path) and Task 6b (banner).
2. Both streams must deliver frames of the exact VAD size; a resampler producing off-by-one chunk sizes would starve or misalign the VAD. Pinned in Task 1 (resample tests) and Task 4 (slicer test).
3. Echo: the user's own voice through speakers is heard by the mic and the remote side is heard by the loopback; the two channels must not be merged into one utterance. Pinned by design (separate sinks and speakers) and asserted in Task 4's combined test.
4. Stopping a meeting must tear down the aggregate device and process tap (no leaked CoreAudio objects across sessions). Pinned in Task 4 (handle drop releases the stream; a `stop` idempotence test with a fake stream).
5. Disabling the setting mid-meeting takes effect at the next start, not mid-session; the UI says so. Pinned in Task 6a (description string).

---

### Task 1: Dependencies and resampler (Lane A)

**Files:**

- Modify: `src-tauri/Cargo.toml` (add `handy-cpal = "=0.18.2"` under `[dependencies]`; add `rubato = "=5.0.0"`), `Cargo.lock`
- Create: `src-tauri/src/meeting/resample.rs`
- Modify: `src-tauri/src/meeting/mod.rs` (`pub mod resample;`)

**Interfaces:**

- `pub struct MonoResampler { .. }` with `MonoResampler::new(in_rate: u32, out_rate: u32, in_channels: u16) -> anyhow::Result<Self>` and `push(&mut self, interleaved: &[f32]) -> Vec<f32>` returning whatever 16 kHz mono samples are ready (may be empty), and `flush(&mut self) -> Vec<f32>`. Implementation: mix channels to mono by averaging, buffer, run `rubato::FftFixedIn::<f32>::new(in_rate as usize, out_rate as usize, 1024, 2, 1)` per full 1024-frame block; pass-through when `in_rate == out_rate`.
- `pub struct FrameSlicer { .. }` with `FrameSlicer::new(frame_len: usize)`, `push(&mut self, samples: &[f32]) -> Vec<Vec<f32>>` returning complete frames, `take_partial(&mut self) -> Option<Vec<f32>>`.

- [ ] **Step 1: Tests**

```rust
    #[test]
    fn passthrough_when_rates_equal() {
        let mut r = MonoResampler::new(16_000, 16_000, 2).unwrap();
        let out = r.push(&[0.5, 0.5, 1.0, 0.0]); // two stereo frames
        assert_eq!(out, vec![0.5, 0.5]);
    }
    #[test]
    fn downsample_48k_to_16k_preserves_duration() {
        let mut r = MonoResampler::new(48_000, 16_000, 1).unwrap();
        let input: Vec<f32> = (0..48_000).map(|i| ((i as f32) * 0.01).sin()).collect();
        let mut out = r.push(&input);
        out.extend(r.flush());
        let n = out.len() as i64;
        assert!((n - 16_000).abs() <= 1024, "got {n} samples for one second");
    }
    #[test]
    fn slicer_emits_exact_frames_and_keeps_remainder() {
        let mut s = FrameSlicer::new(4);
        let frames = s.push(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]);
        assert_eq!(frames, vec![vec![1.0, 2.0, 3.0, 4.0], vec![5.0, 6.0, 7.0, 8.0]]);
        let frames = s.push(&[10.0, 11.0, 12.0]);
        assert_eq!(frames, vec![vec![9.0, 10.0, 11.0, 12.0]]);
        assert!(s.take_partial().is_none());
        s.push(&[1.0]);
        assert_eq!(s.take_partial(), Some(vec![1.0]));
    }
```

- [ ] **Step 2: RED**, **Step 3: Implement**, **Step 4: GREEN** `cargo test --lib meeting::resample`; `cargo check`. Commit `feat: add mono resampler and frame slicer for system audio`.

---

### Task 2: Speaker-aware MeetingSink (Lane B)

**Files:**

- Modify: `src-tauri/src/meeting/capture.rs`

**Interfaces:**

- `MeetingSink::new(vad, tx)` keeps speaker `me`; add `MeetingSink::with_speaker(vad, tx, speaker: &str)`; `emit` uses the stored speaker. `MeetingCapture::start` unchanged (mic path). Add `pub fn push_frame(&mut self, frame: &[f32])` on `MeetingSink` that runs the VAD/segmenter step without an `AudioChunk` (used by system capture).
- Test: `with_speaker("them")` yields utterances tagged `them`; `push_frame` with 25 loud then 40 quiet frames emits one utterance.

Commit `feat: let MeetingSink tag utterances with a speaker and accept raw frames`.

---

### Task 3: Settings, Info.plist (Lane C)

**Files:**

- Modify: `src-tauri/src/settings.rs` (`meeting_system_audio_enabled: bool` default true; command `change_meeting_system_audio_setting`), `src-tauri/src/shortcut/mod.rs`, `src-tauri/src/lib.rs` (register), `src/stores/settingsStore.ts` (updater; tsc deferred to Task 5), `src-tauri/Info.plist` (add `NSAudioCaptureUsageDescription` "Capture the audio of your calls so the assistant can transcribe the other participants locally.")
- Test: default true in `get_default_settings`.

Commit `feat: add system audio setting and macOS audio capture usage description`.

---

### Task 4: System capture and combined source (Lane D)

**Files:**

- Create: `src-tauri/src/meeting/system_capture.rs`
- Modify: `src-tauri/src/meeting/mod.rs`, `src-tauri/src/meeting/session.rs`, `src-tauri/src/lib.rs` (no new events; `MeetingCaptureWarningPayload` gains `source: String`)

**Interfaces:**

- `pub struct SystemCapture { .. }` with `SystemCapture::start(settings: &AppSettings, app: &AppHandle, tx: mpsc::Sender<Utterance>) -> anyhow::Result<Self>` and `stop(self)`, `has_failed(&self) -> bool`.
  - macOS and Windows (`#[cfg(any(target_os = "macos", target_os = "windows"))]`): `let host = handy_cpal::default_host(); let device = host.default_output_device().ok_or(..)?; let cfg = device.default_output_config()?;` build `MonoResampler::new(cfg.sample_rate().0, 16_000, cfg.channels())`, a `FrameSlicer::new(vad.frame_samples())`, and a `MeetingSink::with_speaker(vad, tx, "them")` wrapped in `Arc<Mutex<..>>`; `device.build_input_stream(&cfg.config(), move |data: &[f32], _| { let mono = resampler.push(data); for frame in slicer.push(&mono) { sink.lock().push_frame(&frame) } }, move |e| { failed.store(true); warn!(..) }, None)?; stream.play()?`. Hold `Stream` in the struct; `stop` drops it (tears down tap and aggregate) and flushes the sink (`finish`). The data callback must never block: use `try_lock` on the sink and drop the frame if contended (count drops in an `AtomicU64` for the log).
  - Linux: pick a monitor via `handy_recorder::list_input_devices()` per the Global Constraint; open with `handy_recorder::Recorder::open_with_failure_handler(RecorderConfig { device: Some(id), ..microphone_config(None, None, frame) }, ..)` and `start(MeetingSink::with_speaker(..,"them"))`; stop like `MeetingCapture`.
  - Other targets: `Err(anyhow!("system audio capture is not supported on this platform"))`.
- `session.rs`: replace `MicCapture` with `CombinedCapture { app }`: starts `MeetingCapture` (mic) always; if `settings.meeting_system_audio_enabled`, tries `SystemCapture::start`; on `Err(e)` logs and emits `MeetingCaptureWarningPayload { meeting_id, source: "system_audio", message: <e> }` once (the emit needs `meeting_id`: pass it into `CaptureSource::start` as a new parameter `start(&self, meeting_id: i64, tx)`; update fakes). The returned handle stops both and `has_failed` reports the mic only (system failure already warned). Existing `MeetingCaptureWarningPayload` gets `source: String` (`"microphone"` for the existing ticker path).
- Tests (no hardware): `FrameSlicer`+`MonoResampler`+`MeetingSink` pipeline test with a synthetic 48 kHz stereo tone burst followed by silence yields one `them` utterance; `CombinedHandle` stop idempotence with fakes; session test that a `CaptureSource` emitting both `me` and `them` utterances stores both speakers in order.

Commit `feat: capture system audio as speaker them alongside the microphone`.

---

### Task 5: Bindings regen and typecheck (Lane D)

`cargo check`; `cargo run -- --list-models`; `bunx tsc --noEmit` (Task 3's updater resolves); commit `chore: regenerate bindings for system audio settings`.

---

### Task 6a: Strings and transcript rendering (Lane E)

- i18n: `meetings.transcript.them: "Them"`, `meetings.capture.systemAudioUnavailable: "System audio is unavailable; transcribing your microphone only. {{message}}"`, `meetings.capture.systemAudioHint: "On macOS, allow CASE under System Settings, Privacy & Security, Screen & System Audio Recording, then end and start the meeting again."`, `settings.meeting.systemAudio.label: "Capture system audio"`, `settings.meeting.systemAudio.description: "Hear the other side of the call. Takes effect when the next meeting starts."` in en and 25 locales.
- `TranscriptThread.tsx`: render `speaker === "them"` with label `meetings.transcript.them` and a second colour (`text-text/70` label), `me` as before.
- `MeetingAssistantSettings.tsx`: add the `meeting_system_audio_enabled` toggle.

Commit `feat: render remote speaker lines and add the system audio toggle`.

---

### Task 6b: Capture warning banner (Lane F)

- `meetingStore.ts`: `captureWarning: { source: string; message: string } | null`, set from `meetingCaptureWarningPayload` for the active meeting, cleared on `openMeeting`/`closeMeeting`/`startMeeting`; `MeetingDetail.tsx` shows an `Alert` (warning) with `meetings.capture.systemAudioUnavailable` and the hint when `source === "system_audio"`; keep the existing toast for `"microphone"`.

Commit `feat: show a banner when system audio capture is unavailable`.

---

### Task 7: Format, clippy, docs (Lane F, last)

Gates as before. `docs/ARCHITECTURE.md`: rows for `meeting/resample.rs`, `meeting/system_capture.rs`; capture description updated (mic `me` + loopback `them`); platform table in `docs/VISION.md` updated (macOS working via process tap; Windows via WASAPI loopback, built but unverified; Linux via PulseAudio monitor, built but unverified); delivery order marks step 5 done. Commit `chore: format, clippy, and docs for system audio capture`.

Manual smoke (user): start a meeting while playing a YouTube video; lines tagged Them appear; speak into the mic; lines tagged You appear; deny the permission in System Settings and start again: banner appears, mic lines still flow.
