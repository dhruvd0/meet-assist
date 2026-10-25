//! Observer: every N seconds while a meeting is live, ask the model one
//! question with a small base prompt and tool access; emit at most one
//! intervention per cooldown window.

use super::tester::TesterTraceKind;
use crate::audio_feedback;
use crate::llm_client::{self, ToolLoopError};
use crate::meeting::jev::{GateMode, JevCall, JevCheck, JevDetector};
use crate::meeting::session::MeetingManager;
use crate::meeting::store::{
    Intervention, Meeting, MeetingRecap, ObserverCheck, ProjectStore, TranscriptLine,
};
use crate::meeting::tools::{handle_tool, tool_defs, ToolContext};
use crate::meeting::usage::{MeetingUsage, Purpose, JEV_MODEL};
use crate::settings::{AppSettings, LlmProvider, MeetingSensitivity};
use anyhow::Result;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use specta::Type;
use std::sync::Arc;
use tauri::AppHandle;
use tauri_specta::Event;

/// Minimum gap between any two interventions.
pub const COOLDOWN_MS: i64 = 180_000;
/// Minimum gap between two interventions of the same kind.
pub const SAME_KIND_COOLDOWN_MS: i64 = 600_000;
pub const WINDOW_MS: i64 = 180_000;
pub const MAX_TOOL_CALLS: usize = 5;
pub const DEDUPE_RECENT: usize = 5;
/// Max characters of the meeting's context pasted into the prompt; the model
/// can read sources and the transcript through tools for more.
pub const CONTEXT_MAX_CHARS: usize = 4096;
/// Earlier meetings of the project shown on every check.
pub const EARLIER_MEETINGS: i64 = 3;
/// Meetings with fewer transcript lines are tests or false starts, not history.
pub const EARLIER_MIN_LINES: i64 = 20;
/// Max characters of each summary in the history.
pub const RECAP_MAX_CHARS: usize = 1500;
/// Bound both catch-up work and the durable model-written ledger.
pub const CATCHUP_LINES: usize = 100;
/// A silent memory refresh runs once this much transcript has gone unread by
/// the main model (Jev quiet throughout): a full catch-up batch, or 5 minutes.
const MEMORY_FLOOR_LINES: usize = CATCHUP_LINES;
const MEMORY_FLOOR_MS: i64 = 300_000;
pub const LEDGER_ITEMS: usize = 12;
pub const LEDGER_ITEM_CHARS: usize = 240;

/// Working memory updated on every successful check, including silent ones.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LiveContext {
    pub decisions: Vec<String>,
    pub open_questions: Vec<String>,
    pub contracts: Vec<String>,
    pub parked_topics: Vec<String>,
}

impl LiveContext {
    fn bounded(mut self) -> Self {
        for items in [
            &mut self.decisions,
            &mut self.open_questions,
            &mut self.contracts,
            &mut self.parked_topics,
        ] {
            *items = items
                .iter()
                .map(|s| truncate_chars(s.trim(), LEDGER_ITEM_CHARS).0.to_string())
                .filter(|s| !s.is_empty())
                .take(LEDGER_ITEMS)
                .collect();
        }
        self
    }
}

/// What came before this check: the project's earlier meetings and this
/// meeting's own earlier sessions.
#[derive(Default)]
pub struct History {
    /// Newest first.
    pub earlier: Vec<MeetingRecap>,
    /// This meeting's rolling summary: its sessions before a Continue.
    pub this_meeting: Option<String>,
    pub live: LiveContext,
}

impl History {
    /// The project's earlier meetings (see `EARLIER_*`) and this meeting's
    /// own earlier sessions.
    pub fn load(store: &ProjectStore, meeting: &Meeting) -> Result<Self> {
        Ok(Self {
            earlier: match meeting.project_id {
                Some(pid) => store.recent_meeting_recaps(
                    pid,
                    meeting.id,
                    EARLIER_MIN_LINES,
                    EARLIER_MEETINGS,
                )?,
                None => Vec::new(),
            },
            this_meeting: meeting.rolling_summary.clone(),
            live: LiveContext::default(),
        })
    }

    /// Prompt sections for the history; shared by the observer and the
    /// end-of-session report.
    pub fn render(&self, out: &mut String) {
        out.push_str("## Earlier conversations in this project (newest first)\n");
        if self.earlier.is_empty() {
            out.push_str("(none)");
        }
        for m in &self.earlier {
            let date = chrono::DateTime::from_timestamp_millis(m.created_at)
                .map(|d| d.with_timezone(&chrono::Local).format("%b %-d").to_string())
                .unwrap_or_default();
            let (summary, cut) = truncate_chars(m.summary.trim(), RECAP_MAX_CHARS);
            out.push_str(&format!(
                "### {} ({date})\n{summary}{}\n",
                m.title,
                if cut { " [truncated]" } else { "" }
            ));
        }
        out.push_str("\n## This conversation's earlier sessions\n");
        match self.this_meeting.as_deref().map(str::trim) {
            Some(s) if !s.is_empty() => {
                let (summary, cut) = truncate_chars(s, RECAP_MAX_CHARS);
                out.push_str(summary);
                if cut {
                    out.push_str(" [truncated]");
                }
            }
            _ => out.push_str("(none: this is the first session)"),
        }
    }
}
/// Max characters of an intervention card's text.
pub const CARD_TEXT_MAX_CHARS: usize = 100;
/// Max characters of a "loop" card: a "Circle:" line naming the topic,
/// then a checklist, `LOOP_MAX_LINES` lines in all.
pub const LOOP_TEXT_MAX_CHARS: usize = 280;
pub const LOOP_MAX_LINES: usize = 4;
/// Consecutive judge failures after which the observer slows down.
/// Seconds between ticks while Jev gates checks. The model still runs at
/// most once per configured interval.
pub const JEV_TICK_SECS: u64 = 2;
pub const BACKOFF_AFTER_FAILURES: u32 = 5;
/// Interval multiplier while backing off.
pub const BACKOFF_FACTOR: u64 = 4;
const VALID_KINDS: [&str; 4] = ["loop", "stuck", "info", "idea"];
/// Recent verdicts kept for the "AI is listening" panel.
pub const THOUGHTS_KEPT: usize = 3;

#[derive(Debug, Clone, Deserialize)]
pub struct Verdict {
    pub intervene: bool,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub reason: String,
    pub live_context: LiveContext,
}

pub fn verdict_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "intervene": { "type": "boolean" },
            "kind": { "type": ["string", "null"], "enum": ["loop", "stuck", "info", "idea", null] },
            "text": { "type": ["string", "null"] },
            "reason": { "type": "string" },
            "live_context": {
                "type": "object",
                "properties": {
                    "decisions": {"type": "array", "items": {"type": "string"}, "maxItems": LEDGER_ITEMS},
                    "open_questions": {"type": "array", "items": {"type": "string"}, "maxItems": LEDGER_ITEMS},
                    "contracts": {"type": "array", "items": {"type": "string"}, "maxItems": LEDGER_ITEMS},
                    "parked_topics": {"type": "array", "items": {"type": "string"}, "maxItems": LEDGER_ITEMS}
                },
                "required": ["decisions", "open_questions", "contracts", "parked_topics"],
                "additionalProperties": false
            }
        },
        "required": ["intervene", "kind", "text", "reason", "live_context"],
        "additionalProperties": false
    })
}

pub fn parse_verdict(text: &str) -> Result<Verdict, String> {
    let t = text.trim();
    let t = t
        .strip_prefix("```json")
        .or_else(|| t.strip_prefix("```"))
        .unwrap_or(t);
    let t = t.strip_suffix("```").unwrap_or(t).trim();
    // Models sometimes wrap the object in prose or repeat it; take the first
    // object and ignore what follows.
    let t = t.find('{').map_or(t, |a| &t[a..]);
    serde_json::Deserializer::from_str(t)
        .into_iter::<Verdict>()
        .next()
        .unwrap_or_else(|| serde_json::from_str::<Verdict>(t))
        .map_err(|e| format!("verdict parse: {e}"))
}

/// Truncate to at most `max` characters (never splitting a character).
fn truncate_chars(s: &str, max: usize) -> (&str, bool) {
    match s.char_indices().nth(max) {
        Some((cut, _)) => (&s[..cut], true),
        None => (s, false),
    }
}

/// Whether the observer can run with the current settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Readiness {
    Ready,
    Disabled,
    NoProvider,
    /// Provider label.
    UnsupportedProvider(String),
    /// Provider label.
    MissingApiKey(String),
    /// Provider label.
    NoModel(String),
}

impl Readiness {
    pub fn code(&self) -> &'static str {
        match self {
            Readiness::Ready => "ready",
            Readiness::Disabled => "disabled",
            Readiness::NoProvider => "no_provider",
            Readiness::UnsupportedProvider(_) => "unsupported_provider",
            Readiness::MissingApiKey(_) => "missing_api_key",
            Readiness::NoModel(_) => "no_model",
        }
    }

    pub fn detail(&self) -> Option<String> {
        match self {
            Readiness::UnsupportedProvider(l)
            | Readiness::MissingApiKey(l)
            | Readiness::NoModel(l) => Some(l.clone()),
            _ => None,
        }
    }
}

fn is_local(provider: &LlmProvider) -> bool {
    let Ok(url) = reqwest::Url::parse(&provider.base_url) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if host == "localhost" || host.ends_with(".localhost") {
        return true;
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => ip.is_loopback(),
        Ok(std::net::IpAddr::V6(ip)) => {
            ip.is_loopback() || ip.to_ipv4_mapped().is_some_and(|ip| ip.is_loopback())
        }
        Err(_) => false,
    }
}

pub fn readiness(settings: &AppSettings) -> Readiness {
    if !settings.meeting_assistant_enabled {
        return Readiness::Disabled;
    }
    let Some(provider) = settings.llm_provider() else {
        return Readiness::NoProvider;
    };
    let label = provider.label.clone();
    if !provider.base_url.trim().starts_with("http") {
        return Readiness::UnsupportedProvider(label);
    }
    if settings.llm_api_key().trim().is_empty() && !is_local(provider) {
        return Readiness::MissingApiKey(label);
    }
    if settings
        .llm_model()
        .map(|m| m.trim().is_empty())
        .unwrap_or(true)
    {
        return Readiness::NoModel(label);
    }
    Readiness::Ready
}

pub struct JudgeInput {
    pub system: String,
    pub user: String,
    /// Why the main model is running, for the usage record.
    pub purpose: Purpose,
}

#[derive(Debug)]
pub enum JudgeError {
    ToolsRejected,
    /// Failure detail; read only through the derived `Debug` in log lines,
    /// which dead-code analysis ignores.
    #[allow(dead_code)]
    Other(String),
}

pub trait Judge: Send + Sync {
    fn judge(
        &self,
        input: &JudgeInput,
        tools_ctx: &ToolContext,
        allow_tools: bool,
    ) -> Result<Verdict, JudgeError>;
}

pub struct LlmJudge {
    app: AppHandle,
    /// One HTTP client per observer, rebuilt when the provider or key changes.
    clients: llm_client::ToolClientCache,
}

impl LlmJudge {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            clients: llm_client::ToolClientCache::default(),
        }
    }
}

impl Judge for LlmJudge {
    fn judge(
        &self,
        input: &JudgeInput,
        tools_ctx: &ToolContext,
        allow_tools: bool,
    ) -> Result<Verdict, JudgeError> {
        let settings = crate::settings::get_settings(&self.app);
        if readiness(&settings) != Readiness::Ready {
            return Err(JudgeError::Other("assistant not ready".into()));
        }
        let provider = settings
            .llm_provider()
            .cloned()
            .ok_or_else(|| JudgeError::Other("no provider configured".into()))?;
        let model = settings
            .llm_model()
            .ok_or_else(|| JudgeError::Other("no model configured".into()))?;
        let api_key = settings.llm_api_key();
        let client = self
            .clients
            .get(&provider, &api_key)
            .map_err(JudgeError::Other)?;
        let defs = if allow_tools { tool_defs() } else { Vec::new() };
        let handler = |c: &llm_client::ToolCall| handle_tool(tools_ctx, c);
        let mut usage = llm_client::LlmUsage::default();
        let fut = llm_client::send_with_tools_with_client(
            &client,
            &provider,
            &model,
            input.system.clone(),
            input.user.clone(),
            &defs,
            verdict_schema(),
            MAX_TOOL_CALLS,
            &handler,
            &mut usage,
        );
        let budget = std::time::Duration::from_secs(15);
        let text =
            tauri::async_runtime::block_on(async move { tokio::time::timeout(budget, fut).await });
        let verdict = match text {
            Err(_) => Err(JudgeError::Other("judge timed out".into())),
            Ok(Err(ToolLoopError::ToolsRejected(_))) => Err(JudgeError::ToolsRejected),
            Ok(Err(ToolLoopError::Other(e))) => Err(JudgeError::Other(e)),
            Ok(Ok(t)) => parse_verdict(&t).map_err(JudgeError::Other),
        };
        if let Err(e) = tools_ctx.store.record_usage(
            tools_ctx.meeting_id,
            input.purpose,
            &model,
            verdict.is_ok(),
            &usage,
        ) {
            warn!("observer: record usage: {e:#}");
        }
        verdict
    }
}

pub fn build_prompt(
    brief: &str,
    history: &History,
    context: &str,
    recent: &[Intervention],
    lines: &[TranscriptLine],
    sensitivity: MeetingSensitivity,
    tools_available: bool,
) -> JudgeInput {
    let sensitivity_rule = match sensitivity {
        MeetingSensitivity::Low => {
            "Sensitivity LOW: intervene on unresolved decisions, reopened scope, and blockers only when they threaten the immediate next step."
        }
        MeetingSensitivity::Medium => {
            "Sensitivity MEDIUM: intervene on unresolved decisions, reopened scope, blockers, contract changes, and clearly missing facts."
        }
        MeetingSensitivity::High => {
            "Sensitivity HIGH: also surface a concrete missing assumption or alternative before the room commits to an approach."
        }
    };
    let tools_rule = if tools_available {
        "You have tools to list and search project sources and read transcripts. Use them to check project facts before asserting anything; prefer silence over an unverified claim."
    } else {
        "No tools are available this turn. Do not assert project facts you cannot see in the prompt."
    };
    let system = format!(
        "You are a silent copilot for one person following a live conversation. It can be a meeting or call, an interview, a debate, a class or talk, or a broadcast they are only listening to. You read the live transcript and answer ONE question: should you speak up right now? Silence is the default answer; most checks end in silence.\n\
         Always write generated content in English, regardless of the language of the transcript, input, or existing content. Preserve proper nouns and verbatim quotes in their original form. This applies to nudge text, Circle topics and checklist lines, reasons, and every live_context entry.\n\
         Speakers: `me` is the user's own microphone; `them` is everything else they hear (other people on a call, or audio playing on their computer). The user may only be listening, so `me` can be silent throughout.\n\
         The transcript is raw speech-to-text: it has fragments, misheard words, mixed languages (for example Hindi and English), and the same sentence sometimes appears twice. None of that is a reason to speak, and never comment on the conversation being unclear.\n\
         Signals:\n\
         - kind \"loop\": an unresolved decision resurfaces, incompatible answers remain in play, or a parked/out-of-scope topic is reopened without explicitly changing the scope. There is no minimum time gap or repetition count. A brief acknowledgment (\"yeah\", \"makes sense\") is not resolution: look for an explicit choice, answered constraint, or agreed deferral. Do not interrupt productive exploration, transcript echoes, or an intentional revision whose consequences are already addressed.\n\
         - kind \"stuck\": a concrete next step is blocked by a missing choice, dependency, or conflicting assumptions, even if nobody says they are stuck. Name the exact missing choice; do not invent its answer. Stay silent if the room is already resolving it.\n\
         - kind \"info\": an unanswered factual question you can verify, or a material contract change/conflict grounded in the transcript or memory (payload, result count, identity flow, ownership, or interface). Point out the old-to-new implication that the room has not addressed; a deliberate change already understood is not itself a problem.\n\
         - kind \"idea\": the discussion is converging while a concrete alternative or missing assumption could change the outcome. Ask a specific design question rather than asserting an unverified project fact.\n\
         {sensitivity_rule}\n{tools_rule}\n\
         Use the earlier conversations and earlier sessions below as memory: an open question from them that comes back still unresolved is kind \"loop\" (name when it was first raised, e.g. \"open since Oct 14\"), and reopening something they already decided merits a nudge only when a material conflict or consequence is being missed. Do not repeat their content otherwise.\n\
         Maintain live_context on EVERY check, including silence and candidates that may be suppressed. Return the complete updated ledger, not a delta: decisions (explicit agreements), open_questions (unresolved choices and blockers), contracts (interfaces, ownership, inputs/outputs and meaningful changes), parked_topics (deferrals and MVP boundaries). Carry forward still-relevant entries, remove resolved questions, replace superseded decisions/contracts, and retain the highest-impact items, at most 12 per list and 240 characters each. Include a transcript timestamp or earlier-meeting date in each entry. Do not invent facts, treat your own proposed nudge as an agreement, or record a generic acknowledgment as resolution. Separate a deferred feature from the minimum fallback still needed now.\n\
         Never give generic process advice (summarize, document, align, clarify, take notes, ask for feedback). Only speak about a concrete point from this conversation.\n\
         Answer with JSON only, exactly this shape: {{\"intervene\": boolean, \"kind\": \"loop\"|\"stuck\"|\"info\"|\"idea\"|null, \"text\": string|null, \"reason\": string, \"live_context\": {{\"decisions\": [string], \"open_questions\": [string], \"contracts\": [string], \"parked_topics\": [string]}}}}. \
         \"text\" is null when silent. For kind \"loop\", \"text\" is a checklist of up to four lines separated by \"\\n\", each starting with its label and at most 8 words after it. The first line is always \"Circle: <the topic or pain point the talk keeps circling, in 2 to 4 words>\", a name the room would recognize, like \"Launch market\" or \"Onboarding drop-off\". Then only the lines that apply: \"Decided: <what was already settled>\", \"Decide now: <the open point to settle>\", \"Park: <a point to take up later>\". Include at least a \"Decide now\" or a \"Park\" line, and a \"Decided\" line only for something actually agreed; use the live ledger and supplied transcript to find what was decided and what could be parked; call read_transcript when that evidence is insufficient, rather than requiring a tool call for every loop. For every other kind, \"text\" is one short line of at most 12 words naming the concrete point, like a note passed under the table. Never repeat a recent intervention."
    );
    let mut user = String::new();
    user.push_str("## Project brief\n");
    user.push_str(if brief.trim().is_empty() {
        "(none)"
    } else {
        brief
    });
    user.push_str("\n\n");
    history.render(&mut user);
    user.push_str("\n\n## This conversation's context\n");
    if context.trim().is_empty() {
        user.push_str("(none)");
    } else {
        let (ctx, cut) = truncate_chars(context, CONTEXT_MAX_CHARS);
        user.push_str(ctx);
        if cut {
            user.push_str("\n[truncated]");
        }
    }
    user.push_str("\n\n## Persistent live ledger (update even when silent)\n");
    // Serializing string arrays cannot fail; use the JSON value directly.
    user.push_str(&json!(history.live).to_string());
    user.push_str("\n\n## Recent interventions (do not repeat)\n");
    if recent.is_empty() {
        user.push_str("(none)");
    } else {
        for i in recent {
            user.push_str(&format!("- [{}] {}\n", i.kind, i.text));
        }
    }
    user.push_str("\n\n## Transcript evidence (recent window or ordered catch-up batch)\n");
    for l in lines {
        user.push_str(&format!("[{}] {}: {}\n", l.ts, l.speaker, l.text));
    }
    JudgeInput {
        system,
        user,
        purpose: Purpose::Fallback,
    }
}

#[derive(Debug, Default, Clone)]
pub struct ObserverState {
    pub consecutive_failures: u32,
    pub tools_unavailable: bool,
    /// Short description of the most recent judge failure, cleared on success.
    pub last_error: Option<String>,
    /// The last `THOUGHTS_KEPT` verdicts, oldest first.
    pub thoughts: Vec<ObserverThought>,
    /// Last line Jev scored without flagging; Jev is not asked again until
    /// someone speaks.
    pub jev_through: i64,
    /// When the model last ran, to cap model calls at the check interval.
    pub last_judge_ms: i64,
    /// Jev's latest answer in this meeting.
    pub jev: Option<JevCheck>,
}

/// One judged window: the model's reason, and whether it nudged.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct ObserverThought {
    pub ts: i64,
    pub text: String,
    pub nudged: bool,
}

impl ObserverState {
    fn remember(&mut self, ts: i64, text: &str, nudged: bool) {
        if self.thoughts.len() == THOUGHTS_KEPT {
            self.thoughts.remove(0);
        }
        self.thoughts.push(ObserverThought {
            ts,
            text: text.trim().to_string(),
            nudged,
        });
    }
}

/// Trim a provider error to one short line for the status banner.
pub fn short_error(e: &str) -> String {
    let flat: String = e.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX: usize = 200;
    if flat.chars().count() > MAX {
        let cut: String = flat.chars().take(MAX).collect();
        format!("{cut}…")
    } else {
        flat
    }
}

/// A machine-readable gate outcome and the time until it can clear.
pub fn suppression_reason(
    verdict: &Verdict,
    now_ms: i64,
    recent: &[Intervention],
) -> Option<(&'static str, i64)> {
    if !verdict.intervene {
        return Some(("model_silent", 0));
    }
    let Some(kind) = verdict.kind.as_deref().filter(|k| VALID_KINDS.contains(k)) else {
        return Some(("invalid_kind", 0));
    };
    let Some(text) = verdict
        .text
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    else {
        return Some(("empty_text", 0));
    };
    // Compare the actual card, including truncation, against what was shown.
    let norm = card_text(kind, text).to_lowercase();
    if norm.is_empty() {
        return Some(("empty_text", 0));
    }
    if recent
        .iter()
        .take(DEDUPE_RECENT)
        .any(|i| i.text.trim().to_lowercase() == norm)
    {
        return Some(("duplicate", 0));
    }
    if let Some(last) = recent.first() {
        let remaining = COOLDOWN_MS - now_ms.saturating_sub(last.ts);
        if remaining > 0 {
            return Some(("global_cooldown", remaining));
        }
    }
    if let Some(last) = recent.iter().find(|i| i.kind == kind) {
        let remaining = SAME_KIND_COOLDOWN_MS - now_ms.saturating_sub(last.ts);
        if remaining > 0 {
            return Some(("same_kind_cooldown", remaining));
        }
    }
    None
}

/// The card's text: a checklist of at most `LOOP_MAX_LINES` lines for
/// "loop", one line otherwise, each within its character cap.
pub fn card_text(kind: &str, text: &str) -> String {
    let (max_lines, max_chars) = if kind == "loop" {
        (LOOP_MAX_LINES, LOOP_TEXT_MAX_CHARS)
    } else {
        (1, CARD_TEXT_MAX_CHARS)
    };
    let mut out = String::new();
    // Keep whole lines only: a checklist cut mid-line reads as broken.
    for line in text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .take(max_lines)
    {
        let sep = usize::from(!out.is_empty());
        if out.chars().count() + sep + line.chars().count() > max_chars {
            break;
        }
        if sep == 1 {
            out.push('\n');
        }
        out.push_str(line);
    }
    if out.is_empty() {
        let first = text.trim().lines().next().unwrap_or_default();
        return truncate_chars(first, max_chars).0.to_string();
    }
    out
}

pub fn tick(
    store: &ProjectStore,
    judge: &dyn Judge,
    detector: &JevDetector,
    mm: &MeetingManager,
    settings: &AppSettings,
    state: &mut ObserverState,
    now_ms: i64,
) -> Result<Option<Intervention>> {
    if readiness(settings) != Readiness::Ready || !mm.is_live() {
        return Ok(None);
    }
    let st = mm.state();
    let (Some(meeting_id), Some(session_id)) = (st.meeting_id, st.session_id) else {
        return Ok(None);
    };
    let (through_id, live) = match store.observer_context(meeting_id)? {
        Some((id, json)) => (id, serde_json::from_str::<LiveContext>(&json)?.bounded()),
        None => (0, LiveContext::default()),
    };
    let latest = store.latest_line_id(meeting_id)?.unwrap_or(0);
    if latest <= through_id {
        return Ok(None);
    }
    let batch = store.observer_lines(meeting_id, through_id, CATCHUP_LINES)?;
    let Some(last_line) = batch.last() else {
        return Ok(None);
    };
    let judged_through = last_line.id;
    let memory_due = batch.len() >= MEMORY_FLOOR_LINES
        || batch
            .first()
            .is_some_and(|line| now_ms.saturating_sub(line.ts) >= MEMORY_FLOOR_MS);
    // Rebuild memory in order after downtime, without nudging about stale talk.
    let catching_up = judged_through < latest || last_line.ts < now_ms - WINDOW_MS;

    let meeting = store
        .get_meeting(meeting_id)?
        .ok_or_else(|| anyhow::anyhow!("meeting gone"))?;
    let brief = match meeting.project_id {
        Some(pid) => store.get_project(pid)?.map(|p| p.brief).unwrap_or_default(),
        None => String::new(),
    };
    let mut history = History::load(store, &meeting)?;
    history.live = live;
    // A global cooldown allows at most four cards in a same-kind window.
    // Keep enough history to enforce that gate across restarts as well.
    let recent = store.recent_interventions(meeting_id, 20)?;
    let mut lines = if catching_up {
        Vec::new()
    } else {
        store.lines_since(meeting_id, now_ms - WINDOW_MS)?
    };
    lines.extend(batch);
    lines.sort_by_key(|l| l.id);
    lines.dedup_by_key(|l| l.id);
    let mut input = build_prompt(
        &brief,
        &history,
        &meeting.context,
        &recent,
        &lines,
        settings.meeting_sensitivity,
        !state.tools_unavailable,
    );
    let judge_gap_ms = i64::from(settings.meeting_observer_interval_secs.clamp(10, 120)) * 1000;
    if now_ms.saturating_sub(state.last_judge_ms) < judge_gap_ms {
        return Ok(None);
    }
    let jev_active = !catching_up && super::jev::eligible(settings);
    if jev_active && latest <= state.jev_through {
        return Ok(None);
    }
    let mode = if catching_up {
        mm.trace_tester(
            session_id,
            TesterTraceKind::DetectorBypassed {
                reason: "Catching up on older transcript evidence without nudging".into(),
            },
        );
        GateMode::Evaluate { hint: None }
    } else {
        let (mode, call) = detector.decide(&input.user, memory_due, mm, session_id);
        let jev_ok = match call {
            JevCall::Skipped => None,
            JevCall::Failed => Some(false),
            JevCall::Answered(check) => {
                state.jev = Some(check);
                Some(true)
            }
        };
        if let Some(ok) = jev_ok {
            let one = llm_client::LlmUsage {
                requests: 1,
                ..Default::default()
            };
            if let Err(e) = store.record_usage(meeting_id, Purpose::Jev, JEV_MODEL, ok, &one) {
                warn!("observer: record jev usage: {e:#}");
            }
        }
        mode
    };
    if matches!(mode, GateMode::Deferred) {
        state.jev_through = latest;
        return Ok(None);
    }
    state.last_judge_ms = now_ms;
    if !detector.assistant_ready() || !mm.is_live() || mm.state().session_id != Some(session_id) {
        return Ok(None);
    }
    mode.instruct(&mut input.system);
    input.purpose = match (&mode, catching_up) {
        (_, true) => Purpose::CatchUp,
        (GateMode::Memory, _) => Purpose::Memory,
        (GateMode::Evaluate { hint: Some(_) }, _) => Purpose::Check,
        _ => Purpose::Fallback,
    };
    let ctx = ToolContext {
        store: Arc::new(store.clone_handle()),
        project_id: meeting.project_id,
        meeting_id,
    };

    mm.trace_tester(session_id, TesterTraceKind::Judging);
    let verdict = match judge.judge(&input, &ctx, !state.tools_unavailable) {
        Ok(v) => v,
        Err(JudgeError::ToolsRejected) => {
            warn!("observer: provider rejected tools; falling back without tools");
            state.tools_unavailable = true;
            let purpose = input.purpose;
            let mut input = build_prompt(
                &brief,
                &history,
                &meeting.context,
                &recent,
                &lines,
                settings.meeting_sensitivity,
                false,
            );
            mode.instruct(&mut input.system);
            input.purpose = purpose;
            mm.trace_tester(
                session_id,
                TesterTraceKind::Error {
                    message: "Provider rejected tools; retrying without tools".into(),
                },
            );
            mm.trace_tester(session_id, TesterTraceKind::Judging);
            match judge.judge(&input, &ctx, false) {
                Ok(v) => v,
                Err(e) => {
                    state.consecutive_failures += 1;
                    state.last_error = Some(short_error(&format!("{e:?}")));
                    mm.trace_tester(session_id, TesterTraceKind::Error { message: "Main model request failed after retrying without tools. See assistant status.".into() });
                    warn!("observer: judge failed after fallback: {e:?}");
                    return Ok(None);
                }
            }
        }
        Err(e) => {
            state.consecutive_failures += 1;
            state.last_error = Some(short_error(&format!("{e:?}")));
            mm.trace_tester(
                session_id,
                TesterTraceKind::Error {
                    message: "Main model request failed. See assistant status.".into(),
                },
            );
            warn!("observer: judge failed: {e:?}");
            return Ok(None);
        }
    };
    let current = mm.state();
    let suppression = if catching_up {
        Some(("catching_up", 0))
    } else if current.session_id != Some(session_id) || !mm.is_live() || !detector.assistant_ready()
    {
        Some(("inactive_session", 0))
    } else if matches!(mode, GateMode::Memory) {
        Some(("jev_quiet", 0))
    } else {
        suppression_reason(&verdict, now_ms, &recent)
    };
    let (outcome, remaining_ms) = suppression.unwrap_or(("emitted", 0));
    let text = verdict
        .text
        .as_deref()
        .map(|text| card_text(verdict.kind.as_deref().unwrap_or_default(), text));
    let check = ObserverCheck {
        session_id,
        ts: now_ms,
        through_line_id: judged_through,
        outcome: outcome.into(),
        remaining_ms,
        kind: verdict.kind,
        text,
        reason: verdict.reason,
        context_json: serde_json::to_string(&verdict.live_context.bounded())?,
    };
    let iv = store.save_observer_check(&check)?;
    mm.trace_tester(
        session_id,
        TesterTraceKind::Judged {
            outcome: outcome.into(),
            reason: check.reason.clone(),
            remaining_ms,
        },
    );
    state.consecutive_failures = 0;
    state.last_error = None;
    info!("observer: meeting={meeting_id} session={session_id} through_line={judged_through} outcome={outcome} remaining_ms={remaining_ms} kind={:?} candidate={:?} reason={}", check.kind, check.text, check.reason);
    state.remember(now_ms, &check.reason, iv.is_some());
    Ok(iv)
}

#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct InterventionPayload {
    pub meeting_id: i64,
    pub intervention: Intervention,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct ObserverStatusPayload {
    pub meeting_id: Option<i64>,
    pub tools_unavailable: bool,
    pub consecutive_failures: u32,
    /// Most recent judge failure, if the last tick failed.
    pub last_error: Option<String>,
    /// One of `ready|disabled|no_provider|unsupported_provider|missing_api_key|no_model`.
    pub readiness: String,
    /// Provider label for readiness states that name one.
    pub detail: Option<String>,
    /// The observer's last few verdicts for this meeting, oldest first.
    pub thoughts: Vec<ObserverThought>,
    /// Jev's latest answer, when Jev gates this meeting.
    pub jev: Option<JevCheck>,
    /// Every AI call this meeting has made so far.
    pub usage: Option<MeetingUsage>,
}

/// Sleep between ticks: the interval, or `BACKOFF_FACTOR` times it after
/// `BACKOFF_AFTER_FAILURES` consecutive failures.
pub fn tick_delay_secs(interval_secs: u64, consecutive_failures: u32) -> u64 {
    if consecutive_failures >= BACKOFF_AFTER_FAILURES {
        interval_secs * BACKOFF_FACTOR
    } else {
        interval_secs
    }
}

pub fn spawn_observer(
    app: AppHandle,
    store: Arc<ProjectStore>,
    mm: Arc<MeetingManager>,
    judge: Arc<dyn Judge>,
) {
    let (wake_tx, wake_rx) = std::sync::mpsc::sync_channel(1);
    mm.install_observer_wake(wake_tx);
    let _ = std::thread::Builder::new()
        .name("meet-observer".into())
        .spawn(move || {
            let detector = JevDetector::new(app.clone());
            let mut state = ObserverState::default();
            let mut last_meeting: Option<i64> = None;
            let mut last_status: Option<ObserverStatusPayload> = None;
            loop {
                let settings = crate::settings::get_settings(&app);
                let interval = if super::jev::eligible(&settings) {
                    JEV_TICK_SECS
                } else {
                    settings.meeting_observer_interval_secs.clamp(10, 120) as u64
                };
                let delay = tick_delay_secs(interval, state.consecutive_failures);
                let backoff = state.consecutive_failures >= BACKOFF_AFTER_FAILURES;
                mm.set_observer_backoff(backoff);
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(delay);
                loop {
                    let wait = deadline.saturating_duration_since(std::time::Instant::now());
                    match wake_rx.recv_timeout(wait) {
                        Ok((mid, sid)) => {
                            let current = mm.state();
                            if !backoff
                                && current.meeting_id == Some(mid)
                                && current.session_id == Some(sid)
                                && current.input_mode
                                    == Some(super::store::MeetingInputMode::TextTester)
                                && mm.is_live()
                            {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                // Re-read: settings may have changed while sleeping.
                let settings = crate::settings::get_settings(&app);
                let st = mm.state();
                if st.meeting_id != last_meeting {
                    state = ObserverState::default();
                    last_meeting = st.meeting_id;
                }
                let ready = readiness(&settings);
                let mut emit_status = |state: &ObserverState| {
                    let payload = ObserverStatusPayload {
                        meeting_id: st.meeting_id,
                        tools_unavailable: state.tools_unavailable,
                        consecutive_failures: state.consecutive_failures,
                        last_error: state.last_error.clone(),
                        readiness: ready.code().to_string(),
                        detail: ready.detail(),
                        thoughts: state.thoughts.clone(),
                        jev: state.jev.clone(),
                        usage: st.meeting_id.and_then(|id| store.meeting_usage(id).ok()),
                    };
                    if last_status.as_ref() != Some(&payload) {
                        let _ = payload.clone().emit(&app);
                        last_status = Some(payload);
                    }
                };
                emit_status(&state);
                if !(settings.meeting_assistant_enabled && mm.is_live()) {
                    continue;
                }
                if ready != Readiness::Ready {
                    continue;
                }
                if let Some(mid) = st.meeting_id {
                    // Files the meeting first, so this tick's judge already
                    // sees the project's brief and sources.
                    mm.maybe_file_live(mid);
                }
                let now = chrono::Utc::now().timestamp_millis();
                match tick(
                    &store,
                    judge.as_ref(),
                    &detector,
                    &mm,
                    &settings,
                    &mut state,
                    now,
                ) {
                    Ok(Some(iv)) => {
                        if settings.meeting_beep_enabled {
                            audio_feedback::play_alert_sound(&app);
                        }
                        if let Some(mid) = st.meeting_id {
                            let _ = (InterventionPayload {
                                meeting_id: mid,
                                intervention: iv.clone(),
                            })
                            .emit(&app);
                            crate::intervention::show(&app, mid, &iv);
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        state.consecutive_failures += 1;
                        state.last_error = Some(short_error(&format!("{e:#}")));
                        if let Some(sid) = st.session_id {
                            mm.trace_tester(
                                sid,
                                TesterTraceKind::Error {
                                    message:
                                        "Assistant check could not complete. See assistant status."
                                            .into(),
                                },
                            );
                        }
                        warn!("observer: outcome=check_failed error={e:#}");
                    }
                }
                emit_status(&state);
            }
        });
}
