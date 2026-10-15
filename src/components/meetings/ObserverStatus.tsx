import React from "react";
import { useTranslation } from "react-i18next";
import { useMeetingStore } from "@/stores/meetingStore";
import { useNow } from "./LiveDock";

const ago = (ms: number, lang: string) => {
  const s = Math.max(0, Math.round(ms / 1000));
  const rtf = new Intl.RelativeTimeFormat(lang, { numeric: "auto" });
  return s < 60
    ? rtf.format(-s, "second")
    : rtf.format(-Math.round(s / 60), "minute");
};

/**
 * Shows that the assistant is following the live meeting, with its last few
 * verdicts (why it nudged or stayed quiet), newest first.
 */
export const ObserverStatus: React.FC<{ recording: boolean }> = ({
  recording,
}) => {
  const { t, i18n } = useTranslation();
  const thoughts = useMeetingStore((s) => s.observer.thoughts);
  const now = useNow(recording);
  const newest = [...thoughts].reverse();
  const clock = new Intl.DateTimeFormat(i18n.language, {
    hour: "numeric",
    minute: "2-digit",
    second: "2-digit",
  });

  return (
    <section
      aria-live="polite"
      className="rounded-xl border border-line px-3.5 py-3 text-[13px]"
    >
      <div className="flex items-center gap-2">
        <span
          aria-hidden="true"
          className={`w-2 h-2 rounded-full ${
            recording ? "bg-accent animate-pulse" : "bg-line-strong"
          }`}
        />
        <span className="font-medium text-text">
          {recording
            ? t("meetings.observer.listening")
            : t("meetings.observer.pausedListening")}
        </span>
        {newest[0] && (
          <span className="ms-auto text-xs text-faint">
            {t("meetings.observer.lastCheck", {
              time: ago(now - newest[0].ts, i18n.language),
            })}
          </span>
        )}
      </div>
      {newest.length === 0 ? (
        <p className="mt-1.5 text-faint">{t("meetings.observer.firstCheck")}</p>
      ) : (
        <ul className="mt-2 space-y-1.5">
          {newest.map((th, i) => (
            <li
              key={th.ts}
              className={`flex gap-2 leading-snug ${
                i === 0 ? "text-muted" : "text-faint"
              }`}
            >
              <span className="shrink-0 font-mono text-[11px] tabular pt-px">
                {clock.format(th.ts)}
              </span>
              <span className="min-w-0">
                {th.nudged && (
                  <span className="me-1.5 text-accent font-medium">
                    {t("meetings.observer.nudged")}
                  </span>
                )}
                {th.text}
              </span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
};
