import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import {
  Lightbulb,
  MessageCircleQuestion,
  Repeat,
  Info,
  X,
} from "lucide-react";

type Intervention = {
  id: number;
  session_id: number;
  ts: number;
  kind: string;
  text: string;
  reason: string;
  dismissed: boolean;
  /** Added by the backend so the card can open its meeting. */
  meeting_id: number;
};

const ICON: Record<string, React.ComponentType<{ className?: string }>> = {
  loop: Repeat,
  stuck: MessageCircleQuestion,
  info: Info,
  idea: Lightbulb,
};

const AUTO_HIDE_MS = 30_000;

export const InterventionCard: React.FC = () => {
  const { t } = useTranslation();
  const [iv, setIv] = useState<Intervention | null>(null);
  const hovered = useRef(false);
  const timer = useRef<number | null>(null);

  const hide = async () => {
    if (timer.current) window.clearTimeout(timer.current);
    hovered.current = false;
    try {
      await invoke("hide_intervention_window");
    } catch (err) {
      console.error("hide_intervention_window failed", err);
    }
  };

  const arm = () => {
    if (timer.current) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      if (hovered.current) arm();
      else hide();
    }, AUTO_HIDE_MS);
  };

  useEffect(() => {
    const un = listen<Intervention>("intervention:show", (e) => {
      hovered.current = false;
      setIv(e.payload);
      arm();
    });
    un.catch(console.error);
    return () => {
      un.then((f) => f()).catch(console.error);
      if (timer.current) window.clearTimeout(timer.current);
    };
  }, []);

  if (!iv) return null;
  const Icon = ICON[iv.kind] ?? Info;
  return (
    <div
      className="h-screen w-screen p-3 bg-background text-text rounded-xl border border-mid-gray/30 shadow-lg flex flex-col gap-2"
      onMouseEnter={() => (hovered.current = true)}
      onMouseLeave={() => (hovered.current = false)}
    >
      <div className="flex items-center gap-2">
        <Icon className="w-4 h-4 text-logo-primary shrink-0" />
        <span className="text-xs font-medium uppercase tracking-wide text-mid-gray flex-1">
          {t(`meetings.intervention.kind.${iv.kind}`)}
        </span>
        <button
          type="button"
          aria-label={t("meetings.intervention.dismiss")}
          className="text-mid-gray hover:text-text cursor-pointer"
          title={t("meetings.intervention.dismiss")}
          onClick={async () => {
            try {
              await invoke("dismiss_intervention", { id: iv.id });
            } catch (err) {
              console.error("dismiss_intervention failed", err);
            } finally {
              await hide();
            }
          }}
        >
          <X className="w-4 h-4" aria-hidden="true" />
        </button>
      </div>
      <p className="text-sm leading-snug flex-1 overflow-hidden">{iv.text}</p>
      <div className="flex justify-end">
        <button
          type="button"
          className="text-xs font-medium text-logo-primary hover:underline cursor-pointer"
          onClick={async () => {
            try {
              await invoke("focus_meeting", { meetingId: iv.meeting_id });
            } catch (err) {
              console.error("focus_meeting failed", err);
              await hide();
            }
          }}
        >
          {t("meetings.intervention.open")}
        </button>
      </div>
    </div>
  );
};
