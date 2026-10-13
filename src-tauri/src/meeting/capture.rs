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
    pub samples: Vec<f32>,
    pub captured_at: i64,
}

pub struct MeetingSink {
    vad: CaptureVad,
    segmenter: Segmenter,
    tx: mpsc::Sender<Utterance>,
}

impl MeetingSink {
    pub fn new(mut vad: CaptureVad, tx: mpsc::Sender<Utterance>) -> Self {
        vad.prepare_offline();
        let segmenter = Segmenter::new(SegmenterConfig::defaults(vad.frame_samples()));
        Self { vad, segmenter, tx }
    }

    fn emit(&self, samples: Vec<f32>) {
        let utt = Utterance {
            samples,
            captured_at: chrono::Utc::now().timestamp_millis(),
        };
        if self.tx.send(utt).is_err() {
            warn!("meeting capture: utterance receiver is gone");
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
        let frame = &chunk.samples[..frame_len];
        let speech = match self.vad.push_frame(frame) {
            Ok(s) => s,
            Err(e) => {
                warn!("meeting capture: VAD error: {e:#}");
                None
            }
        };
        if let Some(samples) = self.segmenter.push(speech.as_deref()) {
            self.emit(samples);
        }
    }
}

pub struct MeetingCapture {
    recorder: Recorder<MeetingSink>,
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
        assert!(utt.captured_at > 0);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn sink_ignores_padding_beyond_valid_frames() {
        let vad = CaptureVad::new(Box::new(LoudVad), 0, 0);
        let (tx, rx) = mpsc::channel();
        let mut sink = MeetingSink::new(vad, tx);
        let mut partial = vec![0.2f32; 320];
        for s in partial.iter_mut().skip(100) {
            *s = 0.0;
        }
        // Only 100 valid frames: must not be pushed as a full frame.
        sink.process_chunk(AudioChunk {
            samples: &partial,
            sample_rate: 16_000,
            channels: 1,
            valid_frames: 100,
        });
        assert!(rx.try_recv().is_err());
    }
}
