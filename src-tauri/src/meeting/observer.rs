//! Observer: every N seconds while a meeting is live, ask the model one
//! question with a small base prompt and tool access; emit at most one
//! intervention per cooldown window.

use crate::audio_feedback;
use crate::llm_client::{self, ToolLoopError};
use crate::meeting::session::MeetingManager;
use crate::meeting::store::{Intervention, ProjectStore, TranscriptLine};
use crate::meeting::tools::{handle_tool, tool_defs, ToolContext};
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
/// Max characters of an intervention card's text.
pub const CARD_TEXT_MAX_CHARS: usize = 100;
/// Consecutive judge failures after which the observer slows down.
pub const BACKOFF_AFTER_FAILURES: u32 = 5;
/// Interval multiplier while backing off.
pub const BACKOFF_FACTOR: u64 = 4;
const VALID_KINDS: [&str; 4] = ["loop", "stuck", "info", "idea"];

#[derive(Debug, Clone, Deserialize)]
pub struct Verdict {
    pub intervene: bool,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub reason: String,
}

pub fn verdict_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "intervene": { "type": "boolean" },
            "kind": { "type": ["string", "null"], "enum": ["loop", "stuck", "info", "idea", null] },
            "text": { "type": ["string", "null"] },
            "reason": { "type": "string" }
        },
        "required": ["intervene", "kind", "text", "reason"],
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
    // Models sometimes wrap the object in prose; keep the outermost braces.
    let t = match (t.find('{'), t.rfind('}')) {
        (Some(a), Some(b)) if a < b => &t[a..=b],
        _ => t,
    };
    serde_json::from_str::<Verdict>(t).map_err(|e| format!("verdict parse: {e}"))
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
    let u = provider.base_url.to_ascii_lowercase();
    u.contains("localhost") || u.contains("127.0.0.1")
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
        );
        let budget = std::time::Duration::from_secs(15);
        let text =
            tauri::async_runtime::block_on(async move { tokio::time::timeout(budget, fut).await });
        match text {
            Err(_) => Err(JudgeError::Other("judge timed out".into())),
            Ok(Err(ToolLoopError::ToolsRejected(_))) => Err(JudgeError::ToolsRejected),
            Ok(Err(ToolLoopError::Other(e))) => Err(JudgeError::Other(e)),
            Ok(Ok(t)) => parse_verdict(&t).map_err(JudgeError::Other),
        }
    }
}

pub fn build_prompt(
    brief: &str,
    prev_meeting: Option<(&str, Option<&str>)>,
    context: &str,
    recent: &[Intervention],
    lines: &[TranscriptLine],
    sensitivity: MeetingSensitivity,
    tools_available: bool,
) -> JudgeInput {
    let sensitivity_rule = match sensitivity {
        MeetingSensitivity::Low => {
            "Sensitivity LOW: intervene only on loops and explicit stuck moments."
        }
        MeetingSensitivity::Medium => {
            "Sensitivity MEDIUM: intervene on loops, stuck moments, and clearly missing facts."
        }
        MeetingSensitivity::High => {
            "Sensitivity HIGH: also offer an idea when a specific, concrete alternative is clearly untried."
        }
    };
    let tools_rule = if tools_available {
        "You have tools to list and search project sources and read transcripts. Use them to check project facts before asserting anything; prefer silence over an unverified claim."
    } else {
        "No tools are available this turn. Do not assert project facts you cannot see in the prompt."
    };
    let system = format!(
        "You are a silent meeting copilot for one participant. You read their live transcript and answer ONE question: should you speak up right now? Silence is the default answer; most checks end in silence.\n\
         Speakers: `me` is the user you assist; `them` is everyone else on the call.\n\
         The transcript is raw speech-to-text: it has fragments, misheard words, mixed languages (for example Hindi and English), and the same sentence sometimes appears twice. None of that is a reason to speak, and never comment on the conversation being unclear.\n\
         Signals:\n\
         - kind \"loop\": the same open question comes back in at least two separate exchanges at least two minutes apart, still without a decision. Repeated or echoed lines are not a loop.\n\
         - kind \"stuck\": someone says they do not know how to proceed, and nobody offers a way forward.\n\
         - kind \"info\": a factual question nobody answers that you can answer.\n\
         - kind \"idea\": the discussion is converging while a specific alternative is clearly untried.\n\
         {sensitivity_rule}\n{tools_rule}\n\
         Never give generic process advice (summarize, document, align, clarify, take notes, ask for feedback). Only speak about a concrete point from this meeting.\n\
         Answer with JSON only, exactly this shape: {{\"intervene\": boolean, \"kind\": \"loop\"|\"stuck\"|\"info\"|\"idea\"|null, \"text\": string|null, \"reason\": string}}. \
         \"text\" is one short line of at most 12 words naming the concrete point, like a note passed under the table (shape: \"<the open point> still undecided — <one concrete next step>?\"), or null. Never repeat a recent intervention."
    );
    let mut user = String::new();
    user.push_str("## Project brief\n");
    user.push_str(if brief.trim().is_empty() {
        "(none)"
    } else {
        brief
    });
    user.push_str("\n\n## Previous meeting in this project\n");
    match prev_meeting {
        Some((title, Some(summary))) => user.push_str(&format!("{title}: {summary}")),
        Some((title, None)) => user.push_str(&format!("{title}: (no summary yet)")),
        None => user.push_str("(none)"),
    }
    user.push_str("\n\n## This meeting's context\n");
    if context.trim().is_empty() {
        user.push_str("(none)");
    } else {
        let (ctx, cut) = truncate_chars(context, CONTEXT_MAX_CHARS);
        user.push_str(ctx);
        if cut {
            user.push_str("\n[truncated]");
        }
    }
    user.push_str("\n\n## Recent interventions (do not repeat)\n");
    if recent.is_empty() {
        user.push_str("(none)");
    } else {
        for i in recent {
            user.push_str(&format!("- [{}] {}\n", i.kind, i.text));
        }
    }
    user.push_str("\n\n## Transcript (last 3 minutes)\n");
    for l in lines {
        user.push_str(&format!("[{}] {}: {}\n", l.ts, l.speaker, l.text));
    }
    JudgeInput { system, user }
}

#[derive(Debug, Default, Clone)]
pub struct ObserverState {
    pub last_intervention_ts: i64,
    /// Highest transcript line id judged successfully; failed ticks leave it
    /// alone so the same window is retried.
    pub last_seen_line_id: i64,
    pub consecutive_failures: u32,
    pub tools_unavailable: bool,
    /// Short description of the most recent judge failure, cleared on success.
    pub last_error: Option<String>,
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

pub fn should_emit(
    verdict: &Verdict,
    now_ms: i64,
    state: &ObserverState,
    recent: &[Intervention],
) -> bool {
    if !verdict.intervene {
        return false;
    }
    let Some(kind) = verdict.kind.as_deref() else {
        return false;
    };
    if !VALID_KINDS.contains(&kind) {
        return false;
    }
    let Some(text) = verdict
        .text
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    else {
        return false;
    };
    if now_ms - state.last_intervention_ts < COOLDOWN_MS {
        return false;
    }
    if recent
        .iter()
        .any(|i| i.kind == kind && now_ms - i.ts < SAME_KIND_COOLDOWN_MS)
    {
        return false;
    }
    let norm = text.to_lowercase();
    !recent
        .iter()
        .take(DEDUPE_RECENT)
        .any(|i| i.text.trim().to_lowercase() == norm)
}

pub fn tick(
    store: &ProjectStore,
    judge: &dyn Judge,
    mm: &MeetingManager,
    settings: &AppSettings,
    state: &mut ObserverState,
    now_ms: i64,
) -> Result<Option<Intervention>> {
    let st = mm.state();
    let (Some(meeting_id), Some(session_id)) = (st.meeting_id, st.session_id) else {
        return Ok(None);
    };
    let latest = store.latest_line_id(meeting_id)?.unwrap_or(0);
    if latest <= state.last_seen_line_id {
        return Ok(None);
    }

    let meeting = store
        .get_meeting(meeting_id)?
        .ok_or_else(|| anyhow::anyhow!("meeting gone"))?;
    let brief = match meeting.project_id {
        Some(pid) => store.get_project(pid)?.map(|p| p.brief).unwrap_or_default(),
        None => String::new(),
    };
    let prev = match meeting.project_id {
        Some(pid) => store.previous_meeting_summary(pid, meeting_id)?,
        None => None,
    };
    let recent = store.recent_interventions(meeting_id, DEDUPE_RECENT)?;
    let lines = store.lines_since(meeting_id, now_ms - WINDOW_MS)?;
    let input = build_prompt(
        &brief,
        prev.as_ref().map(|(t, s)| (t.as_str(), s.as_deref())),
        &meeting.context,
        &recent,
        &lines,
        settings.meeting_sensitivity,
        !state.tools_unavailable,
    );
    let ctx = ToolContext {
        store: Arc::new(store.clone_handle()),
        project_id: meeting.project_id,
        meeting_id,
    };

    let verdict = match judge.judge(&input, &ctx, !state.tools_unavailable) {
        Ok(v) => v,
        Err(JudgeError::ToolsRejected) => {
            warn!("observer: provider rejected tools; falling back without tools");
            state.tools_unavailable = true;
            let input = build_prompt(
                &brief,
                prev.as_ref().map(|(t, s)| (t.as_str(), s.as_deref())),
                &meeting.context,
                &recent,
                &lines,
                settings.meeting_sensitivity,
                false,
            );
            match judge.judge(&input, &ctx, false) {
                Ok(v) => v,
                Err(e) => {
                    state.consecutive_failures += 1;
                    state.last_error = Some(short_error(&format!("{e:?}")));
                    warn!("observer: judge failed after fallback: {e:?}");
                    return Ok(None);
                }
            }
        }
        Err(e) => {
            state.consecutive_failures += 1;
            state.last_error = Some(short_error(&format!("{e:?}")));
            warn!("observer: judge failed: {e:?}");
            return Ok(None);
        }
    };
    state.consecutive_failures = 0;
    state.last_error = None;
    state.last_seen_line_id = latest;
    if !should_emit(&verdict, now_ms, state, &recent) {
        info!("observer: no intervention ({})", verdict.reason);
        return Ok(None);
    }
    let kind = verdict.kind.unwrap_or_default();
    let text = verdict.text.unwrap_or_default();
    let (text, _) = truncate_chars(text.trim(), CARD_TEXT_MAX_CHARS);
    let iv = store.add_intervention(session_id, &kind, text, &verdict.reason)?;
    state.last_intervention_ts = now_ms;
    Ok(Some(iv))
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
    let _ = std::thread::Builder::new()
        .name("meet-observer".into())
        .spawn(move || {
            let mut state = ObserverState::default();
            let mut last_meeting: Option<i64> = None;
            let mut last_status: Option<ObserverStatusPayload> = None;
            loop {
                let settings = crate::settings::get_settings(&app);
                let interval = settings.meeting_observer_interval_secs.clamp(10, 120) as u64;
                let delay = tick_delay_secs(interval, state.consecutive_failures);
                std::thread::sleep(std::time::Duration::from_secs(delay));
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
                match tick(&store, judge.as_ref(), &mm, &settings, &mut state, now) {
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
                    Err(e) => warn!("observer tick error: {e:#}"),
                }
                emit_status(&state);
            }
        });
}
