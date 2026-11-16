//! Session recordings. While a session runs, each transcript line keeps its
//! utterance as a WAV. Once the session ends the lines are joined into one
//! Ogg Opus file (`meeting-audio/<meeting>/session-<id>.ogg`) and each line
//! remembers where it sits in that file; the WAVs are then deleted. The
//! timeline holds speech only: the silence between lines was never saved.

use super::store::{LineAudio, ProjectStore};
use anyhow::{anyhow, bail, Context, Result};
use log::{info, warn};
use ogg::writing::{PacketWriteEndInfo, PacketWriter};
use opus::{Application, Bitrate, Channels, Decoder, Encoder};
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Capture rate of every saved utterance.
const SAMPLE_RATE: u32 = 16_000;
/// Opus counts positions at 48 kHz whatever the input rate.
const GRANULE_PER_SAMPLE: u64 = 48_000 / SAMPLE_RATE as u64;
/// 20 ms, the frame size Opus is tuned for.
const FRAME: usize = SAMPLE_RATE as usize / 50;
/// Speech stays clear at this rate and a one-hour meeting is about 11 MB.
const BITRATE: i32 = 24_000;
/// One Ogg page per second, so a seek never reads far.
const FRAMES_PER_PAGE: u64 = 50;
const SERIAL: u32 = 0x4341_5345;
const MAX_PACKET: usize = 4000;

/// One line's utterance, waiting to be joined into the session file.
pub struct PendingLine {
    pub line_id: i64,
    pub wav: PathBuf,
}

fn samples_to_ms(samples: u64) -> i64 {
    (samples * 1000 / SAMPLE_RATE as u64) as i64
}

fn read_wav(path: &Path) -> Result<Vec<i16>> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    if spec.channels != 1 || spec.sample_rate != SAMPLE_RATE || spec.bits_per_sample != 16 {
        bail!("unexpected wav format {spec:?}");
    }
    Ok(reader.samples::<i16>().collect::<Result<Vec<_>, _>>()?)
}

fn opus_head(pre_skip: u16) -> Vec<u8> {
    let mut head = Vec::with_capacity(19);
    head.extend_from_slice(b"OpusHead");
    head.push(1); // version
    head.push(1); // channels
    head.extend_from_slice(&pre_skip.to_le_bytes());
    head.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    head.extend_from_slice(&0i16.to_le_bytes()); // output gain
    head.push(0); // channel mapping family
    head
}

fn opus_tags() -> Vec<u8> {
    let vendor = b"CASE";
    let mut tags = Vec::with_capacity(20);
    tags.extend_from_slice(b"OpusTags");
    tags.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    tags.extend_from_slice(vendor);
    tags.extend_from_slice(&0u32.to_le_bytes());
    tags
}

/// Join the lines' WAVs into one Ogg Opus file at `out`. Returns where each
/// line sits and the total length in samples. A line whose WAV cannot be read
/// is left out.
fn encode(lines: &[PendingLine], out: &Path) -> Result<(Vec<LineAudio>, u64)> {
    let mut encoder = Encoder::new(SAMPLE_RATE, Channels::Mono, Application::Voip)
        .map_err(|e| anyhow!("opus encoder: {e}"))?;
    encoder
        .set_bitrate(Bitrate::Bits(BITRATE))
        .map_err(|e| anyhow!("opus bitrate: {e}"))?;
    let lookahead = encoder
        .get_lookahead()
        .map_err(|e| anyhow!("opus lookahead: {e}"))? as u64;
    let pre_skip = lookahead * GRANULE_PER_SAMPLE;

    let mut writer = PacketWriter::new(BufWriter::new(File::create(out)?));
    writer.write_packet(
        opus_head(pre_skip as u16),
        SERIAL,
        PacketWriteEndInfo::EndPage,
        0,
    )?;
    writer.write_packet(opus_tags(), SERIAL, PacketWriteEndInfo::EndPage, 0)?;

    let mut spans = Vec::with_capacity(lines.len());
    let mut total: u64 = 0;
    let mut pending: Vec<i16> = Vec::with_capacity(FRAME * 2);
    let mut encoded: u64 = 0;
    let mut frames: u64 = 0;
    let mut packet = vec![0u8; MAX_PACKET];
    // One frame is held back so the last one can close the stream.
    let mut held: Option<Vec<u8>> = None;

    let mut push_frame = |frame: &[i16],
                          writer: &mut PacketWriter<'_, BufWriter<File>>,
                          encoded: &mut u64,
                          frames: &mut u64,
                          held: &mut Option<Vec<u8>>|
     -> Result<()> {
        let n = encoder
            .encode(frame, &mut packet)
            .map_err(|e| anyhow!("opus encode: {e}"))?;
        if let Some(prev) = held.replace(packet[..n].to_vec()) {
            let end = if *frames % FRAMES_PER_PAGE == 0 {
                PacketWriteEndInfo::EndPage
            } else {
                PacketWriteEndInfo::NormalPacket
            };
            writer.write_packet(prev, SERIAL, end, *encoded * GRANULE_PER_SAMPLE)?;
        }
        *encoded += FRAME as u64;
        *frames += 1;
        Ok(())
    };

    for line in lines {
        let samples = match read_wav(&line.wav) {
            Ok(s) if !s.is_empty() => s,
            Ok(_) => continue,
            Err(e) => {
                warn!("skipping line audio {}: {e:#}", line.wav.display());
                continue;
            }
        };
        spans.push(LineAudio {
            line_id: line.line_id,
            start_ms: samples_to_ms(total),
            ms: samples_to_ms(samples.len() as u64).max(1),
        });
        total += samples.len() as u64;
        pending.extend_from_slice(&samples);
        let mut at = 0;
        while pending.len() - at >= FRAME {
            push_frame(
                &pending[at..at + FRAME],
                &mut writer,
                &mut encoded,
                &mut frames,
                &mut held,
            )?;
            at += FRAME;
        }
        pending.drain(..at);
    }
    if total == 0 {
        bail!("no readable audio");
    }
    // Pad the tail, plus the encoder's lookahead, with silence.
    pending.resize(pending.len() + lookahead as usize, 0);
    let padded = pending.len().div_ceil(FRAME) * FRAME;
    pending.resize(padded, 0);
    for frame in pending.chunks(FRAME) {
        push_frame(frame, &mut writer, &mut encoded, &mut frames, &mut held)?;
    }
    drop(push_frame);
    // The final position trims the padding on playback.
    let last = held.ok_or_else(|| anyhow!("no audio frames"))?;
    writer.write_packet(
        last,
        SERIAL,
        PacketWriteEndInfo::EndStream,
        pre_skip + total * GRANULE_PER_SAMPLE,
    )?;
    writer.into_inner().flush()?;
    Ok((spans, total))
}

/// Decode a session file back to samples, trimmed to what was recorded.
fn decode(ogg_path: &Path) -> Result<Vec<i16>> {
    let mut reader = ogg::reading::PacketReader::new(BufReader::new(File::open(ogg_path)?));
    let head = reader.read_packet_expected()?;
    if head.data.len() < 12 || &head.data[..8] != b"OpusHead" {
        bail!("not an Opus file");
    }
    let pre_skip = u16::from_le_bytes([head.data[10], head.data[11]]) as u64;
    reader.read_packet_expected()?; // tags
    let mut decoder =
        Decoder::new(SAMPLE_RATE, Channels::Mono).map_err(|e| anyhow!("opus decoder: {e}"))?;
    let mut samples: Vec<i16> = Vec::new();
    let mut frame = vec![0i16; FRAME * 6];
    let mut last_granule = 0;
    while let Some(packet) = reader.read_packet()? {
        let n = decoder
            .decode(&packet.data, &mut frame, false)
            .map_err(|e| anyhow!("opus decode: {e}"))?;
        samples.extend_from_slice(&frame[..n]);
        last_granule = packet.absgp_page();
    }
    let skip = (pre_skip / GRANULE_PER_SAMPLE) as usize;
    let length = (last_granule.saturating_sub(pre_skip) / GRANULE_PER_SAMPLE) as usize;
    if samples.len() < skip + length {
        bail!(
            "file is short: {} samples decoded, {} expected",
            samples.len(),
            skip + length
        );
    }
    Ok(samples[skip..skip + length].to_vec())
}

/// Write a session file as WAV, for a webview that cannot play Opus.
pub fn decode_to_wav(ogg_path: &Path, wav_path: &Path) -> Result<()> {
    let samples = decode(ogg_path)?;
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let tmp = wav_path.with_extension("wav.tmp");
    let mut writer = hound::WavWriter::create(&tmp, spec)?;
    for s in samples {
        writer.write_sample(s)?;
    }
    writer.finalize()?;
    std::fs::rename(&tmp, wav_path)?;
    Ok(())
}

/// Join one ended session's line WAVs into its session file. The WAVs are
/// deleted only after the new file decodes back to the full length and the
/// line positions are stored. Returns false when there was nothing to do.
pub fn compact_session(store: &ProjectStore, session_id: i64) -> Result<bool> {
    let lines = store.pending_line_audio(session_id)?;
    if lines.is_empty() {
        return Ok(false);
    }
    let out = store.session_audio_path(session_id)?;
    let joined = store.session_audio_lines(session_id)?;
    if !joined.is_empty() {
        // Already joined; a crash left these WAVs behind. A line that is not
        // in the recording keeps its WAV.
        if out.is_file() {
            let stale: Vec<PendingLine> = lines
                .into_iter()
                .filter(|l| joined.contains(&l.line_id))
                .collect();
            remove_wavs(&stale);
        }
        return Ok(false);
    }
    let tmp = out.with_extension("ogg.tmp");
    let (spans, total) = encode(&lines, &tmp).context("encode session audio")?;
    let decoded = decode(&tmp).context("verify session audio")?;
    if decoded.len() as u64 != total {
        let _ = std::fs::remove_file(&tmp);
        bail!(
            "session {session_id} verification failed: {} of {total} samples",
            decoded.len()
        );
    }
    std::fs::rename(&tmp, &out)?;
    store.set_session_audio(session_id, &spans)?;
    let joined: Vec<PendingLine> = lines
        .into_iter()
        .filter(|l| spans.iter().any(|s| s.line_id == l.line_id))
        .collect();
    remove_wavs(&joined);
    info!(
        "session {session_id}: joined {} lines into {} ({} s)",
        spans.len(),
        out.display(),
        total / SAMPLE_RATE as u64
    );
    Ok(true)
}

fn remove_wavs(lines: &[PendingLine]) {
    for l in lines {
        if let Err(e) = std::fs::remove_file(&l.wav) {
            warn!("remove {}: {e}", l.wav.display());
        }
    }
}

/// Join every ended session that still has line WAVs, newest first. Runs on
/// a background thread at startup and after a session ends; one run at a time.
pub fn compact_pending(store: &ProjectStore) {
    static RUNNING: OnceLock<Mutex<()>> = OnceLock::new();
    let _one = RUNNING
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let sessions = match store.sessions_with_pending_audio() {
        Ok(s) => s,
        Err(e) => {
            warn!("list sessions to join: {e:#}");
            return;
        }
    };
    for session_id in sessions {
        if let Err(e) = compact_session(store, session_id) {
            warn!("join session {session_id} audio: {e:#}");
        }
    }
}

pub fn spawn_compact_pending(store: std::sync::Arc<ProjectStore>) {
    std::thread::Builder::new()
        .name("session-audio".into())
        .spawn(move || compact_pending(&store))
        .map(|_| ())
        .unwrap_or_else(|e| warn!("spawn session audio thread: {e}"));
}
