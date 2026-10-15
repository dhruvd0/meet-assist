import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import {
  FolderInput,
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

/** `MeetingFiledPayload` from the backend. */
type Filed = {
  meeting_id: number;
  project_id: number | null;
  project_title: string | null;
  meeting_title: string;
  by: string;
  created_project: boolean;
  created_brief: string | null;
};

type Card =
  | { type: "nudge"; iv: Intervention }
  | { type: "filed"; filed: Filed };

const ICON: Record<string, React.ComponentType<{ className?: string }>> = {
  loop: Repeat,
  stuck: MessageCircleQuestion,
  info: Info,
  idea: Lightbulb,
};

const AUTO_HIDE_MS = 30_000;

export const InterventionCard: React.FC = () => {
  const { t } = useTranslation();
  const [card, setCard] = useState<Card | null>(null);
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
    const show = (next: Card) => {
      hovered.current = false;
      setCard(next);
      arm();
    };
    const unNudge = listen<Intervention>("intervention:show", (e) =>
      show({ type: "nudge", iv: e.payload }),
    );
    const unFiled = listen<Filed>("intervention:filed", (e) =>
      show({ type: "filed", filed: e.payload }),
    );
    unNudge.catch(console.error);
    unFiled.catch(console.error);
    return () => {
      unNudge.then((f) => f()).catch(console.error);
      unFiled.then((f) => f()).catch(console.error);
      if (timer.current) window.clearTimeout(timer.current);
    };
  }, []);

  if (!card) return null;

  const meetingId =
    card.type === "nudge" ? card.iv.meeting_id : card.filed.meeting_id;
  const Icon =
    card.type === "nudge" ? (ICON[card.iv.kind] ?? Info) : FolderInput;
  const label =
    card.type === "nudge"
      ? t(`meetings.intervention.kind.${card.iv.kind}`)
      : t("meetings.filing.card.title");
  const body =
    card.type === "nudge"
      ? card.iv.text
      : t(
          card.filed.created_project
            ? "meetings.filing.card.created"
            : "meetings.filing.card.filedInto",
          {
            project: card.filed.project_title ?? "",
            title: card.filed.meeting_title,
          },
        );

  const onDismiss = async () => {
    if (card.type === "nudge") {
      try {
        await invoke("dismiss_intervention", { id: card.iv.id });
      } catch (err) {
        console.error("dismiss_intervention failed", err);
      }
    }
    await hide();
  };

  const openMeeting = async () => {
    try {
      await invoke("focus_meeting", { meetingId });
    } catch (err) {
      console.error("focus_meeting failed", err);
      await hide();
    }
  };

  const undo = async (filed: Filed) => {
    try {
      await invoke("undo_filing", {
        meetingId: filed.meeting_id,
        createdProjectId: filed.created_project ? filed.project_id : null,
        createdBrief: filed.created_brief,
      });
    } catch (err) {
      console.error("undo_filing failed", err);
      await hide();
    }
  };

  const action =
    "text-xs font-medium text-logo-primary hover:underline cursor-pointer";

  return (
    <div
      className="h-screen w-screen p-3 bg-background text-text rounded-xl border border-mid-gray/30 shadow-lg flex flex-col gap-2"
      onMouseEnter={() => (hovered.current = true)}
      onMouseLeave={() => (hovered.current = false)}
    >
      <div className="flex items-center gap-2">
        <Icon className="w-4 h-4 text-logo-primary shrink-0" />
        <span className="text-xs font-medium uppercase tracking-wide text-mid-gray flex-1">
          {label}
        </span>
        <button
          type="button"
          aria-label={t("meetings.intervention.dismiss")}
          className="text-mid-gray hover:text-text cursor-pointer"
          title={t("meetings.intervention.dismiss")}
          onClick={onDismiss}
        >
          <X className="w-4 h-4" aria-hidden="true" />
        </button>
      </div>
      <p className="text-sm leading-snug flex-1 overflow-hidden">{body}</p>
      <div className="flex justify-end gap-3">
        {card.type === "filed" ? (
          <>
            <button
              type="button"
              className={action}
              onClick={() => undo(card.filed)}
            >
              {t("meetings.filing.card.undo")}
            </button>
            <button type="button" className={action} onClick={openMeeting}>
              {t("meetings.filing.card.change")}
            </button>
          </>
        ) : (
          <button type="button" className={action} onClick={openMeeting}>
            {t("meetings.intervention.open")}
          </button>
        )}
      </div>
    </div>
  );
};
