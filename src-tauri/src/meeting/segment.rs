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

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> SegmenterConfig {
        // 16 kHz, 320-sample frames (20 ms): end after 700 ms = 35 noise frames,
        // min 300 ms = 4800 samples, max 30 s.
        SegmenterConfig::defaults(320)
    }

    fn speech_frame() -> Vec<f32> {
        vec![0.1; 320]
    }

    #[test]
    fn emits_after_end_silence() {
        let mut seg = Segmenter::new(cfg());
        for _ in 0..20 {
            assert!(seg.push(Some(&speech_frame())).is_none());
        }
        for _ in 0..34 {
            assert!(seg.push(None).is_none());
        }
        let out = seg.push(None).expect("utterance after 35 noise frames");
        assert_eq!(out.len(), 20 * 320);
    }

    #[test]
    fn drops_utterances_shorter_than_min() {
        let mut seg = Segmenter::new(cfg());
        for _ in 0..5 {
            seg.push(Some(&speech_frame()));
        }
        for _ in 0..40 {
            if let Some(_) = seg.push(None) {
                panic!("5 frames (100 ms) must be dropped");
            }
        }
    }

    #[test]
    fn hard_cuts_at_max_length() {
        let mut c = cfg();
        c.max_utterance_ms = 1000; // 50 frames
        let mut seg = Segmenter::new(c);
        let mut emitted = None;
        for i in 0..60 {
            if let Some(out) = seg.push(Some(&speech_frame())) {
                emitted = Some((i, out.len()));
                break;
            }
        }
        let (at, len) = emitted.expect("hard cut");
        assert_eq!(at, 49);
        assert_eq!(len, 50 * 320);
    }

    #[test]
    fn flush_emits_buffered_speech() {
        let mut seg = Segmenter::new(cfg());
        for _ in 0..20 {
            seg.push(Some(&speech_frame()));
        }
        assert_eq!(seg.flush().unwrap().len(), 20 * 320);
        assert!(seg.flush().is_none());
    }

    #[test]
    fn noise_before_speech_does_not_count() {
        let mut seg = Segmenter::new(cfg());
        for _ in 0..100 {
            assert!(seg.push(None).is_none());
        }
        for _ in 0..20 {
            seg.push(Some(&speech_frame()));
        }
        for _ in 0..34 {
            seg.push(None);
        }
        assert!(seg.push(None).is_some());
    }
}
