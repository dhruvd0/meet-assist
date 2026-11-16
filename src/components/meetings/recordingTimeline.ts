import type { MeetingDetail } from "@/bindings";

/** One session's recording, placed on the meeting's timeline. */
export interface Track {
  sessionId: number;
  offsetMs: number;
  durationMs: number;
}

/** A line that can be played, in timeline time. */
export interface TimedLine {
  lineId: number;
  startMs: number;
  endMs: number;
}

/** A nudge, pinned to the line the meeting had reached when it appeared. */
export interface NudgeMarker {
  id: number;
  kind: string;
  text: string;
  lineId: number;
  atMs: number;
}

export interface Timeline {
  tracks: Track[];
  lines: TimedLine[];
  markers: NudgeMarker[];
  durationMs: number;
}

/**
 * The meeting's recordings as one timeline: sessions back to back, speech
 * only. The silence between lines was never recorded, so a nudge sits at the
 * end of the last line spoken before it.
 */
export const buildTimeline = (detail: MeetingDetail | null): Timeline => {
  const empty = { tracks: [], lines: [], markers: [], durationMs: 0 };
  if (!detail || detail.audio.length === 0) return empty;

  const span = new Map(detail.audio.map((a) => [a.line_id, a]));
  const tracks: Track[] = [];
  const lines: TimedLine[] = [];
  const bySession = new Map<number, { ts: number; line: TimedLine }[]>();
  let offsetMs = 0;

  for (const s of detail.sessions) {
    const timed: { ts: number; line: TimedLine }[] = [];
    let durationMs = 0;
    for (const l of detail.lines) {
      const a = l.session_id === s.id ? span.get(l.id) : undefined;
      if (!a) continue;
      timed.push({
        ts: l.ts,
        line: {
          lineId: l.id,
          startMs: offsetMs + a.start_ms,
          endMs: offsetMs + a.start_ms + a.ms,
        },
      });
      durationMs = Math.max(durationMs, a.start_ms + a.ms);
    }
    if (timed.length === 0) continue;
    timed.sort((x, y) => x.line.startMs - y.line.startMs);
    tracks.push({ sessionId: s.id, offsetMs, durationMs });
    lines.push(...timed.map((x) => x.line));
    bySession.set(s.id, timed);
    offsetMs += durationMs;
  }

  const markers: NudgeMarker[] = [];
  for (const iv of detail.interventions) {
    const timed = bySession.get(iv.session_id);
    if (!timed) continue;
    const before = timed.filter((x) => x.ts <= iv.ts);
    const at = before.length > 0 ? before[before.length - 1] : timed[0];
    markers.push({
      id: iv.id,
      kind: iv.kind,
      text: iv.text,
      lineId: at.line.lineId,
      atMs: before.length > 0 ? at.line.endMs : at.line.startMs,
    });
  }

  return { tracks, lines, markers, durationMs: offsetMs };
};

export const trackAt = (timeline: Timeline, ms: number): number => {
  const i = timeline.tracks.findIndex((t) => ms < t.offsetMs + t.durationMs);
  return i === -1 ? timeline.tracks.length - 1 : i;
};

export const lineAt = (timeline: Timeline, ms: number): number | null => {
  let found: number | null = null;
  for (const l of timeline.lines) {
    if (l.startMs > ms) break;
    found = l.lineId;
  }
  return found;
};
