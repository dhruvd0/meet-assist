import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useMeetingStore } from "@/stores/meetingStore";
import { SettingsGroup } from "../ui/SettingsGroup";
import { Textarea } from "../ui/Textarea";

export const ContextEditor: React.FC<{
  meetingId: number;
  context: string;
}> = ({ meetingId, context }) => {
  const { t } = useTranslation();
  const saveContext = useMeetingStore((s) => s.saveContext);
  const [value, setValue] = useState(context);
  const timer = useRef<number | null>(null);

  const latest = useRef({ meetingId, value, dirty: false });
  latest.current = { ...latest.current, meetingId, value };

  // Resync only when switching meetings, so store echoes of our own saves
  // never overwrite in-flight typing.
  useEffect(() => {
    setValue(context);
    latest.current.dirty = false;
  }, [meetingId]);

  // On unmount: stop the timer and flush any pending edit.
  useEffect(
    () => () => {
      if (timer.current) window.clearTimeout(timer.current);
      const { meetingId: id, value: v, dirty } = latest.current;
      if (dirty) saveContext(id, v);
    },
    [saveContext],
  );

  const onChange = (next: string) => {
    setValue(next);
    latest.current = { ...latest.current, value: next, dirty: true };
    if (timer.current) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      latest.current.dirty = false;
      saveContext(meetingId, next);
    }, 600);
  };

  return (
    <SettingsGroup
      title={t("meetings.context.title")}
      description={t("meetings.context.description")}
    >
      <div className="p-4">
        <Textarea
          className="w-full"
          placeholder={t("meetings.context.placeholder")}
          value={value}
          onChange={(e) => onChange(e.target.value)}
        />
      </div>
    </SettingsGroup>
  );
};
