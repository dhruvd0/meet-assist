import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import type { MeetingDetail } from "@/bindings";
import { commands } from "@/bindings";
import {
  buildTimeline,
  lineAt,
  trackAt,
  type NudgeMarker,
} from "./recordingTimeline";

/** How far before a nudge playback starts, so its lead-up is heard. */
const NUDGE_LEAD_MS = 5000;

/** Older webviews cannot play Ogg Opus; they get a decoded copy instead. */
const needsWav = () =>
  new Audio().canPlayType('audio/ogg; codecs="opus"') === "";

/**
 * Plays a meeting's recordings as one scrubbable timeline. Sessions play back
 * to back; `seek` moves anywhere on it, `playLine` starts at a transcript
 * line, `playNudge` starts a few seconds before a nudge.
 */
export const useMeetingPlayer = (detail: MeetingDetail | null) => {
  const timeline = useMemo(() => buildTimeline(detail), [detail]);
  const timelineRef = useRef(timeline);
  timelineRef.current = timeline;

  const audioRef = useRef<HTMLAudioElement | null>(null);
  const loadedTrack = useRef<number | null>(null);
  const request = useRef(0);
  const [playing, setPlaying] = useState(false);
  const [positionMs, setPositionMs] = useState(0);
  // Nothing is highlighted until the user first plays or seeks.
  const [started, setStarted] = useState(false);
  const [focusLineId, setFocusLineId] = useState<number | null>(null);

  const position = () => {
    const audio = audioRef.current;
    const track =
      loadedTrack.current === null
        ? undefined
        : timelineRef.current.tracks[loadedTrack.current];
    if (!audio || !track) return null;
    return Math.min(
      track.offsetMs + audio.currentTime * 1000,
      track.offsetMs + track.durationMs,
    );
  };

  const seek = useCallback(async (ms: number, play: boolean) => {
    const tl = timelineRef.current;
    if (tl.tracks.length === 0) return;
    const target = Math.max(0, Math.min(ms, tl.durationMs));
    const index = trackAt(tl, target);
    const track = tl.tracks[index];
    const mine = ++request.current;
    setStarted(true);
    setPositionMs(target);

    const audio = audioRef.current ?? new Audio();
    if (!audioRef.current) {
      audioRef.current = audio;
      audio.preload = "auto";
      audio.onplay = () => setPlaying(true);
      audio.onpause = () => setPlaying(false);
      audio.onended = () => {
        const next = (loadedTrack.current ?? 0) + 1;
        const tracks = timelineRef.current.tracks;
        if (next < tracks.length) void seek(tracks[next].offsetMs, true);
        else setPositionMs(timelineRef.current.durationMs);
      };
    }

    if (loadedTrack.current !== index) {
      audio.pause();
      const res = await commands.getSessionAudioPath(
        track.sessionId,
        needsWav(),
      );
      if (mine !== request.current) return;
      if (res.status !== "ok" || !res.data) return;
      loadedTrack.current = index;
      audio.src = convertFileSrc(res.data);
      await new Promise<void>((resolve) => {
        audio.onloadedmetadata = () => resolve();
        audio.onerror = () => resolve();
      });
      if (mine !== request.current) return;
    }
    audio.currentTime = (target - track.offsetMs) / 1000;
    if (play) await audio.play().catch(() => setPlaying(false));
  }, []);

  // Follow the playhead while it moves.
  useEffect(() => {
    if (!playing) return;
    let frame = requestAnimationFrame(function tick() {
      const at = position();
      if (at !== null) setPositionMs(at);
      frame = requestAnimationFrame(tick);
    });
    return () => cancelAnimationFrame(frame);
  }, [playing]);

  // A different meeting starts from nothing.
  const meetingId = detail?.meeting.id;
  useEffect(() => {
    return () => {
      request.current += 1;
      audioRef.current?.pause();
      audioRef.current = null;
      loadedTrack.current = null;
      setPlaying(false);
      setPositionMs(0);
      setStarted(false);
      setFocusLineId(null);
    };
  }, [meetingId]);

  const toggle = useCallback(() => {
    if (audioRef.current && !audioRef.current.paused)
      return audioRef.current.pause();
    const at = positionMs >= timelineRef.current.durationMs ? 0 : positionMs;
    void seek(at, true);
  }, [positionMs, seek]);

  const activeLineId = started ? lineAt(timeline, positionMs) : null;

  const playLine = useCallback(
    (lineId: number) => {
      const line = timelineRef.current.lines.find((l) => l.lineId === lineId);
      if (!line) return;
      if (playing && activeLineId === lineId) return audioRef.current?.pause();
      void seek(line.startMs, true);
    },
    [activeLineId, playing, seek],
  );

  const playNudge = useCallback(
    (marker: NudgeMarker) => {
      setFocusLineId(marker.lineId);
      void seek(marker.atMs - NUDGE_LEAD_MS, true);
    },
    [seek],
  );

  return {
    available: timeline.durationMs > 0,
    durationMs: timeline.durationMs,
    markers: timeline.markers,
    playableLines: useMemo(
      () => new Set(timeline.lines.map((l) => l.lineId)),
      [timeline],
    ),
    playing,
    positionMs,
    activeLineId,
    focusLineId,
    toggle,
    seek,
    playLine,
    playNudge,
  };
};

export type MeetingPlayer = ReturnType<typeof useMeetingPlayer>;
