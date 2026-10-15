//! Meeting session manager: Idle -> Live <-> Paused -> Ended. Owns capture
//! lifetime and the transcriber thread; emits state and transcript events.

use crate::managers::transcription::TranscriptionManager;
use crate::meeting::capture::{MeetingCapture, Utterance};
use crate::meeting::store::{Meeting, MeetingStatus, ProjectStore, TranscriptLine};
use crate::meeting::summary::{run_summary_job, SessionSummaryPayload, Summarizer};
use crate::meeting::system_capture::SystemCapture;
use anyhow::{anyhow, Result};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tauri::AppHandle;
use tauri_specta::Event;

/// How long pause/end wait for the transcriber to finish queued utterances.
const WORKER_DRAIN_TIMEOUT: Duration = Duration::from_secs(10);

/// On speakers, the mic also hears the call, so the same speech arrives on
/// both channels. Lines this close in time are compared for echo.
const ECHO_WINDOW_MS: i64 = 15_000;
/// Shorter lines ("Yeah.") are never treated as echo.
const ECHO_MIN_WORDS: usize = 3;
/// Share of the shorter line's words that must appear in the other line.
const ECHO_OVERLAP: f64 = 0.8;

fn words(text: &str) -> std::collections::HashSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Whether `a` and `b` are the same speech transcribed twice: most of the
/// shorter line's words appear in the longer one.
fn is_echo(a: &str, b: &str) -> bool {
    let (a, b) = (words(a), words(b));
    let shorter = a.len().min(b.len());
    shorter >= ECHO_MIN_WORDS && a.intersection(&b).count() as f64 >= ECHO_OVERLAP * shorter as f64
}

pub trait Transcriber: Send + Sync {
    fn transcribe(&self, samples: Vec<f32>) -> Result<String>;
}

pub struct ModelTranscriber(pub Arc<TranscriptionManager>);

impl Transcriber for ModelTranscriber {
    fn transcribe(&self, samples: Vec<f32>) -> Result<String> {
        // Idle unload may have dropped the model between utterances; the load
        // is a no-op when it is already warm, and transcribe waits on it.
        self.0.initiate_model_load();
        self.0.transcribe(samples)
    }
}

pub trait CaptureHandle: Send {
    /// Stop capturing and drop the utterance sender.
    fn stop(self: Box<Self>);
    /// Whether the device stopped delivering audio.
    fn has_failed(&self) -> bool;
}

pub trait CaptureSource: Send + Sync {
    /// Start capturing for `meeting_id`, sending utterances to `tx`.
    fn start(&self, meeting_id: i64, tx: mpsc::Sender<Utterance>)
        -> Result<Box<dyn CaptureHandle>>;
}

/// The production source: the microphone ("me") always, plus system audio
/// ("them") when enabled, both on one utterance channel.
struct CombinedCapture {
    app: AppHandle,
}

impl CaptureHandle for MeetingCapture {
    fn stop(self: Box<Self>) {
        MeetingCapture::stop(*self);
    }
    fn has_failed(&self) -> bool {
        MeetingCapture::has_failed(self)
    }
}

impl CaptureHandle for SystemCapture {
    fn stop(self: Box<Self>) {
        SystemCapture::stop(*self);
    }
    fn has_failed(&self) -> bool {
        SystemCapture::has_failed(self)
    }
}

/// Both capture streams of one run. Stopping stops both; failure reflects
/// the microphone only, since a system-audio failure is warned at start.
struct CombinedHandle {
    mic: Box<dyn CaptureHandle>,
    system: Option<Box<dyn CaptureHandle>>,
}

impl CaptureHandle for CombinedHandle {
    fn stop(self: Box<Self>) {
        let CombinedHandle { mic, system } = *self;
        if let Some(system) = system {
            system.stop();
        }
        mic.stop();
    }
    fn has_failed(&self) -> bool {
        self.mic.has_failed()
    }
}

impl CaptureSource for CombinedCapture {
    fn start(
        &self,
        meeting_id: i64,
        tx: mpsc::Sender<Utterance>,
    ) -> Result<Box<dyn CaptureHandle>> {
        let settings = crate::settings::get_settings(&self.app);
        let mic = Box::new(MeetingCapture::start(&settings, &self.app, tx.clone())?);
        let system: Option<Box<dyn CaptureHandle>> = if settings.meeting_system_audio_enabled {
            match SystemCapture::start(&settings, &self.app, tx) {
                Ok(c) => Some(Box::new(c)),
                Err(e) => {
                    // The meeting goes on with the microphone alone.
                    warn!("system audio capture unavailable: {e:#}");
                    let payload = MeetingCaptureWarningPayload {
                        meeting_id,
                        source: "system_audio".into(),
                        message: format!("{e:#}"),
                    };
                    if let Err(e) = payload.emit(&self.app) {
                        error!("emit capture warning: {e}");
                    }
                    None
                }
            }
        } else {
            None
        };
        Ok(Box::new(CombinedHandle { mic, system }))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct MeetingStatePayload {
    pub meeting_id: Option<i64>,
    pub session_id: Option<i64>,
    pub status: Option<MeetingStatus>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct TranscriptLinePayload {
    pub meeting_id: i64,
    pub line: TranscriptLine,
}

/// Emitted once per capture run when a capture source fails: the
/// microphone stops delivering audio, or system audio cannot start.
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct MeetingCaptureWarningPayload {
    pub meeting_id: i64,
    /// `"microphone"` or `"system_audio"`.
    pub source: String,
    pub message: String,
}

/// Shared so the failure ticker can poll the handle while `Active` owns it.
type SharedCapture = Arc<Mutex<Option<Box<dyn CaptureHandle>>>>;

struct Active {
    meeting_id: i64,
    session_id: i64,
    project_id: Option<i64>,
    status: MeetingStatus,
    capture: Option<SharedCapture>,
    worker: Option<JoinHandle<()>>,
}

pub struct MeetingManager {
    store: Arc<ProjectStore>,
    transcriber: Arc<dyn Transcriber>,
    capture: Arc<dyn CaptureSource>,
    /// Brief-access state read by `state()`; never held across a drain.
    active: Mutex<Option<Active>>,
    /// Serializes start/pause/resume/end so a slow drain cannot interleave
    /// with another transition, without blocking `state()`.
    transition: Mutex<()>,
    /// True while capture runs; keeps the transcription model warm.
    live: AtomicBool,
    app: Option<AppHandle>,
    summarizer: Option<Arc<dyn Summarizer>>,
    /// Serialises summary jobs: the rolling summary is a chain.
    summary_lock: Arc<Mutex<()>>,
    /// Files unfiled meetings into projects; `None` without an app handle.
    filer: Option<Arc<crate::meeting::filing::LlmFiler>>,
    filing: Arc<crate::meeting::filing::FilingTracker>,
}

/// Whether transcript text may go to the cloud provider: the assistant
/// opt-in must be on. No settings (tests) counts as allowed.
pub fn cloud_allowed(settings: Option<&crate::settings::AppSettings>) -> bool {
    settings.is_none_or(|s| s.meeting_assistant_enabled)
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Stop capture (dropping the utterance sender) and wait for the worker to
/// transcribe what is already queued. Returns the worker if it is still busy
/// after the timeout, so the caller can wait for it before using the lines.
fn stop_and_drain(
    capture: Option<SharedCapture>,
    worker: Option<JoinHandle<()>>,
) -> Option<JoinHandle<()>> {
    if let Some(shared) = capture {
        if let Some(handle) = lock(&shared).take() {
            handle.stop();
        }
    }
    let worker = worker?;
    let deadline = Instant::now() + WORKER_DRAIN_TIMEOUT;
    while !worker.is_finished() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    if worker.is_finished() {
        if worker.join().is_err() {
            error!("meet-transcriber panicked");
        }
        None
    } else {
        warn!(
            "meet-transcriber still busy after {:?}; leaving it to finish",
            WORKER_DRAIN_TIMEOUT
        );
        Some(worker)
    }
}

impl MeetingManager {
    pub fn new(app: &AppHandle, store: Arc<ProjectStore>, tm: Arc<TranscriptionManager>) -> Self {
        Self {
            store,
            transcriber: Arc::new(ModelTranscriber(tm)),
            capture: Arc::new(CombinedCapture { app: app.clone() }),
            active: Mutex::new(None),
            transition: Mutex::new(()),
            live: AtomicBool::new(false),
            app: Some(app.clone()),
            summarizer: Some(Arc::new(crate::meeting::summary::LlmSummarizer {
                app: app.clone(),
            })),
            summary_lock: Arc::new(Mutex::new(())),
            filer: Some(Arc::new(crate::meeting::filing::LlmFiler::new(app.clone()))),
            filing: Arc::new(crate::meeting::filing::FilingTracker::default()),
        }
    }

    /// Summarize a session on a background thread and emit the result.
    /// No-op without a summarizer.
    pub fn spawn_summary(&self, session_id: i64) {
        self.spawn_summaries(vec![session_id], None, None);
    }

    /// Summarize sessions one after another (the rolling summary is a chain,
    /// so they must not run concurrently) on one background thread.
    /// `pending` is a transcriber still finishing queued utterances; the
    /// thread joins it first so the summary sees every line. `file_meeting`
    /// gets one last filing attempt before the summaries, so they see its
    /// project.
    fn spawn_summaries(
        &self,
        session_ids: Vec<i64>,
        pending: Option<JoinHandle<()>>,
        file_meeting: Option<i64>,
    ) {
        let Some(summarizer) = self.summarizer.clone() else {
            return;
        };
        if session_ids.is_empty() {
            return;
        }
        if !cloud_allowed(
            self.app
                .as_ref()
                .map(crate::settings::get_settings)
                .as_ref(),
        ) {
            log::debug!("meeting assistant is off; not sending sessions for summary");
            return;
        }
        let store = Arc::clone(&self.store);
        let app = self.app.clone();
        let summary_lock = Arc::clone(&self.summary_lock);
        let filer = self.filer.clone();
        let filing = Arc::clone(&self.filing);
        let spawned = std::thread::Builder::new()
            .name("meet-summary".into())
            .spawn(move || {
                if let Some(worker) = pending {
                    if worker.join().is_err() {
                        error!("meet-transcriber panicked");
                    }
                }
                if let (Some(filer), Some(app), Some(meeting_id)) =
                    (filer.as_ref(), app.as_ref(), file_meeting)
                {
                    crate::meeting::filing::final_attempt(filer, &store, &filing, app, meeting_id);
                }
                for session_id in session_ids {
                    let result = {
                        let _guard = lock(&summary_lock);
                        run_summary_job(&store, summarizer.as_ref(), session_id)
                    };
                    match result {
                        Ok(Some(summary)) => {
                            let Ok(Some(session)) = store.get_session(session_id) else {
                                continue;
                            };
                            let rolling_summary = store
                                .get_meeting(session.meeting_id)
                                .ok()
                                .flatten()
                                .and_then(|m| m.rolling_summary)
                                .unwrap_or_default();
                            if let Some(app) = &app {
                                let payload = SessionSummaryPayload {
                                    meeting_id: session.meeting_id,
                                    session_id,
                                    summary,
                                    rolling_summary,
                                };
                                if let Err(e) = payload.emit(app) {
                                    error!("emit session summary: {e}");
                                }
                            }
                        }
                        Ok(None) => {}
                        Err(e) => warn!("summary for session {session_id} failed: {e:#}"),
                    }
                }
            });
        if let Err(e) = spawned {
            error!("spawn meet-summary: {e}");
        }
    }

    pub fn store(&self) -> &ProjectStore {
        &self.store
    }

    fn lock_active(&self) -> std::sync::MutexGuard<'_, Option<Active>> {
        lock(&self.active)
    }

    /// Whether a meeting is capturing audio right now.
    pub fn is_live(&self) -> bool {
        self.live.load(Ordering::SeqCst)
    }

    /// Meeting id of the running (live or paused) meeting, if any.
    #[allow(dead_code)] // public accessor with no caller yet
    pub fn active_meeting_id(&self) -> Option<i64> {
        self.lock_active().as_ref().map(|a| a.meeting_id)
    }

    /// Session id of the running (live or paused) meeting, if any.
    #[allow(dead_code)] // public accessor with no caller yet
    pub fn active_session_id(&self) -> Option<i64> {
        self.lock_active().as_ref().map(|a| a.session_id)
    }

    /// Project of the running (live or paused) meeting, if any.
    #[allow(dead_code)] // public accessor with no caller yet
    pub fn active_project_id(&self) -> Option<i64> {
        self.lock_active().as_ref().and_then(|a| a.project_id)
    }

    /// Points the running meeting's cached project at `project_id` after it
    /// was filed or moved. No-op for any other meeting.
    pub fn set_active_project(&self, meeting_id: i64, project_id: Option<i64>) {
        if let Some(a) = self.lock_active().as_mut() {
            if a.meeting_id == meeting_id {
                a.project_id = project_id;
            }
        }
    }

    /// Live filing attempt for the running meeting when one is due. The
    /// observer calls this before its tick, under the same gates.
    pub fn maybe_file_live(&self, meeting_id: i64) {
        use crate::meeting::filing;
        let Some(filer) = &self.filer else {
            return;
        };
        let Ok(Some(meeting)) = self.store.get_meeting(meeting_id) else {
            return;
        };
        if !filing::eligible(&meeting) {
            return;
        }
        let lines = match self.store.count_meeting_lines(meeting_id) {
            Ok(n) => n,
            Err(e) => {
                warn!("filing: count lines failed: {e:#}");
                return;
            }
        };
        if !self.filing.live_due(meeting_id, lines) {
            return;
        }
        self.filing.record_attempt(meeting_id, lines);
        if let Some(filed) = filing::attempt(filer, &self.store, &self.filing, meeting_id) {
            // The user may have moved the meeting while the run was going;
            // the store is the truth.
            if let Ok(Some(stored)) = self.store.get_meeting(meeting_id) {
                self.set_active_project(meeting_id, stored.project_id);
                if let Some(app) = &self.app {
                    if filing::still_ai_filed(&stored, &filed) {
                        filing::notify(app, &filed);
                    }
                }
            }
        }
    }

    pub fn state(&self) -> MeetingStatePayload {
        match self.lock_active().as_ref() {
            Some(a) => MeetingStatePayload {
                meeting_id: Some(a.meeting_id),
                session_id: Some(a.session_id),
                status: Some(a.status),
            },
            None => MeetingStatePayload {
                meeting_id: None,
                session_id: None,
                status: None,
            },
        }
    }

    fn emit_state(&self) {
        if let Some(app) = &self.app {
            if let Err(e) = self.state().emit(app) {
                error!("emit meeting state: {e}");
            }
            let busy = self.state().meeting_id.is_some();
            crate::tray::set_tray_state(
                app,
                if busy {
                    crate::tray::TrayIconState::Recording
                } else {
                    crate::tray::TrayIconState::Idle
                },
            );
        }
    }

    fn start_capture_and_worker(
        &self,
        meeting_id: i64,
        session_id: i64,
    ) -> Result<(SharedCapture, JoinHandle<()>)> {
        let (tx, rx) = mpsc::channel::<Utterance>();
        let handle = self.capture.start(meeting_id, tx)?;
        let shared: SharedCapture = Arc::new(Mutex::new(Some(handle)));
        let store = Arc::clone(&self.store);
        let transcriber = Arc::clone(&self.transcriber);
        let app = self.app.clone();
        let spawned = std::thread::Builder::new()
            .name("meet-transcriber".into())
            .spawn(move || {
                for utt in rx {
                    match Self::transcribe_and_store(&store, transcriber.as_ref(), session_id, utt)
                    {
                        Ok(Some(line)) => {
                            if let Some(app) = &app {
                                if let Err(e) =
                                    (TranscriptLinePayload { meeting_id, line }).emit(app)
                                {
                                    error!("emit transcript line: {e}");
                                }
                            }
                        }
                        Ok(None) => {}
                        Err(e) => warn!("transcribe utterance: {e:#}"),
                    }
                }
                info!("meet-transcriber for session {} exited", session_id);
            });
        let worker = match spawned {
            Ok(w) => w,
            Err(e) => {
                stop_and_drain(Some(shared), None);
                return Err(e.into());
            }
        };
        if let Some(app) = self.app.clone() {
            Self::spawn_failure_ticker(app, meeting_id, Arc::clone(&shared));
        }
        Ok((shared, worker))
    }

    /// Poll the capture once a second; warn the UI once if the microphone
    /// fails, then exit. Also exits when the capture is stopped.
    fn spawn_failure_ticker(app: AppHandle, meeting_id: i64, shared: SharedCapture) {
        let spawned = std::thread::Builder::new()
            .name("meet-capture-watch".into())
            .spawn(move || loop {
                std::thread::sleep(Duration::from_secs(1));
                let failed = match lock(&shared).as_ref() {
                    None => return,
                    Some(h) => h.has_failed(),
                };
                if failed {
                    let payload = MeetingCaptureWarningPayload {
                        meeting_id,
                        source: "microphone".into(),
                        message: "Check the input device, then pause and resume.".into(),
                    };
                    if let Err(e) = payload.emit(&app) {
                        error!("emit capture warning: {e}");
                    }
                    return;
                }
            });
        if let Err(e) = spawned {
            warn!("meeting capture watcher not started: {e}");
        }
    }

    fn transcribe_and_store(
        store: &ProjectStore,
        transcriber: &dyn Transcriber,
        session_id: i64,
        utt: Utterance,
    ) -> Result<Option<TranscriptLine>> {
        let Utterance {
            speaker,
            samples,
            started_at,
        } = utt;
        let text = match transcriber.transcribe(samples.clone()) {
            Ok(t) => t,
            Err(first) => {
                warn!("transcribe failed once ({first:#}); retrying");
                std::thread::sleep(Duration::from_millis(500));
                transcriber.transcribe(samples)?
            }
        };
        let text = text.trim();
        if text.is_empty() {
            return Ok(None);
        }
        let nearby = store.session_lines_between(
            session_id,
            started_at - ECHO_WINDOW_MS,
            started_at + ECHO_WINDOW_MS,
        )?;
        if nearby
            .iter()
            .any(|l| l.speaker != speaker && is_echo(text, &l.text))
        {
            debug!("dropping {speaker} line echoed from the other channel: {text}");
            return Ok(None);
        }
        Ok(Some(
            store.append_line(session_id, &speaker, text, started_at)?,
        ))
    }

    /// Public for tests and for a future system-audio path; the worker uses
    /// the same function.
    #[allow(dead_code)]
    pub fn ingest(&self, utt: Utterance) -> Result<Option<TranscriptLine>> {
        let session_id = self
            .lock_active()
            .as_ref()
            .map(|a| a.session_id)
            .ok_or_else(|| anyhow!("no active meeting"))?;
        Self::transcribe_and_store(&self.store, self.transcriber.as_ref(), session_id, utt)
    }

    /// The title a new meeting gets when created at `ts_ms`.
    pub fn auto_title_at(ts_ms: i64) -> String {
        chrono::DateTime::from_timestamp_millis(ts_ms)
            .unwrap_or_else(chrono::Utc::now)
            .with_timezone(&chrono::Local)
            .format("Meeting %b %-d %H:%M")
            .to_string()
    }

    pub fn start(&self, project_id: Option<i64>, meeting_id: Option<i64>) -> Result<Meeting> {
        let _t = lock(&self.transition);
        if self.lock_active().is_some() {
            return Err(anyhow!("A meeting is already running"));
        }
        let mut previous_status = None;
        let meeting = match meeting_id {
            Some(id) => {
                let m = self
                    .store
                    .get_meeting(id)?
                    .ok_or_else(|| anyhow!("meeting {} not found", id))?;
                previous_status = Some(m.status);
                self.store.set_meeting_status(id, MeetingStatus::Live)?;
                Meeting {
                    status: MeetingStatus::Live,
                    ..m
                }
            }
            None => self.store.create_meeting(
                project_id,
                &Self::auto_title_at(chrono::Utc::now().timestamp_millis()),
            )?,
        };
        let session = self.store.create_session(meeting.id)?;
        let (capture, worker) = match self.start_capture_and_worker(meeting.id, session.id) {
            Ok(c) => c,
            Err(e) => {
                // Undo the partial start; always surface the capture error.
                match previous_status {
                    Some(prev) => {
                        if let Err(ce) = self.store.end_session(session.id) {
                            warn!("cleanup after failed start (end session): {ce:#}");
                        }
                        if let Err(ce) = self.store.set_meeting_status(meeting.id, prev) {
                            warn!("cleanup after failed start (restore status): {ce:#}");
                        }
                    }
                    None => {
                        if let Err(ce) = self.store.delete_meeting(meeting.id) {
                            warn!("cleanup after failed start (delete meeting): {ce:#}");
                        }
                    }
                }
                self.live.store(false, Ordering::SeqCst);
                return Err(e);
            }
        };
        *self.lock_active() = Some(Active {
            meeting_id: meeting.id,
            session_id: session.id,
            project_id: meeting.project_id,
            status: MeetingStatus::Live,
            capture: Some(capture),
            worker: Some(worker),
        });
        self.live.store(true, Ordering::SeqCst);
        self.emit_state();
        self.play_feedback(crate::audio_feedback::SoundType::Start);
        Ok(meeting)
    }

    pub fn pause(&self) -> Result<()> {
        let _t = lock(&self.transition);
        let (meeting_id, capture, worker) = {
            let mut guard = self.lock_active();
            let a = guard.as_mut().ok_or_else(|| anyhow!("no active meeting"))?;
            if a.status != MeetingStatus::Live {
                return Err(anyhow!("meeting is not live"));
            }
            (a.meeting_id, a.capture.take(), a.worker.take())
        };
        // Lines already spoken land before the meeting reads as paused.
        stop_and_drain(capture, worker);
        self.live.store(false, Ordering::SeqCst);
        if let Some(a) = self.lock_active().as_mut() {
            a.status = MeetingStatus::Paused;
        }
        let result = self
            .store
            .set_meeting_status(meeting_id, MeetingStatus::Paused);
        // Capture is stopped either way; the UI must see that.
        self.emit_state();
        result
    }

    pub fn resume(&self) -> Result<()> {
        let _t = lock(&self.transition);
        let (meeting_id, session_id) = {
            let guard = self.lock_active();
            let a = guard.as_ref().ok_or_else(|| anyhow!("no active meeting"))?;
            if a.status != MeetingStatus::Paused {
                return Err(anyhow!("meeting is not paused"));
            }
            (a.meeting_id, a.session_id)
        };
        let (capture, worker) = self.start_capture_and_worker(meeting_id, session_id)?;
        if let Some(a) = self.lock_active().as_mut() {
            a.capture = Some(capture);
            a.worker = Some(worker);
            a.status = MeetingStatus::Live;
        }
        self.live.store(true, Ordering::SeqCst);
        let result = self
            .store
            .set_meeting_status(meeting_id, MeetingStatus::Live);
        self.emit_state();
        result
    }

    pub fn end(&self) -> Result<()> {
        let _t = lock(&self.transition);
        let (meeting_id, session_id, capture, worker) = {
            let mut guard = self.lock_active();
            let a = guard.as_mut().ok_or_else(|| anyhow!("no active meeting"))?;
            (
                a.meeting_id,
                a.session_id,
                a.capture.take(),
                a.worker.take(),
            )
        };
        let leftover = stop_and_drain(capture, worker);
        self.live.store(false, Ordering::SeqCst);
        let result = self.store.end_session(session_id).and_then(|()| {
            self.store
                .set_meeting_status(meeting_id, MeetingStatus::Ended)
        });
        // Capture is gone; the meeting is no longer active even if the DB
        // write failed, and the UI must see that.
        *self.lock_active() = None;
        self.emit_state();
        if result.is_ok() {
            self.play_feedback(crate::audio_feedback::SoundType::Stop);
            self.spawn_summaries(vec![session_id], leftover, Some(meeting_id));
        }
        result
    }

    /// Start/end cue for every trigger (UI, shortcut, tray, CLI). No-op
    /// without an app handle or with audio feedback off.
    fn play_feedback(&self, sound: crate::audio_feedback::SoundType) {
        if let Some(app) = &self.app {
            crate::audio_feedback::play_feedback_sound(app, sound);
        }
    }

    /// End a meeting that is not running here (e.g. paused before a restart):
    /// close its open sessions and mark it ended.
    pub fn end_idle(&self, meeting_id: i64) -> Result<()> {
        let _t = lock(&self.transition);
        if self.lock_active().as_ref().map(|a| a.meeting_id) == Some(meeting_id) {
            return Err(anyhow!("meeting {} is running; end it instead", meeting_id));
        }
        self.store.end_open_sessions(meeting_id)?;
        self.store
            .set_meeting_status(meeting_id, MeetingStatus::Ended)?;
        let ids = self
            .store
            .sessions_without_summary(meeting_id)?
            .into_iter()
            .map(|s| s.id)
            .collect();
        self.spawn_summaries(ids, None, None);
        Ok(())
    }
}
