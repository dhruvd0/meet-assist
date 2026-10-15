import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { ask } from "@tauri-apps/plugin-dialog";
import { Loader2, Mic, Trash2 } from "lucide-react";
import { useProjectStore } from "@/stores/projectStore";
import { useMeetingStore } from "@/stores/meetingStore";
import { Button } from "../ui/Button";
import { DocSection } from "../shared";
import { BriefEditor } from "./BriefEditor";
import { SourceList } from "./SourceList";
import { MeetingFeed } from "../meetings/MeetingFeed";

/** A space (project): its brief, sources, and meetings, set as one page. */
export const ProjectDetail: React.FC<{ onDeleted: () => void }> = ({
  onDeleted,
}) => {
  const { t } = useTranslation();
  const project = useProjectStore((s) =>
    s.projects.find((p) => p.id === s.activeProjectId),
  );
  const renameProject = useProjectStore((s) => s.renameProject);
  const deleteProject = useProjectStore((s) => s.deleteProject);
  const meetings = useMeetingStore((s) => s.meetings);
  const live = useMeetingStore((s) => s.live);
  const startMeeting = useMeetingStore((s) => s.startMeeting);
  const pending = useMeetingStore((s) => s.controlPending);
  const projectsLoading = useProjectStore((s) => s.loading);
  const projectsError = useProjectStore((s) => s.projectsError);
  const [editing, setEditing] = useState(false);
  const [title, setTitle] = useState("");
  const [deleting, setDeleting] = useState(false);

  if (!project)
    return projectsLoading ? (
      <p
        role="status"
        className="p-8 flex items-center justify-center gap-2 text-sm text-faint"
      >
        <Loader2 className="w-4 h-4 animate-spin" />
        {t("common.loading")}
      </p>
    ) : (
      <p role="alert" className="p-8 text-center text-sm text-error">
        {projectsError ?? t("projects.errors.notFound")}
      </p>
    );

  const commit = () => {
    const next = title.trim();
    if (next && next !== project.title) renameProject(project.id, next);
    setEditing(false);
  };

  return (
    <div
      key={project.id}
      className="w-full max-w-[720px] mx-auto px-8 pt-6 pb-32 page-in"
    >
      <div className="flex items-start gap-3">
        {editing ? (
          <input
            autoFocus
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            onBlur={commit}
            onKeyDown={(e) => {
              if (e.key === "Enter") commit();
              if (e.key === "Escape") setEditing(false);
            }}
            className="flex-1 min-w-0 -mx-2 px-2 rounded-lg bg-surface font-serif text-[32px] leading-tight tracking-[-0.01em] text-text focus:outline-none"
          />
        ) : (
          <h1
            title={t("projects.rename")}
            onClick={() => {
              setTitle(project.title);
              setEditing(true);
            }}
            className="flex-1 min-w-0 font-serif text-[32px] leading-tight tracking-[-0.01em] text-text cursor-text break-words"
          >
            {project.title}
          </h1>
        )}
        <div className="flex items-center gap-1.5 pt-1.5 shrink-0">
          <button
            type="button"
            title={t("projects.delete")}
            aria-label={t("projects.delete")}
            className="w-8 h-8 inline-flex items-center justify-center rounded-lg text-faint hover:text-error hover:bg-error/10 transition-colors cursor-pointer"
            disabled={deleting}
            onClick={async () => {
              const ok = await ask(t("projects.deleteConfirm"), {
                title: t("projects.delete"),
                kind: "warning",
              });
              if (!ok) return;
              setDeleting(true);
              try {
                if (await deleteProject(project.id)) onDeleted();
              } finally {
                setDeleting(false);
              }
            }}
          >
            {deleting ? (
              <Loader2 className="w-4 h-4 animate-spin" />
            ) : (
              <Trash2 className="w-4 h-4" />
            )}
          </button>
          <Button
            disabled={live.meeting_id !== null || pending !== null}
            onClick={() => startMeeting(project.id, null)}
          >
            {pending === "start" ? (
              <Loader2 className="w-3.5 h-3.5 animate-spin" />
            ) : (
              <Mic className="w-3.5 h-3.5" />
            )}
            {t("meetings.newInProject")}
          </Button>
        </div>
      </div>

      <BriefEditor projectId={project.id} brief={project.brief} />
      <SourceList projectId={project.id} />
      <DocSection title={t("meetings.title")}>
        <div className="-mx-3">
          <MeetingFeed
            inSpace
            meetings={meetings.filter((m) => m.project_id === project.id)}
            emptyText={t("projects.meetingsEmpty")}
          />
        </div>
      </DocSection>
    </div>
  );
};
