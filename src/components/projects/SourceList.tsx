import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Loader2,
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
import { DocSection } from "../shared";
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
  const [saving, setSaving] = useState(false);
  useEffect(() => setValue(source.digest ?? ""), [source.digest]);
  const unchanged = value.trim() === (source.digest ?? "").trim();
  const save = async () => {
    setSaving(true);
    try {
      await setCaption(source.id, value);
    } finally {
      setSaving(false);
    }
  };
  return (
    <div className="mt-2 space-y-1">
      <div className="flex items-center gap-2">
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
          disabled={saving || unchanged}
          onClick={save}
        >
          {saving && <Loader2 className="w-3.5 h-3.5 animate-spin" />}
          {t("projects.sources.saveCaption")}
        </Button>
      </div>
    </div>
  );
};

export const SourceList: React.FC<{ projectId: number }> = ({ projectId }) => {
  const { t } = useTranslation();
  const sources = useProjectStore((s) => s.sources);
  const sourcesState = useProjectStore((s) => s.sourcesState);
  const sourcesError = useProjectStore((s) => s.sourcesError);
  const loadSources = useProjectStore((s) => s.loadSources);
  const [busy, setBusy] = useState<Record<number, "reindex" | "remove">>({});
  const run = async (
    id: number,
    kind: "reindex" | "remove",
    action: (id: number) => Promise<void>,
  ) => {
    setBusy((b) => ({ ...b, [id]: kind }));
    try {
      await action(id);
    } finally {
      setBusy(({ [id]: _, ...rest }) => rest);
    }
  };
  const removeSource = useProjectStore((s) => s.removeSource);
  const reindexSource = useProjectStore((s) => s.reindexSource);
  const [open, setOpen] = useState(false);

  return (
    <DocSection
      title={t("projects.sources.title")}
      description={t("projects.sources.description")}
      action={
        <Button size="sm" variant="secondary" onClick={() => setOpen(true)}>
          <Plus className="w-3.5 h-3.5" />
          {t("projects.sources.add")}
        </Button>
      }
    >
      {sourcesState === "loading" && sources.length === 0 ? (
        <p
          role="status"
          className="py-2 flex items-center gap-2 text-sm text-faint"
        >
          <Loader2 className="w-4 h-4 animate-spin" />
          {t("common.loading")}
        </p>
      ) : sourcesState === "error" ? (
        <div className="py-2 flex items-center gap-3">
          <p role="alert" className="text-sm text-error">
            {sourcesError}
          </p>
          <Button
            size="sm"
            variant="secondary"
            onClick={() => loadSources(projectId)}
          >
            {t("meetings.summary.retry")}
          </Button>
        </div>
      ) : sources.length === 0 ? (
        <p className="py-2 text-sm text-faint">{t("projects.sources.empty")}</p>
      ) : (
        sources.map((s) => {
          const Icon = KIND_ICON[s.kind];
          return (
            <div
              key={s.id}
              className="group -mx-3 px-3 py-2.5 rounded-xl hover:bg-surface transition-colors"
            >
              <div className="flex items-center gap-3">
                <span className="w-8 h-8 shrink-0 inline-flex items-center justify-center rounded-lg border border-line bg-surface text-faint">
                  <Icon className="w-4 h-4" />
                </span>
                <div className="flex-1 min-w-0">
                  <div className="text-sm text-text truncate">{s.title}</div>
                  <div className="text-xs text-faint truncate">
                    {s.kind === "text"
                      ? t("projects.sources.kind.text")
                      : s.locator}
                  </div>
                  {s.status === "failed" && s.error && (
                    <div className="text-xs text-error mt-1">{s.error}</div>
                  )}
                </div>
                <Badge variant={statusVariant(s.status)}>
                  {t(`projects.sources.status.${s.status}`)}
                </Badge>
                <Button
                  variant="ghost"
                  size="sm"
                  title={t("projects.sources.reindex")}
                  disabled={s.status === "pending" || busy[s.id] !== undefined}
                  onClick={() => run(s.id, "reindex", reindexSource)}
                >
                  <RefreshCw
                    className={`w-4 h-4 ${
                      s.status === "pending" || busy[s.id] === "reindex"
                        ? "animate-spin"
                        : ""
                    }`}
                  />
                </Button>
                <Button
                  variant="danger-ghost"
                  size="sm"
                  title={t("projects.sources.remove")}
                  disabled={busy[s.id] !== undefined}
                  onClick={() => run(s.id, "remove", removeSource)}
                >
                  {busy[s.id] === "remove" ? (
                    <Loader2 className="w-4 h-4 animate-spin" />
                  ) : (
                    <Trash2 className="w-4 h-4" />
                  )}
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
    </DocSection>
  );
};
