import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useProjectStore } from "@/stores/projectStore";
import { SettingsGroup } from "../ui/SettingsGroup";
import { Textarea } from "../ui/Textarea";

export const BriefEditor: React.FC<{ projectId: number; brief: string }> = ({
  projectId,
  brief,
}) => {
  const { t } = useTranslation();
  const saveBrief = useProjectStore((s) => s.saveBrief);
  const [value, setValue] = useState(brief);
  const timer = useRef<number | null>(null);

  const latest = useRef({ projectId, value, dirty: false });
  latest.current = { ...latest.current, projectId, value };

  // Resync only when switching projects, so store echoes of our own saves
  // never overwrite in-flight typing.
  useEffect(() => {
    setValue(brief);
    latest.current.dirty = false;
  }, [projectId]);

  // On unmount: stop the timer and flush any pending edit.
  useEffect(
    () => () => {
      if (timer.current) window.clearTimeout(timer.current);
      const { projectId: id, value: v, dirty } = latest.current;
      if (dirty) saveBrief(id, v);
    },
    [saveBrief],
  );

  const onChange = (next: string) => {
    setValue(next);
    latest.current = { ...latest.current, value: next, dirty: true };
    if (timer.current) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      latest.current.dirty = false;
      saveBrief(projectId, next);
    }, 600);
  };

  return (
    <SettingsGroup
      title={t("projects.brief.title")}
      description={t("projects.brief.description")}
    >
      <div className="p-4">
        <Textarea
          className="w-full"
          placeholder={t("projects.brief.placeholder")}
          value={value}
          onChange={(e) => onChange(e.target.value)}
        />
      </div>
    </SettingsGroup>
  );
};
