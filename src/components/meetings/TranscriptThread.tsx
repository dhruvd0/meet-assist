import React, { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { Pause, Play } from "lucide-react";
import type { MeetingDetail } from "@/bindings";
import { formatDateTime } from "@/utils/dateFormat";

const clock = (ms: number, locale: string) =>
  new Intl.DateTimeFormat(locale, {
    hour: "numeric",
    minute: "2-digit",
  }).format(ms);

/**
 * Transcript lines by session. "You" is set in the accent; "Them" in muted.
 * Each line can play the audio it was transcribed from.
 */
export const TranscriptThread: React.FC<{
  detail: MeetingDetail;
  autoscroll: boolean;
  playingId: number | null;
  onPlay: (lineId: number) => void;
}> = ({ detail, autoscroll, playingId, onPlay }) => {
  const { t, i18n } = useTranslation();
  const endRef = useRef<HTMLDivElement>(null);
  const playingRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (autoscroll)
      endRef.current?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [detail.lines.length, autoscroll]);

  useEffect(() => {
    playingRef.current?.scrollIntoView({
      block: "nearest",
      behavior: "smooth",
    });
  }, [playingId]);

  const sessionIndex = new Map(detail.sessions.map((s, i) => [s.id, i + 1]));

  if (detail.lines.length === 0)
    return (
      <p className="py-1 text-sm text-faint">
        {t("meetings.transcript.empty")}
      </p>
    );

  return (
    <div className="space-y-6">
      {detail.sessions.map((s) => {
        const lines = detail.lines.filter((l) => l.session_id === s.id);
        if (lines.length === 0) return null;
        return (
          <div key={s.id}>
            {detail.sessions.length > 1 && (
              <div className="mb-2 text-xs text-faint">
                {t("meetings.transcript.session", {
                  n: sessionIndex.get(s.id),
                })}
                {" · "}
                {formatDateTime(
                  String(Math.floor(s.started_at / 1000)),
                  i18n.language,
                )}
              </div>
            )}
            <div className="space-y-0.5">
              {lines.map((l) => {
                const me = l.speaker !== "them";
                const playing = playingId === l.id;
                return (
                  <div
                    key={l.id}
                    ref={playing ? playingRef : undefined}
                    className={`group -mx-2 px-2 py-1 rounded-lg grid grid-cols-[3.25rem_1fr_auto] gap-x-3 text-[14.5px] leading-6 transition-colors ${
                      playing ? "bg-accent/10" : ""
                    }`}
                  >
                    <span
                      className={`text-[13px] leading-6 font-medium ${me ? "text-accent" : "text-faint"}`}
                    >
                      {me
                        ? t("meetings.transcript.me")
                        : t("meetings.transcript.them")}
                    </span>
                    <span className={me ? "text-text" : "text-muted"}>
                      {l.text}
                    </span>
                    <span className="flex items-start gap-1.5">
                      <span className="font-mono text-[11px] leading-6 text-faint tabular opacity-0 group-hover:opacity-100 transition-opacity">
                        {clock(l.ts, i18n.language)}
                      </span>
                      <button
                        type="button"
                        onClick={() => onPlay(l.id)}
                        title={
                          playing
                            ? t("meetings.transcript.pauseLine")
                            : t("meetings.transcript.playLine")
                        }
                        aria-label={
                          playing
                            ? t("meetings.transcript.pauseLine")
                            : t("meetings.transcript.playLine")
                        }
                        className={`w-6 h-6 inline-flex items-center justify-center rounded-md transition-[opacity,color,background-color] cursor-pointer focus-visible:opacity-100 ${
                          playing
                            ? "opacity-100 text-accent"
                            : "opacity-0 group-hover:opacity-100 text-faint hover:text-text hover:bg-surface-hover"
                        }`}
                      >
                        {playing ? (
                          <Pause className="w-3 h-3 fill-current" />
                        ) : (
                          <Play className="w-3 h-3 fill-current" />
                        )}
                      </button>
                    </span>
                  </div>
                );
              })}
            </div>
          </div>
        );
      })}
      <div ref={endRef} />
    </div>
  );
};
