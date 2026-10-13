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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_toolkit::vad::{VadFrame, VoiceActivityDetector};
    use handy_recorder::AudioChunk;
    use std::sync::mpsc;

    /// Detector that calls a frame speech when its mean absolute amplitude
    /// exceeds 0.05, with no smoothing. 320-sample frames.
    struct LoudVad;
    impl VoiceActivityDetector for LoudVad {
        fn push_frame<'a>(&'a mut self, frame: &'a [f32]) -> anyhow::Result<VadFrame<'a>> {
            let mean = frame.iter().map(|s| s.abs()).sum::<f32>() / frame.len().max(1) as f32;
            Ok(if mean > 0.05 {
                VadFrame::Speech(frame)
            } else {
                VadFrame::Noise
            })
        }
        fn frame_samples(&self) -> usize {
            320
        }
        fn set_hangover_frames(&mut self, _frames: usize) {}
        fn reset(&mut self) {}
    }

    fn chunk(samples: &[f32]) -> AudioChunk<'_> {
        AudioChunk {
            samples,
            sample_rate: 16_000,
            channels: 1,
            valid_frames: samples.len(),
        }
    }

    #[test]
    fn sink_emits_an_utterance_after_speech_then_silence() {
        let vad = CaptureVad::new(Box::new(LoudVad), 0, 0);
        let (tx, rx) = mpsc::channel();
        let mut sink = MeetingSink::new(vad, tx);
        let loud = vec![0.2f32; 320];
        let quiet = vec![0.0f32; 320];
        for _ in 0..25 {
            sink.process_chunk(chunk(&loud));
        }
        for _ in 0..40 {
            sink.process_chunk(chunk(&quiet));
        }
        let utt = rx.try_recv().expect("one utterance");
        assert_eq!(utt.samples.len(), 25 * 320);
        assert!(utt.started_at > 0);
        assert_eq!(utt.speaker, "me");
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn sink_ignores_padding_beyond_valid_frames() {
        let vad = CaptureVad::new(Box::new(LoudVad), 0, 0);
        let (tx, rx) = mpsc::channel();
        let mut sink = MeetingSink::new(vad, tx);
        let loud = vec![0.2f32; 320];
        let quiet = vec![0.0f32; 320];
        let mut partial = vec![0.2f32; 320];
        for s in partial.iter_mut().skip(100) {
            *s = 0.0;
        }
        for _ in 0..25 {
            sink.process_chunk(chunk(&loud));
        }
        // Only 100 valid frames: must not be pushed as a full frame.
        sink.process_chunk(AudioChunk {
            samples: &partial,
            sample_rate: 16_000,
            channels: 1,
            valid_frames: 100,
        });
        for _ in 0..40 {
            sink.process_chunk(chunk(&quiet));
        }
        let utt = rx.try_recv().expect("one utterance");
        assert_eq!(utt.samples.len(), 25 * 320);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn sink_finish_flushes_trailing_utterance() {
        let vad = CaptureVad::new(Box::new(LoudVad), 0, 0);
        let (tx, rx) = mpsc::channel();
        let mut sink = MeetingSink::new(vad, tx);
        let loud = vec![0.2f32; 320];
        for _ in 0..25 {
            sink.process_chunk(chunk(&loud));
        }
        assert!(rx.try_recv().is_err());
        sink.finish();
        let utt = rx.try_recv().expect("flushed utterance");
        assert_eq!(utt.samples.len(), 25 * 320);
        assert!(rx.try_recv().is_err());
    }

    /// Like `LoudVad`, but the first speech frame comes with 10 frames
    /// (200 ms) of prefill, as Silero's smoothing wrapper delivers it.
    struct PrefillVad {
        out: Vec<f32>,
        in_speech: bool,
    }
    impl VoiceActivityDetector for PrefillVad {
        fn push_frame<'a>(&'a mut self, frame: &'a [f32]) -> anyhow::Result<VadFrame<'a>> {
            let mean = frame.iter().map(|s| s.abs()).sum::<f32>() / frame.len().max(1) as f32;
            if mean <= 0.05 {
                self.in_speech = false;
                return Ok(VadFrame::Noise);
            }
            if self.in_speech {
                return Ok(VadFrame::Speech(frame));
            }
            self.in_speech = true;
            self.out = vec![0.0; 320 * 10];
            self.out.extend_from_slice(frame);
            Ok(VadFrame::Speech(&self.out))
        }
        fn frame_samples(&self) -> usize {
            320
        }
        fn set_hangover_frames(&mut self, _frames: usize) {}
        fn reset(&mut self) {}
    }

    #[test]
    fn utterance_started_at_precedes_emit_by_prefill() {
        let vad = CaptureVad::new(
            Box::new(PrefillVad {
                out: vec![],
                in_speech: false,
            }),
            0,
            0,
        );
        let (tx, rx) = mpsc::channel();
        let mut sink = MeetingSink::new(vad, tx);
        let loud = vec![0.2f32; 320];
        let quiet = vec![0.0f32; 320];
        let before = chrono::Utc::now().timestamp_millis();
        for _ in 0..25 {
            sink.process_chunk(chunk(&loud));
        }
        for _ in 0..40 {
            sink.process_chunk(chunk(&quiet));
        }
        let emitted_at = chrono::Utc::now().timestamp_millis();
        let utt = rx.try_recv().expect("one utterance");
        assert_eq!(utt.samples.len(), 35 * 320, "prefill plus 25 frames");
        assert!(utt.started_at <= emitted_at);
        // First push happened in [before, emitted_at]; prefill is 200 ms.
        assert!(utt.started_at <= emitted_at - 200, "{}", utt.started_at);
        assert!(utt.started_at >= before - 200, "{}", utt.started_at);
    }

    #[test]
    fn end_silence_is_shortened_by_detector_hangover() {
        // 10 frames of hangover = 200 ms: the segmenter waits 500 ms, not 700.
        let vad = CaptureVad::new(Box::new(LoudVad), 10, 0);
        let (tx, rx) = mpsc::channel();
        let mut sink = MeetingSink::new(vad, tx);
        let loud = vec![0.2f32; 320];
        let quiet = vec![0.0f32; 320];
        for _ in 0..25 {
            sink.process_chunk(chunk(&loud));
        }
        for _ in 0..30 {
            sink.process_chunk(chunk(&quiet));
        }
        let utt = rx
            .try_recv()
            .expect("utterance ends within 30 quiet frames once hangover is counted");
        assert_eq!(utt.samples.len(), 25 * 320);
        for _ in 0..10 {
            sink.process_chunk(chunk(&quiet));
        }
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn with_speaker_tags_utterances() {
        let vad = CaptureVad::new(Box::new(LoudVad), 0, 0);
        let (tx, rx) = mpsc::channel();
        let mut sink = MeetingSink::with_speaker(vad, tx, "them");
        let loud = vec![0.2f32; 320];
        let quiet = vec![0.0f32; 320];
        for _ in 0..25 {
            sink.process_chunk(chunk(&loud));
        }
        for _ in 0..40 {
            sink.process_chunk(chunk(&quiet));
        }
        let utt = rx.try_recv().expect("one utterance");
        assert_eq!(utt.speaker, "them");
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn push_frame_emits_an_utterance_without_audio_chunks() {
        let vad = CaptureVad::new(Box::new(LoudVad), 0, 0);
        let (tx, rx) = mpsc::channel();
        let mut sink = MeetingSink::with_speaker(vad, tx, "them");
        let loud = vec![0.2f32; 320];
        let quiet = vec![0.0f32; 320];
        for _ in 0..25 {
            sink.push_frame(&loud);
        }
        for _ in 0..40 {
            sink.push_frame(&quiet);
        }
        let utt = rx.try_recv().expect("one utterance");
        assert_eq!(utt.samples.len(), 25 * 320);
        assert_eq!(utt.speaker, "them");
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn push_frame_ignores_short_frames() {
        let vad = CaptureVad::new(Box::new(LoudVad), 0, 0);
        let (tx, rx) = mpsc::channel();
        let mut sink = MeetingSink::with_speaker(vad, tx, "them");
        let short = vec![0.2f32; 100];
        let quiet = vec![0.0f32; 320];
        for _ in 0..30 {
            sink.push_frame(&short);
        }
        for _ in 0..40 {
            sink.push_frame(&quiet);
        }
        assert!(rx.try_recv().is_err());
    }
}
