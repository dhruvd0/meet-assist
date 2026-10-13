import React, { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import type { MeetingDetail } from "@/bindings";
import { SettingsGroup } from "../ui/SettingsGroup";
import { formatDateTime } from "@/utils/dateFormat";

export const TranscriptThread: React.FC<{
  detail: MeetingDetail;
  autoscroll: boolean;
}> = ({ detail, autoscroll }) => {
  const { t, i18n } = useTranslation();
  const boxRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (autoscroll && boxRef.current)
      boxRef.current.scrollTop = boxRef.current.scrollHeight;
  }, [detail.lines.length, autoscroll]);

  const sessionIndex = new Map(detail.sessions.map((s, i) => [s.id, i + 1]));

  return (
    <SettingsGroup title={t("meetings.transcript.title")}>
      {detail.lines.length === 0 ? (
        <p className="p-4 text-sm text-mid-gray">
          {t("meetings.transcript.empty")}
        </p>
      ) : (
        <div
          ref={boxRef}
          className="p-4 space-y-3 max-h-[50vh] overflow-y-auto"
        >
          {detail.sessions.map((s) => {
            const lines = detail.lines.filter((l) => l.session_id === s.id);
            if (lines.length === 0) return null;
            return (
              <div key={s.id} className="space-y-1">
                <div className="text-xs font-medium text-mid-gray uppercase tracking-wide">
                  {t("meetings.transcript.session", {
                    n: sessionIndex.get(s.id),
                  })}
                  {" · "}
                  {formatDateTime(
                    String(Math.floor(s.started_at / 1000)),
                    i18n.language,
                  )}
                </div>
                {lines.map((l) => (
                  <div key={l.id} className="flex gap-2 text-sm">
                    <span className="shrink-0 text-logo-primary font-semibold">
                      {t("meetings.transcript.me")}
                    </span>
                    <span className="text-text">{l.text}</span>
                  </div>
                ))}
              </div>
            );
          })}
        </div>
      )}
    </SettingsGroup>
  );
};
