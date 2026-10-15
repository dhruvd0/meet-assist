import React, { useEffect } from "react";
import { useTranslation } from "react-i18next";
import type { Meeting } from "@/bindings";
import { useMeetingStore } from "@/stores/meetingStore";
import { useProjectStore } from "@/stores/projectStore";
import Badge from "../ui/Badge";
import { Select } from "../ui/Select";

const UNFILED = "unfiled";

/** The meeting's project, changeable by the user; marks assistant filings. */
export const MeetingProjectPicker: React.FC<{ meeting: Meeting }> = ({
  meeting,
}) => {
  const { t } = useTranslation();
  const projects = useProjectStore((s) => s.projects);
  const loadProjects = useProjectStore((s) => s.loadProjects);
  const moveMeeting = useMeetingStore((s) => s.moveMeeting);

  useEffect(() => {
    loadProjects();
  }, [loadProjects]);

  const options = [
    { value: UNFILED, label: t("meetings.filing.unfiled") },
    ...projects.map((p) => ({ value: String(p.id), label: p.title })),
  ];

  return (
    <div className="flex items-center gap-2">
      <span className="text-xs font-medium text-mid-gray uppercase tracking-wide">
        {t("meetings.filing.project")}
      </span>
      <Select
        className="w-64"
        value={
          meeting.project_id === null ? UNFILED : String(meeting.project_id)
        }
        options={options}
        onChange={(value) => {
          const next =
            value === null || value === UNFILED ? null : Number(value);
          if (next !== meeting.project_id) moveMeeting(meeting.id, next);
        }}
      />
      {meeting.filed_by === "ai" && (
        <Badge variant="secondary">{t("meetings.filing.byAssistant")}</Badge>
      )}
    </div>
  );
};
