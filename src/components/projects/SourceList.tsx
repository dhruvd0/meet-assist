import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  FileText,
  Folder,
  Image as ImageIcon,
  Link as LinkIcon,
  Plus,
  RefreshCw,
  Trash2,
  Type,
} from "lucide-react";
import type { Source, SourceKind } from "@/bindings";
import { useProjectStore } from "@/stores/projectStore";
import Badge from "../ui/Badge";
import { Button } from "../ui/Button";
import { Input } from "../ui/Input";
import { SettingsGroup } from "../ui/SettingsGroup";
import { AddSourceDialog } from "./AddSourceDialog";

const KIND_ICON: Record<
  SourceKind,
  React.ComponentType<{ className?: string }>
> = {
  text: Type,
  file: FileText,
  folder: Folder,
  link: LinkIcon,
  image: ImageIcon,
};

const statusVariant = (s: Source["status"]) =>
  s === "ready" ? "success" : s === "failed" ? "primary" : "secondary";

const CaptionRow: React.FC<{ source: Source }> = ({ source }) => {
  const { t } = useTranslation();
  const setCaption = useProjectStore((s) => s.setCaption);
  const [value, setValue] = useState(source.digest ?? "");
  useEffect(() => setValue(source.digest ?? ""), [source.digest]);
  return (
    <div className="flex items-center gap-2 mt-2">
      <Input
        variant="compact"
        className="flex-1"
        placeholder={t("projects.sources.captionPlaceholder")}
        value={value}
        onChange={(e) => setValue(e.target.value)}
      />
      <Button
        size="sm"
        variant="secondary"
        onClick={() => setCaption(source.id, value)}
      >
        {t("projects.sources.saveCaption")}
      </Button>
    </div>
  );
};

export const SourceList: React.FC<{ projectId: number }> = ({ projectId }) => {
  const { t } = useTranslation();
  const sources = useProjectStore((s) => s.sources);
  const removeSource = useProjectStore((s) => s.removeSource);
  const reindexSource = useProjectStore((s) => s.reindexSource);
  const [open, setOpen] = useState(false);

  return (
    <SettingsGroup
      title={t("projects.sources.title")}
      description={t("projects.sources.description")}
    >
      <div className="flex justify-end p-3">
        <Button size="sm" onClick={() => setOpen(true)}>
          <Plus className="w-4 h-4 inline mr-1" />
          {t("projects.sources.add")}
        </Button>
      </div>
      {sources.length === 0 ? (
        <p className="p-4 text-sm text-mid-gray">
          {t("projects.sources.empty")}
        </p>
      ) : (
        sources.map((s) => {
          const Icon = KIND_ICON[s.kind];
          return (
            <div key={s.id} className="p-4">
              <div className="flex items-center gap-3">
                <Icon className="w-4 h-4 text-logo-primary shrink-0" />
                <div className="flex-1 min-w-0">
                  <div className="text-sm font-semibold text-text truncate">
                    {s.title}
                  </div>
                  <div className="text-xs text-mid-gray truncate">
                    {s.kind === "text"
                      ? t("projects.sources.kind.text")
                      : s.locator}
                  </div>
                  {s.status === "failed" && s.error && (
                    <div className="text-xs text-red-500 mt-1">{s.error}</div>
                  )}
                </div>
                <Badge variant={statusVariant(s.status)}>
                  {t(`projects.sources.status.${s.status}`)}
                </Badge>
                <Button
                  variant="ghost"
                  size="sm"
                  title={t("projects.sources.reindex")}
                  onClick={() => reindexSource(s.id)}
                >
                  <RefreshCw className="w-4 h-4" />
                </Button>
                <Button
                  variant="danger-ghost"
                  size="sm"
                  title={t("projects.sources.remove")}
                  onClick={() => removeSource(s.id)}
                >
                  <Trash2 className="w-4 h-4" />
                </Button>
              </div>
              {s.kind === "image" && <CaptionRow source={s} />}
            </div>
          );
        })
      )}
      <AddSourceDialog
        projectId={projectId}
        open={open}
        onOpenChange={setOpen}
      />
    </SettingsGroup>
  );
};
