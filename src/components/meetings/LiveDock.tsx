import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, Pause, Play, Square } from "lucide-react";
import { commands } from "@/bindings";
import { useMeetingStore } from "@/stores/meetingStore";
import { ok, toastError } from "@/lib/result";

/** Start time (ms) of the running session, or null when nothing runs. */
export const useLiveSessionStart = (): number | null => {
  const meetingId = useMeetingStore((s) => s.live.meeting_id);
  const sessionId = useMeetingStore((s) => s.live.session_id);
  const [start, setStart] = useState<number | null>(null);

  useEffect(() => {
    setStart(null);
    if (meetingId === null || sessionId === null) return;
    let cancelled = false;
    commands.getMeetingDetail(meetingId).then((res) => {
      if (cancelled || res.status !== "ok" || !res.data) return;
      const s = res.data.sessions.find((x) => x.id === sessionId);
      if (s) setStart(s.started_at);
    });
    return () => {
      cancelled = true;
    };
  }, [meetingId, sessionId]);

  return start;
};

/** Re-renders once a second while `active`. */
export const useNow = (active: boolean) => {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    if (!active) return;
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [active]);
  return now;
};

export const formatElapsed = (ms: number) => {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const mm = String(m).padStart(h > 0 ? 2 : 1, "0");
  const ss = String(s).padStart(2, "0");
  return h > 0 ? `${h}:${mm}:${ss}` : `${mm}:${ss}`;
};

/** Three bars that move while the meeting records. */
const Level: React.FC<{ active: boolean }> = ({ active }) => (
  <span className="flex items-center gap-[3px] h-4" aria-hidden="true">
    {[0, 1, 2, 3].map((i) => (
      <span
        key={i}
        className={`w-[3px] rounded-full bg-current ${active ? "level-bar" : ""}`}
        style={{
          height: active ? undefined : 4,
          animationDelay: `${i * 140}ms`,
        }}
      />
    ))}
  </span>
);

/**
 * Floating control for the running meeting, on every page: what is recording,
 * for how long, and pause / end. Clicking the title opens the meeting.
 * `floating` renders it alone in the always-on-top bar window shown while the
 * app is in the background, where opening brings the main window forward.
 */
export const LiveDock: React.FC<{ floating?: boolean }> = ({
  floating = false,
}) => {
  const { t } = useTranslation();
  const live = useMeetingStore((s) => s.live);
  const meetings = useMeetingStore((s) => s.meetings);
  const detail = useMeetingStore((s) => s.detail);
  const activeMeetingId = useMeetingStore((s) => s.activeMeetingId);
  const openMeeting = useMeetingStore((s) => s.openMeeting);
  const aiReady = useMeetingStore((s) => s.observer.readiness === "ready");
  const pause = useMeetingStore((s) => s.pause);
  const resume = useMeetingStore((s) => s.resume);
  const end = useMeetingStore((s) => s.end);
  const pending = useMeetingStore((s) => s.controlPending);
  const controlError = useMeetingStore((s) => s.controlError);
  const start = useLiveSessionStart();
  const recording = live.status === "live";
  const now = useNow(recording && start !== null);

  if (live.meeting_id === null || live.status === null) return null;

  const title =
    (detail?.meeting.id === live.meeting_id ? detail.meeting.title : null) ??
    meetings.find((m) => m.id === live.meeting_id)?.title ??
    "";
  const onThisMeeting = !floating && activeMeetingId === live.meeting_id;
  const open = async () => {
    const id = live.meeting_id!;
    if (!floating) return openMeeting(id);
    try {
      await ok(commands.focusMeeting(id));
    } catch (e) {
      toastError("meetings.intervention.errors.open", e);
    }
  };

  return (
    <div
      className={
        floating
          ? "h-screen w-screen flex items-center justify-center px-2"
          : "pointer-events-none absolute inset-x-0 bottom-5 flex justify-center px-6 z-20"
      }
    >
      <div
        role="region"
        aria-label={t("meetings.dock.label")}
        className={`pointer-events-auto flex items-center gap-1 h-12 ps-2 pe-1.5 rounded-full bg-surface/95 border border-line-strong backdrop-blur-md max-w-full ${
          floating
            ? "shadow-[0_4px_12px_-4px_rgb(0_0_0/0.4)]"
            : "shadow-[0_16px_40px_-12px_rgb(0_0_0/0.45),0_2px_6px_-2px_rgb(0_0_0/0.2)]"
        }`}
      >
        <span
          data-tauri-drag-region={floating || undefined}
          className={`w-9 h-9 shrink-0 rounded-full inline-flex items-center justify-center ${
            recording
              ? "bg-accent/15 text-accent"
              : "bg-surface-hover text-faint"
          }`}
        >
          <Level active={recording} />
        </span>
        <button
          type="button"
          disabled={onThisMeeting}
          onClick={open}
          title={onThisMeeting ? undefined : t("meetings.dock.open")}
          className="min-w-0 flex flex-col items-start px-2 text-start enabled:cursor-pointer enabled:hover:opacity-80"
        >
          <span className="text-[13px] leading-4 text-text truncate max-w-[16rem]">
            {title}
          </span>
          {floating && controlError ? (
            <span
              role="alert"
              className="text-[11.5px] leading-4 text-error truncate max-w-[16rem]"
              title={controlError}
            >
              {controlError}
            </span>
          ) : (
            <span className="text-[11.5px] leading-4 text-faint">
              {recording
                ? t("meetings.dock.recording")
                : t("meetings.status.paused")}
              {recording && start !== null && (
                <span className="font-mono tabular ms-1.5">
                  {formatElapsed(now - start)}
                </span>
              )}
              {recording && aiReady && (
                <span className="text-accent">
                  {" · "}
                  {t("meetings.dock.aiListening")}
                </span>
              )}
            </span>
          )}
        </button>
        <span className="w-px h-6 bg-line mx-1" />
        <button
          type="button"
          onClick={recording ? pause : resume}
          disabled={pending !== null}
          aria-busy={pending === "pause" || pending === "resume"}
          title={
            recording
              ? t("meetings.controls.pause")
              : t("meetings.controls.resume")
          }
          aria-label={
            recording
              ? t("meetings.controls.pause")
              : t("meetings.controls.resume")
          }
          className="w-9 h-9 inline-flex items-center justify-center rounded-full text-muted hover:text-text hover:bg-surface-hover transition-colors cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed"
        >
          {pending === "pause" || pending === "resume" ? (
            <Loader2 className="w-4 h-4 animate-spin" />
          ) : recording ? (
            <Pause className="w-4 h-4" />
          ) : (
            <Play className="w-4 h-4" />
          )}
        </button>
        <button
          type="button"
          onClick={() => end(null)}
          disabled={pending !== null}
          aria-busy={pending === "end"}
          className="h-9 px-3.5 inline-flex items-center gap-1.5 rounded-full bg-text text-background text-[13px] font-medium hover:bg-text/85 transition-colors cursor-pointer disabled:opacity-60 disabled:cursor-not-allowed"
        >
          {pending === "end" ? (
            <Loader2 className="w-3 h-3 animate-spin" />
          ) : (
            <Square className="w-3 h-3 fill-current" />
          )}
          {t("meetings.controls.end")}
        </button>
      </div>
    </div>
  );
};
