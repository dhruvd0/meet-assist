import React from "react";
import { useTranslation } from "react-i18next";
import { Pause, Play, Square } from "lucide-react";
import { useMeetingStore } from "@/stores/meetingStore";
import Badge from "../ui/Badge";
import { Button } from "../ui/Button";

export const MeetingControls: React.FC<{ meetingId: number }> = ({
  meetingId,
}) => {
  const { t } = useTranslation();
  const live = useMeetingStore((s) => s.live);
  const pause = useMeetingStore((s) => s.pause);
  const resume = useMeetingStore((s) => s.resume);
  const end = useMeetingStore((s) => s.end);
  const startMeeting = useMeetingStore((s) => s.startMeeting);
  const detail = useMeetingStore((s) => s.detail);

  const isThis = live.meeting_id === meetingId;
  const status = isThis ? live.status : (detail?.meeting.status ?? null);
  const otherRunning = live.meeting_id !== null && !isThis;

  if (!isThis) {
    return (
      <div className="flex items-center gap-2">
        {status && (
          <Badge variant="secondary">{t(`meetings.status.${status}`)}</Badge>
        )}
        <Button
          size="sm"
          disabled={otherRunning}
          onClick={() =>
            startMeeting(detail?.meeting.project_id ?? null, meetingId)
          }
        >
          <Play className="w-4 h-4 inline mr-1" />
          {t("meetings.continue")}
        </Button>
        {status === "paused" && (
          <Button size="sm" variant="danger" onClick={() => end(meetingId)}>
            <Square className="w-4 h-4 inline mr-1" />
            {t("meetings.controls.end")}
          </Button>
        )}
      </div>
    );
  }

  return (
    <div className="flex items-center gap-2">
      <Badge variant={status === "live" ? "success" : "secondary"}>
        {status === "live"
          ? t("meetings.controls.listening")
          : t(`meetings.status.${status ?? "paused"}`)}
      </Badge>
      {status === "live" ? (
        <Button size="sm" variant="secondary" onClick={pause}>
          <Pause className="w-4 h-4 inline mr-1" />
          {t("meetings.controls.pause")}
        </Button>
      ) : (
        <Button size="sm" variant="secondary" onClick={resume}>
          <Play className="w-4 h-4 inline mr-1" />
          {t("meetings.controls.resume")}
        </Button>
      )}
      <Button size="sm" variant="danger" onClick={() => end(null)}>
        <Square className="w-4 h-4 inline mr-1" />
        {t("meetings.controls.end")}
      </Button>
    </div>
  );
};
