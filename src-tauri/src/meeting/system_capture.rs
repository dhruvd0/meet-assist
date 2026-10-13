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
use std::sync::{mpsc, Arc};

/// Model input rate.
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
const TARGET_RATE: u32 = 16_000;

/// Frames buffered between the real-time callback and the VAD worker.
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
const FRAME_QUEUE: usize = 64;

/// Real-time half of loopback capture: downmix + resample, slice into VAD
/// frames, and `try_send` each frame to the worker. Never blocks, never
/// logs, never runs inference. Owned by the device callback; dropping it
/// flushes the resampler tail and closes the channel.
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
pub(crate) struct LoopbackPipeline {
    resampler: MonoResampler,
    slicer: FrameSlicer,
    tx: mpsc::SyncSender<Vec<f32>>,
    dropped: Arc<AtomicU64>,
}

#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
impl LoopbackPipeline {
    pub(crate) fn new(
        resampler: MonoResampler,
        frame_len: usize,
        tx: mpsc::SyncSender<Vec<f32>>,
        dropped: Arc<AtomicU64>,
    ) -> Self {
        Self {
            resampler,
            slicer: FrameSlicer::new(frame_len),
            tx,
            dropped,
        }
    }

    /// Feed one device buffer of interleaved samples.
    pub(crate) fn push(&mut self, interleaved: &[f32]) {
        let mono = self.resampler.push(interleaved);
        self.send_frames(&mono);
    }

    fn send_frames(&mut self, mono: &[f32]) {
        for frame in self.slicer.push(mono) {
            if self.tx.try_send(frame).is_err() {
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

impl Drop for LoopbackPipeline {
    /// Runs when the stream (and its callback) is torn down: push the
    /// resampler tail through the slicer. The sender drops right after,
    /// which tells the worker to finish.
    fn drop(&mut self) {
        let tail = self.resampler.flush();
        self.send_frames(&tail);
    }
}

/// Spawn the `meet-sysaudio` worker: owns the sink, runs VAD on each frame,
/// and flushes the trailing utterance when the channel closes.
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
pub(crate) fn spawn_worker(
    mut sink: MeetingSink,
    rx: mpsc::Receiver<Vec<f32>>,
    stopping: Arc<AtomicBool>,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name("meet-sysaudio".into())
        .spawn(move || {
            loop {
                match rx.recv_timeout(std::time::Duration::from_millis(500)) {
                    Ok(frame) => sink.push_frame(&frame),
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    // Guard against a leaked callback closure keeping the
                    // sender alive after `stop`: bail once asked to stop.
                    Err(mpsc::RecvTimeoutError::Timeout) if stopping.load(Ordering::Acquire) => {
                        break
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
            sink.finish();
        })
}

/// macOS 14.2 introduced process taps, which handy-cpal's loopback needs.
#[cfg(target_os = "macos")]
fn check_macos_version() -> Result<()> {
    use objc2_foundation::NSProcessInfo;
    let v = NSProcessInfo::processInfo().operatingSystemVersion();
    if (v.majorVersion, v.minorVersion) < (14, 2) {
        anyhow::bail!("system audio capture needs macOS 14.2 or newer");
    }
    Ok(())
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
    worker: std::thread::JoinHandle<()>,
    stopping: Arc<AtomicBool>,
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

        #[cfg(target_os = "macos")]
        check_macos_version()?;
        let host = handy_cpal::default_host();
        let device = host
            .default_output_device()
            .context("no default output device")?;
        // Output config: loopback streams take the output's format. Never
        // ask an output device for its input config (errors on WASAPI).
        // handy-cpal only uses loopback on macOS when the device has no
        // input; otherwise it would capture the microphone as "them".
        #[cfg(target_os = "macos")]
        if device
            .supported_input_configs()
            .map(|mut c| c.next().is_some())
            .unwrap_or(false)
        {
            anyhow::bail!(
                "system audio needs an output-only device; current output also has an input"
            );
        }
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
        let sink = MeetingSink::with_speaker(vad, tx, "them");
        let (frame_tx, frame_rx) = mpsc::sync_channel(FRAME_QUEUE);
        let dropped = Arc::new(AtomicU64::new(0));
        let failed = Arc::new(AtomicBool::new(false));
        let mut pipeline = LoopbackPipeline::new(resampler, frame, frame_tx, Arc::clone(&dropped));
        let stopping = Arc::new(AtomicBool::new(false));
        let worker = spawn_worker(sink, frame_rx, Arc::clone(&stopping))
            .context("spawn system audio worker")?;
        let flag = Arc::clone(&failed);
        let stream = device
            .build_input_stream(
                cfg.config(),
                move |data: &[f32], _: &handy_cpal::InputCallbackInfo| pipeline.push(data),
                move |e: handy_cpal::Error| {
                    // A glitch is not a dead stream.
                    if e.kind() == handy_cpal::ErrorKind::Xrun {
                        return;
                    }
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
            worker,
            stopping,
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
                worker,
                dropped,
                stopping,
                ..
            } = self;
            stopping.store(true, Ordering::Release);
            // Dropping the stream tears down the tap and the callback; the
            // pipeline's Drop flushes the resampler tail and closes the
            // channel, so the worker drains, finishes the sink, and exits.
            drop(stream);
            if worker.join().is_err() {
                warn!("system audio: worker panicked");
            }
            let n = dropped.load(Ordering::Relaxed);
            if n > 0 {
                warn!("system audio: dropped {n} frames (worker queue full)");
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
    use std::sync::{mpsc, Arc};

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

    type Worker = std::thread::JoinHandle<()>;

    fn pipeline() -> (
        LoopbackPipeline,
        Worker,
        Arc<AtomicU64>,
        mpsc::Receiver<crate::meeting::capture::Utterance>,
    ) {
        let vad = CaptureVad::new(Box::new(LoudVad), 0, 0);
        let (tx, rx) = mpsc::channel();
        let sink = MeetingSink::with_speaker(vad, tx, "them");
        let (ftx, frx) = mpsc::sync_channel(FRAME_QUEUE);
        let worker = spawn_worker(sink, frx, Arc::new(AtomicBool::new(false))).unwrap();
        let dropped = Arc::new(AtomicU64::new(0));
        let p = LoopbackPipeline::new(
            MonoResampler::new(48_000, 16_000, 2).unwrap(),
            320,
            ftx,
            Arc::clone(&dropped),
        );
        (p, worker, dropped, rx)
    }

    #[test]
    fn stereo_48k_tone_burst_then_silence_yields_one_them_utterance() {
        let (mut p, worker, dropped, rx) = pipeline();
        let mut audio = stereo_48k(1.0, 0.3);
        audio.extend(stereo_48k(1.5, 0.0));
        // 10 ms device callbacks: 480 stereo frames each.
        for block in audio.chunks(960) {
            p.push(block);
        }
        drop(p);
        worker.join().unwrap();
        let utt = rx.try_recv().expect("one utterance");
        assert_eq!(utt.speaker, "them");
        // About one second at 16 kHz, give or take resampler edges.
        let secs = utt.samples.len() as f32 / 16_000.0;
        assert!((0.9..=1.1).contains(&secs), "{secs} s");
        assert!(rx.try_recv().is_err(), "exactly one utterance");
        assert_eq!(dropped.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn full_channel_counts_drops_instead_of_blocking() {
        let (ftx, _frx) = mpsc::sync_channel(2);
        let dropped = Arc::new(AtomicU64::new(0));
        let mut p = LoopbackPipeline::new(
            MonoResampler::new(48_000, 16_000, 2).unwrap(),
            320,
            ftx,
            Arc::clone(&dropped),
        );
        // 0.5 s is ~25 frames into a 2-slot channel nobody drains.
        p.push(&stereo_48k(0.5, 0.3));
        assert!(dropped.load(Ordering::Relaxed) > 10);
    }

    #[test]
    fn closing_the_channel_flushes_trailing_utterance() {
        let (mut p, worker, _dropped, rx) = pipeline();
        p.push(&stereo_48k(1.0, 0.3));
        drop(p);
        worker.join().unwrap();
        let utt = rx.try_recv().expect("flushed utterance");
        assert_eq!(utt.speaker, "them");
        assert!(rx.try_recv().is_err());
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
