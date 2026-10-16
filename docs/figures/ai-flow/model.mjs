// Pure model of one meeting as the assistant sees it. Every number comes from
// src-tauri/src/meeting/{observer,filing,summary}.rs and settings.rs defaults.
export const M = {
  TICK: 20, // meeting_observer_interval_secs default
  WINDOW: 180, // observer::WINDOW_MS
  COOLDOWN: 180, // observer::COOLDOWN_MS
  SAME_KIND: 600, // observer::SAME_KIND_COOLDOWN_MS
  FILING_MIN: 20, // filing::FILING_MIN_LINES
  FILING_RETRY: 10, // filing::FILING_RETRY_LINES
  FILING_MAX: 3, // filing::FILING_MAX_ATTEMPTS
  END: 1080, // this meeting ends at 18:00
  HOLD: 110, // seconds of summarising shown after the end
  V: 120 / 180, // tape units per second: the 3-minute window is 120 units long
};

// Deterministic transcript: ~165 utterances, speakers alternating in runs.
export const LINES = (() => {
  let seed = 11;
  const rnd = () => (seed = (seed * 16807) % 2147483647) / 2147483647;
  const out = [];
  let t = 3;
  let who = "them";
  while (t < M.END - 2) {
    if (rnd() < 0.45) who = who === "me" ? "them" : "me";
    out.push({ t, who, len: 2.5 + rnd() * 6 });
    t += 3.5 + rnd() * 6;
  }
  return out;
})();

// What the judge answers on a few ticks; every other tick is silent.
// tools are indexes into [list_sources, search_context, read_source, read_transcript, list_meetings].
export const SCRIPT = {
  300: { tools: [0] },
  440: { kind: "loop", tools: [3], rows: [1, 0.78, 0.86, 0.6] },
  520: { kind: "stuck", tools: [] },
  700: { kind: "info", tools: [1, 2], rows: [0.92] },
  900: { kind: "loop", tools: [3], rows: [1, 0.7, 0.8] },
  1000: { tools: [3, 4] },
};

export const KINDS = ["loop", "stuck", "info", "idea"];

export const countAt = (t) => {
  let n = 0;
  while (n < LINES.length && LINES[n].t <= t) n++;
  return n;
};

export function timeline() {
  const ticks = [];
  const emitted = [];
  let lastEmit = -1e9;
  let filedAt = null;
  const attempts = [];
  let lastAttemptLines = null;
  for (let t = M.TICK; t <= M.END; t += M.TICK) {
    const n = countAt(t);
    let filing = null;
    // filing::FilingTracker::live_due, run before the judge in the same tick.
    if (
      filedAt === null &&
      n >= M.FILING_MIN &&
      attempts.length < M.FILING_MAX &&
      (lastAttemptLines === null || n - lastAttemptLines >= M.FILING_RETRY)
    ) {
      attempts.push(t);
      lastAttemptLines = n;
      // The first try sees too little to know the subject and answers "wait".
      if (attempts.length === 2) {
        filedAt = t;
        filing = "filed";
      } else filing = "wait";
    }
    const v = SCRIPT[t] || {};
    let outcome = "silent";
    if (v.kind) {
      // observer::should_emit
      const same = emitted.some(
        (e) => e.kind === v.kind && t - e.t < M.SAME_KIND,
      );
      if (t - lastEmit < M.COOLDOWN) outcome = "cooldown";
      else if (same) outcome = "same";
      else {
        outcome = "emit";
        lastEmit = t;
        emitted.push({ t, kind: v.kind, rows: v.rows || [0.9] });
      }
    }
    ticks.push({
      t,
      n,
      filing,
      filed: filedAt !== null,
      kind: v.kind || null,
      outcome,
      tools: v.tools || [],
    });
  }
  return { ticks, emitted, filedAt, attempts };
}

const clamp01 = (v) => Math.min(1, Math.max(0, v));
const ease = (v) => v * v * (3 - 2 * v);
// Rise over `a`, hold, fall over `d`: the pins, drawers and rods on a tick.
const envelope = (dt, a, hold, d) =>
  dt < 0
    ? 0
    : dt < a
      ? ease(dt / a)
      : dt < a + hold
        ? 1
        : ease(clamp01(1 - (dt - a - hold) / d));

const fmt = (s) =>
  `${String(Math.floor(s / 60)).padStart(2, "0")}:${String(Math.floor(s % 60)).padStart(2, "0")}`;

export function stateAt(t, T) {
  const live = Math.min(t, M.END);
  const n = countAt(live);
  let tick = null;
  for (const k of T.ticks) if (k.t <= t) tick = k;
  const since = tick ? t - tick.t : 1e9;
  const press = tick && t <= M.END + 8 ? envelope(since, 1.2, 3, 2.5) : 0;
  const filed = T.filedAt !== null && t >= T.filedAt;
  let card = null;
  for (const e of T.emitted) if (e.t <= t) card = e;
  const cooldown = card ? Math.max(0, M.COOLDOWN - (t - card.t)) : 0;
  let sameKind = 0;
  for (const e of T.emitted)
    if (e.t <= t) sameKind = Math.max(sameKind, M.SAME_KIND - (t - e.t));
  const ended = t >= M.END;
  let event = "listening";
  if (ended)
    event =
      t < M.END + 40
        ? "ended · session summary"
        : t < M.END + 70
          ? "merging rolling summary"
          : "summary filed with meeting";
  else if (tick && since < 9) {
    if (tick.filing === "wait") event = "filing: subject not clear yet";
    else if (tick.filing === "filed") event = "filed into a project";
    else if (tick.outcome === "emit") event = `nudge · ${tick.kind}`;
    else if (tick.outcome === "cooldown")
      event = `held · ${tick.kind} inside 3-min cooldown`;
    else if (tick.outcome === "same")
      event = `held · ${tick.kind} again inside 10 min`;
    else
      event = tick.tools.length
        ? `silent · ${tick.tools.length} tool call${tick.tools.length > 1 ? "s" : ""}`
        : "silent";
  }
  return {
    t,
    live,
    n,
    tick,
    tickNo: tick ? tick.t / M.TICK : 0,
    press,
    tools: tick && press > 0 ? tick.tools : [],
    filed,
    folder: T.filedAt === null ? 0 : ease(clamp01((t - T.filedAt) / 14)),
    card,
    rise: card ? ease(clamp01((t - card.t) / 5)) : 0,
    ring: card
      ? Math.exp(-Math.max(0, t - card.t) / 2.5) * (t >= card.t ? 1 : 0)
      : 0,
    strike: card ? envelope(t - card.t, 0.4, 0.3, 0.8) : 0,
    cooldown,
    sameKind,
    nudges: T.emitted.filter((e) => e.t <= t).length,
    ended,
    sheets: ended
      ? [
          ease(clamp01((t - M.END - 6) / 14)),
          ease(clamp01((t - M.END - 44) / 14)),
        ]
      : [0, 0],
    readout: `${fmt(live)} · ${n} lines · ${event}`,
  };
}
