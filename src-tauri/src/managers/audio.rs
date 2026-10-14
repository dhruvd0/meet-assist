use crate::audio_toolkit::{
    vad::{
        frames_for_duration_ms, EarshotVad, SmoothedVad, VAD_OFFLINE_HANGOVER_MS, VAD_ONSET_MS,
        VAD_PREFILL_MS, VAD_STREAMING_HANGOVER_MS,
    },
    CaptureVad, SileroVad, VoiceActivityDetector,
};
use crate::settings::VadBackend;
use handy_recorder::{Channels, RecorderConfig};
use log::info;
use tauri::Manager;

const SILERO_VAD_THRESHOLD: f32 = 0.3;
const EARSHOT_VAD_THRESHOLD: f32 = 0.5;

pub(crate) fn create_capture_vad(
    backend: VadBackend,
    app_handle: &tauri::AppHandle,
) -> Result<CaptureVad, anyhow::Error> {
    let detector: Box<dyn VoiceActivityDetector> = match backend {
        VadBackend::Silero => {
            let vad_path = app_handle
                .path()
                .resolve(
                    "resources/models/silero-vad-v6.2-F32.gguf",
                    tauri::path::BaseDirectory::Resource,
                )
                .map_err(|e| anyhow::anyhow!("Failed to resolve VAD path: {e}"))?;
            Box::new(
                SileroVad::new(vad_path, SILERO_VAD_THRESHOLD)
                    .map_err(|e| anyhow::anyhow!("Failed to create SileroVad: {e}"))?,
            )
        }
        VadBackend::Earshot => Box::new(
            EarshotVad::new(EARSHOT_VAD_THRESHOLD)
                .map_err(|e| anyhow::anyhow!("Failed to create EarshotVad: {e}"))?,
        ),
    };

    // Earshot uses 16 ms frames while Silero uses 32 ms. Convert the existing
    // time-based capture profile to each detector's frame size so selecting a
    // backend does not shorten pre-roll, onset, or post-speech audio.
    let frame_samples = detector.frame_samples();
    let prefill_frames = frames_for_duration_ms(VAD_PREFILL_MS, frame_samples);
    let offline_hangover_frames = frames_for_duration_ms(VAD_OFFLINE_HANGOVER_MS, frame_samples);
    let streaming_hangover_frames =
        frames_for_duration_ms(VAD_STREAMING_HANGOVER_MS, frame_samples);
    let onset_frames = frames_for_duration_ms(VAD_ONSET_MS, frame_samples);
    let smoothed_vad = SmoothedVad::new(
        detector,
        prefill_frames,
        offline_hangover_frames,
        onset_frames,
    );

    info!(
        "Initialized {:?} VAD backend ({} samples/frame)",
        backend, frame_samples
    );

    Ok(CaptureVad::new(
        Box::new(smoothed_vad),
        offline_hangover_frames,
        streaming_hangover_frames,
    ))
}

/// 16 kHz mono in VAD-frame chunks, from `device` (`None`: the system
/// default). `channel` picks one zero-based input channel; `None` averages
/// them all.
pub(crate) fn microphone_config(
    device: Option<String>,
    channel: Option<u16>,
    frames_per_chunk: usize,
) -> RecorderConfig {
    RecorderConfig {
        device,
        channels: channel.map_or(Channels::MixToMono, Channels::Only),
        frames_per_chunk: Some(frames_per_chunk),
        ..RecorderConfig::speech()
    }
}
