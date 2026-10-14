//! Turns the VAD's per-frame speech/noise stream into utterances. Pure and
//! unit-tested; knows nothing about microphones or models.

pub struct SegmenterConfig {
    pub sample_rate: usize,
    pub frame_samples: usize,
    pub end_silence_ms: u64,
    pub min_utterance_ms: u64,
    pub max_utterance_ms: u64,
}

impl SegmenterConfig {
    pub fn defaults(frame_samples: usize) -> Self {
        Self {
            sample_rate: 16_000,
            frame_samples,
            end_silence_ms: 700,
            min_utterance_ms: 300,
            max_utterance_ms: 30_000,
        }
    }

    fn ms_to_samples(&self, ms: u64) -> usize {
        (self.sample_rate as u64 * ms / 1000) as usize
    }

    fn ms_to_frames(&self, ms: u64) -> usize {
        let frame_ms = (self.frame_samples as u64 * 1000) / self.sample_rate as u64;
        (ms / frame_ms.max(1)) as usize
    }
}

pub struct Segmenter {
    end_silence_frames: usize,
    min_samples: usize,
    max_samples: usize,
    buffer: Vec<f32>,
    silence_run: usize,
}

impl Segmenter {
    pub fn new(cfg: SegmenterConfig) -> Self {
        Self {
            end_silence_frames: cfg.ms_to_frames(cfg.end_silence_ms).max(1),
            min_samples: cfg.ms_to_samples(cfg.min_utterance_ms),
            max_samples: cfg
                .ms_to_samples(cfg.max_utterance_ms)
                .max(cfg.frame_samples),
            buffer: Vec::new(),
            silence_run: 0,
        }
    }

    /// Feed one VAD frame. `Some(samples)` = speech passed by the VAD (may be
    /// longer than one frame when the VAD prepends prefill); `None` = noise.
    pub fn push(&mut self, speech: Option<&[f32]>) -> Option<Vec<f32>> {
        match speech {
            Some(samples) => {
                self.silence_run = 0;
                self.buffer.extend_from_slice(samples);
                if self.buffer.len() >= self.max_samples {
                    return self.take();
                }
                None
            }
            None => {
                if self.buffer.is_empty() {
                    return None;
                }
                self.silence_run += 1;
                if self.silence_run >= self.end_silence_frames {
                    return self.take();
                }
                None
            }
        }
    }

    /// True when no utterance is in progress.
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    pub fn flush(&mut self) -> Option<Vec<f32>> {
        self.take()
    }

    fn take(&mut self) -> Option<Vec<f32>> {
        self.silence_run = 0;
        let out = std::mem::take(&mut self.buffer);
        if out.len() >= self.min_samples {
            Some(out)
        } else {
            None
        }
    }
}
