import React from "react";
import { useTranslation } from "react-i18next";
import type { Session } from "@/bindings";

interface Issue {
  kind: string;
  topic: string;
  note: string;
  minutes: number;
}

interface Report {
  score: number;
  verdict: string;
  issues: Issue[];
}

const ISSUE_KINDS = [
  "circle",
  "argument",
  "tangent",
  "repetition",
  "recurring",
];

const num = (v: unknown) => (typeof v === "number" && isFinite(v) ? v : 0);
const str = (v: unknown) => (typeof v === "string" ? v : "");

/** The efficiency report stored in a session summary, if it has one.
 * Summaries written before the report existed have none. */
const parseReport = (summary: string | null): Report | null => {
  if (!summary) return null;
  let e: unknown;
  try {
    e = (JSON.parse(summary) as { efficiency?: unknown })?.efficiency;
  } catch {
    return null;
  }
  if (!e || typeof e !== "object") return null;
  const r = e as Record<string, unknown>;
  const issues = Array.isArray(r.issues) ? r.issues : [];
  return {
    score: Math.min(100, Math.max(0, Math.round(num(r.score)))),
    verdict: str(r.verdict),
    issues: issues
      .filter((i): i is Record<string, unknown> => !!i && typeof i === "object")
      .map((i) => ({
        kind: str(i.kind),
        topic: str(i.topic),
        note: str(i.note),
        minutes: Math.max(0, Math.round(num(i.minutes))),
      }))
      .filter((i) => ISSUE_KINDS.includes(i.kind)),
  };
};

/** Reports of a meeting's sessions, newest first, with 1-based session numbers. */
export const sessionReports = (sessions: Session[]) =>
  sessions
    .map((s, i) => ({ id: s.id, n: i + 1, report: parseReport(s.summary) }))
    .filter((x): x is { id: number; n: number; report: Report } => !!x.report)
    .reverse();

const IssueList: React.FC<{ issues: Issue[] }> = ({ issues }) => {
  const { t } = useTranslation();
  return (
    <ul className="space-y-2.5">
      {issues.map((issue, i) => (
        <li key={i} className="text-[13px] leading-snug">
          <div className="flex flex-wrap items-baseline gap-x-1.5">
            <span className="text-[11px] font-medium uppercase tracking-wide text-accent">
              {t(`meetings.summary.efficiency.kind.${issue.kind}`)}
            </span>
            <span className="font-medium text-text">{issue.topic}</span>
            {issue.minutes > 0 && (
              <span className="text-faint">
                {t("meetings.summary.efficiency.minutes", {
                  count: issue.minutes,
                })}
              </span>
            )}
          </div>
          {issue.note && <p className="mt-0.5 text-muted">{issue.note}</p>}
        </li>
      ))}
    </ul>
  );
};

const ReportView: React.FC<{ report: Report }> = ({ report }) => {
  const { t } = useTranslation();
  // Issues carried over from earlier conversations in the space get their
  // own group; the rest are about this session alone.
  const here = report.issues.filter((i) => i.kind !== "recurring");
  const space = report.issues.filter((i) => i.kind === "recurring");
  return (
    <div>
      <div className="flex items-baseline gap-2.5">
        <span className="font-mono tabular text-[20px] leading-none text-text">
          {report.score}
          <span className="text-[12px] text-faint">/100</span>
        </span>
        {report.verdict && (
          <p className="text-[13px] leading-snug text-muted">
            {report.verdict}
          </p>
        )}
      </div>
      {here.length === 0 ? (
        <p className="mt-2 text-[13px] text-faint">
          {t("meetings.summary.efficiency.none")}
        </p>
      ) : (
        <div className="mt-3">
          <IssueList issues={here} />
        </div>
      )}
      {space.length > 0 && (
        <div className="mt-4">
          <h3 className="mb-2 text-[12px] font-medium text-muted">
            {t("meetings.summary.efficiency.acrossSpace")}
          </h3>
          <IssueList issues={space} />
        </div>
      )}
    </div>
  );
};

/** How each session used its time; sessions are labelled when there are several. */
export const EfficiencyReports: React.FC<{
  reports: ReturnType<typeof sessionReports>;
  labelled: boolean;
}> = ({ reports, labelled }) => {
  const { t } = useTranslation();
  return (
    <div className="space-y-5">
      {reports.map(({ id, n, report }) => (
        <div key={id}>
          {labelled && (
            <div className="mb-1.5 text-[12px] text-faint">
              {t("meetings.transcript.session", { n })}
            </div>
          )}
          <ReportView report={report} />
        </div>
      ))}
    </div>
  );
};
