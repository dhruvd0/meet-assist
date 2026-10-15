import React from "react";
import { useTranslation } from "react-i18next";
import { Mic } from "lucide-react";
import { useMeetingStore } from "@/stores/meetingStore";
import { useProjectStore } from "@/stores/projectStore";
import { useSettings } from "@/hooks/useSettings";
import { useOsType } from "@/hooks/useOsType";
import { formatKeyCombination } from "@/lib/utils/keyboard";
import { Button } from "../ui/Button";
import { MeetingFeed } from "./MeetingFeed";
import { useLiveSessionStart } from "./LiveDock";

/** Granola-style date block: big day number beside month and weekday. */
const DateBlock: React.FC<{ ms: number; locale: string }> = ({
  ms,
  locale,
}) => (
  <div className="flex items-start gap-2.5 w-36 shrink-0">
    <span className="font-serif text-[34px] leading-none text-text tabular">
      {new Intl.DateTimeFormat(locale, { day: "numeric" }).format(ms)}
    </span>
    <span className="flex flex-col pt-0.5 text-xs leading-4">
      <span className="text-text">
        {new Intl.DateTimeFormat(locale, { month: "long" }).format(ms)}
      </span>
      <span className="text-faint">
        {new Intl.DateTimeFormat(locale, { weekday: "short" }).format(ms)}
      </span>
    </span>
  </div>
);

const Keycaps: React.FC<{ combo: string }> = ({ combo }) => (
  <span className="inline-flex items-center gap-1 align-middle">
    {combo.split(" + ").map((k) => (
      <kbd
        key={k}
        className="inline-flex items-center h-5 px-1.5 rounded-md border border-line-strong bg-background font-mono text-[11px] text-muted"
      >
        {k}
      </kbd>
    ))}
  </span>
);

/** Today's card: the running meeting, or how to start one. */
const NowCard: React.FC = () => {
  const { t, i18n } = useTranslation();
  const live = useMeetingStore((s) => s.live);
  const meetings = useMeetingStore((s) => s.meetings);
  const openMeeting = useMeetingStore((s) => s.openMeeting);
  const startMeeting = useMeetingStore((s) => s.startMeeting);
  const projects = useProjectStore((s) => s.projects);
  const { settings } = useSettings();
  const osType = useOsType();
  const started = useLiveSessionStart();

  const binding = settings?.bindings?.toggle_meeting?.current_binding ?? "";
  const shortcut = binding ? formatKeyCombination(binding, osType) : "";
  const running =
    live.meeting_id !== null
      ? meetings.find((m) => m.id === live.meeting_id)
      : undefined;
  const space =
    running?.project_id != null
      ? projects.find((p) => p.id === running.project_id)?.title
      : null;

  return (
    <div className="flex items-center gap-4 rounded-2xl bg-surface border border-line px-6 py-5">
      <DateBlock ms={Date.now()} locale={i18n.language} />
      <span
        className={`w-[3px] self-stretch rounded-full ${running ? "bg-accent" : "bg-line-strong"}`}
      />
      {running ? (
        <>
          <button
            type="button"
            onClick={() => openMeeting(running.id)}
            className="flex-1 min-w-0 text-start cursor-pointer group"
          >
            <span className="block text-[15px] text-text truncate group-hover:underline underline-offset-4 decoration-line-strong">
              {running.title}
            </span>
            <span className="block text-[13px] text-faint truncate">
              {[
                live.status === "live"
                  ? t("meetings.dock.recording")
                  : t("meetings.status.paused"),
                started !== null
                  ? t("home.startedAt", {
                      time: new Intl.DateTimeFormat(i18n.language, {
                        hour: "numeric",
                        minute: "2-digit",
                      }).format(started),
                    })
                  : null,
                space ?? t("meetings.filing.unfiled"),
              ]
                .filter(Boolean)
                .join(" · ")}
            </span>
          </button>
          <Button variant="secondary" onClick={() => openMeeting(running.id)}>
            {t("home.openNotes")}
          </Button>
        </>
      ) : (
        <>
          <div className="flex-1 min-w-0">
            <p className="text-[15px] text-text">{t("home.idleTitle")}</p>
            <p className="text-[13px] text-faint leading-relaxed">
              {shortcut ? (
                <>
                  {t("home.idleHintBefore")} <Keycaps combo={shortcut} />{" "}
                  {t("home.idleHintAfter")}
                </>
              ) : (
                t("home.idleHintNoShortcut")
              )}
            </p>
          </div>
          <Button onClick={() => startMeeting(null, null)}>
            <Mic className="w-3.5 h-3.5" />
            {t("meetings.start")}
          </Button>
        </>
      )}
    </div>
  );
};

export const HomeView: React.FC = () => {
  const { t } = useTranslation();
  const meetings = useMeetingStore((s) => s.meetings);
  const loading = useMeetingStore((s) => s.loading);

  return (
    <div className="w-full max-w-[720px] mx-auto px-8 pt-6 pb-32 page-in">
      <h1 className="font-serif text-[32px] leading-tight tracking-[-0.01em] text-text mb-5">
        {t("home.title")}
      </h1>
      <NowCard />
      <div className="mt-10">
        {loading && meetings.length === 0 ? (
          <p className="py-10 text-center text-sm text-faint">
            {t("common.loading")}
          </p>
        ) : (
          <MeetingFeed meetings={meetings} emptyText={t("home.empty")} />
        )}
      </div>
    </div>
  );
};
