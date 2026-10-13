import React from "react";
import { useTranslation } from "react-i18next";
import type { Session } from "@/bindings";
import { Button } from "../ui/Button";

interface ParsedSummary {
  decisions: string[];
  open_questions: string[];
  action_items: string[];
  recap: string;
}

const strings = (v: unknown): string[] =>
  Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : [];

const parseSummary = (raw: string): ParsedSummary | null => {
  try {
    const v = JSON.parse(raw);
    if (!v || typeof v !== "object") return null;
    return {
      decisions: strings(v.decisions),
      open_questions: strings(v.open_questions),
      action_items: strings(v.action_items),
      recap: typeof v.recap === "string" ? v.recap : "",
    };
  } catch {
    return null;
  }
};

const List: React.FC<{ title: string; items: string[] }> = ({
  title,
  items,
}) =>
  items.length === 0 ? null : (
    <div className="space-y-1">
      <div className="text-xs font-medium text-mid-gray uppercase tracking-wide">
        {title}
      </div>
      <ul className="list-disc pl-5 text-sm text-text space-y-0.5">
        {items.map((item, i) => (
          <li key={i}>{item}</li>
        ))}
      </ul>
    </div>
  );

export const SessionSummary: React.FC<{
  session: Session;
  meetingEnded: boolean;
  onRetry: () => void;
}> = ({ session, meetingEnded, onRetry }) => {
  const { t } = useTranslation();
  const ended = session.ended_at != null || meetingEnded;

  if (session.summary === null) {
    if (!ended) return null;
    return (
      <div className="flex items-center gap-3 p-4">
        <span className="text-sm text-mid-gray">
          {t("meetings.summary.none")}
        </span>
        <Button size="sm" variant="secondary" onClick={onRetry}>
          {t("meetings.summary.retry")}
        </Button>
      </div>
    );
  }

  const parsed = parseSummary(session.summary);
  if (!parsed) {
    return (
      <div className="p-4 space-y-1">
        <div className="text-xs text-mid-gray">
          {t("meetings.summary.failed")}
        </div>
        <p className="text-sm text-text whitespace-pre-wrap">
          {session.summary}
        </p>
      </div>
    );
  }

  return (
    <div className="p-4 space-y-3">
      {parsed.recap && (
        <div className="space-y-1">
          <div className="text-xs font-medium text-mid-gray uppercase tracking-wide">
            {t("meetings.summary.recap")}
          </div>
          <p className="text-sm text-text">{parsed.recap}</p>
        </div>
      )}
      <List title={t("meetings.summary.decisions")} items={parsed.decisions} />
      <List
        title={t("meetings.summary.openQuestions")}
        items={parsed.open_questions}
      />
      <List
        title={t("meetings.summary.actionItems")}
        items={parsed.action_items}
      />
    </div>
  );
};
