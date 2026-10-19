import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type TesterSpeaker, type TesterTrace } from "@/bindings";
import { useMeetingStore } from "@/stores/meetingStore";
import { Button } from "../ui/Button";
import { Textarea } from "../ui/Textarea";
import { DocSection } from "../shared";

type Turn = { speaker: TesterSpeaker; text: string };
type ParsedScript =
  | { kind: "valid"; turns: Turn[] }
  | { kind: "invalid"; line: number };
type Draft = { sessionId: number | null; text: string };
const drafts = new Map<number, Draft>();

export function seedTesterDraft(meetingId: number, text: string) {
  drafts.set(meetingId, { sessionId: null, text });
}

export function parseTesterScript(script: string): ParsedScript {
  if (script.length > 40_000) return { kind: "invalid", line: 1 };
  const turns: Turn[] = [];
  for (const [index, line] of script.split("\n").entries()) {
    if (!line.trim()) continue;
    const match = /^(me|them):\s*(.+)$/i.exec(line.trim());
    const speaker = match?.[1]?.toLowerCase();
    const text = match?.[2]?.trim();
    if (
      (speaker !== "me" && speaker !== "them") ||
      !text ||
      text.length > 2_000
    )
      return { kind: "invalid", line: index + 1 };
    turns.push({ speaker, text });
  }
  return { kind: "valid", turns };
}

function remainingScript(script: string) {
  const lines = script.split("\n");
  const first = lines.findIndex((line) => line.trim().length > 0);
  return lines.slice(first + 1).join("\n");
}

function TraceLine({ trace }: { trace: TesterTrace }) {
  const { t } = useTranslation();
  const kind = trace.kind;
  switch (kind.type) {
    case "detecting":
    case "judging":
      return <span>{t(`meetings.tester.trace.${kind.type}`)}</span>;
    case "detection":
      return (
        <span>
          {t(`meetings.tester.trace.${kind.outcome}`)}
          {kind.kind && ` · ${t(`meetings.intervention.kind.${kind.kind}`)}`}
          <span className="block text-xs text-muted tabular-nums">
            {t("meetings.tester.trace.scores", {
              loop: kind.scores.loop_score.toFixed(2),
              stuck: kind.scores.stuck.toFixed(2),
              info: kind.scores.info.toFixed(2),
              idea: kind.scores.idea.toFixed(2),
            })}
          </span>
        </span>
      );
    case "detector_fallback":
    case "detector_bypassed":
      return (
        <span>
          {t(`meetings.tester.trace.${kind.type}`, { reason: kind.reason })}
        </span>
      );
    case "judged":
      return (
        <span>
          {t("meetings.tester.trace.judged", { outcome: kind.outcome })}
          {kind.reason && (
            <span className="block text-xs text-muted">{kind.reason}</span>
          )}
          {kind.remaining_ms > 0 && (
            <span className="block text-xs text-muted">
              {t("meetings.tester.trace.cooldown", {
                seconds: Math.ceil(kind.remaining_ms / 1000),
              })}
            </span>
          )}
        </span>
      );
    case "error":
      return <span className="text-error">{kind.message}</span>;
    default: {
      const exhaustive: never = kind;
      return exhaustive;
    }
  }
}

export function LiveTesterPanel({
  meetingId,
  sessionId,
}: {
  meetingId: number;
  sessionId: number;
}) {
  const { t, i18n } = useTranslation();
  const live = useMeetingStore((s) => s.live);
  const controlPending = useMeetingStore((s) => s.controlPending);
  const [script, setScript] = useState(() => {
    const draft = drafts.get(meetingId);
    return draft && (draft.sessionId === null || draft.sessionId === sessionId)
      ? draft.text
      : "";
  });
  const [playing, setPlaying] = useState(false);
  const [busy, setBusy] = useState(false);
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [traceError, setTraceError] = useState<string | null>(null);
  const [traces, setTraces] = useState<TesterTrace[]>([]);
  const sending = useRef(false);
  const mounted = useRef(true);
  const parsed = parseTesterScript(script);
  const liveHere =
    live.meeting_id === meetingId &&
    live.session_id === sessionId &&
    live.status === "live";
  const canSend =
    liveHere &&
    controlPending === null &&
    parsed.kind === "valid" &&
    parsed.turns.length > 0;

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    const refresh = async () => {
      try {
        const result = await commands.getTesterTrace(meetingId, sessionId);
        if (cancelled) return;
        setTraces(result);
        setTraceError(null);
      } catch (cause) {
        if (!cancelled) setTraceError(String(cause));
      }
      if (!cancelled) timer = setTimeout(refresh, 1000);
    };
    void refresh();
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [meetingId, sessionId]);

  const sendNext = async () => {
    const current = useMeetingStore.getState();
    if (
      sending.current ||
      !canSend ||
      parsed.kind !== "valid" ||
      current.controlPending !== null ||
      current.live.meeting_id !== meetingId ||
      current.live.session_id !== sessionId ||
      current.live.status !== "live"
    )
      return;
    const turn = parsed.turns[0];
    if (!turn) return;
    sending.current = true;
    setBusy(true);
    setError(null);
    try {
      const result = await commands.appendTesterTurn(
        meetingId,
        sessionId,
        turn.speaker,
        turn.text,
      );
      if (result.status === "ok") {
        const next = remainingScript(script);
        drafts.set(meetingId, { sessionId, text: next });
        if (mounted.current) setScript(next);
      } else if (mounted.current) {
        setError(result.error);
        setPlaying(false);
      }
    } catch (cause) {
      if (mounted.current) {
        setError(String(cause));
        setPlaying(false);
      }
    } finally {
      sending.current = false;
      if (mounted.current) setBusy(false);
    }
  };

  useEffect(() => {
    if (!canSend) setPlaying(false);
    if (!playing || !canSend || busy) return;
    const timer = setTimeout(() => {
      void sendNext();
    }, 2500);
    return () => clearTimeout(timer);
    // Trace polling re-renders every second; keying on playback state keeps
    // the timer alive long enough to fire.
  }, [playing, canSend, busy, script]);

  const checkNow = async () => {
    setChecking(true);
    setError(null);
    try {
      const result = await commands.requestTesterCheck(meetingId, sessionId);
      if (result.status !== "ok") setError(result.error);
    } catch (cause) {
      setError(String(cause));
    } finally {
      setChecking(false);
    }
  };

  return (
    <DocSection title={t("meetings.tester.title")}>
      <p className="text-sm text-muted mb-3">{t("meetings.tester.noAudio")}</p>
      <label
        htmlFor={`tester-script-${sessionId}`}
        className="block text-sm font-medium mb-2"
      >
        {t("meetings.tester.remaining")}
      </label>
      <Textarea
        id={`tester-script-${sessionId}`}
        rows={7}
        value={script}
        disabled={busy || playing || !liveHere}
        className="w-full"
        placeholder={t("meetings.tester.scriptHint")}
        onChange={(event) => {
          const text = event.target.value;
          setScript(text);
          drafts.set(meetingId, { sessionId, text });
        }}
      />
      <p className="text-xs text-muted mt-2">
        {t("meetings.tester.scriptHint")}
      </p>
      {parsed.kind === "invalid" && (
        <p role="alert" className="text-sm text-error mt-2">
          {t("meetings.tester.invalid", { line: parsed.line })}
        </p>
      )}
      <div className="flex flex-wrap items-center gap-2 mt-3">
        <Button
          size="sm"
          onClick={() => void sendNext()}
          disabled={!canSend || busy || playing}
        >
          {busy ? t("meetings.tester.sending") : t("meetings.tester.sendNext")}
        </Button>
        <Button
          size="sm"
          variant="secondary"
          onClick={() => setPlaying(!playing)}
          disabled={!playing && (!canSend || busy)}
        >
          {playing
            ? t("meetings.tester.pausePlayback")
            : t("meetings.tester.play")}
        </Button>
        <Button
          size="sm"
          variant="ghost"
          onClick={() => void checkNow()}
          disabled={!liveHere || checking || controlPending !== null}
        >
          {checking
            ? t("meetings.tester.checking")
            : t("meetings.tester.checkNow")}
        </Button>
      </div>
      <p className="text-xs text-muted mt-2">{t("meetings.tester.timing")}</p>
      {error && (
        <p role="alert" className="text-sm text-error mt-2">
          {error}
        </p>
      )}
      <h3 className="text-sm font-medium mt-6 mb-2">
        {t("meetings.tester.trace.title")}
      </h3>
      {traceError && (
        <p role="alert" className="text-sm text-error">
          {traceError}
        </p>
      )}
      {traces.length === 0 ? (
        <p className="text-sm text-muted">
          {t("meetings.tester.trace.waiting")}
        </p>
      ) : (
        <ol
          className="max-h-64 overflow-y-auto space-y-3 text-sm"
          aria-label={t("meetings.tester.trace.title")}
        >
          {traces.slice(-12).map((trace, index) => (
            <li key={`${trace.ts}-${index}`} className="flex gap-3">
              <time className="shrink-0 text-xs text-muted tabular-nums pt-0.5">
                {new Intl.DateTimeFormat(i18n.language, {
                  hour: "2-digit",
                  minute: "2-digit",
                  second: "2-digit",
                }).format(trace.ts)}
              </time>
              <TraceLine trace={trace} />
            </li>
          ))}
        </ol>
      )}
    </DocSection>
  );
}
