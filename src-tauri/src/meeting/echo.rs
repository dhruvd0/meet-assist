//! Acoustic echo check: without headphones the microphone also hears the
//! speakers, so the other side reappears as garbled `me` lines that a text
//! comparison misses. The system-audio stream records what the speakers
//! played; a microphone utterance whose loudness follows that recording is
//! the speakers, not the user.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

const SAMPLE_RATE: i64 = 16_000;
/// How much speaker output to keep; longer than any utterance plus slack.
const KEEP_MS: i64 = 75_000;
/// Loudness is compared in 20 ms frames.
const FRAME: usize = 320;
/// Utterances are matched in 2 s blocks, each searched across the window, so
/// pauses the segmenter dropped from the utterance do not break alignment.
const BLOCK_FRAMES: usize = 100;
/// Shortest block worth comparing (0.5 s).
const MIN_BLOCK_FRAMES: usize = 25;
/// Correlation at which a block counts as the speakers. Measured on saved
/// meetings: echoed blocks 0.5-0.9, unrelated audio mostly below 0.2.
const BLOCK_MATCH: f32 = 0.5;
/// Mean loudness below which the speakers count as silent for a block.
const SILENT_RMS: f32 = 1e-3;
/// Slack before the utterance start, for clock skew between the streams.
pub const LEAD_MS: i64 = 1_000;

struct Buffer {
    samples: VecDeque<f32>,
    /// Wall-clock ms of the newest sample.
    end_ms: i64,
}

/// Recent speaker output (16 kHz mono), shared by the system-audio stream,
/// which appends, and the microphone stream, which checks against it.
#[derive(Clone)]
pub struct EchoReference(Arc<Mutex<Buffer>>);

impl Default for EchoReference {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(Buffer {
            samples: VecDeque::new(),
            end_ms: 0,
        })))
    }
}

impl EchoReference {
    fn lock(&self) -> std::sync::MutexGuard<'_, Buffer> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Append one frame of speaker output captured now.
    pub fn push(&self, frame: &[f32], now_ms: i64) {
        let mut b = self.lock();
        b.samples.extend(frame.iter().copied());
        let keep = (KEEP_MS * SAMPLE_RATE / 1000) as usize;
        let excess = b.samples.len().saturating_sub(keep);
        b.samples.drain(..excess);
        b.end_ms = now_ms;
    }

    /// Speaker output from `start_ms` to the newest sample.
    pub fn since(&self, start_ms: i64) -> Vec<f32> {
        let b = self.lock();
        let back = ((b.end_ms - start_ms).max(0) * SAMPLE_RATE / 1000) as usize;
        let from = b.samples.len().saturating_sub(back);
        b.samples.range(from..).copied().collect()
    }
}

fn envelope(samples: &[f32]) -> Vec<f32> {
    samples
        .as_chunks::<FRAME>()
        .0
        .iter()
        .map(|f| (f.iter().map(|x| x * x).sum::<f32>() / FRAME as f32).sqrt())
        .collect()
}

fn correlation(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len() as f32;
    let (ma, mb) = (a.iter().sum::<f32>() / n, b.iter().sum::<f32>() / n);
    let (mut cov, mut va, mut vb) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        cov += (x - ma) * (y - mb);
        va += (x - ma) * (x - ma);
        vb += (y - mb) * (y - mb);
    }
    if va <= 1e-12 || vb <= 1e-12 {
        return 0.0;
    }
    cov / (va * vb).sqrt()
}

/// Whether `mic` (an utterance) is the speakers playing `reference`: at
/// least half its 2 s blocks follow some stretch of audible speaker output.
pub fn is_echo(mic: &[f32], reference: &[f32]) -> bool {
    let mic = envelope(mic);
    let reference = envelope(reference);
    let blocks: Vec<&[f32]> = mic
        .chunks(BLOCK_FRAMES)
        .filter(|b| b.len() >= MIN_BLOCK_FRAMES)
        .collect();
    if blocks.is_empty() {
        return false;
    }
    let matched = blocks
        .iter()
        .filter(|block| {
            reference.windows(block.len()).step_by(2).any(|w| {
                w.iter().sum::<f32>() / w.len() as f32 >= SILENT_RMS
                    && correlation(block, w) >= BLOCK_MATCH
            })
        })
        .count();
    matched * 2 >= blocks.len()
}
