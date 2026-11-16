import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { AudioLines, Mic, PhoneOff, X } from "lucide-react";
import { commands, events, type DetectionPrompt } from "@/bindings";
import { formatElapsed, useNow } from "./LiveDock";
import { PillShell } from "./PillShell";

/** The floating bar's current question, kept in step with the backend. */
export const useDetectionPrompt = (): DetectionPrompt | null => {
  const [prompt, setPrompt] = useState<DetectionPrompt | null>(null);
  useEffect(() => {
    let current = true;
    void commands.getMeetingDetection().then((p) => {
      if (current) setPrompt(p);
    });
    const unlisten = events.meetingDetectionPayload.listen((e) =>
      setPrompt(e.payload.prompt),
    );
    return () => {
      current = false;
      void unlisten.then((f) => f());
    };
  }, []);
  return prompt;
};

const ICON = { call: Mic, voices: AudioLines, call_ended: PhoneOff };

/**
 * The floating bar when CASE has noticed a meeting: a call app using the
 * microphone, people talking, or the recorded call ending. One click starts
 * (or ends) the meeting; ignoring it records nothing.
 */
export const DetectionBar: React.FC<{ prompt: DetectionPrompt }> = ({
  prompt,
}) => {
  const { t } = useTranslation();
  const ended = prompt.kind === "call_ended";
  const now = useNow(ended);
  const Icon = ICON[prompt.kind];
  const answer = (accept: boolean) =>
    void commands.answerMeetingDetection(accept);

  return (
    <PillShell floating label={t("meetings.detect.label")}>
      <span
        data-tauri-drag-region
        className="w-9 h-9 shrink-0 rounded-full inline-flex items-center justify-center bg-accent/15 text-accent"
      >
        <Icon className="w-4 h-4" />
      </span>
      <span className="min-w-0 flex flex-col items-start px-2">
        <span className="text-[13px] leading-4 text-text truncate max-w-[14rem]">
          {t(`meetings.detect.${prompt.kind}.title`)}
        </span>
        <span className="text-[11.5px] leading-4 text-faint truncate max-w-[14rem]">
          {ended && prompt.deadline !== null ? (
            <>
              {t("meetings.detect.call_ended.stopping")}
              <span className="font-mono tabular ms-1.5">
                {formatElapsed(prompt.deadline - now)}
              </span>
            </>
          ) : prompt.app ? (
            t("meetings.detect.call.app", { app: prompt.app })
          ) : (
            t(`meetings.detect.${prompt.kind}.hint`)
          )}
        </span>
      </span>
      <span className="w-px h-6 bg-line mx-1" />
      {ended && (
        <button
          type="button"
          onClick={() => answer(false)}
          className="h-9 px-3 inline-flex items-center rounded-full text-[13px] text-muted hover:text-text hover:bg-surface-hover transition-colors cursor-pointer"
        >
          {t("meetings.detect.call_ended.keep")}
        </button>
      )}
      <button
        type="button"
        onClick={() => answer(true)}
        className="h-9 px-3.5 inline-flex items-center rounded-full bg-text text-background text-[13px] font-medium hover:bg-text/85 transition-colors cursor-pointer"
      >
        {ended
          ? t("meetings.detect.call_ended.stop")
          : t("meetings.detect.start")}
      </button>
      {!ended && (
        <button
          type="button"
          onClick={() => answer(false)}
          title={t("meetings.detect.dismiss")}
          aria-label={t("meetings.detect.dismiss")}
          className="w-9 h-9 inline-flex items-center justify-center rounded-full text-faint hover:text-text hover:bg-surface-hover transition-colors cursor-pointer"
        >
          <X className="w-4 h-4" />
        </button>
      )}
    </PillShell>
  );
};
