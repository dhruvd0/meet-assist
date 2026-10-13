//! Meeting session manager: Idle -> Live <-> Paused -> Ended. Owns capture
//! lifetime and the transcriber thread; emits state and transcript events.

use crate::managers::audio::AudioRecordingManager;
use crate::managers::transcription::TranscriptionManager;
use crate::meeting::capture::{MeetingCapture, Utterance};
use crate::meeting::store::{Meeting, MeetingStatus, ProjectStore, TranscriptLine};
use anyhow::{anyhow, Result};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use tauri::AppHandle;
use tauri_specta::Event;

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
    fn stop(self: Box<Self>);
}

pub trait CaptureSource: Send + Sync {
    fn start(&self, tx: mpsc::Sender<Utterance>) -> Result<Box<dyn CaptureHandle>>;
}

struct MicCapture {
    app: AppHandle,
}

impl CaptureHandle for MeetingCapture {
    fn stop(self: Box<Self>) {
        MeetingCapture::stop(*self);
    }
}

impl CaptureSource for MicCapture {
    fn start(&self, tx: mpsc::Sender<Utterance>) -> Result<Box<dyn CaptureHandle>> {
        let settings = crate::settings::get_settings(&self.app);
        Ok(Box::new(MeetingCapture::start(&settings, &self.app, tx)?))
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

struct Active {
    meeting_id: i64,
    session_id: i64,
    status: MeetingStatus,
    capture: Option<Box<dyn CaptureHandle>>,
}

pub struct MeetingManager {
    store: Arc<ProjectStore>,
    transcriber: Arc<dyn Transcriber>,
    capture: Arc<dyn CaptureSource>,
    dictation_recording: Arc<dyn Fn() -> bool + Send + Sync>,
    active: Mutex<Option<Active>>,
    app: Option<AppHandle>,
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
            capture: Arc::new(MicCapture { app: app.clone() }),
            dictation_recording: Arc::new(move || rm.is_recording()),
            active: Mutex::new(None),
            app: Some(app.clone()),
        }
    }

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
            app: None,
        }
    }

    pub fn store(&self) -> &ProjectStore {
        &self.store
    }

    fn lock_active(&self) -> std::sync::MutexGuard<'_, Option<Active>> {
        self.active.lock().unwrap_or_else(|p| p.into_inner())
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
    ) -> Result<Box<dyn CaptureHandle>> {
        let (tx, rx) = mpsc::channel::<Utterance>();
        let handle = self.capture.start(tx)?;
        let store = Arc::clone(&self.store);
        let transcriber = Arc::clone(&self.transcriber);
        let app = self.app.clone();
        std::thread::Builder::new()
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
            })?;
        Ok(handle)
    }

    fn transcribe_and_store(
        store: &ProjectStore,
        transcriber: &dyn Transcriber,
        session_id: i64,
        utt: Utterance,
    ) -> Result<Option<TranscriptLine>> {
        let samples = utt.samples;
        let text = match transcriber.transcribe(samples.clone()) {
            Ok(t) => t,
            Err(first) => {
                warn!("transcribe failed once ({first:#}); retrying");
                std::thread::sleep(std::time::Duration::from_millis(500));
                transcriber.transcribe(samples)?
            }
        };
        let text = text.trim();
        if text.is_empty() {
            return Ok(None);
        }
        Ok(Some(store.append_line(session_id, "me", text)?))
    }

    /// Public for tests and for a future system-audio path; the worker uses
    /// the same function.
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
        let mut guard = self.lock_active();
        if guard.is_some() {
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
        let capture = match self.start_capture_and_worker(meeting.id, session.id) {
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
                return Err(e);
            }
        };
        *guard = Some(Active {
            meeting_id: meeting.id,
            session_id: session.id,
            status: MeetingStatus::Live,
            capture: Some(capture),
        });
        drop(guard);
        self.emit_state();
        Ok(meeting)
    }

    pub fn pause(&self) -> Result<()> {
        let mut guard = self.lock_active();
        let a = guard.as_mut().ok_or_else(|| anyhow!("no active meeting"))?;
        if a.status != MeetingStatus::Live {
            return Err(anyhow!("meeting is not live"));
        }
        if let Some(c) = a.capture.take() {
            c.stop();
        }
        a.status = MeetingStatus::Paused;
        self.store
            .set_meeting_status(a.meeting_id, MeetingStatus::Paused)?;
        drop(guard);
        self.emit_state();
        Ok(())
    }

    pub fn resume(&self) -> Result<()> {
        if (self.dictation_recording)() {
            return Err(anyhow!("Dictation is recording; stop it before resuming"));
        }
        let mut guard = self.lock_active();
        let a = guard.as_mut().ok_or_else(|| anyhow!("no active meeting"))?;
        if a.status != MeetingStatus::Paused {
            return Err(anyhow!("meeting is not paused"));
        }
        let capture = self.start_capture_and_worker(a.meeting_id, a.session_id)?;
        a.capture = Some(capture);
        a.status = MeetingStatus::Live;
        self.store
            .set_meeting_status(a.meeting_id, MeetingStatus::Live)?;
        drop(guard);
        self.emit_state();
        Ok(())
    }

    pub fn end(&self) -> Result<()> {
        let mut guard = self.lock_active();
        let a = guard.take().ok_or_else(|| anyhow!("no active meeting"))?;
        if let Some(c) = a.capture {
            c.stop();
        }
        self.store.end_session(a.session_id)?;
        self.store
            .set_meeting_status(a.meeting_id, MeetingStatus::Ended)?;
        drop(guard);
        self.emit_state();
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
    }
    impl Transcriber for FakeTranscriber {
        fn transcribe(&self, samples: Vec<f32>) -> anyhow::Result<String> {
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
    }
    impl CaptureSource for FakeSource {
        fn start(&self, tx: mpsc::Sender<Utterance>) -> anyhow::Result<Box<dyn CaptureHandle>> {
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
            samples: vec![0.1; 4800],
            captured_at: 1,
        })
        .unwrap();
        tx.send(Utterance {
            samples: vec![],
            captured_at: 2,
        })
        .unwrap(); // blank text, dropped
        tx.send(Utterance {
            samples: vec![0.1; 1600],
            captured_at: 3,
        })
        .unwrap();
        let lines = wait_lines(&m, meeting.id, 2);
        assert_eq!(lines[0].text, "heard 4800 samples");
        assert_eq!(lines[1].text, "heard 1600 samples");
        assert!(lines.iter().all(|l| l.speaker == "me"));
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
        let (_d, m, cap, _dict) = mgr(false);
        let meeting = m.start(None, None).unwrap();
        let session_id = m.state().session_id.unwrap();
        m.pause().unwrap();
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
        m.end().unwrap();
        assert_eq!(cap.stops.load(Ordering::SeqCst), 2, "end stops capture");
        let d = m.store().get_meeting_detail(meeting.id).unwrap().unwrap();
        assert_eq!(d.meeting.status, MeetingStatus::Ended);
        assert!(d.sessions[0].ended_at.is_some());
        assert_eq!(m.state().meeting_id, None);
    }

    #[test]
    fn continue_ended_meeting_opens_second_session() {
        let (_d, m, _cap, _dict) = mgr(false);
        let meeting = m.start(Some(1).filter(|_| false), None).unwrap();
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
            samples: vec![0.1; 4800],
            captured_at: 1,
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
}
