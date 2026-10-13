//! System audio ("them") capture: loopback of the default output device on
//! macOS (CoreAudio process tap) and Windows (WASAPI loopback), a PulseAudio
//! monitor source on Linux. Feeds a `MeetingSink` tagged `them`.

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
use crate::managers::audio::create_capture_vad;
use crate::meeting::capture::{MeetingSink, Utterance};
use crate::meeting::resample::{FrameSlicer, MonoResampler};
use crate::settings::AppSettings;
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
use anyhow::anyhow;
#[allow(unused_imports)]
use anyhow::{Context, Result};
#[allow(unused_imports)]
use log::{info, warn};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, TryLockError};

/// Model input rate.
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
const TARGET_RATE: u32 = 16_000;

/// The sink shared between the real-time callback and `stop`. `None` once
/// `stop` has flushed it.
pub(crate) type SharedSink = Arc<Mutex<Option<MeetingSink>>>;

/// Real-time half of loopback capture: downmix + resample, slice into VAD
/// frames, hand each frame to the sink without ever blocking. Owned by the
/// device callback.
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
pub(crate) struct LoopbackPipeline {
    resampler: MonoResampler,
    slicer: FrameSlicer,
    sink: SharedSink,
    dropped: Arc<AtomicU64>,
}

#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
impl LoopbackPipeline {
    pub(crate) fn new(
        resampler: MonoResampler,
        frame_len: usize,
        sink: SharedSink,
        dropped: Arc<AtomicU64>,
    ) -> Self {
        Self {
            resampler,
            slicer: FrameSlicer::new(frame_len),
            sink,
            dropped,
        }
    }

    /// Feed one device buffer of interleaved samples.
    pub(crate) fn push(&mut self, interleaved: &[f32]) {
        let mono = self.resampler.push(interleaved);
        for frame in self.slicer.push(&mono) {
            match self.sink.try_lock() {
                Ok(mut guard) => {
                    if let Some(sink) = guard.as_mut() {
                        sink.push_frame(&frame);
                    }
                }
                Err(TryLockError::Poisoned(p)) => {
                    if let Some(sink) = p.into_inner().as_mut() {
                        sink.push_frame(&frame);
                    }
                }
                Err(TryLockError::WouldBlock) => {
                    self.dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }
}

/// Take the sink out of the shared slot and flush its trailing utterance.
/// Later calls are no-ops.
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
pub(crate) fn finish_sink(sink: &SharedSink) {
    let taken = sink.lock().unwrap_or_else(|p| p.into_inner()).take();
    if let Some(sink) = taken {
        sink.finish();
    }
}

/// Choose the PulseAudio monitor of the default output: the first monitor
/// whose name contains the default output's name, else the first monitor.
/// `devices` are `(id, name, is_monitor)`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn pick_monitor(
    devices: &[(String, String, bool)],
    default_output: Option<&str>,
) -> Option<String> {
    let monitors = || devices.iter().filter(|(_, _, m)| *m);
    default_output
        .filter(|name| !name.is_empty())
        .and_then(|name| monitors().find(|(_, n, _)| n.contains(name)))
        .or_else(|| monitors().next())
        .map(|(id, _, _)| id.clone())
}

pub struct SystemCapture {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    stream: handy_cpal::Stream,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    sink: SharedSink,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    dropped: Arc<AtomicU64>,
    #[cfg(target_os = "linux")]
    recorder: handy_recorder::Recorder<MeetingSink>,
    /// Set by the stream error callback; read via `has_failed`.
    failed: Arc<AtomicBool>,
}

impl SystemCapture {
    /// Open loopback of the default output device and start delivering
    /// `them` utterances to `tx`.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub fn start(
        settings: &AppSettings,
        app: &tauri::AppHandle,
        tx: mpsc::Sender<Utterance>,
    ) -> Result<Self> {
        use handy_cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

        let host = handy_cpal::default_host();
        let device = host
            .default_output_device()
            .context("no default output device")?;
        // Output config: loopback streams take the output's format. Never
        // ask an output device for its input config (errors on WASAPI).
        let cfg = device
            .default_output_config()
            .context("query output device config")?;
        if cfg.sample_format() != handy_cpal::SampleFormat::F32 {
            anyhow::bail!(
                "system audio: unsupported output sample format {:?}",
                cfg.sample_format()
            );
        }
        let vad = create_capture_vad(settings.vad_backend, app).context("create VAD")?;
        let frame = vad.frame_samples();
        let resampler = MonoResampler::new(cfg.sample_rate(), TARGET_RATE, cfg.channels())
            .context("create resampler")?;
        let sink: SharedSink =
            Arc::new(Mutex::new(Some(MeetingSink::with_speaker(vad, tx, "them"))));
        let dropped = Arc::new(AtomicU64::new(0));
        let failed = Arc::new(AtomicBool::new(false));
        let mut pipeline =
            LoopbackPipeline::new(resampler, frame, Arc::clone(&sink), Arc::clone(&dropped));
        let flag = Arc::clone(&failed);
        let stream = device
            .build_input_stream(
                cfg.config(),
                move |data: &[f32], _: &handy_cpal::InputCallbackInfo| pipeline.push(data),
                move |e| {
                    if !flag.swap(true, Ordering::AcqRel) {
                        warn!("system audio: stream error: {e}");
                    }
                },
                None,
            )
            .context("open system audio loopback")?;
        stream.play().context("start system audio loopback")?;
        info!(
            "system audio capture started ({} Hz, {} ch, {} samples/frame)",
            cfg.sample_rate(),
            cfg.channels(),
            frame
        );
        Ok(Self {
            stream,
            sink,
            dropped,
            failed,
        })
    }

    /// Open the default output's PulseAudio monitor source.
    #[cfg(target_os = "linux")]
    pub fn start(
        settings: &AppSettings,
        app: &tauri::AppHandle,
        tx: mpsc::Sender<Utterance>,
    ) -> Result<Self> {
        use crate::managers::audio::microphone_config;
        use handy_cpal::traits::{DeviceTrait, HostTrait};
        use handy_recorder::{Recorder, RecorderConfig};

        let devices: Vec<(String, String, bool)> = handy_recorder::list_input_devices()
            .map_err(|e| anyhow::anyhow!("list input devices: {e}"))?
            .into_iter()
            .map(|d| (d.id, d.name, d.is_monitor))
            .collect();
        let default_output = handy_cpal::default_host()
            .default_output_device()
            .and_then(|d| d.description().ok())
            .map(|d| d.name().to_string());
        let id = pick_monitor(&devices, default_output.as_deref())
            .context("no PulseAudio monitor source for system audio")?;
        let vad = create_capture_vad(settings.vad_backend, app).context("create VAD")?;
        let frame = vad.frame_samples();
        let config = RecorderConfig {
            device: Some(id.clone()),
            ..microphone_config(None, None, frame)
        };
        let failed = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&failed);
        let recorder = Recorder::open_with_failure_handler(config, move |e| {
            warn!("system audio: monitor failed: {e}");
            flag.store(true, Ordering::Release);
        })
        .map_err(|e| anyhow::anyhow!("open monitor source: {e}"))?;
        recorder
            .start(MeetingSink::with_speaker(vad, tx, "them"))
            .map_err(|e| anyhow::anyhow!("start monitor source: {}", e.error))?;
        info!("system audio capture started on monitor {id}");
        Ok(Self { recorder, failed })
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    pub fn start(
        _settings: &AppSettings,
        _app: &tauri::AppHandle,
        _tx: mpsc::Sender<Utterance>,
    ) -> Result<Self> {
        Err(anyhow!(
            "system audio capture is not supported on this platform"
        ))
    }

    /// Whether the stream reported an error (device lost, tap revoked).
    pub fn has_failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }

    /// Stop the stream (tearing down the tap), then flush the trailing
    /// utterance.
    pub fn stop(self) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            let Self {
                stream,
                sink,
                dropped,
                ..
            } = self;
            // Dropping the stream joins the callback and destroys the tap and
            // aggregate device; afterwards nothing contends for the sink.
            drop(stream);
            finish_sink(&sink);
            let n = dropped.load(Ordering::Relaxed);
            if n > 0 {
                warn!("system audio: dropped {n} frames on sink contention");
            }
        }
        #[cfg(target_os = "linux")]
        {
            match self.recorder.stop() {
                Ok(stopped) => stopped.sink.finish(),
                Err(e) => warn!("system audio: stop failed: {e}"),
            }
            if let Err(e) = self.recorder.close() {
                warn!("system audio: close failed: {e}");
            }
        }
        info!("system audio capture stopped");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_toolkit::audio::CaptureVad;
    use crate::audio_toolkit::vad::{VadFrame, VoiceActivityDetector};
    use crate::meeting::capture::MeetingSink;
    use crate::meeting::resample::MonoResampler;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{mpsc, Arc, Mutex};

    /// Copied from `capture.rs` tests: speech when mean |x| > 0.05.
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

    fn stereo_48k(seconds: f32, amplitude: f32) -> Vec<f32> {
        let frames = (48_000.0 * seconds) as usize;
        let mut out = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let s = amplitude * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48_000.0).sin();
            out.push(s);
            out.push(s);
        }
        out
    }

    fn pipeline() -> (
        LoopbackPipeline,
        SharedSink,
        Arc<AtomicU64>,
        mpsc::Receiver<crate::meeting::capture::Utterance>,
    ) {
        let vad = CaptureVad::new(Box::new(LoudVad), 0, 0);
        let (tx, rx) = mpsc::channel();
        let sink: SharedSink =
            Arc::new(Mutex::new(Some(MeetingSink::with_speaker(vad, tx, "them"))));
        let dropped = Arc::new(AtomicU64::new(0));
        let p = LoopbackPipeline::new(
            MonoResampler::new(48_000, 16_000, 2).unwrap(),
            320,
            Arc::clone(&sink),
            Arc::clone(&dropped),
        );
        (p, sink, dropped, rx)
    }

    #[test]
    fn stereo_48k_tone_burst_then_silence_yields_one_them_utterance() {
        let (mut p, _sink, dropped, rx) = pipeline();
        let mut audio = stereo_48k(1.0, 0.3);
        audio.extend(stereo_48k(1.5, 0.0));
        // 10 ms device callbacks: 480 stereo frames each.
        for block in audio.chunks(960) {
            p.push(block);
        }
        let utt = rx.try_recv().expect("one utterance");
        assert_eq!(utt.speaker, "them");
        // About one second at 16 kHz, give or take resampler edges.
        let secs = utt.samples.len() as f32 / 16_000.0;
        assert!((0.9..=1.1).contains(&secs), "{secs} s");
        assert!(rx.try_recv().is_err(), "exactly one utterance");
        assert_eq!(dropped.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn contended_sink_drops_frames_instead_of_blocking() {
        let (mut p, sink, dropped, rx) = pipeline();
        let guard = sink.lock().unwrap();
        p.push(&stereo_48k(0.5, 0.3));
        assert!(dropped.load(Ordering::Relaxed) > 0);
        drop(guard);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn finish_sink_flushes_trailing_utterance_once() {
        let (mut p, sink, _dropped, rx) = pipeline();
        p.push(&stereo_48k(1.0, 0.3));
        finish_sink(&sink);
        let utt = rx.try_recv().expect("flushed utterance");
        assert_eq!(utt.speaker, "them");
        finish_sink(&sink);
        assert!(rx.try_recv().is_err(), "second finish is a no-op");
        // Frames after finish go nowhere and do not panic.
        p.push(&stereo_48k(0.1, 0.3));
    }

    #[test]
    fn pick_monitor_prefers_default_output_then_first_monitor() {
        let devs = vec![
            ("mic".to_string(), "Built-in Mic".to_string(), false),
            ("m1".to_string(), "Monitor of HDMI".to_string(), true),
            ("m2".to_string(), "Monitor of Speakers".to_string(), true),
        ];
        assert_eq!(pick_monitor(&devs, Some("Speakers")).as_deref(), Some("m2"));
        assert_eq!(pick_monitor(&devs, Some("Headset")).as_deref(), Some("m1"));
        assert_eq!(pick_monitor(&devs, None).as_deref(), Some("m1"));
        assert_eq!(pick_monitor(&devs[..1], Some("Speakers")), None);
    }
}
