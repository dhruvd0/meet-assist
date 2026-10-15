//! Mono downmix + resampling and fixed-size framing for system audio capture.

use anyhow::Result;
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler, WindowFunction};

// Only the macOS/Windows loopback path resamples; Linux reads a 16 kHz
// PulseAudio monitor through handy-recorder.
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
const CHUNK: usize = 1024;

/// Converts interleaved multi-channel audio to mono at a target sample rate.
pub struct MonoResampler {
    channels: usize,
    inner: Option<Fft<f32>>,
    buf: Vec<f32>,
}

#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
impl MonoResampler {
    pub fn new(in_rate: u32, out_rate: u32, in_channels: u16) -> Result<Self> {
        let inner = if in_rate == out_rate {
            None
        } else {
            Some(Fft::<f32>::new_custom(
                in_rate as usize,
                out_rate as usize,
                CHUNK,
                2,
                1,
                WindowFunction::BlackmanHarris2,
                FixedSync::Input,
            )?)
        };
        Ok(Self {
            channels: in_channels.max(1) as usize,
            inner,
            buf: Vec::new(),
        })
    }

    fn mix(&self, interleaved: &[f32]) -> Vec<f32> {
        interleaved
            .chunks_exact(self.channels)
            .map(|f| f.iter().sum::<f32>() / self.channels as f32)
            .collect()
    }

    /// Feed interleaved samples; returns whatever mono output is ready.
    pub fn push(&mut self, interleaved: &[f32]) -> Vec<f32> {
        let mono = self.mix(interleaved);
        let Some(rs) = self.inner.as_mut() else {
            return mono;
        };
        self.buf.extend_from_slice(&mono);
        let mut out = Vec::new();
        loop {
            let need = rs.input_frames_next();
            if self.buf.len() < need {
                break;
            }
            let block: Vec<f32> = self.buf.drain(..need).collect();
            match Self::run(rs, &block) {
                Some(mut o) => out.append(&mut o),
                None => break,
            }
        }
        out
    }

    /// Pad any buffered remainder with zeros to one chunk and process it.
    pub fn flush(&mut self) -> Vec<f32> {
        let Some(rs) = self.inner.as_mut() else {
            return Vec::new();
        };
        if self.buf.is_empty() {
            return Vec::new();
        }
        let need = rs.input_frames_next();
        let mut block = std::mem::take(&mut self.buf);
        block.resize(need, 0.0);
        Self::run(rs, &block).unwrap_or_default()
    }

    fn run(rs: &mut Fft<f32>, block: &[f32]) -> Option<Vec<f32>> {
        let input = InterleavedSlice::new(block, 1, block.len()).ok()?;
        rs.process(&input, None).ok().map(|o| o.take_data())
    }
}

/// Slices a sample stream into fixed-length frames, keeping the remainder.
pub struct FrameSlicer {
    frame_len: usize,
    buf: Vec<f32>,
}

impl FrameSlicer {
    pub fn new(frame_len: usize) -> Self {
        Self {
            frame_len: frame_len.max(1),
            buf: Vec::new(),
        }
    }

    pub fn push(&mut self, samples: &[f32]) -> Vec<Vec<f32>> {
        self.buf.extend_from_slice(samples);
        let mut frames = Vec::new();
        while self.buf.len() >= self.frame_len {
            frames.push(self.buf.drain(..self.frame_len).collect());
        }
        frames
    }
}
