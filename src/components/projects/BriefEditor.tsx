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

  useEffect(() => setValue(brief), [brief, projectId]);

  const onChange = (next: string) => {
    setValue(next);
    if (timer.current) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => saveBrief(projectId, next), 600);
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
