import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowLeft, Pencil } from "lucide-react";
import { useProjectStore } from "@/stores/projectStore";
import { Button } from "../ui/Button";
import { Input } from "../ui/Input";
import { BriefEditor } from "./BriefEditor";
import { SourceList } from "./SourceList";

export const ProjectDetail: React.FC = () => {
  const { t } = useTranslation();
  const activeProjectId = useProjectStore((s) => s.activeProjectId);
  const project = useProjectStore((s) =>
    s.projects.find((p) => p.id === s.activeProjectId),
  );
  const closeProject = useProjectStore((s) => s.closeProject);
  const renameProject = useProjectStore((s) => s.renameProject);
  const [editing, setEditing] = useState(false);
  const [title, setTitle] = useState(project?.title ?? "");

  if (activeProjectId === null || !project) return null;

  const commit = () => {
    const next = title.trim();
    if (next && next !== project.title) renameProject(project.id, next);
    setEditing(false);
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center gap-2">
        <Button
          variant="ghost"
          size="sm"
          onClick={closeProject}
          title={t("projects.back")}
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
            {project.title}
          </h1>
        )}
        <Button
          variant="ghost"
          size="sm"
          title={t("projects.rename")}
          onClick={() => {
            setTitle(project.title);
            setEditing(true);
          }}
        >
          <Pencil className="w-4 h-4" />
        </Button>
      </div>
      <BriefEditor projectId={project.id} brief={project.brief} />
      <SourceList projectId={project.id} />
    </div>
  );
};
