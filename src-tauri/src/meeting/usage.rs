//! Per-meeting record of every AI call: Jev checks and main-model calls, with
//! requests, tool calls, tokens, and an estimated cost. Rows are stored as
//! measured; cost is priced on read so a price change never rewrites history.

use super::store::ProjectStore;
use crate::llm_client::LlmUsage;
use anyhow::Result;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use specta::Type;

/// The model Jev answers with; it reports no token usage.
pub const JEV_MODEL: &str = "jev-latest";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    /// One Jev opportunity check.
    Jev,
    /// Main model run because Jev flagged a kind.
    Check,
    /// Silent main-model refresh of the meeting's live memory.
    Memory,
    /// Main model folding in older transcript after downtime, without nudging.
    CatchUp,
    /// Main model run because Jev failed or is not in use.
    Fallback,
    /// Filing the meeting into a project and titling it.
    Filing,
    /// End-of-session summary.
    Summary,
}

impl Purpose {
    pub fn as_str(self) -> &'static str {
        match self {
            Purpose::Jev => "jev",
            Purpose::Check => "check",
            Purpose::Memory => "memory",
            Purpose::CatchUp => "catch_up",
            Purpose::Fallback => "fallback",
            Purpose::Filing => "filing",
            Purpose::Summary => "summary",
        }
    }
}

/// USD per million tokens: input, cached input, output.
struct Price {
    input: f64,
    cached: f64,
    output: f64,
}

/// OpenAI's published standard-tier prices, checked on this date. Models
/// missing here show tokens without a cost rather than a guessed one.
pub const PRICES_CHECKED: &str = "2026-10-21";

fn price(model: &str) -> Option<Price> {
    let id = model.rsplit('/').next().unwrap_or(model);
    let (input, cached, output) = match id {
        "gpt-6-luna" => (0.10, 0.01, 0.50),
        "gpt-5.6-luna" => (0.20, 0.02, 1.20),
        "gpt-5.4-nano" => (0.20, 0.02, 1.25),
        "gpt-5.4-mini" => (0.75, 0.075, 4.50),
        "gpt-5-nano" => (0.05, 0.005, 0.40),
        "gpt-5-mini" => (0.25, 0.025, 2.00),
        "gpt-4.1-nano" => (0.10, 0.025, 0.40),
        "gpt-4.1-mini" => (0.40, 0.10, 1.60),
        "gpt-4o-mini" => (0.15, 0.075, 0.60),
        _ => return None,
    };
    Some(Price {
        input,
        cached,
        output,
    })
}

fn cost(model: &str, input: i64, cached: i64, output: i64) -> Option<f64> {
    let p = price(model)?;
    let uncached = (input - cached).max(0) as f64;
    Some((uncached * p.input + cached as f64 * p.cached + output as f64 * p.output) / 1_000_000.0)
}

/// Totals for one purpose and model within a meeting.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct UsageLine {
    pub purpose: String,
    pub model: String,
    /// Calls made (one per Jev check, one per main-model run).
    pub calls: i64,
    pub failed: i64,
    pub requests: i64,
    pub tool_calls: i64,
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub output_tokens: i64,
    /// `None` when the model has no known price (Jev, unlisted models).
    pub cost_usd: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct MeetingUsage {
    pub lines: Vec<UsageLine>,
    pub jev_checks: i64,
    pub model_calls: i64,
    pub tool_calls: i64,
    /// Sum of the priced lines.
    pub cost_usd: f64,
    /// True when some calls could not be priced, so `cost_usd` is a floor.
    pub partial_cost: bool,
    pub prices_checked: String,
}

impl ProjectStore {
    pub fn record_usage(
        &self,
        meeting_id: i64,
        purpose: Purpose,
        model: &str,
        ok: bool,
        usage: &LlmUsage,
    ) -> Result<()> {
        self.conn()?.execute(
            "INSERT INTO llm_usage (meeting_id, ts, purpose, model, ok, requests, tool_calls,
               input_tokens, cached_input_tokens, output_tokens)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                meeting_id,
                chrono::Utc::now().timestamp_millis(),
                purpose.as_str(),
                model,
                ok,
                usage.requests,
                usage.tool_calls,
                usage.input_tokens,
                usage.cached_input_tokens,
                usage.output_tokens
            ],
        )?;
        Ok(())
    }

    pub fn meeting_usage(&self, meeting_id: i64) -> Result<MeetingUsage> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT purpose, model, COUNT(*), SUM(ok = 0), SUM(requests), SUM(tool_calls),
                    SUM(input_tokens), SUM(cached_input_tokens), SUM(output_tokens)
             FROM llm_usage WHERE meeting_id = ?1
             GROUP BY purpose, model ORDER BY MIN(id)",
        )?;
        let lines = stmt
            .query_map(params![meeting_id], |r| {
                let model: String = r.get(1)?;
                let (input, cached, output): (i64, i64, i64) = (r.get(6)?, r.get(7)?, r.get(8)?);
                Ok(UsageLine {
                    purpose: r.get(0)?,
                    cost_usd: cost(&model, input, cached, output),
                    model,
                    calls: r.get(2)?,
                    failed: r.get(3)?,
                    requests: r.get(4)?,
                    tool_calls: r.get(5)?,
                    input_tokens: input,
                    cached_input_tokens: cached,
                    output_tokens: output,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let jev = |l: &&UsageLine| l.purpose == Purpose::Jev.as_str();
        Ok(MeetingUsage {
            jev_checks: lines.iter().filter(jev).map(|l| l.calls).sum(),
            model_calls: lines.iter().filter(|l| !jev(l)).map(|l| l.calls).sum(),
            tool_calls: lines.iter().map(|l| l.tool_calls).sum(),
            cost_usd: lines.iter().filter_map(|l| l.cost_usd).sum(),
            partial_cost: lines.iter().any(|l| l.cost_usd.is_none()),
            prices_checked: PRICES_CHECKED.to_string(),
            lines,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_totals_split_jev_from_main_model_and_price_known_models() {
        let dir = tempfile::tempdir().unwrap();
        let s = ProjectStore::open(dir.path().join("meet.db")).unwrap();
        let m = s.create_meeting(None, "M").unwrap();
        let jev = LlmUsage {
            requests: 1,
            ..Default::default()
        };
        s.record_usage(m.id, Purpose::Jev, JEV_MODEL, true, &jev)
            .unwrap();
        s.record_usage(m.id, Purpose::Jev, JEV_MODEL, false, &jev)
            .unwrap();
        let model = LlmUsage {
            requests: 2,
            tool_calls: 1,
            input_tokens: 1_000_000,
            cached_input_tokens: 500_000,
            output_tokens: 100_000,
        };
        s.record_usage(m.id, Purpose::Check, "gpt-6-luna", true, &model)
            .unwrap();

        let u = s.meeting_usage(m.id).unwrap();
        assert_eq!((u.jev_checks, u.model_calls, u.tool_calls), (2, 1, 1));
        assert_eq!(u.lines[0].failed, 1, "failed Jev check counted");
        let expected = 0.5 * 0.10 + 0.5 * 0.01 + 0.1 * 0.50;
        assert!(
            (u.cost_usd - expected).abs() < 1e-9,
            "cached tokens billed at the cached rate"
        );
        assert!(u.partial_cost, "Jev has no price, so the total is a floor");
    }
}
