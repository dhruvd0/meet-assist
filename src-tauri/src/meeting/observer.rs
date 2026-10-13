//! Observer: every N seconds while a meeting is live, ask the model one
//! question with a small base prompt and tool access; emit at most one
//! intervention per cooldown window.

use crate::audio_feedback;
use crate::llm_client::{self, ToolLoopError};
use crate::meeting::session::MeetingManager;
use crate::meeting::store::{Intervention, ProjectStore, TranscriptLine};
use crate::meeting::tools::{handle_tool, tool_defs, ToolContext};
use crate::settings::{AppSettings, MeetingSensitivity};
use anyhow::Result;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use specta::Type;
use std::sync::Arc;
use tauri::AppHandle;
use tauri_specta::Event;

pub const COOLDOWN_MS: i64 = 60_000;
pub const WINDOW_MS: i64 = 180_000;
pub const MAX_TOOL_CALLS: usize = 5;
pub const DEDUPE_RECENT: usize = 3;
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
    serde_json::from_str::<Verdict>(t).map_err(|e| format!("verdict parse: {e}"))
}

pub struct JudgeInput {
    pub system: String,
    pub user: String,
}

#[derive(Debug)]
pub enum JudgeError {
    ToolsRejected,
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
    pub app: AppHandle,
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
        let defs = if allow_tools { tool_defs() } else { Vec::new() };
        let handler = |c: &llm_client::ToolCall| handle_tool(tools_ctx, c);
        let fut = llm_client::send_with_tools(
            &provider,
            api_key,
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
    user.push_str(if context.trim().is_empty() {
        "(none)"
    } else {
        context
    });
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
    pub last_seen_line_ts: i64,
    pub consecutive_failures: u32,
    pub tools_unavailable: bool,
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
    let latest = store.latest_line_ts(meeting_id)?.unwrap_or(0);
    if latest <= state.last_seen_line_ts {
        return Ok(None);
    }
    state.last_seen_line_ts = latest;

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
                    warn!("observer: judge failed after fallback: {e:?}");
                    return Ok(None);
                }
            }
        }
        Err(e) => {
            state.consecutive_failures += 1;
            warn!("observer: judge failed: {e:?}");
            return Ok(None);
        }
    };
    state.consecutive_failures = 0;
    if !should_emit(&verdict, now_ms, state, &recent) {
        info!("observer: no intervention ({})", verdict.reason);
        return Ok(None);
    }
    let kind = verdict.kind.unwrap_or_default();
    let text = verdict.text.unwrap_or_default();
    let iv = store.add_intervention(session_id, &kind, text.trim(), &verdict.reason)?;
    state.last_intervention_ts = now_ms;
    Ok(Some(iv))
}

#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct InterventionPayload {
    pub meeting_id: i64,
    pub intervention: Intervention,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct ObserverStatusPayload {
    pub meeting_id: Option<i64>,
    pub tools_unavailable: bool,
    pub consecutive_failures: u32,
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
            loop {
                let settings = crate::settings::get_settings(&app);
                let interval = settings.meeting_observer_interval_secs.clamp(10, 120) as u64;
                std::thread::sleep(std::time::Duration::from_secs(interval));
                if !mm.is_live() {
                    continue;
                }
                let st = mm.state();
                if st.meeting_id != last_meeting {
                    state = ObserverState::default();
                    last_meeting = st.meeting_id;
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
                        }
                        crate::intervention::show(&app, &iv);
                    }
                    Ok(None) => {}
                    Err(e) => warn!("observer tick error: {e:#}"),
                }
                let _ = (ObserverStatusPayload {
                    meeting_id: st.meeting_id,
                    tools_unavailable: state.tools_unavailable,
                    consecutive_failures: state.consecutive_failures,
                })
                .emit(&app);
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
        fn start(&self, _tx: mpsc::Sender<Utterance>) -> anyhow::Result<Box<dyn CaptureHandle>> {
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
            last_seen_line_ts: 0,
            consecutive_failures: 0,
            tools_unavailable: false,
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
    fn parse_verdict_strips_fences() {
        let v: Verdict = parse_verdict(
            "```json\n{\"intervene\":true,\"kind\":\"info\",\"text\":\"t\",\"reason\":\"r\"}\n```",
        )
        .unwrap();
        assert!(v.intervene);
        assert!(parse_verdict("not json").is_err());
    }
}
