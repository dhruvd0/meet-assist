import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  CalendarDays,
  ChevronRight,
  Loader2,
  Pause,
  Play,
  Square,
} from "lucide-react";
import { useMeetingStore } from "@/stores/meetingStore";
import { Alert } from "../ui/Alert";
import Badge from "../ui/Badge";
import { Button } from "../ui/Button";
import { DocSection } from "../shared";
import { ContextEditor } from "./ContextEditor";
import { InterventionTimeline } from "./InterventionTimeline";
import { EfficiencyReports, sessionReports } from "./EfficiencyReport";
import { SessionSummary } from "./SessionSummary";
import { MeetingProjectPicker } from "./MeetingProjectPicker";
import { TranscriptThread } from "./TranscriptThread";
import { NotesMarkdown } from "./NotesMarkdown";
import { ObserverStatus } from "./ObserverStatus";
import { useTranscriptPlayer } from "./useTranscriptPlayer";

/** One meeting as a page of notes: summary first, transcript last. */
export const MeetingDetail: React.FC = () => {
  const { t, i18n } = useTranslation();
  const detail = useMeetingStore((s) => s.detail);
  const live = useMeetingStore((s) => s.live);
  const observer = useMeetingStore((s) => s.observer);
  const captureWarning = useMeetingStore((s) => s.captureWarning);
  const summarize = useMeetingStore((s) => s.summarize);
  const renameMeeting = useMeetingStore((s) => s.renameMeeting);
  const startMeeting = useMeetingStore((s) => s.startMeeting);
  const pending = useMeetingStore((s) => s.controlPending);
  const detailError = useMeetingStore((s) => s.detailError);
  const end = useMeetingStore((s) => s.end);
  const [editing, setEditing] = useState(false);
  const [title, setTitle] = useState("");
  const isLive = detail !== null && live.meeting_id === detail.meeting.id;
  const [showTranscript, setShowTranscript] = useState(isLive);
  const player = useTranscriptPlayer(detail?.lines ?? []);

  // A meeting that is recording opens with its transcript showing.
  const meetingId = detail?.meeting.id;
  useEffect(() => {
    setShowTranscript(isLive);
  }, [isLive, meetingId]);

  if (!detail)
    return detailError ? (
      <p role="alert" className="p-8 text-center text-sm text-error">
        {detailError}
      </p>
    ) : (
      <p
        role="status"
        className="p-8 flex items-center justify-center gap-2 text-sm text-faint"
      >
        <Loader2 className="w-4 h-4 animate-spin" />
        {t("common.loading")}
      </p>
    );
  const m = detail.meeting;
  const status = isLive ? live.status : m.status;
  const otherRunning = live.meeting_id !== null && !isLive;

  const commit = () => {
    const next = title.trim();
    if (next && next !== m.title) renameMeeting(m.id, next);
    setEditing(false);
  };

  const first = detail.sessions[0]?.started_at ?? m.created_at;
  const when = new Intl.DateTimeFormat(i18n.language, {
    weekday: "short",
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(first);

  // Newest session first; a session still running has no notes yet.
  const summarized = [...detail.sessions]
    .reverse()
    .filter(
      (s) => s.summary !== null || s.ended_at != null || m.status === "ended",
    );

  const nudgesTitle = t("meetings.intervention.timeline");
  const reportTitle = t("meetings.summary.efficiency.title");
  const reports = sessionReports(detail.sessions);
  const reportsView = reports.length > 0 && (
    <EfficiencyReports
      reports={reports}
      labelled={detail.sessions.length > 1}
    />
  );

  return (
    // Nudges sit in a right rail when the window has room, inline otherwise.
    <div
      key={m.id}
      className="w-full max-w-[1100px] mx-auto px-8 pt-2 pb-32 page-in select-text cursor-auto @min-[960px]:grid @min-[960px]:grid-cols-[minmax(0,1fr)_288px] @min-[960px]:gap-12"
    >
      <article className="w-full max-w-[720px] min-w-0 mx-auto @min-[960px]:mx-0 @min-[960px]:justify-self-end">
        {editing ? (
          <textarea
            autoFocus
            rows={1}
            value={title}
            onChange={(e) => setTitle(e.target.value.replace(/\n/g, ""))}
            onBlur={commit}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                commit();
              }
              if (e.key === "Escape") setEditing(false);
            }}
            className="w-full -mx-2 px-2 rounded-lg bg-surface resize-none font-serif text-[32px] leading-tight tracking-[-0.01em] text-text focus:outline-none [field-sizing:content]"
          />
        ) : (
          <h1
            title={t("meetings.rename")}
            onClick={() => {
              setTitle(m.title);
              setEditing(true);
            }}
            className="font-serif text-[32px] leading-tight tracking-[-0.01em] text-text text-balance cursor-text"
          >
            {m.title}
          </h1>
        )}

        <div className="mt-4 flex flex-wrap items-center gap-2">
          <span className="inline-flex items-center gap-1.5 h-8 px-3 rounded-full border border-line font-mono text-[12px] text-muted">
            <CalendarDays className="w-3.5 h-3.5" />
            {when}
          </span>
          <MeetingProjectPicker meeting={m} />
          {(status === "live" || status === "paused") && (
            <Badge variant={status === "live" ? "primary" : "secondary"}>
              {status === "live" && (
                <span className="w-1.5 h-1.5 rounded-full bg-accent live-dot" />
              )}
              {t(`meetings.status.${status}`)}
            </Badge>
          )}
          {!isLive && (
            <div className="ms-auto flex items-center gap-2">
              {status === "paused" && (
                <Button
                  variant="ghost"
                  disabled={pending !== null}
                  onClick={() => end(m.id)}
                >
                  {pending === "end" ? (
                    <Loader2 className="w-3 h-3 animate-spin" />
                  ) : (
                    <Square className="w-3 h-3" />
                  )}
                  {t("meetings.controls.end")}
                </Button>
              )}
              <Button
                variant="secondary"
                disabled={otherRunning || pending !== null}
                onClick={() => startMeeting(m.project_id, m.id)}
              >
                {pending === "start" ? (
                  <Loader2 className="w-3.5 h-3.5 animate-spin" />
                ) : (
                  <Play className="w-3.5 h-3.5" />
                )}
                {t("meetings.continue")}
              </Button>
            </div>
          )}
        </div>

        <div className="mt-6 space-y-2 empty:hidden">
          {isLive && observer.readiness === "ready" && (
            <ObserverStatus recording={live.status === "live"} />
          )}
          {captureWarning?.source === "system_audio" &&
            captureWarning.meeting_id === m.id && (
              <Alert variant="warning">
                {t("meetings.capture.systemAudioUnavailable", {
                  message: captureWarning.message,
                })}{" "}
                {t("meetings.capture.systemAudioHint")}
              </Alert>
            )}
          {observer.readiness !== "ready" && isLive && (
            <Alert variant="info">
              {t(`meetings.observer.readiness.${observer.readiness}`, {
                detail: observer.detail ?? "",
              })}
            </Alert>
          )}
          {observer.tools_unavailable && (
            <Alert variant="info">
              {t("meetings.observer.toolsUnavailable")}
            </Alert>
          )}
          {observer.last_error && (
            <Alert variant="warning">
              {observer.consecutive_failures >= 3
                ? t("meetings.observer.failing", {
                    count: observer.consecutive_failures,
                  })
                : t("meetings.observer.lastError")}{" "}
              {observer.last_error}
            </Alert>
          )}
        </div>

        {/* With one session the session notes already say it all. */}
        {m.rolling_summary && detail.sessions.length > 1 && (
          <DocSection title={t("meetings.summary.rolling")}>
            <NotesMarkdown text={m.rolling_summary} />
          </DocSection>
        )}

        {summarized.map((s) => (
          <DocSection
            key={s.id}
            title={
              detail.sessions.length > 1
                ? `${t("meetings.summary.title")} · ${t(
                    "meetings.transcript.session",
                    { n: detail.sessions.indexOf(s) + 1 },
                  )}`
                : t("meetings.summary.title")
            }
          >
            <SessionSummary
              session={s}
              meetingEnded={m.status === "ended"}
              onRetry={() => summarize(s.id)}
            />
          </DocSection>
        ))}

        <ContextEditor meetingId={m.id} context={m.context} />

        {reportsView && (
          <DocSection title={reportTitle} className="@min-[960px]:hidden">
            {reportsView}
          </DocSection>
        )}

        <DocSection title={nudgesTitle} className="@min-[960px]:hidden">
          <InterventionTimeline items={detail.interventions} />
        </DocSection>

        <DocSection
          title={t("meetings.transcript.title")}
          action={
            <div className="flex items-center gap-1">
              {detail.lines.length > 0 && (
                <button
                  type="button"
                  onClick={() => {
                    if (player.playingId !== null) return player.stop();
                    setShowTranscript(true);
                    player.playFrom(detail.lines[0].id);
                  }}
                  className="inline-flex items-center gap-1.5 h-7 px-2.5 rounded-full border border-line text-xs text-muted hover:text-text hover:border-line-strong transition-colors cursor-pointer"
                >
                  {player.playingId !== null ? (
                    <Pause className="w-3 h-3 fill-current" />
                  ) : (
                    <Play className="w-3 h-3 fill-current" />
                  )}
                  {player.playingId !== null
                    ? t("meetings.transcript.stopAll")
                    : t("meetings.transcript.playAll")}
                </button>
              )}
              <button
                type="button"
                onClick={() => setShowTranscript((v) => !v)}
                aria-expanded={showTranscript}
                className="inline-flex items-center gap-1 h-7 px-2 rounded-lg text-xs text-faint hover:text-text hover:bg-surface transition-colors cursor-pointer"
              >
                {showTranscript
                  ? t("meetings.transcript.hide")
                  : t("meetings.transcript.show", {
                      count: detail.lines.length,
                    })}
                <ChevronRight
                  className={`w-3.5 h-3.5 transition-transform ${showTranscript ? "rotate-90" : ""}`}
                />
              </button>
            </div>
          }
        >
          {showTranscript && (
            <TranscriptThread
              detail={detail}
              autoscroll={isLive && live.status === "live"}
              playingId={player.playingId}
              onPlay={player.play}
            />
          )}
        </DocSection>
      </article>

      <aside className="hidden @min-[960px]:block min-w-0">
        <div className="sticky top-0 pt-[7px] max-h-[calc(100vh-3rem)] overflow-y-auto pb-24">
          {reportsView && (
            <section aria-label={reportTitle} className="mb-8">
              <h2 className="flex items-center h-8 text-[13px] font-medium text-muted">
                {reportTitle}
              </h2>
              <div className="mt-2">{reportsView}</div>
            </section>
          )}
          <h2 className="flex items-center gap-2 h-8 text-[13px] font-medium text-muted">
            {nudgesTitle}
            {detail.interventions.length > 0 && (
              <span className="font-mono text-[11px] text-faint tabular">
                {detail.interventions.length}
              </span>
            )}
          </h2>
          <div className="mt-2">
            <InterventionTimeline items={detail.interventions} compact />
          </div>
        </div>
      </aside>
    </div>
  );
};
