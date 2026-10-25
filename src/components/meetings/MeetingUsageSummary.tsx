import React from "react";
import { useTranslation } from "react-i18next";
import type { MeetingUsage } from "@/bindings";

/** Every AI call a meeting made: counts, tokens, and estimated cost. */
export const MeetingUsageSummary: React.FC<{ usage: MeetingUsage }> = ({
  usage,
}) => {
  const { t, i18n } = useTranslation();
  if (usage.lines.length === 0) return null;
  const num = new Intl.NumberFormat(i18n.language);
  const usd = new Intl.NumberFormat(i18n.language, {
    style: "currency",
    currency: "USD",
    maximumFractionDigits: 4,
  });
  const cost = usd.format(usage.cost_usd);

  return (
    <details className="group text-[12.5px] text-faint">
      <summary className="cursor-pointer list-none flex flex-wrap gap-x-3 gap-y-1 hover:text-muted">
        <span>
          {t("meetings.usage.jevChecks", { count: usage.jev_checks })}
        </span>
        <span>
          {t("meetings.usage.modelCalls", { count: usage.model_calls })}
        </span>
        <span>
          {t("meetings.usage.toolCalls", { count: usage.tool_calls })}
        </span>
        <span>
          {usage.partial_cost
            ? t("meetings.usage.costPartial", { cost })
            : t("meetings.usage.cost", { cost })}
        </span>
        <span className="text-accent group-open:hidden">
          {t("meetings.usage.details")}
        </span>
      </summary>
      <table className="mt-2 w-full tabular text-[12px]">
        <thead>
          <tr className="text-start">
            <th className="text-start font-normal pe-3">
              {t("meetings.usage.col.purpose")}
            </th>
            <th className="text-start font-normal pe-3">
              {t("meetings.usage.col.model")}
            </th>
            <th className="text-end font-normal pe-3">
              {t("meetings.usage.col.calls")}
            </th>
            <th className="text-end font-normal pe-3">
              {t("meetings.usage.col.tools")}
            </th>
            <th className="text-end font-normal pe-3">
              {t("meetings.usage.col.tokens")}
            </th>
            <th className="text-end font-normal">
              {t("meetings.usage.col.cost")}
            </th>
          </tr>
        </thead>
        <tbody className="text-muted">
          {usage.lines.map((l) => (
            <tr key={`${l.purpose}:${l.model}`}>
              <td className="pe-3">
                {t(`meetings.usage.purpose.${l.purpose}`, {
                  defaultValue: l.purpose,
                })}
              </td>
              <td className="pe-3 font-mono text-[11px]">{l.model}</td>
              <td className="pe-3 text-end">
                {num.format(l.calls)}
                {l.failed > 0 &&
                  ` ${t("meetings.usage.failed", { count: l.failed })}`}
              </td>
              <td className="pe-3 text-end">{num.format(l.tool_calls)}</td>
              <td className="pe-3 text-end">
                {l.input_tokens + l.output_tokens > 0
                  ? t("meetings.usage.tokens", {
                      input: num.format(l.input_tokens),
                      cached: num.format(l.cached_input_tokens),
                      output: num.format(l.output_tokens),
                    })
                  : t("meetings.usage.notReported")}
              </td>
              <td className="text-end">
                {l.cost_usd === null
                  ? t("meetings.usage.notReported")
                  : usd.format(l.cost_usd)}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <p className="mt-2">
        {t("meetings.usage.footnote", { date: usage.prices_checked })}
      </p>
    </details>
  );
};
