import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import {
  Loader2,
  FolderInput,
  Lightbulb,
  MessageCircleQuestion,
  Repeat,
  Info,
  X,
} from "lucide-react";
import { NudgeText } from "@/components/meetings/NudgeText";

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
const WIDTH = 380;
const MAX_HEIGHT = 320;

export const InterventionCard: React.FC = () => {
  const { t } = useTranslation();
  const [card, setCard] = useState<Card | null>(null);
  const hovered = useRef(false);
  /** The card action still running, and the last action's failure. */
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const timer = useRef<number | null>(null);
  const root = useRef<HTMLDivElement>(null);

  // The window fits the card: a one-line nudge stays compact, a checklist
  // gets the room it needs.
  useEffect(() => {
    const el = root.current;
    if (!el) return;
    const fit = () => {
      const height = Math.min(Math.ceil(el.scrollHeight), MAX_HEIGHT);
      getCurrentWindow()
        .setSize(new LogicalSize(WIDTH, height))
        .catch(console.error);
    };
    fit();
    const observer = new ResizeObserver(fit);
    observer.observe(el);
    return () => observer.disconnect();
  }, [card]);

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
      setFailure(null);
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

  // Runs one card action; on failure the card stays open with the reason
  // instead of closing as if it had worked.
  const act = async (run: () => Promise<unknown>, failKey: string) => {
    if (busy) return;
    setBusy(true);
    setFailure(null);
    hovered.current = true;
    try {
      await run();
      return true;
    } catch (err) {
      console.error(failKey, err);
      setFailure(t(failKey, { error: String(err) }));
      return false;
    } finally {
      setBusy(false);
    }
  };

  const onDismiss = async () => {
    // A failed dismiss still closes the card: the nudge stays visible in
    // the meeting's timeline, where it can be dismissed again.
    if (card.type === "nudge")
      await act(
        () => invoke("dismiss_intervention", { id: card.iv.id }),
        "meetings.intervention.errors.dismiss",
      );
    await hide();
  };

  const openMeeting = () =>
    act(
      () => invoke("focus_meeting", { meetingId }),
      "meetings.intervention.errors.open",
    );

  const undo = (filed: Filed) =>
    act(
      () =>
        invoke("undo_filing", {
          meetingId: filed.meeting_id,
          createdProjectId: filed.created_project ? filed.project_id : null,
          createdBrief: filed.created_brief,
        }),
      "meetings.filing.card.undoFailed",
    );

  const action =
    "text-xs font-medium text-accent hover:underline cursor-pointer disabled:opacity-50 disabled:cursor-default";

  return (
    <div
      ref={root}
      className="w-screen max-h-screen overflow-hidden px-3.5 pt-3 pb-2.5 bg-background text-text rounded-xl border border-line shadow-lg flex flex-col gap-1.5"
      onMouseEnter={() => (hovered.current = true)}
      onMouseLeave={() => (hovered.current = false)}
    >
      <div className="flex items-center gap-2">
        <Icon className="w-4 h-4 text-accent shrink-0" />
        <span className="text-xs font-medium text-muted flex-1">{label}</span>
        <button
          type="button"
          aria-label={t("meetings.intervention.dismiss")}
          className="text-muted hover:text-text cursor-pointer"
          title={t("meetings.intervention.dismiss")}
          onClick={onDismiss}
        >
          <X className="w-4 h-4" aria-hidden="true" />
        </button>
      </div>
      <NudgeText text={body} className="text-[14px] leading-[1.4]" />
      {failure && (
        <p role="alert" className="text-xs text-error leading-snug">
          {failure}
        </p>
      )}
      <div className="flex justify-end items-center gap-3 pt-0.5">
        {busy && (
          <Loader2
            className="w-3.5 h-3.5 animate-spin text-faint"
            aria-hidden="true"
          />
        )}
        {card.type === "filed" ? (
          <>
            <button
              type="button"
              className={action}
              disabled={busy}
              onClick={() => undo(card.filed)}
            >
              {t("meetings.filing.card.undo")}
            </button>
            <button
              type="button"
              className={action}
              disabled={busy}
              onClick={openMeeting}
            >
              {t("meetings.filing.card.change")}
            </button>
          </>
        ) : (
          <button
            type="button"
            className={action}
            disabled={busy}
            onClick={openMeeting}
          >
            {t("meetings.intervention.open")}
          </button>
        )}
      </div>
    </div>
  );
};
