//! Meeting session manager: Idle -> Live <-> Paused -> Ended. Owns capture
//! lifetime and the transcriber thread; emits state and transcript events.

use crate::managers::audio::AudioRecordingManager;
use crate::managers::transcription::TranscriptionManager;
use crate::meeting::capture::{MeetingCapture, Utterance};
use crate::meeting::store::{Meeting, MeetingStatus, ProjectStore, TranscriptLine};
use crate::meeting::summary::{run_summary_job, SessionSummaryPayload, Summarizer};
use crate::meeting::system_capture::SystemCapture;
use anyhow::{anyhow, Result};
use log::{error, info, warn};
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
    dictation_recording: Arc<dyn Fn() -> bool + Send + Sync>,
    /// Brief-access state read by `state()`; never held across a drain.
    active: Mutex<Option<Active>>,
    /// Serializes start/pause/resume/end so a slow drain cannot interleave
    /// with another transition, without blocking `state()`.
    transition: Mutex<()>,
    /// True while capture runs; keeps the transcription model warm.
    live: AtomicBool,
    app: Option<AppHandle>,
    summarizer: Option<Arc<dyn Summarizer>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Stop capture (dropping the utterance sender) and wait for the worker to
/// transcribe what is already queued.
fn stop_and_drain(capture: Option<SharedCapture>, worker: Option<JoinHandle<()>>) {
    if let Some(shared) = capture {
        if let Some(handle) = lock(&shared).take() {
            handle.stop();
        }
    }
    let Some(worker) = worker else {
        return;
    };
    let deadline = Instant::now() + WORKER_DRAIN_TIMEOUT;
    while !worker.is_finished() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    if worker.is_finished() {
        if worker.join().is_err() {
            error!("meet-transcriber panicked");
        }
    } else {
        warn!(
            "meet-transcriber still busy after {}s; leaving it to finish",
            WORKER_DRAIN_TIMEOUT.as_secs()
        );
    }
}

impl MeetingManager {
    pub fn new(
        app: &AppHandle,
        store: Arc<ProjectStore>,
        tm: Arc<TranscriptionManager>,
        rm: Arc<AudioRecordingManager>,
    ) -> Self {
        Self {
            store,
            transcriber: Arc::new(ModelTranscriber(tm)),
            capture: Arc::new(CombinedCapture { app: app.clone() }),
            dictation_recording: Arc::new(move || rm.is_recording()),
            active: Mutex::new(None),
            transition: Mutex::new(()),
            live: AtomicBool::new(false),
            app: Some(app.clone()),
            summarizer: Some(Arc::new(crate::meeting::summary::LlmSummarizer {
                app: app.clone(),
            })),
        }
    }

    #[cfg(test)]
    pub fn new_for_test(
        store: Arc<ProjectStore>,
        transcriber: Arc<dyn Transcriber>,
        capture: Arc<dyn CaptureSource>,
        dictation_recording: Arc<AtomicBool>,
    ) -> Self {
        Self {
            store,
            transcriber,
            capture,
            dictation_recording: Arc::new(move || dictation_recording.load(Ordering::SeqCst)),
            active: Mutex::new(None),
            transition: Mutex::new(()),
            live: AtomicBool::new(false),
            app: None,
            summarizer: None,
        }
    }

    #[cfg(test)]
    pub fn with_summarizer(mut self, s: Arc<dyn Summarizer>) -> Self {
        self.summarizer = Some(s);
        self
    }

    /// Summarize a session on a background thread and emit the result.
    /// No-op without a summarizer.
    pub fn spawn_summary(&self, session_id: i64) {
        self.spawn_summaries(vec![session_id]);
    }

    /// Summarize sessions one after another (the rolling summary is a chain,
    /// so they must not run concurrently) on one background thread.
    fn spawn_summaries(&self, session_ids: Vec<i64>) {
        let Some(summarizer) = self.summarizer.clone() else {
            return;
        };
        if session_ids.is_empty() {
            return;
        }
        let store = Arc::clone(&self.store);
        let app = self.app.clone();
        let spawned = std::thread::Builder::new()
            .name("meet-summary".into())
            .spawn(move || {
                for session_id in session_ids {
                    match run_summary_job(&store, summarizer.as_ref(), session_id) {
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
    pub fn active_project_id(&self) -> Option<i64> {
        self.lock_active().as_ref().and_then(|a| a.project_id)
    }

    #[cfg(test)]
    fn worker_finished(&self) -> bool {
        self.lock_active()
            .as_ref()
            .and_then(|a| a.worker.as_ref())
            .is_none_or(|w| w.is_finished())
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

    fn auto_title() -> String {
        chrono::Local::now()
            .format("Meeting %b %-d %H:%M")
            .to_string()
    }

    pub fn start(&self, project_id: Option<i64>, meeting_id: Option<i64>) -> Result<Meeting> {
        if (self.dictation_recording)() {
            return Err(anyhow!(
                "Dictation is recording; stop it before starting a meeting"
            ));
        }
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
            None => self.store.create_meeting(project_id, &Self::auto_title())?,
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
        if (self.dictation_recording)() {
            return Err(anyhow!("Dictation is recording; stop it before resuming"));
        }
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
        stop_and_drain(capture, worker);
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
            self.spawn_summary(session_id);
        }
        result
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
        self.spawn_summaries(ids);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meeting::store::ProjectStore;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Mutex;

    struct FakeTranscriber {
        calls: AtomicUsize,
        fail_first: bool,
        delay_ms: u64,
    }
    impl Transcriber for FakeTranscriber {
        fn transcribe(&self, samples: Vec<f32>) -> anyhow::Result<String> {
            std::thread::sleep(std::time::Duration::from_millis(self.delay_ms));
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail_first && n == 0 {
                return Err(anyhow::anyhow!("Model is not loaded for transcription."));
            }
            Ok(if samples.is_empty() {
                String::new()
            } else {
                format!("heard {} samples", samples.len())
            })
        }
    }

    struct FakeCapture {
        senders: Mutex<Vec<Option<mpsc::Sender<Utterance>>>>,
        stops: AtomicUsize,
        fail_next: AtomicBool,
    }
    struct FakeSource(Arc<FakeCapture>);
    struct FakeHandle {
        cap: Arc<FakeCapture>,
        index: usize,
    }
    impl CaptureHandle for FakeHandle {
        fn stop(self: Box<Self>) {
            self.cap.stops.fetch_add(1, Ordering::SeqCst);
            self.cap.senders.lock().unwrap()[self.index] = None;
        }
        fn has_failed(&self) -> bool {
            false
        }
    }
    impl CaptureSource for FakeSource {
        fn start(
            &self,
            _meeting_id: i64,
            tx: mpsc::Sender<Utterance>,
        ) -> anyhow::Result<Box<dyn CaptureHandle>> {
            if self.0.fail_next.swap(false, Ordering::SeqCst) {
                return Err(anyhow::anyhow!("capture failed"));
            }
            let mut senders = self.0.senders.lock().unwrap();
            senders.push(Some(tx));
            Ok(Box::new(FakeHandle {
                cap: Arc::clone(&self.0),
                index: senders.len() - 1,
            }))
        }
    }

    fn mgr(
        fail_first: bool,
    ) -> (
        tempfile::TempDir,
        MeetingManager,
        Arc<FakeCapture>,
        Arc<AtomicBool>,
    ) {
        mgr_with_delay(fail_first, 0)
    }

    fn mgr_with_delay(
        fail_first: bool,
        delay_ms: u64,
    ) -> (
        tempfile::TempDir,
        MeetingManager,
        Arc<FakeCapture>,
        Arc<AtomicBool>,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(ProjectStore::open(dir.path().join("meet.db")).unwrap());
        let cap = Arc::new(FakeCapture {
            senders: Mutex::new(vec![]),
            stops: AtomicUsize::new(0),
            fail_next: AtomicBool::new(false),
        });
        let dict = Arc::new(AtomicBool::new(false));
        let m = MeetingManager::new_for_test(
            store,
            Arc::new(FakeTranscriber {
                calls: AtomicUsize::new(0),
                fail_first,
                delay_ms,
            }),
            Arc::new(FakeSource(cap.clone())),
            dict.clone(),
        );
        (dir, m, cap, dict)
    }

    fn wait_lines(m: &MeetingManager, meeting_id: i64, n: usize) -> Vec<TranscriptLine> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let d = m.store().get_meeting_detail(meeting_id).unwrap().unwrap();
            if d.lines.len() >= n {
                return d.lines;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for {} lines",
                n
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    #[test]
    fn start_creates_live_meeting_and_session_then_lines_arrive() {
        let (_d, m, cap, _dict) = mgr(false);
        let meeting = m.start(None, None).unwrap();
        assert_eq!(meeting.status, MeetingStatus::Live);
        let st = m.state();
        assert_eq!(st.meeting_id, Some(meeting.id));
        assert!(st.session_id.is_some());
        let tx = cap.senders.lock().unwrap()[0].clone().unwrap();
        tx.send(Utterance {
            speaker: "me".into(),
            samples: vec![0.1; 4800],
            started_at: 1,
        })
        .unwrap();
        tx.send(Utterance {
            speaker: "me".into(),
            samples: vec![],
            started_at: 2,
        })
        .unwrap(); // blank text, dropped
        tx.send(Utterance {
            speaker: "me".into(),
            samples: vec![0.1; 1600],
            started_at: 3,
        })
        .unwrap();
        let lines = wait_lines(&m, meeting.id, 2);
        assert_eq!(lines[0].text, "heard 4800 samples");
        assert_eq!(lines[1].text, "heard 1600 samples");
        assert!(lines.iter().all(|l| l.speaker == "me"));
    }

    struct SlowSummarizer;
    impl Summarizer for SlowSummarizer {
        fn summarize_session(
            &self,
            _t: &str,
            _c: &str,
            _l: &[TranscriptLine],
        ) -> anyhow::Result<crate::meeting::summary::SessionSummary> {
            std::thread::sleep(std::time::Duration::from_millis(200));
            Ok(crate::meeting::summary::SessionSummary {
                recap: "done".into(),
                ..Default::default()
            })
        }
        fn merge_rolling(
            &self,
            _p: Option<&str>,
            _n: &crate::meeting::summary::SessionSummary,
        ) -> anyhow::Result<String> {
            Ok("rolled".into())
        }
    }

    #[test]
    fn end_returns_before_the_summary_finishes_and_summary_lands() {
        let (_d, m, cap, _dict) = mgr(false);
        let m = m.with_summarizer(Arc::new(SlowSummarizer));
        let meeting = m.start(None, None).unwrap();
        let sid = m.state().session_id.unwrap();
        let tx = cap.senders.lock().unwrap()[0].clone().unwrap();
        tx.send(Utterance {
            speaker: "me".into(),
            samples: vec![0.1; 4800],
            started_at: 1,
        })
        .unwrap();
        drop(tx);
        wait_lines(&m, meeting.id, 1);
        let t0 = std::time::Instant::now();
        m.end().unwrap();
        assert!(t0.elapsed() < std::time::Duration::from_millis(50));
        assert!(m
            .store()
            .get_session(sid)
            .unwrap()
            .unwrap()
            .summary
            .is_none());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if m.store()
                .get_session(sid)
                .unwrap()
                .unwrap()
                .summary
                .is_some()
            {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "summary never landed");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    #[test]
    fn start_refuses_while_dictation_recording_or_already_live() {
        let (_d, m, _cap, dict) = mgr(false);
        dict.store(true, Ordering::SeqCst);
        assert!(m.start(None, None).is_err());
        dict.store(false, Ordering::SeqCst);
        m.start(None, None).unwrap();
        assert!(m.start(None, None).is_err());
    }

    #[test]
    fn pause_resume_keep_session_and_end_closes_it() {
        // A slow transcriber: pause and end must wait for queued utterances.
        let (_d, m, cap, _dict) = mgr_with_delay(false, 200);
        let meeting = m.start(None, None).unwrap();
        let session_id = m.state().session_id.unwrap();
        cap.senders.lock().unwrap()[0]
            .as_ref()
            .unwrap()
            .send(Utterance {
                speaker: "me".into(),
                samples: vec![0.1; 4800],
                started_at: 1,
            })
            .unwrap();
        m.pause().unwrap();
        assert!(m.worker_finished(), "pause drains the transcriber");
        let lines = m
            .store()
            .get_meeting_detail(meeting.id)
            .unwrap()
            .unwrap()
            .lines;
        assert_eq!(
            lines.len(),
            1,
            "queued utterance stored before pause returns"
        );
        assert_eq!(cap.stops.load(Ordering::SeqCst), 1, "pause stops capture");
        assert!(cap.senders.lock().unwrap()[0].is_none());
        assert_eq!(m.state().status, Some(MeetingStatus::Paused));
        assert_eq!(
            m.store().get_meeting(meeting.id).unwrap().unwrap().status,
            MeetingStatus::Paused
        );
        m.resume().unwrap();
        assert_eq!(m.state().session_id, Some(session_id));
        assert_eq!(
            cap.senders.lock().unwrap().len(),
            2,
            "capture restarted on resume"
        );
        cap.senders.lock().unwrap()[1]
            .as_ref()
            .unwrap()
            .send(Utterance {
                speaker: "me".into(),
                samples: vec![0.1; 1600],
                started_at: 2,
            })
            .unwrap();
        m.end().unwrap();
        assert_eq!(cap.stops.load(Ordering::SeqCst), 2, "end stops capture");
        let d = m.store().get_meeting_detail(meeting.id).unwrap().unwrap();
        assert_eq!(d.lines.len(), 2, "end drains the transcriber");
        assert_eq!(d.meeting.status, MeetingStatus::Ended);
        assert!(d.sessions[0].ended_at.is_some());
        assert_eq!(m.state().meeting_id, None);
    }

    #[test]
    fn is_live_tracks_capture_across_transitions() {
        let (_d, m, cap, _dict) = mgr(false);
        assert!(!m.is_live());
        m.start(None, None).unwrap();
        assert!(m.is_live());
        m.pause().unwrap();
        assert!(!m.is_live());
        m.resume().unwrap();
        assert!(m.is_live());
        m.end().unwrap();
        assert!(!m.is_live());
        cap.fail_next.store(true, Ordering::SeqCst);
        assert!(m.start(None, None).is_err());
        assert!(!m.is_live(), "failed start is not live");
    }

    #[test]
    fn line_keeps_utterance_speaker_and_start_time() {
        let (_d, m, cap, _dict) = mgr(false);
        let meeting = m.start(None, None).unwrap();
        cap.senders.lock().unwrap()[0]
            .as_ref()
            .unwrap()
            .send(Utterance {
                speaker: "them".into(),
                samples: vec![0.1; 4800],
                started_at: 1_234,
            })
            .unwrap();
        let lines = wait_lines(&m, meeting.id, 1);
        assert_eq!(lines[0].speaker, "them");
        assert_eq!(lines[0].ts, 1_234);
    }

    #[test]
    fn active_project_id_follows_running_meeting() {
        let (_d, m, _cap, _dict) = mgr(false);
        let p = m.store().create_project("P").unwrap();
        assert_eq!(m.active_project_id(), None);
        m.start(Some(p.id), None).unwrap();
        assert_eq!(m.active_project_id(), Some(p.id));
        m.pause().unwrap();
        assert_eq!(m.active_project_id(), Some(p.id), "paused still counts");
        m.end().unwrap();
        assert_eq!(m.active_project_id(), None);
    }

    #[test]
    fn end_idle_ends_a_paused_meeting_but_not_the_active_one() {
        let (_d, m, _cap, _dict) = mgr(false);
        let paused = m.store().create_meeting(None, "Old").unwrap();
        m.store().create_session(paused.id).unwrap();
        m.store()
            .set_meeting_status(paused.id, MeetingStatus::Paused)
            .unwrap();
        let running = m.start(None, None).unwrap();
        assert!(m.end_idle(running.id).is_err(), "active meeting refused");
        assert_eq!(m.state().meeting_id, Some(running.id));
        m.end_idle(paused.id).unwrap();
        let d = m.store().get_meeting_detail(paused.id).unwrap().unwrap();
        assert_eq!(d.meeting.status, MeetingStatus::Ended);
        assert!(d.sessions.iter().all(|x| x.ended_at.is_some()));
        assert!(m.end_idle(9_999).is_err(), "missing meeting is an error");
    }

    #[test]
    fn continue_ended_meeting_opens_second_session() {
        let (_d, m, _cap, _dict) = mgr(false);
        let meeting = m.start(None, None).unwrap();
        m.end().unwrap();
        let again = m.start(None, Some(meeting.id)).unwrap();
        assert_eq!(again.id, meeting.id);
        let d = m.store().get_meeting_detail(meeting.id).unwrap().unwrap();
        assert_eq!(d.sessions.len(), 2);
        assert_eq!(d.meeting.status, MeetingStatus::Live);
    }

    #[test]
    fn transcriber_error_is_retried_once_then_line_stored() {
        let (_d, m, cap, _dict) = mgr(true);
        let meeting = m.start(None, None).unwrap();
        let tx = cap.senders.lock().unwrap()[0].clone().unwrap();
        tx.send(Utterance {
            speaker: "me".into(),
            samples: vec![0.1; 4800],
            started_at: 1,
        })
        .unwrap();
        let lines = wait_lines(&m, meeting.id, 1);
        assert_eq!(lines[0].text, "heard 4800 samples");
    }

    #[test]
    fn start_failure_restores_ended_status_and_discards_empty_meeting() {
        let (_d, m, cap, _dict) = mgr(false);
        let meeting = m.start(None, None).unwrap();
        m.end().unwrap();
        cap.fail_next.store(true, Ordering::SeqCst);
        assert!(m.start(None, Some(meeting.id)).is_err());
        let d = m.store().get_meeting_detail(meeting.id).unwrap().unwrap();
        assert_eq!(d.meeting.status, MeetingStatus::Ended);
        // No delete_session in the store: the aborted session stays but is closed.
        assert_eq!(d.sessions.len(), 2);
        assert!(d.sessions.iter().all(|x| x.ended_at.is_some()));
        assert_eq!(m.state().meeting_id, None);

        let before = m.store().list_meetings(None).unwrap().len();
        cap.fail_next.store(true, Ordering::SeqCst);
        assert!(m.start(None, None).is_err());
        assert_eq!(m.store().list_meetings(None).unwrap().len(), before);
    }

    /// Records stops; reports a preset failure state.
    struct CountingHandle {
        stops: Arc<AtomicUsize>,
        failed: bool,
    }
    impl CaptureHandle for CountingHandle {
        fn stop(self: Box<Self>) {
            self.stops.fetch_add(1, Ordering::SeqCst);
        }
        fn has_failed(&self) -> bool {
            self.failed
        }
    }

    fn counting(failed: bool) -> (Arc<AtomicUsize>, Box<dyn CaptureHandle>) {
        let stops = Arc::new(AtomicUsize::new(0));
        let h = Box::new(CountingHandle {
            stops: Arc::clone(&stops),
            failed,
        });
        (stops, h)
    }

    #[test]
    fn combined_handle_stops_both_streams_once() {
        let (mic_stops, mic) = counting(false);
        let (sys_stops, sys) = counting(false);
        let h: Box<dyn CaptureHandle> = Box::new(CombinedHandle {
            mic,
            system: Some(sys),
        });
        h.stop();
        assert_eq!(mic_stops.load(Ordering::SeqCst), 1);
        assert_eq!(sys_stops.load(Ordering::SeqCst), 1);

        let (mic_stops, mic) = counting(false);
        let h: Box<dyn CaptureHandle> = Box::new(CombinedHandle { mic, system: None });
        h.stop();
        assert_eq!(mic_stops.load(Ordering::SeqCst), 1, "mic-only stops");
    }

    #[test]
    fn combined_handle_failure_reports_the_microphone_only() {
        let (_, mic) = counting(false);
        let (_, sys) = counting(true);
        let h = CombinedHandle {
            mic,
            system: Some(sys),
        };
        assert!(!h.has_failed(), "system failure is warned separately");
        let (_, mic) = counting(true);
        let h = CombinedHandle { mic, system: None };
        assert!(h.has_failed());
    }

    /// A source that, like `CombinedCapture`, runs a mic and a system stream
    /// on one channel; each emits one utterance as soon as it starts.
    struct TwoStreamSource;
    impl CaptureSource for TwoStreamSource {
        fn start(
            &self,
            _meeting_id: i64,
            tx: mpsc::Sender<Utterance>,
        ) -> anyhow::Result<Box<dyn CaptureHandle>> {
            let sys_tx = tx.clone();
            tx.send(Utterance {
                speaker: "me".into(),
                samples: vec![0.1; 4800],
                started_at: 1,
            })?;
            sys_tx.send(Utterance {
                speaker: "them".into(),
                samples: vec![0.1; 3200],
                started_at: 2,
            })?;
            struct Holds(#[allow(dead_code)] mpsc::Sender<Utterance>);
            impl CaptureHandle for Holds {
                fn stop(self: Box<Self>) {}
                fn has_failed(&self) -> bool {
                    false
                }
            }
            Ok(Box::new(CombinedHandle {
                mic: Box::new(Holds(tx)),
                system: Some(Box::new(Holds(sys_tx))),
            }))
        }
    }

    #[test]
    fn mic_and_system_utterances_store_both_speakers_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(ProjectStore::open(dir.path().join("meet.db")).unwrap());
        let m = MeetingManager::new_for_test(
            store,
            Arc::new(FakeTranscriber {
                calls: AtomicUsize::new(0),
                fail_first: false,
                delay_ms: 0,
            }),
            Arc::new(TwoStreamSource),
            Arc::new(AtomicBool::new(false)),
        );
        let meeting = m.start(None, None).unwrap();
        let lines = wait_lines(&m, meeting.id, 2);
        let speakers: Vec<&str> = lines.iter().map(|l| l.speaker.as_str()).collect();
        assert_eq!(speakers, vec!["me", "them"]);
        assert_eq!(lines[0].text, "heard 4800 samples");
        assert_eq!(lines[1].text, "heard 3200 samples");
        m.end().unwrap();
    }
}
