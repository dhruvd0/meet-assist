import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { FolderKanban, Plus, Trash2 } from "lucide-react";
import { useProjectStore } from "@/stores/projectStore";
import { Button } from "../ui/Button";
import { Input } from "../ui/Input";
import { SettingsGroup } from "../ui/SettingsGroup";
import { formatRelativeTime } from "@/utils/dateFormat";

export const ProjectList: React.FC = () => {
  const { t, i18n } = useTranslation();
  const projects = useProjectStore((s) => s.projects);
  const loading = useProjectStore((s) => s.loading);
  const createProject = useProjectStore((s) => s.createProject);
  const openProject = useProjectStore((s) => s.openProject);
  const deleteProject = useProjectStore((s) => s.deleteProject);
  const [title, setTitle] = useState("");

  const submit = async () => {
    const name = title.trim();
    if (!name) return;
    const created = await createProject(name);
    if (created) {
      setTitle("");
      openProject(created.id);
    }
  };

  return (
    <div className="space-y-6">
      <SettingsGroup
        title={t("projects.title")}
        description={t("projects.description")}
      >
        <div className="flex items-center gap-2 p-4">
          <Input
            variant="compact"
            className="flex-1"
            placeholder={t("projects.newPlaceholder")}
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") submit();
            }}
          />
          <Button size="sm" onClick={submit} disabled={!title.trim()}>
            <Plus className="w-4 h-4 inline mr-1" />
            {t("projects.create")}
          </Button>
        </div>
      </SettingsGroup>

      <SettingsGroup>
        {loading && projects.length === 0 ? (
          <p className="p-4 text-sm text-mid-gray">{t("common.loading")}</p>
        ) : projects.length === 0 ? (
          <p className="p-4 text-sm text-mid-gray">{t("projects.empty")}</p>
        ) : (
          projects.map((p) => (
            <div key={p.id} className="flex items-center gap-3 p-4">
              <FolderKanban className="w-5 h-5 text-logo-primary shrink-0" />
              <button
                className="flex-1 text-left cursor-pointer"
                onClick={() => openProject(p.id)}
              >
                <div className="text-sm font-semibold text-text">{p.title}</div>
                <div className="text-xs text-mid-gray">
                  {formatRelativeTime(
                    String(Math.floor(p.updated_at / 1000)),
                    i18n.language,
                  )}
                </div>
              </button>
              <Button
                variant="danger-ghost"
                size="sm"
                title={t("projects.delete")}
                onClick={() => {
                  if (window.confirm(t("projects.deleteConfirm"))) {
                    deleteProject(p.id);
                  }
                }}
              >
                <Trash2 className="w-4 h-4" />
              </Button>
            </div>
          ))
        )}
      </SettingsGroup>
    </div>
  );
};
