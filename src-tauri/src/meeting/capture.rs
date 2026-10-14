//! Microphone capture for meetings: its own handy-recorder stream, the shared
//! VAD, and the segmenter. Emits utterances on a channel; owns no model.

use crate::audio_toolkit::audio::CaptureVad;
use crate::managers::audio::{create_capture_vad, microphone_config};
use crate::meeting::segment::{Segmenter, SegmenterConfig};
use crate::settings::AppSettings;
use anyhow::{anyhow, Context, Result};
use handy_recorder::{AudioChunk, Recorder, Sink};
use log::{info, warn};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

pub struct Utterance {
    /// Who spoke. The microphone path is always the user ("me").
    pub speaker: String,
    pub samples: Vec<f32>,
    /// Wall-clock ms when the utterance's first sample was captured.
    pub started_at: i64,
}

const SAMPLE_RATE: usize = 16_000;

pub struct MeetingSink {
    vad: CaptureVad,
    segmenter: Segmenter,
    tx: mpsc::Sender<Utterance>,
    started_at: Option<i64>,
    speaker: String,
}

impl MeetingSink {
    pub fn new(vad: CaptureVad, tx: mpsc::Sender<Utterance>) -> Self {
        Self::with_speaker(vad, tx, "me")
    }

    /// Like `new`, tagging every emitted utterance with `speaker`.
    pub fn with_speaker(mut vad: CaptureVad, tx: mpsc::Sender<Utterance>, speaker: &str) -> Self {
        vad.prepare_offline();
        let frame = vad.frame_samples();
        let mut cfg = SegmenterConfig::defaults(frame);
        // The detector already labels `hangover` frames after real speech as
        // speech; count them toward the end silence instead of waiting the
        // full silence again after them.
        let hangover_ms = (vad.offline_hangover_frames() * frame * 1000 / SAMPLE_RATE) as u64;
        cfg.end_silence_ms = cfg.end_silence_ms.saturating_sub(hangover_ms).max(200);
        let segmenter = Segmenter::new(cfg);
        Self {
            vad,
            segmenter,
            tx,
            started_at: None,
            speaker: speaker.to_string(),
        }
    }

    fn emit(&mut self, samples: Vec<f32>) {
        let now = chrono::Utc::now().timestamp_millis();
        let started_at = self.started_at.take().unwrap_or(now);
        let utt = Utterance {
            speaker: self.speaker.clone(),
            samples,
            started_at,
        };
        if self.tx.send(utt).is_err() {
            warn!("meeting capture: utterance receiver is gone");
        }
    }

    /// Run one VAD frame through the detector and segmenter, emitting an
    /// utterance when one ends. `frame` must be `frame_samples()` long.
    pub fn push_frame(&mut self, frame: &[f32]) {
        let frame_len = self.vad.frame_samples();
        let Some(frame) = frame.get(..frame_len) else {
            return;
        };
        let speech = match self.vad.push_frame(frame) {
            Ok(s) => s,
            Err(e) => {
                warn!("meeting capture: VAD error: {e:#}");
                None
            }
        };
        if let Some(samples) = speech.as_deref() {
            if self.segmenter.is_empty() {
                // First speech of a new utterance. Anything beyond this frame
                // is detector prefill, captured before now.
                let prefill = samples.len().saturating_sub(frame_len);
                let prefill_ms = (prefill * 1000 / SAMPLE_RATE) as i64;
                self.started_at = Some(chrono::Utc::now().timestamp_millis() - prefill_ms);
            }
        }
        if let Some(samples) = self.segmenter.push(speech.as_deref()) {
            self.emit(samples);
        }
    }

    /// Flush a trailing utterance when capture stops.
    pub fn finish(mut self) {
        if let Some(samples) = self.segmenter.flush() {
            self.emit(samples);
        }
    }
}

impl Sink for MeetingSink {
    fn process_chunk(&mut self, chunk: AudioChunk<'_>) {
        let frame_len = self.vad.frame_samples();
        if chunk.valid_frames < frame_len {
            // Final zero-padded chunk; never feed padding to the detector.
            return;
        }
        self.push_frame(chunk.samples);
    }
}

pub struct MeetingCapture {
    recorder: Recorder<MeetingSink>,
    /// Set by the recorder error callback; read via `has_failed`.
    failed: Arc<AtomicBool>,
}

impl MeetingCapture {
    /// Open the configured microphone and start delivering utterances to `tx`.
    pub fn start(
        settings: &AppSettings,
        app: &tauri::AppHandle,
        tx: mpsc::Sender<Utterance>,
    ) -> Result<Self> {
        let vad = create_capture_vad(settings.vad_backend, app).context("create VAD")?;
        let frame = vad.frame_samples();
        let config = microphone_config(
            settings.selected_microphone.clone(),
            settings.selected_channel,
            frame,
        );
        let failed = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&failed);
        let recorder = Recorder::open_with_failure_handler(config, move |e| {
            warn!("meeting capture: recorder failed: {e}");
            flag.store(true, Ordering::Release);
        })
        .map_err(|e| anyhow!("open microphone: {e}"))?;
        let sink = MeetingSink::new(vad, tx);
        recorder
            .start(sink)
            .map_err(|e| anyhow!("start microphone: {}", e.error))?;
        info!("meeting capture started ({} samples/frame)", frame);
        Ok(Self { recorder, failed })
    }

    /// Whether the recorder reported a failure (device lost, stream error).
    pub fn has_failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }

    /// Stop the stream, flush the trailing utterance, close the device.
    pub fn stop(self) {
        match self.recorder.stop() {
            Ok(stopped) => stopped.sink.finish(),
            Err(e) => warn!("meeting capture: stop failed: {e}"),
        }
        if let Err(e) = self.recorder.close() {
            warn!("meeting capture: close failed: {e}");
        }
    }
}
