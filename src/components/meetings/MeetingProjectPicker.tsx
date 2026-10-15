import React from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, Folder, Sparkles } from "lucide-react";
import type { Meeting } from "@/bindings";
import { useMeetingStore } from "@/stores/meetingStore";
import { useProjectStore } from "@/stores/projectStore";

const UNFILED = "unfiled";

/**
 * The meeting's space as a chip; a native select sits over it so the menu is
 * the platform's own. Marks meetings the assistant filed.
 */
export const MeetingProjectPicker: React.FC<{ meeting: Meeting }> = ({
  meeting,
}) => {
  const { t } = useTranslation();
  const projects = useProjectStore((s) => s.projects);
  const moveMeeting = useMeetingStore((s) => s.moveMeeting);

  const current = projects.find((p) => p.id === meeting.project_id);
  const label = current?.title ?? t("meetings.filing.unfiled");

  return (
    <label
      className="relative inline-flex items-center gap-1.5 h-8 ps-3 pe-2.5 rounded-full border border-line text-[13px] text-muted hover:text-text hover:border-line-strong focus-within:border-accent/70 transition-colors cursor-pointer"
      title={
        meeting.filed_by === "ai"
          ? t("meetings.filing.byAssistant")
          : t("meetings.filing.project")
      }
    >
      {meeting.filed_by === "ai" ? (
        <Sparkles className="w-3.5 h-3.5 text-accent" />
      ) : (
        <Folder className="w-3.5 h-3.5" />
      )}
      <span className="max-w-[14rem] truncate">{label}</span>
      <ChevronDown className="w-3.5 h-3.5 text-faint" />
      <select
        aria-label={t("meetings.filing.project")}
        className="absolute inset-0 opacity-0 cursor-pointer"
        value={
          meeting.project_id === null ? UNFILED : String(meeting.project_id)
        }
        onChange={(e) => {
          const v = e.target.value;
          const next = v === UNFILED ? null : Number(v);
          if (next !== meeting.project_id) moveMeeting(meeting.id, next);
        }}
      >
        <option value={UNFILED}>{t("meetings.filing.unfiled")}</option>
        {projects.map((p) => (
          <option key={p.id} value={String(p.id)}>
            {p.title}
          </option>
        ))}
      </select>
    </label>
  );
};
