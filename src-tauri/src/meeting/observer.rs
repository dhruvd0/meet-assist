//! Observer: every N seconds while a meeting is live, ask the model one
//! question with a small base prompt and tool access; emit at most one
//! intervention per cooldown window.

use crate::audio_feedback;
use crate::llm_client::{self, ToolLoopError};
use crate::meeting::session::MeetingManager;
use crate::meeting::store::{Intervention, ProjectStore, TranscriptLine};
use crate::meeting::tools::{handle_tool, tool_defs, ToolContext};
use crate::settings::{AppSettings, MeetingSensitivity, PostProcessProvider};
use anyhow::Result;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use specta::Type;
use std::sync::{Arc, Mutex};
use tauri::AppHandle;
use tauri_specta::Event;

pub const COOLDOWN_MS: i64 = 60_000;
pub const WINDOW_MS: i64 = 180_000;
pub const MAX_TOOL_CALLS: usize = 5;
pub const DEDUPE_RECENT: usize = 3;
/// Max characters of the meeting's context pasted into the prompt; the model
/// can read sources and the transcript through tools for more.
pub const CONTEXT_MAX_CHARS: usize = 4096;
/// Max characters of an intervention card's text.
pub const CARD_TEXT_MAX_CHARS: usize = 280;
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

fn is_local(provider: &PostProcessProvider) -> bool {
    let u = provider.base_url.to_ascii_lowercase();
    u.contains("localhost") || u.contains("127.0.0.1")
}

pub fn readiness(settings: &AppSettings) -> Readiness {
    if !settings.meeting_assistant_enabled {
        return Readiness::Disabled;
    }
    let Some(provider) = settings.meeting_provider() else {
        return Readiness::NoProvider;
    };
    let label = provider.label.clone();
    if provider.id == crate::settings::APPLE_INTELLIGENCE_PROVIDER_ID
        || !provider.base_url.trim().starts_with("http")
    {
        return Readiness::UnsupportedProvider(label);
    }
    if settings.meeting_api_key().trim().is_empty() && !is_local(provider) {
        return Readiness::MissingApiKey(label);
    }
    if settings
        .meeting_model()
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
    /// Keyed by provider id, base URL and a hash of the API key.
    client: Mutex<Option<(String, reqwest::Client)>>,
}

impl LlmJudge {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            client: Mutex::new(None),
        }
    }

    fn client_for(
        &self,
        provider: &PostProcessProvider,
        api_key: &str,
    ) -> Result<reqwest::Client, String> {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        api_key.hash(&mut h);
        let key = format!("{}|{}|{:x}", provider.id, provider.base_url, h.finish());
        let mut slot = self.client.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((k, c)) = slot.as_ref() {
            if *k == key {
                return Ok(c.clone());
            }
        }
        let c = llm_client::create_tool_client(provider, api_key)?;
        *slot = Some((key, c.clone()));
        Ok(c)
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
            .meeting_provider()
            .cloned()
            .ok_or_else(|| JudgeError::Other("no provider configured".into()))?;
        let model = settings
            .meeting_model()
            .ok_or_else(|| JudgeError::Other("no model configured".into()))?;
        let api_key = settings.meeting_api_key();
        let client = self
            .client_for(&provider, &api_key)
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
            "Sensitivity HIGH: also offer ideas freely when an obvious alternative is untried."
        }
    };
    let tools_rule = if tools_available {
        "You have tools to list and search project sources and read transcripts. Use them to check project facts before asserting anything; prefer silence over an unverified claim."
    } else {
        "No tools are available this turn. Do not assert project facts you cannot see in the prompt."
    };
    let system = format!(
        "You are a silent meeting copilot for one participant. You read their live transcript and answer ONE question: should you speak up right now? Silence is the default answer.\n\
         Speakers: `me` is the user you assist; `them` is everyone else on the call.\n\
         Signals: the same point restated three or more times without a decision (kind \"loop\"); a long pause or someone saying they do not know how to proceed (kind \"stuck\"); a factual question nobody answers (kind \"info\"); the discussion converging while an obvious alternative is untried (kind \"idea\").\n\
         {sensitivity_rule}\n{tools_rule}\n\
         Answer with JSON only, exactly this shape: {{\"intervene\": boolean, \"kind\": \"loop\"|\"stuck\"|\"info\"|\"idea\"|null, \"text\": string|null, \"reason\": string}}. \
         \"text\" is one to three sentences addressed to the participant, or null. Never repeat a recent intervention."
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meeting::capture::Utterance;
    use crate::meeting::session::{CaptureHandle, CaptureSource, MeetingManager, Transcriber};
    use crate::meeting::store::{Intervention, ProjectStore};
    use std::sync::atomic::AtomicBool;
    use std::sync::{mpsc, Arc, Mutex};

    struct NoCapture;
    struct NoHandle;
    impl CaptureHandle for NoHandle {
        fn stop(self: Box<Self>) {}
        fn has_failed(&self) -> bool {
            false
        }
    }
    impl CaptureSource for NoCapture {
        fn start(
            &self,
            _meeting_id: i64,
            _tx: mpsc::Sender<Utterance>,
        ) -> anyhow::Result<Box<dyn CaptureHandle>> {
            Ok(Box::new(NoHandle))
        }
    }
    struct NoTranscriber;
    impl Transcriber for NoTranscriber {
        fn transcribe(&self, _s: Vec<f32>) -> anyhow::Result<String> {
            Ok(String::new())
        }
    }

    struct ScriptedJudge {
        verdicts: Mutex<Vec<Result<Verdict, JudgeError>>>,
        calls: Mutex<Vec<bool>>,
    }
    impl Judge for ScriptedJudge {
        fn judge(
            &self,
            _i: &JudgeInput,
            _c: &ToolContext,
            allow_tools: bool,
        ) -> Result<Verdict, JudgeError> {
            self.calls.lock().unwrap().push(allow_tools);
            self.verdicts.lock().unwrap().remove(0)
        }
    }

    fn setup() -> (
        tempfile::TempDir,
        Arc<ProjectStore>,
        Arc<MeetingManager>,
        i64,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(ProjectStore::open(dir.path().join("meet.db")).unwrap());
        let mm = Arc::new(MeetingManager::new_for_test(
            store.clone(),
            Arc::new(NoTranscriber),
            Arc::new(NoCapture),
            Arc::new(AtomicBool::new(false)),
        ));
        let m = mm.start(None, None).unwrap();
        let sid = mm.state().session_id.unwrap();
        store
            .append_line(sid, "me", "we keep going around on the retry budget", 1000)
            .unwrap();
        (dir, store, mm, m.id)
    }

    fn v(intervene: bool, kind: &str, text: &str) -> Verdict {
        Verdict {
            intervene,
            kind: Some(kind.into()),
            text: Some(text.into()),
            reason: "r".into(),
        }
    }

    #[test]
    fn prompt_contains_ordered_sections_and_schema() {
        let lines = vec![TranscriptLine {
            id: 1,
            session_id: 1,
            ts: 1000,
            speaker: "me".into(),
            text: "hello".into(),
        }];
        let p = build_prompt(
            "Project brief",
            Some(("Old meeting", Some("old summary"))),
            "agenda",
            &[],
            &lines,
            MeetingSensitivity::Low,
            true,
        );
        let b = p.user.find("Project brief").unwrap();
        let o = p.user.find("old summary").unwrap();
        let a = p.user.find("agenda").unwrap();
        let h = p.user.find("hello").unwrap();
        assert!(b < o && o < a && a < h);
        assert!(p.system.contains("\"intervene\""));
        assert!(
            p.system
                .to_lowercase()
                .contains("only on loops and explicit stuck")
                || p.system.to_lowercase().contains("low")
        );
    }

    #[test]
    fn should_emit_applies_cooldown_and_dedupe() {
        let state = ObserverState {
            last_intervention_ts: 100_000,
            last_seen_line_id: 0,
            consecutive_failures: 0,
            tools_unavailable: false,
            last_error: None,
        };
        let recent = vec![Intervention {
            id: 1,
            session_id: 1,
            ts: 100_000,
            kind: "loop".into(),
            text: "You said that twice".into(),
            reason: "".into(),
            dismissed: false,
        }];
        assert!(
            !should_emit(&v(true, "loop", "new text"), 130_000, &state, &recent),
            "within 60 s cooldown"
        );
        assert!(should_emit(
            &v(true, "loop", "new text"),
            170_000,
            &state,
            &recent
        ));
        assert!(
            !should_emit(
                &v(true, "loop", "  you said THAT twice "),
                170_000,
                &state,
                &recent
            ),
            "dedupe"
        );
        assert!(!should_emit(
            &v(false, "loop", "x"),
            170_000,
            &state,
            &recent
        ));
        assert!(!should_emit(
            &v(true, "weird", "x"),
            170_000,
            &state,
            &recent
        ));
        assert!(!should_emit(
            &Verdict {
                intervene: true,
                kind: Some("idea".into()),
                text: None,
                reason: "".into()
            },
            170_000,
            &state,
            &recent
        ));
    }

    #[test]
    fn tick_stores_intervention_and_skips_when_no_new_lines() {
        let (_d, store, mm, mid) = setup();
        let judge = ScriptedJudge {
            verdicts: Mutex::new(vec![Ok(v(true, "loop", "You are circling"))]),
            calls: Mutex::new(vec![]),
        };
        let settings = crate::settings::get_default_settings();
        let mut st = ObserverState::default();
        let out = tick(&store, &judge, &mm, &settings, &mut st, 200_000).unwrap();
        assert_eq!(out.unwrap().text, "You are circling");
        assert_eq!(store.list_interventions(mid).unwrap().len(), 1);
        // No new lines since: judge must not be called again.
        let out = tick(&store, &judge, &mm, &settings, &mut st, 300_000).unwrap();
        assert!(out.is_none());
        assert_eq!(judge.calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn tick_falls_back_without_tools_and_remembers() {
        let (_d, store, mm, _mid) = setup();
        let judge = ScriptedJudge {
            verdicts: Mutex::new(vec![
                Err(JudgeError::ToolsRejected),
                Ok(v(false, "loop", "")),
            ]),
            calls: Mutex::new(vec![]),
        };
        let settings = crate::settings::get_default_settings();
        let mut st = ObserverState::default();
        let out = tick(&store, &judge, &mm, &settings, &mut st, 200_000).unwrap();
        assert!(out.is_none());
        assert_eq!(*judge.calls.lock().unwrap(), vec![true, false]);
        assert!(st.tools_unavailable);
    }

    #[test]
    fn tick_counts_failures_and_bad_json_does_not_error() {
        let (_d, store, mm, _mid) = setup();
        let judge = ScriptedJudge {
            verdicts: Mutex::new(vec![Err(JudgeError::Other("boom".into()))]),
            calls: Mutex::new(vec![]),
        };
        let settings = crate::settings::get_default_settings();
        let mut st = ObserverState::default();
        let out = tick(&store, &judge, &mm, &settings, &mut st, 200_000).unwrap();
        assert!(out.is_none());
        assert_eq!(st.consecutive_failures, 1);
    }

    #[test]
    fn failed_tick_retries_same_window() {
        let (_d, store, mm, _mid) = setup();
        let judge = ScriptedJudge {
            verdicts: Mutex::new(vec![
                Err(JudgeError::Other("timeout".into())),
                Ok(v(false, "loop", "")),
            ]),
            calls: Mutex::new(vec![]),
        };
        let settings = crate::settings::get_default_settings();
        let mut st = ObserverState::default();
        assert!(tick(&store, &judge, &mm, &settings, &mut st, 200_000)
            .unwrap()
            .is_none());
        assert_eq!(st.last_seen_line_id, 0, "failure must not advance");
        // No new lines, but the failed window is retried.
        assert!(tick(&store, &judge, &mm, &settings, &mut st, 220_000)
            .unwrap()
            .is_none());
        assert_eq!(judge.calls.lock().unwrap().len(), 2);
        assert_eq!(st.consecutive_failures, 0);
        assert!(st.last_seen_line_id > 0);
    }

    #[test]
    fn tick_caps_card_text() {
        let (_d, store, mm, mid) = setup();
        let long = "é".repeat(400);
        let judge = ScriptedJudge {
            verdicts: Mutex::new(vec![Ok(v(true, "info", &long))]),
            calls: Mutex::new(vec![]),
        };
        let settings = crate::settings::get_default_settings();
        let mut st = ObserverState::default();
        let iv = tick(&store, &judge, &mm, &settings, &mut st, 200_000)
            .unwrap()
            .unwrap();
        assert_eq!(iv.text.chars().count(), CARD_TEXT_MAX_CHARS);
        assert_eq!(
            store.list_interventions(mid).unwrap()[0]
                .text
                .chars()
                .count(),
            CARD_TEXT_MAX_CHARS
        );
    }

    #[test]
    fn backoff_after_repeated_failures() {
        assert_eq!(tick_delay_secs(20, 0), 20);
        assert_eq!(tick_delay_secs(20, 4), 20);
        assert_eq!(tick_delay_secs(20, 5), 80);
        assert_eq!(tick_delay_secs(20, 9), 80);
    }

    #[test]
    fn prompt_explains_speakers_and_caps_context() {
        let ctx = "x".repeat(CONTEXT_MAX_CHARS + 500);
        let p = build_prompt("", None, &ctx, &[], &[], MeetingSensitivity::Medium, true);
        assert!(p.system.contains("`me` is the user you assist"));
        assert!(p.system.contains("`them` is everyone else"));
        assert!(p.user.contains("[truncated]"));
        assert!(!p.user.contains(&"x".repeat(CONTEXT_MAX_CHARS + 1)));
        assert!(p.user.contains(&"x".repeat(CONTEXT_MAX_CHARS)));
        let short = build_prompt(
            "",
            None,
            "agenda",
            &[],
            &[],
            MeetingSensitivity::Medium,
            true,
        );
        assert!(!short.user.contains("[truncated]"));
    }

    fn settings_with(id: &str, base_url: &str, key: &str, model: &str) -> AppSettings {
        let mut s = crate::settings::get_default_settings();
        s.meeting_assistant_enabled = true;
        s.post_process_providers.retain(|p| p.id != id);
        s.post_process_providers.push(PostProcessProvider {
            id: id.into(),
            label: format!("{id} label"),
            base_url: base_url.into(),
            allow_base_url_edit: true,
            models_endpoint: None,
            supports_structured_output: false,
            api_format: crate::settings::PostProcessApiFormat::OpenAi,
        });
        s.meeting_provider_id = id.into();
        s.post_process_api_keys.insert(id.into(), key.into());
        if model.is_empty() {
            s.post_process_models.remove(id);
        } else {
            s.post_process_models.insert(id.into(), model.into());
        }
        s
    }

    #[test]
    fn readiness_reports_each_blocker() {
        let mut off = settings_with("openai", "https://api.openai.com/v1", "k", "gpt");
        off.meeting_assistant_enabled = false;
        assert_eq!(readiness(&off), Readiness::Disabled);

        let mut none = settings_with("openai", "https://api.openai.com/v1", "k", "gpt");
        none.meeting_provider_id = "missing".into();
        none.post_process_provider_id = "also-missing".into();
        assert_eq!(readiness(&none), Readiness::NoProvider);

        let apple = settings_with(
            crate::settings::APPLE_INTELLIGENCE_PROVIDER_ID,
            "apple-intelligence://local",
            "",
            "m",
        );
        assert_eq!(readiness(&apple).code(), "unsupported_provider");
        let odd = settings_with("weird", "file:///x", "k", "m");
        assert_eq!(
            readiness(&odd),
            Readiness::UnsupportedProvider("weird label".into())
        );

        let nokey = settings_with("openai", "https://api.openai.com/v1", " ", "gpt");
        assert_eq!(
            readiness(&nokey),
            Readiness::MissingApiKey("openai label".into())
        );
        assert_eq!(readiness(&nokey).detail().as_deref(), Some("openai label"));

        let local = settings_with("custom", "http://localhost:11434/v1", "", "llama");
        assert_eq!(readiness(&local), Readiness::Ready);
        let loopback = settings_with("lm", "http://127.0.0.1:1234/v1", "", "m");
        assert_eq!(readiness(&loopback), Readiness::Ready);

        let nomodel = settings_with("openai", "https://api.openai.com/v1", "k", "");
        assert_eq!(readiness(&nomodel).code(), "no_model");

        let ok = settings_with("openai", "https://api.openai.com/v1", "k", "gpt");
        assert_eq!(readiness(&ok), Readiness::Ready);
        assert_eq!(readiness(&ok).code(), "ready");
    }

    #[test]
    fn parse_verdict_tolerates_surrounding_prose() {
        let obj = "{\"intervene\":false,\"kind\":null,\"text\":null,\"reason\":\"quiet\"}";
        let lead = parse_verdict(&format!("Here is my answer: {obj}")).unwrap();
        assert_eq!(lead.reason, "quiet");
        let trail = parse_verdict(&format!("{obj}\nHope that helps!")).unwrap();
        assert_eq!(trail.reason, "quiet");
        let fenced = parse_verdict(&format!("Sure.\n```json\n{obj}\n```\nDone.")).unwrap();
        assert_eq!(fenced.reason, "quiet");
    }

    #[test]
    fn parse_verdict_strips_fences() {
        let v: Verdict = parse_verdict(
            "```json\n{\"intervene\":true,\"kind\":\"info\",\"text\":\"t\",\"reason\":\"r\"}\n```",
        )
        .unwrap();
        assert!(v.intervene);
        assert!(parse_verdict("not json").is_err());
    }
}
