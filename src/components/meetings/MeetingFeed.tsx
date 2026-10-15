import React from "react";
import { useTranslation } from "react-i18next";
import { ask } from "@tauri-apps/plugin-dialog";
import { FileText, Trash2 } from "lucide-react";
import type { Meeting } from "@/bindings";
import { useMeetingStore } from "@/stores/meetingStore";
import { useProjectStore } from "@/stores/projectStore";

const DAY_MS = 86_400_000;

const startOfDay = (ms: number) => {
  const d = new Date(ms);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
};

/** "Today", "Yesterday", then "Tue, Oct 13" (with the year once it differs). */
const dayLabel = (dayStart: number, locale: string) => {
  const diff = Math.round((startOfDay(Date.now()) - dayStart) / DAY_MS);
  if (diff === 0 || diff === 1) {
    const s = new Intl.RelativeTimeFormat(locale, { numeric: "auto" }).format(
      -diff,
      "day",
    );
    return s.charAt(0).toLocaleUpperCase(locale) + s.slice(1);
  }
  const sameYear =
    new Date(dayStart).getFullYear() === new Date().getFullYear();
  return new Intl.DateTimeFormat(locale, {
    weekday: "short",
    month: "short",
    day: "numeric",
    ...(sameYear ? {} : { year: "numeric" }),
  }).format(dayStart);
};

const timeLabel = (ms: number, locale: string) =>
  new Intl.DateTimeFormat(locale, {
    hour: "numeric",
    minute: "2-digit",
  }).format(ms);

/** Meetings grouped by the day they started, newest first. */
export const MeetingFeed: React.FC<{
  meetings: Meeting[];
  emptyText: string;
  /** Hide the space name on rows (the page already names the space). */
  inSpace?: boolean;
}> = ({ meetings, emptyText, inSpace = false }) => {
  const { t, i18n } = useTranslation();
  const live = useMeetingStore((s) => s.live);
  const observer = useMeetingStore((s) => s.observer);
  const openMeeting = useMeetingStore((s) => s.openMeeting);
  const deleteMeeting = useMeetingStore((s) => s.deleteMeeting);
  const projects = useProjectStore((s) => s.projects);

  if (meetings.length === 0) {
    return (
      <p className="py-10 text-center text-sm text-faint max-w-sm mx-auto leading-relaxed">
        {emptyText}
      </p>
    );
  }

  const groups = new Map<number, Meeting[]>();
  for (const m of [...meetings].sort((a, b) => b.created_at - a.created_at)) {
    const day = startOfDay(m.created_at);
    groups.set(day, [...(groups.get(day) ?? []), m]);
  }

  const subtitle = (m: Meeting) => {
    const parts: string[] = [];
    if (!inSpace) {
      if (m.project_id !== null)
        parts.push(projects.find((p) => p.id === m.project_id)?.title ?? "");
      else if (live.meeting_id === m.id && m.filed_by === null)
        parts.push(
          observer.readiness === "ready"
            ? t("meetings.filing.pending")
            : t("meetings.filing.pickProject"),
        );
      else parts.push(t("meetings.filing.unfiled"));
    }
    const status = live.meeting_id === m.id ? live.status : m.status;
    if (status === "live" || status === "paused")
      parts.push(t(`meetings.status.${status}`));
    return parts.filter(Boolean).join(" · ");
  };

  return (
    <div className="space-y-7">
      {[...groups.entries()].map(([day, rows]) => (
        <section key={day}>
          <h3 className="px-3 mb-1 text-[13px] font-medium text-faint">
            {dayLabel(day, i18n.language)}
          </h3>
          <ul>
            {rows.map((m) => {
              const isLive = live.meeting_id === m.id && live.status === "live";
              return (
                <li key={m.id} className="group relative">
                  <button
                    type="button"
                    onClick={() => openMeeting(m.id)}
                    className="w-full flex items-center gap-3.5 px-3 py-2.5 rounded-xl text-start hover:bg-surface transition-colors cursor-pointer"
                  >
                    <span
                      className={`relative w-9 h-9 shrink-0 inline-flex items-center justify-center rounded-lg border ${
                        isLive
                          ? "border-accent/40 bg-accent/10 text-accent"
                          : "border-line bg-surface text-faint"
                      }`}
                    >
                      <FileText className="w-4 h-4" />
                      {isLive && (
                        <span className="absolute -top-0.5 -end-0.5 w-2 h-2 rounded-full bg-accent live-dot" />
                      )}
                    </span>
                    <span className="flex-1 min-w-0">
                      <span className="block text-[14.5px] leading-5 text-text truncate">
                        {m.title}
                      </span>
                      <span className="block text-[13px] leading-5 text-faint truncate">
                        {subtitle(m)}
                      </span>
                    </span>
                    <span className="shrink-0 font-mono text-[11.5px] text-faint tabular group-hover:opacity-0 transition-opacity">
                      {timeLabel(m.created_at, i18n.language)}
                    </span>
                  </button>
                  <button
                    type="button"
                    title={t("meetings.delete")}
                    aria-label={t("meetings.delete")}
                    disabled={live.meeting_id === m.id}
                    onClick={async () => {
                      const ok = await ask(t("meetings.deleteConfirm"), {
                        title: t("meetings.delete"),
                        kind: "warning",
                      });
                      if (ok) deleteMeeting(m.id);
                    }}
                    className="absolute end-2 top-1/2 -translate-y-1/2 w-8 h-8 inline-flex items-center justify-center rounded-lg text-faint opacity-0 group-hover:opacity-100 focus-visible:opacity-100 hover:text-error hover:bg-error/10 disabled:hidden transition-opacity cursor-pointer"
                  >
                    <Trash2 className="w-4 h-4" />
                  </button>
                </li>
              );
            })}
          </ul>
        </section>
      ))}
    </div>
  );
};
