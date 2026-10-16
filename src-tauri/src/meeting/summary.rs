//! End-of-meeting summaries: a structured per-session summary plus a rolling
//! summary that carries the meeting's state across sessions.

use crate::llm_client;
use crate::meeting::observer::History;
use crate::meeting::store::{ProjectStore, TranscriptLine};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use specta::Type;
use std::time::Duration;
use tauri::AppHandle;

/// Time budget for one summarizer LLM call.
const LLM_BUDGET: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
pub struct SessionSummary {
    #[serde(default)]
    pub decisions: Vec<String>,
    #[serde(default)]
    pub open_questions: Vec<String>,
    #[serde(default)]
    pub action_items: Vec<String>,
    #[serde(default)]
    pub recap: String,
    /// How well the talk used its time. Absent on summaries written before
    /// the report existed.
    #[serde(default)]
    pub efficiency: Option<EfficiencyReport>,
}

/// End-of-session efficiency report: where the conversation lost time.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
pub struct EfficiencyReport {
    /// 0–100: share of the talk that moved things forward.
    #[serde(default)]
    pub score: f64,
    /// One line on how the session went.
    #[serde(default)]
    pub verdict: String,
    #[serde(default)]
    pub issues: Vec<EfficiencyIssue>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
pub struct EfficiencyIssue {
    /// One of `EFFICIENCY_KINDS`.
    #[serde(default)]
    pub kind: String,
    /// The subject in 2–4 words.
    #[serde(default)]
    pub topic: String,
    /// What happened, in one sentence.
    #[serde(default)]
    pub note: String,
    /// Rough minutes spent on it.
    #[serde(default)]
    pub minutes: f64,
}

/// Ways a conversation loses time, as the report names them.
pub const EFFICIENCY_KINDS: [&str; 5] =
    ["circle", "argument", "tangent", "repetition", "recurring"];

/// Emitted when a session summary has been stored.
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct SessionSummaryPayload {
    pub meeting_id: i64,
    pub session_id: i64,
    pub summary: SessionSummary,
    pub rolling_summary: String,
}

/// Progress of a session summary that is not done yet: `generating` while
/// the job runs, `failed` (with `error`) when it gave up. Success arrives as
/// `SessionSummaryPayload`.
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct SessionSummaryStatusPayload {
    pub meeting_id: i64,
    pub session_id: i64,
    pub status: String,
    pub error: Option<String>,
}

pub trait Summarizer: Send + Sync {
    fn summarize_session(
        &self,
        title: &str,
        context: &str,
        history: &History,
        lines: &[TranscriptLine],
    ) -> Result<SessionSummary>;
    fn merge_rolling(&self, previous: Option<&str>, new_session: &SessionSummary)
        -> Result<String>;
}

pub fn session_summary_schema() -> Value {
    let list = json!({ "type": "array", "items": { "type": "string" } });
    json!({
        "type": "object",
        "properties": {
            "decisions": list,
            "open_questions": list,
            "action_items": list,
            "recap": { "type": "string" },
            "efficiency": {
                "type": "object",
                "properties": {
                    "score": { "type": "integer" },
                    "verdict": { "type": "string" },
                    "issues": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "kind": { "type": "string", "enum": EFFICIENCY_KINDS },
                                "topic": { "type": "string" },
                                "note": { "type": "string" },
                                "minutes": { "type": "integer" }
                            },
                            "required": ["kind", "topic", "note", "minutes"],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["score", "verdict", "issues"],
                "additionalProperties": false
            }
        },
        "required": ["decisions", "open_questions", "action_items", "recap", "efficiency"],
        "additionalProperties": false
    })
}

fn rolling_schema() -> Value {
    json!({
        "type": "object",
        "properties": { "rolling_summary": { "type": "string" } },
        "required": ["rolling_summary"],
        "additionalProperties": false
    })
}

/// (system, user) prompts for summarizing one session's transcript.
pub fn session_prompt(
    title: &str,
    context: &str,
    history: &History,
    lines: &[TranscriptLine],
) -> (String, String) {
    let system = "You summarize one session of a conversation (a meeting or call, interview, \
debate, class or talk, or a broadcast) for the user, who may have taken part or only listened. \
Transcript lines are labelled `me` (the user's microphone) or `them` (everything else they \
heard). Reply with JSON: `decisions` (what was decided or settled), `open_questions` (raised \
but unresolved), `action_items` (who does what, when known), `recap` (a three-line recap \
of what was said), and `efficiency`, a frank report on where the talk lost time: \
`score` (0-100, the share of the talk that moved things forward: new information, \
progress, or a settled point), `verdict` (one plain sentence), and `issues`, each with \
`kind`, `topic` (2-4 words), `note` (one sentence naming what happened), and `minutes` \
(rough time spent, from the timestamps). Kinds: \"circle\" (the same question or point \
returns without progress), \"argument\" (back-and-forth disagreement that produced no \
resolution or new information), \"tangent\" (a detour away from the subject), \
\"repetition\" (a point restated without adding anything), \"recurring\" (a question, \
problem or debate that the earlier conversations in this space or this conversation's \
earlier sessions already raised and that comes back here still unsettled; its note names \
where it came up before, e.g. \"also open in Hotel scoring on Oct 15\"; use it only \
with that history to point to). List only issues the \
transcript clearly shows, worst first; a focused session has none. Judge by the kind \
of conversation: a debate or broadcast is not inefficient for lacking decisions. \
None of these are issues: agreeing, acknowledging or confirming (\"that makes sense\", \
\"yeah\", \"okay\", restating a point to confirm it) is progress, not a circle or \
repetition; a `me` line that garbles a `them` line from the seconds before is the same \
audio heard twice by the microphone, not repetition; misheard or garbled words are \
speech-to-text errors and say nothing about the conversation. Score only what was \
said; when too little is clear to judge, give no issues and a verdict that says so. Use only what the transcript \
says; do not invent items. Use empty arrays when there is nothing to report."
        .to_string();
    let mut user = format!("Conversation: {title}\n");
    if !context.trim().is_empty() {
        user.push_str(&format!("Context:\n{}\n", context.trim()));
    }
    user.push('\n');
    history.render(&mut user);
    user.push_str("\n\nTranscript (minutes:seconds from the start):\n");
    let start = lines.first().map_or(0, |l| l.ts);
    for l in lines {
        let secs = (l.ts - start).max(0) / 1000;
        user.push_str(&format!(
            "[{}:{:02}] {}: {}\n",
            secs / 60,
            secs % 60,
            l.speaker,
            l.text
        ));
    }
    (system, user)
}

/// Plain-text rendering of a session summary, used as merge input.
pub fn format_session_summary_for_rolling(s: &SessionSummary) -> String {
    fn section(out: &mut String, name: &str, items: &[String]) {
        out.push_str(name);
        out.push_str(":\n");
        if items.is_empty() {
            out.push_str("- (none)\n");
        }
        for i in items {
            out.push_str(&format!("- {i}\n"));
        }
    }
    let mut out = String::new();
    section(&mut out, "Decisions", &s.decisions);
    section(&mut out, "Open questions", &s.open_questions);
    section(&mut out, "Action items", &s.action_items);
    out.push_str(&format!("Recap: {}\n", s.recap));
    out
}

/// (system, user) prompts merging the previous rolling summary with a new
/// session summary.
pub fn rolling_prompt(previous: Option<&str>, new_session: &SessionSummary) -> (String, String) {
    let system = "You maintain a running summary of a conversation that spans several sessions. \
Merge the previous rolling summary with the new session summary. Keep it under 300 words, \
keep decisions and open questions, and drop questions that the new session resolved. \
Reply with JSON: {\"rolling_summary\": string}."
        .to_string();
    let prev = previous
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .unwrap_or("(none yet)");
    let user = format!(
        "Previous rolling summary:\n{prev}\n\nNew session summary:\n{}",
        format_session_summary_for_rolling(new_session)
    );
    (system, user)
}

pub struct LlmSummarizer {
    pub app: AppHandle,
}

impl LlmSummarizer {
    fn call(&self, system: String, user: String, schema: Value) -> Result<Value> {
        let settings = crate::settings::get_settings(&self.app);
        let provider = settings
            .llm_provider()
            .cloned()
            .ok_or_else(|| anyhow!("no provider configured"))?;
        let model = settings
            .llm_model()
            .ok_or_else(|| anyhow!("no model configured"))?;
        let api_key = settings.llm_api_key();
        let fut = async move {
            llm_client::complete_json(&provider, api_key, &model, system, user, schema).await
        };
        tauri::async_runtime::block_on(async move { tokio::time::timeout(LLM_BUDGET, fut).await })
            .map_err(|_| anyhow!("summary timed out"))?
            .map_err(|e| anyhow!(e))
    }
}

impl Summarizer for LlmSummarizer {
    fn summarize_session(
        &self,
        title: &str,
        context: &str,
        history: &History,
        lines: &[TranscriptLine],
    ) -> Result<SessionSummary> {
        let (system, user) = session_prompt(title, context, history, lines);
        let mut v = self.call(system, user, session_summary_schema())?;
        // Providers without structured output only see the report's shape in
        // the prompt. A malformed report is dropped, never the summary.
        let report = v.get_mut("efficiency").map(Value::take);
        let mut summary: SessionSummary = serde_json::from_value(v)?;
        summary.efficiency = report
            .and_then(|r| serde_json::from_value::<EfficiencyReport>(r).ok())
            .map(|mut r| {
                r.score = r.score.clamp(0.0, 100.0).round();
                r.issues
                    .retain(|i| EFFICIENCY_KINDS.contains(&i.kind.as_str()));
                for i in &mut r.issues {
                    i.minutes = i.minutes.max(0.0).round();
                }
                r
            });
        Ok(summary)
    }

    fn merge_rolling(
        &self,
        previous: Option<&str>,
        new_session: &SessionSummary,
    ) -> Result<String> {
        let (system, user) = rolling_prompt(previous, new_session);
        let v = self.call(system, user, rolling_schema())?;
        v.get("rolling_summary")
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow!("rolling summary missing from reply"))
    }
}

/// Summarize one session and fold it into the meeting's rolling summary.
/// `Ok(None)` when the session has no transcript, is still open, or already
/// has a summary. All-or-nothing: both summaries are written only after both
/// LLM calls succeed. Callers must serialise jobs (see `MeetingManager`).
pub fn run_summary_job(
    store: &ProjectStore,
    summarizer: &dyn Summarizer,
    session_id: i64,
) -> Result<Option<SessionSummary>> {
    let session = store
        .get_session(session_id)?
        .ok_or_else(|| anyhow!("session {session_id} not found"))?;
    if session.summary.is_some() || session.ended_at.is_none() {
        return Ok(None);
    }
    let meeting = store
        .get_meeting(session.meeting_id)?
        .ok_or_else(|| anyhow!("meeting {} not found", session.meeting_id))?;
    let lines = store.session_lines(session_id)?;
    if lines.is_empty() {
        return Ok(None);
    }
    let history = History::load(store, &meeting)?;
    let summary =
        summarizer.summarize_session(&meeting.title, &meeting.context, &history, &lines)?;
    let rolling = summarizer.merge_rolling(meeting.rolling_summary.as_deref(), &summary)?;
    // `set_rolling_summary` truncates at its cap rather than erroring.
    store.set_session_summary(session_id, &serde_json::to_string(&summary)?)?;
    store.set_rolling_summary(meeting.id, &rolling)?;
    Ok(Some(summary))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct Fake {
        previous_seen: Mutex<Vec<Option<String>>>,
    }
    impl Summarizer for Fake {
        fn summarize_session(
            &self,
            _t: &str,
            _c: &str,
            _h: &History,
            lines: &[TranscriptLine],
        ) -> Result<SessionSummary> {
            Ok(SessionSummary {
                decisions: vec!["ship it".into()],
                open_questions: vec![],
                action_items: vec![],
                recap: format!("{} lines", lines.len()),
                efficiency: None,
            })
        }
        fn merge_rolling(&self, previous: Option<&str>, _n: &SessionSummary) -> Result<String> {
            self.previous_seen
                .lock()
                .unwrap()
                .push(previous.map(String::from));
            Ok(format!("rolled {}", previous.unwrap_or("-")))
        }
    }

    fn setup() -> (tempfile::TempDir, ProjectStore, i64, i64) {
        let dir = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(dir.path().join("m.db")).unwrap();
        let m = store.create_meeting(None, "Kickoff").unwrap();
        let s = store.create_session(m.id).unwrap();
        store.end_session(s.id).unwrap();
        (dir, store, m.id, s.id)
    }

    #[test]
    fn job_stores_session_and_rolling_summary() {
        let (_d, store, mid, sid) = setup();
        store.append_line(sid, "me", "hello", 1).unwrap();
        let fake = Fake {
            previous_seen: Mutex::new(vec![]),
        };
        let out = run_summary_job(&store, &fake, sid).unwrap().unwrap();
        assert_eq!(out.recap, "1 lines");
        let stored = store.get_session(sid).unwrap().unwrap().summary.unwrap();
        assert!(stored.contains("ship it"));
        assert_eq!(
            store.get_meeting(mid).unwrap().unwrap().rolling_summary,
            Some("rolled -".into())
        );
    }

    struct FailMerge;
    impl Summarizer for FailMerge {
        fn summarize_session(
            &self,
            _t: &str,
            _c: &str,
            _h: &History,
            _l: &[TranscriptLine],
        ) -> Result<SessionSummary> {
            Ok(SessionSummary::default())
        }
        fn merge_rolling(&self, _p: Option<&str>, _n: &SessionSummary) -> Result<String> {
            Err(anyhow!("merge down"))
        }
    }

    #[test]
    fn merge_failure_stores_nothing() {
        let (_d, store, mid, sid) = setup();
        store.set_rolling_summary(mid, "earlier").unwrap();
        store.append_line(sid, "me", "hello", 1).unwrap();
        assert!(run_summary_job(&store, &FailMerge, sid).is_err());
        assert!(store.get_session(sid).unwrap().unwrap().summary.is_none());
        assert_eq!(
            store.get_meeting(mid).unwrap().unwrap().rolling_summary,
            Some("earlier".into())
        );
    }
}
