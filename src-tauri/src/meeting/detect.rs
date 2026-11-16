//! Meeting detection: notice that a meeting has probably started and offer to
//! record it, the way a person would tap you on the shoulder. Nothing is
//! recorded until the user accepts.
//!
//! Two signals:
//! - a call: a meeting app or browser is using the microphone (macOS);
//! - voices: sustained speech at the microphone while nothing is recording.
//!   The audio is only measured, in memory; it is never stored or sent.
//!
//! While a meeting records, a call app letting go of the microphone raises
//! "call ended", which ends the meeting unless the user says to keep going.

use crate::audio_toolkit::audio::CaptureVad;
use crate::managers::audio::{create_capture_vad, microphone_config};
use crate::meeting::session::MeetingManager;
use crate::settings::{get_settings, AppSettings};
use handy_recorder::{AudioChunk, Recorder, Sink};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

const POLL: Duration = Duration::from_secs(2);
/// A call app must hold the microphone this long; a mic test is shorter.
const CALL_HOLD: Duration = Duration::from_secs(4);
/// The microphone must stay free this long before a call counts as over.
const CALL_GONE: Duration = Duration::from_secs(10);
/// How long "call ended" waits for an answer before ending the meeting.
const END_COUNTDOWN: Duration = Duration::from_secs(60);
/// Voices: this much speech inside the window means a conversation.
const VOICE_WINDOW: Duration = Duration::from_secs(30);
const VOICE_SPEECH: Duration = Duration::from_secs(20);
/// An unanswered voices prompt goes away by itself.
const VOICE_PROMPT_FOR: Duration = Duration::from_secs(30);
/// After the user accepts, how long the meeting gets to start before the
/// watcher looks for a call again.
const START_GRACE: Duration = Duration::from_secs(20);
/// A dismissed prompt stays quiet this long for the same app (or voices).
const SNOOZE: Duration = Duration::from_secs(30 * 60);
/// Frames quieter than this skip the voice detector (RMS of [-1, 1] samples).
const SILENCE_RMS: f32 = 0.004;
const SAMPLE_RATE: usize = 16_000;
const VOICES: &str = "voices";

/// Apps whose microphone use means a call, each matched as a whole word of
/// the lowercased app name ("Google Chrome", "zoom.us"). Dictation tools and
/// recorders are deliberately absent.
const CALL_APPS: &[&str] = &[
    "zoom", "teams", "msteams", "slack", "discord", "facetime", "whatsapp", "webex", "skype",
    "telegram", "signal", "meet", "chrome", "chromium", "safari", "arc", "brave", "edge",
    "firefox", "opera", "vivaldi", "dia",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DetectionKind {
    /// A call app is using the microphone.
    Call,
    /// People are talking near the microphone.
    Voices,
    /// The call the meeting was recording has ended.
    CallEnded,
}

/// What the floating bar is asking the user right now.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct DetectionPrompt {
    pub kind: DetectionKind,
    /// The app on the call, when known.
    pub app: Option<String>,
    /// Unix ms when "call ended" ends the meeting by itself.
    pub deadline: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct MeetingDetectionPayload {
    pub prompt: Option<DetectionPrompt>,
}

/// A process other than this app that has the microphone open.
#[derive(Clone, Debug, PartialEq)]
pub struct MicUser {
    pub app: String,
}

#[derive(Default)]
struct State {
    prompt: Option<DetectionPrompt>,
    /// When the current prompt goes away or acts by itself.
    prompt_until: Option<Instant>,
    snoozed: HashMap<String, Instant>,
    /// The user's answer, picked up by the watcher on its next pass.
    answer: Option<bool>,
}

pub struct MeetingDetector {
    app: AppHandle,
    state: Mutex<State>,
}

impl MeetingDetector {
    pub fn prompt(&self) -> Option<DetectionPrompt> {
        self.state.lock().unwrap().prompt.clone()
    }

    /// The user's answer to the current prompt: start or end the meeting
    /// (`accept`), or leave things as they are.
    pub fn answer(&self, accept: bool) {
        let mut state = self.state.lock().unwrap();
        if state.prompt.is_some() {
            state.answer = Some(accept);
        }
    }

    fn set_prompt(&self, prompt: Option<DetectionPrompt>, until: Option<Instant>) {
        {
            let mut state = self.state.lock().unwrap();
            if state.prompt == prompt {
                return;
            }
            state.prompt = prompt.clone();
            state.prompt_until = until;
            state.answer = None;
        }
        info!("meeting detection: prompt {prompt:?}");
        if let Err(e) = (MeetingDetectionPayload { prompt }).emit(&self.app) {
            warn!("emit meeting detection: {e}");
        }
        crate::live_pill::sync(&self.app);
    }

    fn snoozed(&self, key: &str) -> bool {
        let mut state = self.state.lock().unwrap();
        state.snoozed.retain(|_, until| *until > Instant::now());
        state.snoozed.contains_key(key)
    }

    fn snooze(&self, key: &str) {
        self.state
            .lock()
            .unwrap()
            .snoozed
            .insert(key.to_string(), Instant::now() + SNOOZE);
    }
}

pub fn spawn(app: &AppHandle) {
    let detector = Arc::new(MeetingDetector {
        app: app.clone(),
        state: Mutex::new(State::default()),
    });
    app.manage(detector.clone());
    let spawned = std::thread::Builder::new()
        .name("meet-detect".into())
        .spawn(move || watch(&detector));
    if let Err(e) = spawned {
        warn!("spawn meet-detect: {e}");
    }
}

/// What the watcher remembers between passes.
#[derive(Default)]
struct Watch {
    /// Since when a call app has held the microphone.
    call_since: Option<Instant>,
    /// A call app used the microphone during the running meeting.
    call_in_meeting: bool,
    /// Since when the microphone has been free during that meeting.
    free_since: Option<Instant>,
    listener: Option<VoiceListener>,
    listener_failed: bool,
    /// The user accepted and the meeting is starting; no new prompt until
    /// then.
    starting_until: Option<Instant>,
}

fn watch(detector: &Arc<MeetingDetector>) {
    let app = &detector.app;
    let mut w = Watch::default();
    loop {
        std::thread::sleep(POLL);
        let Some(mm) = app.try_state::<Arc<MeetingManager>>() else {
            continue;
        };
        let settings = get_settings(app);
        let recording = mm.state().meeting_id.is_some();
        let callers = if settings.meeting_detect_calls {
            call_apps(&mic_users())
        } else {
            Vec::new()
        };

        // Act on the user's answer, or on a prompt whose time is up.
        let (prompt, answer, expired) = {
            let mut state = detector.state.lock().unwrap();
            let expired = state.prompt_until.is_some_and(|t| t <= Instant::now());
            (state.prompt.clone(), state.answer.take(), expired)
        };
        if let Some(prompt) = prompt {
            let ended = prompt.kind == DetectionKind::CallEnded;
            match (answer, expired) {
                (Some(true), _) if ended => end_meeting(mm.inner()),
                (Some(true), _) => {
                    // The microphone goes to the meeting.
                    w.listener = None;
                    w.starting_until = Some(Instant::now() + START_GRACE);
                    start_meeting(app);
                }
                (Some(false), _) if ended => w.call_in_meeting = false,
                (Some(false), _) => {
                    detector.snooze(prompt.app.as_deref().unwrap_or(VOICES));
                }
                (None, true) if ended => end_meeting(mm.inner()),
                (None, true) => detector.snooze(prompt.app.as_deref().unwrap_or(VOICES)),
                (None, false) => {}
            }
            if answer.is_some() || expired {
                w.call_since = None;
                w.free_since = None;
                detector.set_prompt(None, None);
                continue;
            }
        }

        if recording {
            w.listener = None;
            w.call_since = None;
            w.starting_until = None;
            if !callers.is_empty() {
                w.call_in_meeting = true;
                w.free_since = None;
                if detector.prompt().is_some() {
                    // The call came back.
                    detector.set_prompt(None, None);
                }
            } else if w.call_in_meeting {
                let since = *w.free_since.get_or_insert_with(Instant::now);
                if since.elapsed() >= CALL_GONE && detector.prompt().is_none() {
                    let deadline =
                        chrono::Utc::now().timestamp_millis() + END_COUNTDOWN.as_millis() as i64;
                    detector.set_prompt(
                        Some(DetectionPrompt {
                            kind: DetectionKind::CallEnded,
                            app: None,
                            deadline: Some(deadline),
                        }),
                        Some(Instant::now() + END_COUNTDOWN),
                    );
                }
            }
            continue;
        }

        // Nothing is recording.
        w.call_in_meeting = false;
        w.free_since = None;
        if w.starting_until.is_some_and(|t| t > Instant::now()) {
            continue;
        }
        w.starting_until = None;
        if detector
            .prompt()
            .is_some_and(|p| p.kind == DetectionKind::CallEnded)
        {
            // The meeting was ended some other way.
            detector.set_prompt(None, None);
        }

        let caller = callers.iter().find(|c| !detector.snoozed(&c.app));
        match caller {
            Some(caller) => {
                let since = *w.call_since.get_or_insert_with(Instant::now);
                if since.elapsed() >= CALL_HOLD {
                    detector.set_prompt(
                        Some(DetectionPrompt {
                            kind: DetectionKind::Call,
                            app: Some(caller.app.clone()),
                            deadline: None,
                        }),
                        None,
                    );
                }
            }
            None => {
                w.call_since = None;
                if detector
                    .prompt()
                    .is_some_and(|p| p.kind == DetectionKind::Call)
                {
                    // The call ended before the user answered.
                    detector.set_prompt(None, None);
                }
            }
        }

        listen_for_voices(detector, &settings, &mut w);
    }
}

/// Starting and ending open and close capture, which can take a while; the
/// watcher must keep watching meanwhile.
fn in_background(work: impl FnOnce() + Send + 'static) {
    if let Err(e) = std::thread::Builder::new()
        .name("meet-detect-act".into())
        .spawn(work)
    {
        warn!("meeting detection: spawn: {e}");
    }
}

fn start_meeting(app: &AppHandle) {
    let app = app.clone();
    in_background(move || crate::meeting::toggle::start_meeting(&app));
}

fn end_meeting(mm: &Arc<MeetingManager>) {
    let mm = Arc::clone(mm);
    in_background(move || {
        if mm.state().meeting_id.is_none() {
            return;
        }
        if let Err(e) = mm.end() {
            warn!("meeting detection: end failed: {e:#}");
        }
    });
}

/// Keep the voice listener running while idle and allowed, and raise the
/// voices prompt when it has heard a conversation.
fn listen_for_voices(detector: &Arc<MeetingDetector>, settings: &AppSettings, w: &mut Watch) {
    let wanted = settings.meeting_detect_voices && settings.onboarding_completed;
    if !wanted {
        w.listener = None;
        w.listener_failed = false;
        return;
    }
    if w.listener.is_none() && !w.listener_failed {
        match VoiceListener::start(settings, &detector.app) {
            Ok(l) => w.listener = Some(l),
            Err(e) => {
                // One warning, not one per pass; turning the setting off and
                // on tries again.
                warn!("meeting detection: cannot listen for voices: {e:#}");
                w.listener_failed = true;
            }
        }
    }
    let Some(listener) = &w.listener else {
        return;
    };
    if detector.prompt().is_some() || detector.snoozed(VOICES) {
        return;
    }
    if listener.speech_in_window() >= VOICE_SPEECH {
        listener.reset();
        detector.set_prompt(
            Some(DetectionPrompt {
                kind: DetectionKind::Voices,
                app: None,
                deadline: None,
            }),
            Some(Instant::now() + VOICE_PROMPT_FOR),
        );
    }
}

/// Speech heard per moment, newest last; only durations are kept.
#[derive(Default)]
struct SpeechLog {
    heard: VecDeque<(Instant, Duration)>,
}

impl SpeechLog {
    fn add(&mut self, speech: Duration) {
        let now = Instant::now();
        self.heard.push_back((now, speech));
        while self
            .heard
            .front()
            .is_some_and(|(at, _)| now.duration_since(*at) > VOICE_WINDOW)
        {
            self.heard.pop_front();
        }
    }

    fn total(&self) -> Duration {
        let now = Instant::now();
        self.heard
            .iter()
            .filter(|(at, _)| now.duration_since(*at) <= VOICE_WINDOW)
            .map(|(_, d)| *d)
            .sum()
    }
}

struct VoiceSink {
    vad: CaptureVad,
    log: Arc<Mutex<SpeechLog>>,
}

impl Sink for VoiceSink {
    fn process_chunk(&mut self, chunk: AudioChunk<'_>) {
        let frame_len = self.vad.frame_samples();
        let Some(frame) = chunk.samples.get(..frame_len) else {
            return;
        };
        if chunk.valid_frames < frame_len {
            return;
        }
        let energy = frame.iter().map(|s| s * s).sum::<f32>() / frame_len as f32;
        if energy.sqrt() < SILENCE_RMS {
            return;
        }
        if matches!(self.vad.push_frame(frame), Ok(Some(_))) {
            let frame_time = Duration::from_millis((frame_len * 1000 / SAMPLE_RATE) as u64);
            self.log.lock().unwrap().add(frame_time);
        }
    }
}

/// The always-on microphone stream behind voices detection.
struct VoiceListener {
    recorder: Option<Recorder<VoiceSink>>,
    log: Arc<Mutex<SpeechLog>>,
}

impl VoiceListener {
    fn start(settings: &AppSettings, app: &AppHandle) -> anyhow::Result<Self> {
        let mut vad = create_capture_vad(settings.vad_backend, app)?;
        vad.prepare_offline();
        let config = microphone_config(
            settings.selected_microphone.clone(),
            settings.selected_channel,
            vad.frame_samples(),
        );
        let recorder = Recorder::open_with_failure_handler(config, |e| {
            warn!("meeting detection: microphone failed: {e}");
        })
        .map_err(|e| anyhow::anyhow!("open microphone: {e}"))?;
        let log = Arc::new(Mutex::new(SpeechLog::default()));
        recorder
            .start(VoiceSink {
                vad,
                log: Arc::clone(&log),
            })
            .map_err(|e| anyhow::anyhow!("start microphone: {}", e.error))?;
        info!("meeting detection: listening for voices");
        Ok(Self {
            recorder: Some(recorder),
            log,
        })
    }

    fn speech_in_window(&self) -> Duration {
        self.log.lock().unwrap().total()
    }

    fn reset(&self) {
        self.log.lock().unwrap().heard.clear();
    }
}

impl Drop for VoiceListener {
    fn drop(&mut self) {
        let Some(recorder) = self.recorder.take() else {
            return;
        };
        if let Err(e) = recorder.stop() {
            warn!("meeting detection: stop microphone: {e}");
        }
        if let Err(e) = recorder.close() {
            warn!("meeting detection: close microphone: {e}");
        }
        info!("meeting detection: stopped listening for voices");
    }
}

/// The microphone users that are call apps, one entry per app.
fn call_apps(users: &[MicUser]) -> Vec<MicUser> {
    let mut apps: Vec<MicUser> = Vec::new();
    for user in users {
        let name = user.app.to_lowercase();
        let is_call_app = name
            .split(|c: char| !c.is_alphanumeric())
            .any(|word| CALL_APPS.contains(&word));
        if is_call_app && !apps.contains(user) {
            apps.push(user.clone());
        }
    }
    apps
}

/// The app a process belongs to, from its executable path: the outermost
/// `.app` bundle's name, else the file name. A browser's helper processes
/// resolve to the browser.
fn app_name(path: &str) -> String {
    let bundle = path
        .split('/')
        .find_map(|part| part.strip_suffix(".app"))
        .map(str::to_string);
    let name = bundle.unwrap_or_else(|| path.rsplit('/').next().unwrap_or(path).to_string());
    match name.as_str() {
        // System processes that hold the microphone for an app.
        "avconferenced" => "FaceTime".to_string(),
        n if n.starts_with("com.apple.WebKit") => "Safari".to_string(),
        _ => name,
    }
}

#[cfg(not(target_os = "macos"))]
pub fn mic_users() -> Vec<MicUser> {
    Vec::new()
}

/// Other processes that have a microphone open, from Core Audio's process
/// list (macOS 14.2+; empty on older systems).
#[cfg(target_os = "macos")]
pub fn mic_users() -> Vec<MicUser> {
    use std::ffi::c_void;

    #[repr(C)]
    struct Address {
        selector: u32,
        scope: u32,
        element: u32,
    }
    #[link(name = "CoreAudio", kind = "framework")]
    extern "C" {
        fn AudioObjectGetPropertyDataSize(
            object: u32,
            address: *const Address,
            qualifier_size: u32,
            qualifier: *const c_void,
            size: *mut u32,
        ) -> i32;
        fn AudioObjectGetPropertyData(
            object: u32,
            address: *const Address,
            qualifier_size: u32,
            qualifier: *const c_void,
            size: *mut u32,
            data: *mut c_void,
        ) -> i32;
    }
    const SYSTEM_OBJECT: u32 = 1;
    const SCOPE_GLOBAL: u32 = u32::from_be_bytes(*b"glob");
    const PROCESS_LIST: u32 = u32::from_be_bytes(*b"prs#");
    const PROCESS_PID: u32 = u32::from_be_bytes(*b"ppid");
    const PROCESS_RUNNING_INPUT: u32 = u32::from_be_bytes(*b"piri");

    fn address(selector: u32) -> Address {
        Address {
            selector,
            scope: SCOPE_GLOBAL,
            element: 0,
        }
    }
    /// One fixed-size property of an audio object.
    fn read<T: Default>(object: u32, selector: u32) -> Option<T> {
        let mut value = T::default();
        let mut size = std::mem::size_of::<T>() as u32;
        // SAFETY: `value` is a plain value of the size passed in `size`.
        let status = unsafe {
            AudioObjectGetPropertyData(
                object,
                &address(selector),
                0,
                std::ptr::null(),
                &mut size,
                (&mut value as *mut T).cast(),
            )
        };
        (status == 0).then_some(value)
    }

    let list = address(PROCESS_LIST);
    let mut size = 0u32;
    // SAFETY: `size` receives the byte length of the process list.
    if unsafe {
        AudioObjectGetPropertyDataSize(SYSTEM_OBJECT, &list, 0, std::ptr::null(), &mut size)
    } != 0
    {
        return Vec::new();
    }
    let mut objects = vec![0u32; size as usize / std::mem::size_of::<u32>()];
    // SAFETY: `objects` holds `size` bytes of u32 object ids.
    if unsafe {
        AudioObjectGetPropertyData(
            SYSTEM_OBJECT,
            &list,
            0,
            std::ptr::null(),
            &mut size,
            objects.as_mut_ptr().cast(),
        )
    } != 0
    {
        return Vec::new();
    }
    objects.truncate(size as usize / std::mem::size_of::<u32>());

    let own = std::process::id() as i32;
    let own_name = executable_path(own).map(|p| app_name(&p).to_lowercase());
    let mut users = Vec::new();
    for object in objects {
        if read::<u32>(object, PROCESS_RUNNING_INPUT).unwrap_or(0) == 0 {
            continue;
        }
        let Some(pid) = read::<i32>(object, PROCESS_PID) else {
            continue;
        };
        if pid == own {
            continue;
        }
        let Some(path) = executable_path(pid) else {
            continue;
        };
        let app = app_name(&path);
        // Another copy of this app recording is not a call.
        if Some(app.to_lowercase()) == own_name {
            continue;
        }
        users.push(MicUser { app });
    }
    users
}

#[cfg(target_os = "macos")]
fn executable_path(pid: i32) -> Option<String> {
    let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: `buf` is writable for the length passed.
    let len = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
    (len > 0).then(|| String::from_utf8_lossy(&buf[..len as usize]).into_owned())
}
