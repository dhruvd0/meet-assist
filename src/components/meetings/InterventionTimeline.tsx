import React from "react";
import { useTranslation } from "react-i18next";
import type { Intervention } from "@/bindings";
import { useMeetingStore } from "@/stores/meetingStore";
import Badge from "../ui/Badge";
import { Button } from "../ui/Button";
import { SettingsGroup } from "../ui/SettingsGroup";
import { formatDateTime } from "@/utils/dateFormat";

export const InterventionTimeline: React.FC<{ items: Intervention[] }> = ({
  items,
}) => {
  const { t, i18n } = useTranslation();
  const dismiss = useMeetingStore((s) => s.dismiss);
  return (
    <SettingsGroup title={t("meetings.intervention.timeline")}>
      {items.length === 0 ? (
        <p className="p-4 text-sm text-mid-gray">
          {t("meetings.intervention.empty")}
        </p>
      ) : (
        items.map((iv) => (
          <div key={iv.id} className="p-4 flex items-start gap-3">
            <Badge variant={iv.dismissed ? "secondary" : "primary"}>
              {t(`meetings.intervention.kind.${iv.kind}`, {
                defaultValue: iv.kind,
              })}
            </Badge>
            <div className="flex-1 min-w-0">
              <div className="text-sm text-text whitespace-pre-line">
                {iv.text}
              </div>
              <div className="text-xs text-mid-gray">
                {formatDateTime(
                  String(Math.floor(iv.ts / 1000)),
                  i18n.language,
                )}
              </div>
            </div>
            {!iv.dismissed && (
              <Button size="sm" variant="ghost" onClick={() => dismiss(iv.id)}>
                {t("meetings.intervention.dismiss")}
              </Button>
            )}
          </div>
        ))
      )}
    </SettingsGroup>
  );
};
