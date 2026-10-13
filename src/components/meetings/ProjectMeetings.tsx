import React, { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Plus } from "lucide-react";
import { useMeetingStore } from "@/stores/meetingStore";
import Badge from "../ui/Badge";
import { Button } from "../ui/Button";
import { SettingsGroup } from "../ui/SettingsGroup";

export const ProjectMeetings: React.FC<{ projectId: number }> = ({
  projectId,
}) => {
  const { t } = useTranslation();
  const meetings = useMeetingStore((s) => s.meetings);
  const live = useMeetingStore((s) => s.live);
  const loadMeetings = useMeetingStore((s) => s.loadMeetings);
  const refreshLive = useMeetingStore((s) => s.refreshLive);
  const subscribe = useMeetingStore((s) => s.subscribe);
  const openMeeting = useMeetingStore((s) => s.openMeeting);
  const startMeeting = useMeetingStore((s) => s.startMeeting);

  useEffect(() => {
    loadMeetings(projectId);
    refreshLive();
    const unlisten = subscribe();
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [projectId, loadMeetings, refreshLive, subscribe]);

  // Reload when the live meeting changes (start/end) so the list stays current.
  useEffect(() => {
    loadMeetings(projectId);
  }, [projectId, loadMeetings, live.meeting_id, live.status]);

  const rows = meetings.filter((m) => m.project_id === projectId);

  return (
    <SettingsGroup title={t("meetings.title")}>
      <div className="flex items-center gap-2 p-4">
        <Button
          size="sm"
          disabled={live.meeting_id !== null}
          onClick={() => startMeeting(projectId, null)}
        >
          <Plus className="w-4 h-4 inline mr-1" />
          {t("meetings.newInProject")}
        </Button>
      </div>
      {rows.length === 0 ? (
        <p className="p-4 text-sm text-mid-gray">{t("meetings.empty")}</p>
      ) : (
        rows.map((m) => (
          <div key={m.id} className="flex items-center gap-3 p-4">
            <div className="flex-1 min-w-0">
              <div className="text-sm font-semibold text-text truncate">
                {m.title}
              </div>
              {m.rolling_summary && (
                <div className="text-xs text-text/60 truncate">
                  {m.rolling_summary.split("\n")[0]}
                </div>
              )}
            </div>
            <Badge variant={m.status === "live" ? "success" : "secondary"}>
              {t(`meetings.status.${m.status}`)}
            </Badge>
            <Button
              size="sm"
              variant="secondary"
              onClick={() => openMeeting(m.id)}
            >
              {t("meetings.open")}
            </Button>
          </div>
        ))
      )}
    </SettingsGroup>
  );
};
