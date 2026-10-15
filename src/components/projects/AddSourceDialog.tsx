import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type SourceKind } from "@/bindings";
import { useProjectStore } from "@/stores/projectStore";
import { Button } from "../ui/Button";
import { Dialog } from "../ui/Dialog";
import { Input } from "../ui/Input";
import { Textarea } from "../ui/Textarea";

const KINDS: SourceKind[] = ["text", "file", "folder", "link", "image"];

export const AddSourceDialog: React.FC<{
  projectId: number;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}> = ({ projectId, open, onOpenChange }) => {
  const { t } = useTranslation();
  const addSource = useProjectStore((s) => s.addSource);
  const [kind, setKind] = useState<SourceKind>("text");
  const [title, setTitle] = useState("");
  const [locator, setLocator] = useState("");
  const [busy, setBusy] = useState(false);

  const reset = () => {
    setKind("text");
    setTitle("");
    setLocator("");
  };

  const pick = async () => {
    const res = await commands.pickSourcePath(kind);
    if (res.status === "ok" && res.data) {
      setLocator(res.data);
      if (!title) setTitle(res.data.split(/[\\/]/).pop() ?? "");
    }
  };

  const submit = async () => {
    if (!locator.trim()) return;
    setBusy(true);
    const created = await addSource(
      projectId,
      kind,
      title.trim() || t(`projects.sources.kind.${kind}`),
      locator,
    );
    setBusy(false);
    if (created) {
      reset();
      onOpenChange(false);
    }
  };

  const needsPicker = kind === "file" || kind === "folder" || kind === "image";

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o) reset();
        onOpenChange(o);
      }}
      title={t("projects.sources.dialog.title")}
      closeLabel={t("projects.sources.dialog.close")}
      footer={
        <div className="flex justify-end gap-2">
          <Button
            variant="secondary"
            size="sm"
            onClick={() => onOpenChange(false)}
          >
            {t("projects.sources.dialog.cancel")}
          </Button>
          <Button size="sm" onClick={submit} disabled={busy || !locator.trim()}>
            {t("projects.sources.dialog.add")}
          </Button>
        </div>
      }
    >
      <div className="space-y-3">
        <label className="block text-xs text-muted">
          {t("projects.sources.dialog.kind")}
          <div className="flex gap-2 mt-1 flex-wrap">
            {KINDS.map((k) => (
              <Button
                key={k}
                size="sm"
                variant={k === kind ? "primary-soft" : "secondary"}
                onClick={() => {
                  setKind(k);
                  setLocator("");
                }}
              >
                {t(`projects.sources.kind.${k}`)}
              </Button>
            ))}
          </div>
        </label>

        <label className="block text-xs text-muted">
          {t("projects.sources.dialog.titleLabel")}
          <Input
            variant="compact"
            className="w-full mt-1"
            placeholder={t("projects.sources.dialog.titlePlaceholder")}
            value={title}
            onChange={(e) => setTitle(e.target.value)}
          />
        </label>

        {kind === "text" && (
          <label className="block text-xs text-muted">
            {t("projects.sources.dialog.textLabel")}
            <Textarea
              variant="compact"
              className="w-full mt-1"
              placeholder={t("projects.sources.dialog.textPlaceholder")}
              value={locator}
              onChange={(e) => setLocator(e.target.value)}
            />
          </label>
        )}

        {kind === "link" && (
          <label className="block text-xs text-muted">
            {t("projects.sources.dialog.linkLabel")}
            <Input
              variant="compact"
              className="w-full mt-1"
              placeholder={t("projects.sources.dialog.linkPlaceholder")}
              value={locator}
              onChange={(e) => setLocator(e.target.value)}
            />
          </label>
        )}

        {needsPicker && (
          <div className="text-xs text-muted">
            {t("projects.sources.dialog.pathLabel")}
            <div className="flex items-center gap-2 mt-1">
              <Input
                variant="compact"
                className="flex-1"
                value={locator}
                readOnly
              />
              <Button size="sm" variant="secondary" onClick={pick}>
                {t("projects.sources.dialog.pick")}
              </Button>
            </div>
          </div>
        )}
      </div>
    </Dialog>
  );
};
