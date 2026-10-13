import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowLeft, Pencil } from "lucide-react";
import { useMeetingStore } from "@/stores/meetingStore";
import { Button } from "../ui/Button";
import { Input } from "../ui/Input";
import { ContextEditor } from "./ContextEditor";
import { MeetingControls } from "./MeetingControls";
import { TranscriptThread } from "./TranscriptThread";

export const MeetingDetail: React.FC<{ onBack: () => void }> = ({ onBack }) => {
  const { t } = useTranslation();
  const detail = useMeetingStore((s) => s.detail);
  const live = useMeetingStore((s) => s.live);
  const renameMeeting = useMeetingStore((s) => s.renameMeeting);
  const refreshLive = useMeetingStore((s) => s.refreshLive);
  const subscribe = useMeetingStore((s) => s.subscribe);
  const [editing, setEditing] = useState(false);
  const [title, setTitle] = useState("");

  useEffect(() => {
    refreshLive();
    const unlisten = subscribe();
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [refreshLive, subscribe]);

  if (!detail)
    return <p className="p-4 text-sm text-mid-gray">{t("common.loading")}</p>;
  const m = detail.meeting;

  const commit = () => {
    const next = title.trim();
    if (next && next !== m.title) renameMeeting(m.id, next);
    setEditing(false);
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center gap-2">
        <Button
          variant="ghost"
          size="sm"
          onClick={onBack}
          title={t("meetings.back")}
        >
          <ArrowLeft className="w-4 h-4" />
        </Button>
        {editing ? (
          <Input
            variant="compact"
            className="flex-1"
            value={title}
            autoFocus
            onChange={(e) => setTitle(e.target.value)}
            onBlur={commit}
            onKeyDown={(e) => {
              if (e.key === "Enter") commit();
              if (e.key === "Escape") setEditing(false);
            }}
          />
        ) : (
          <h1 className="flex-1 text-lg font-semibold text-text truncate">
            {m.title}
          </h1>
        )}
        <Button
          variant="ghost"
          size="sm"
          title={t("meetings.rename")}
          onClick={() => {
            setTitle(m.title);
            setEditing(true);
          }}
        >
          <Pencil className="w-4 h-4" />
        </Button>
        <MeetingControls meetingId={m.id} />
      </div>
      <ContextEditor meetingId={m.id} context={m.context} />
      <TranscriptThread
        detail={detail}
        autoscroll={live.meeting_id === m.id && live.status === "live"}
      />
    </div>
  );
};
