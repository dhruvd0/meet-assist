import React from "react";
import { useTranslation } from "react-i18next";
import { ask } from "@tauri-apps/plugin-dialog";
import { Mic, Trash2 } from "lucide-react";
import { useMeetingStore } from "@/stores/meetingStore";
import Badge from "../ui/Badge";
import { Button } from "../ui/Button";
import { SettingsGroup } from "../ui/SettingsGroup";
import { formatRelativeTime } from "@/utils/dateFormat";

export const MeetingList: React.FC = () => {
  const { t, i18n } = useTranslation();
  const meetings = useMeetingStore((s) => s.meetings);
  const loading = useMeetingStore((s) => s.loading);
  const live = useMeetingStore((s) => s.live);
  const openMeeting = useMeetingStore((s) => s.openMeeting);
  const startMeeting = useMeetingStore((s) => s.startMeeting);
  const deleteMeeting = useMeetingStore((s) => s.deleteMeeting);

  return (
    <div className="space-y-6">
      <SettingsGroup
        title={t("meetings.title")}
        description={t("meetings.description")}
      >
        <div className="flex items-center gap-2 p-4">
          <Button
            size="sm"
            disabled={live.meeting_id !== null}
            onClick={() => startMeeting(null, null)}
          >
            <Mic className="w-4 h-4 inline mr-1" />
            {t("meetings.quickStart")}
          </Button>
        </div>
      </SettingsGroup>

      <SettingsGroup>
        {loading && meetings.length === 0 ? (
          <p className="p-4 text-sm text-mid-gray">{t("common.loading")}</p>
        ) : meetings.length === 0 ? (
          <p className="p-4 text-sm text-mid-gray">{t("meetings.empty")}</p>
        ) : (
          meetings.map((m) => (
            <div key={m.id} className="flex items-center gap-3 p-4">
              <Mic className="w-5 h-5 text-logo-primary shrink-0" />
              <button
                className="flex-1 text-left cursor-pointer"
                title={t("meetings.open")}
                onClick={() => openMeeting(m.id)}
              >
                <div className="text-sm font-semibold text-text">{m.title}</div>
                {m.rolling_summary && (
                  <div className="text-xs text-text/60 truncate max-w-md">
                    {m.rolling_summary.split("\n")[0]}
                  </div>
                )}
                <div className="text-xs text-mid-gray">
                  {formatRelativeTime(
                    String(Math.floor(m.updated_at / 1000)),
                    i18n.language,
                  )}
                </div>
              </button>
              <Badge variant={m.status === "live" ? "success" : "secondary"}>
                {t(`meetings.status.${m.status}`)}
              </Badge>
              <Button
                variant="danger-ghost"
                size="sm"
                title={t("meetings.delete")}
                disabled={live.meeting_id === m.id}
                onClick={async () => {
                  const ok = await ask(t("meetings.deleteConfirm"), {
                    title: t("meetings.delete"),
                    kind: "warning",
                  });
                  if (ok) deleteMeeting(m.id);
                }}
              >
                <Trash2 className="w-4 h-4" />
              </Button>
            </div>
          ))
        )}
      </SettingsGroup>
    </div>
  );
};
