import React from "react";
import { useTranslation } from "react-i18next";
import {
  Info,
  Lightbulb,
  MessageCircleQuestion,
  Repeat,
  X,
} from "lucide-react";
import type { Intervention } from "@/bindings";
import { NudgeText } from "./NudgeText";
import { useMeetingStore } from "@/stores/meetingStore";

const ICON: Record<string, React.ComponentType<{ className?: string }>> = {
  loop: Repeat,
  stuck: MessageCircleQuestion,
  info: Info,
  idea: Lightbulb,
};

/**
 * The assistant's nudges for this meeting. `compact` is the right-rail form:
 * newest first, smaller type.
 */
export const InterventionTimeline: React.FC<{
  items: Intervention[];
  compact?: boolean;
}> = ({ items, compact = false }) => {
  const { t, i18n } = useTranslation();
  const dismiss = useMeetingStore((s) => s.dismiss);

  if (items.length === 0)
    return (
      <p className="py-1 text-sm text-faint">
        {t("meetings.intervention.empty")}
      </p>
    );

  return (
    <ul className="space-y-2">
      {(compact ? [...items].reverse() : items).map((iv) => {
        const Icon = ICON[iv.kind] ?? Info;
        return (
          <li
            key={iv.id}
            className={`group flex items-start rounded-xl border ${compact ? "gap-2.5 px-3 py-2.5" : "gap-3 px-3.5 py-3"} ${
              iv.dismissed
                ? "border-line bg-transparent"
                : "border-accent/25 bg-accent/[0.06]"
            }`}
          >
            <Icon
              className={`w-4 h-4 mt-[3px] shrink-0 ${iv.dismissed ? "text-faint" : "text-accent"}`}
            />
            <div className="flex-1 min-w-0">
              <div className="flex items-baseline gap-2 text-xs">
                <span
                  className={`font-medium ${iv.dismissed ? "text-faint" : "text-text"}`}
                >
                  {t(`meetings.intervention.kind.${iv.kind}`, {
                    defaultValue: iv.kind,
                  })}
                </span>
                <span className="font-mono text-[11px] text-faint tabular">
                  {new Intl.DateTimeFormat(i18n.language, {
                    hour: "numeric",
                    minute: "2-digit",
                  }).format(iv.ts)}
                </span>
              </div>
              <NudgeText
                text={iv.text}
                className={`mt-0.5 ${compact ? "text-[13.5px] leading-[1.45rem]" : "text-[14.5px] leading-6"} ${iv.dismissed ? "text-muted" : "text-text"}`}
              />
            </div>
            {!iv.dismissed && (
              <button
                type="button"
                onClick={() => dismiss(iv.id)}
                title={t("meetings.intervention.dismiss")}
                aria-label={t("meetings.intervention.dismiss")}
                className="w-7 h-7 -me-1.5 -mt-1 shrink-0 inline-flex items-center justify-center rounded-lg text-faint hover:text-text hover:bg-surface-hover transition-colors cursor-pointer"
              >
                <X className="w-3.5 h-3.5" />
              </button>
            )}
          </li>
        );
      })}
    </ul>
  );
};
