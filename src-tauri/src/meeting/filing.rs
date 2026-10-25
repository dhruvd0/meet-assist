//! Assistant filing: files an unfiled meeting into a project (existing or
//! new) and gives it a real title, through a tool loop. The model chooses;
//! Rust enforces the rules (one filing per run, a user move always wins).

use crate::llm_client::{self, ToolCall, ToolClientCache, ToolDef, ToolLoopError};
use crate::meeting::observer::{readiness, Readiness};
use crate::meeting::store::{Meeting, ProjectStore, TranscriptLine, FILED_BY_AI, FILED_BY_USER};
use crate::meeting::usage::Purpose;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use specta::Type;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;
use tauri::AppHandle;
use tauri_specta::Event;

/// Transcript lines before the first live attempt.
pub const FILING_MIN_LINES: i64 = 20;
/// New lines needed between live attempts.
pub const FILING_RETRY_LINES: i64 = 10;
/// Live attempts per meeting.
pub const FILING_MAX_ATTEMPTS: u32 = 3;
/// Lines needed for the attempt made when the meeting ends.
pub const FILING_FINAL_MIN_LINES: i64 = 5;
const MAX_TOOL_CALLS: usize = 8;
const BUDGET: Duration = Duration::from_secs(30);
const TRANSCRIPT_MAX_CHARS: usize = 12_000;
const BRIEF_EXCERPT_CHARS: usize = 300;
const TITLE_MAX_CHARS: usize = 80;
const BRIEF_MAX_CHARS: usize = 2000;

/// A meeting's project changed, by the assistant or the user.
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct MeetingFiledPayload {
    pub meeting_id: i64,
    pub project_id: Option<i64>,
    pub project_title: Option<String>,
    pub meeting_title: String,
    /// `"ai"` or `"user"`.
    pub by: String,
    /// The assistant created `project_id` in this run (Undo may delete it).
    pub created_project: bool,
    /// Brief the assistant wrote for a created project; Undo deletes the
    /// project only while its brief still equals this.
    pub created_brief: Option<String>,
}

/// Nobody has chosen this meeting's project yet.
pub fn needs_filing(m: &Meeting) -> bool {
    m.project_id.is_none() && m.filed_by.is_none()
}

/// `title` is the one stamped at creation (`created_at`), not a name anyone
/// chose. create_meeting stamps its own time, so allow the minute before.
fn is_auto_title(title: &str, created_at: i64) -> bool {
    let auto = crate::meeting::session::MeetingManager::auto_title_at;
    title == auto(created_at) || title == auto(created_at - 60_000)
}

/// The assistant still has work on this meeting: filing it, naming it, or
/// both. A meeting started inside a project still needs a name; one the
/// user moved by hand is theirs, title included.
pub fn eligible(m: &Meeting) -> bool {
    if m.input_mode == crate::meeting::store::MeetingInputMode::TextTester {
        return false;
    }
    needs_filing(m)
        || (m.filed_by.as_deref() != Some(FILED_BY_USER) && is_auto_title(&m.title, m.created_at))
}

/// Emitted when the assistant names a meeting without filing it.
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct MeetingTitledPayload {
    pub meeting_id: i64,
    pub title: String,
}

#[derive(Default)]
struct Track {
    attempts: u32,
    last_lines: i64,
    unavailable: bool,
}

/// Per-meeting attempt bookkeeping, in memory only.
#[derive(Default)]
pub struct FilingTracker(Mutex<HashMap<i64, Track>>);

impl FilingTracker {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<i64, Track>> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn live_due(&self, meeting_id: i64, lines: i64) -> bool {
        if lines < FILING_MIN_LINES {
            return false;
        }
        match self.lock().get(&meeting_id) {
            None => true,
            Some(t) => {
                !t.unavailable
                    && t.attempts < FILING_MAX_ATTEMPTS
                    && lines - t.last_lines >= FILING_RETRY_LINES
            }
        }
    }

    pub fn final_due(&self, meeting_id: i64, lines: i64) -> bool {
        lines >= FILING_FINAL_MIN_LINES
            && !self.lock().get(&meeting_id).is_some_and(|t| t.unavailable)
    }

    pub fn record_attempt(&self, meeting_id: i64, lines: i64) {
        let mut m = self.lock();
        let t = m.entry(meeting_id).or_default();
        t.attempts += 1;
        t.last_lines = lines;
    }

    pub fn mark_unavailable(&self, meeting_id: i64) {
        self.lock().entry(meeting_id).or_default().unavailable = true;
    }

    pub fn forget(&self, meeting_id: i64) {
        self.lock().remove(&meeting_id);
    }
}

/// Tools for a run that only names the meeting (its project is chosen).
pub fn title_tool_defs() -> Vec<ToolDef> {
    tool_defs()
        .into_iter()
        .filter(|t| t.name == "rename_meeting")
        .collect()
}

pub fn tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "list_projects".into(),
            description: "List every project with a brief excerpt, meeting count, and last use (unix ms).".into(),
            input_schema: json!({"type":"object","properties":{},"additionalProperties":false}),
        },
        ToolDef {
            name: "read_project".into(),
            description: "Read one project: full brief, source digests, and summaries of its 3 most recent meetings.".into(),
            input_schema: json!({"type":"object","properties":{"project_id":{"type":"integer"}},"required":["project_id"],"additionalProperties":false}),
        },
        ToolDef {
            name: "assign_meeting".into(),
            description: "File this meeting into an existing project. Only one filing per run.".into(),
            input_schema: json!({"type":"object","properties":{"project_id":{"type":"integer"}},"required":["project_id"],"additionalProperties":false}),
        },
        ToolDef {
            name: "create_project".into(),
            description: "Create a project and file this meeting into it. Use only when no existing project fits. Reuses a project with the same title.".into(),
            input_schema: json!({"type":"object","properties":{"title":{"type":"string"},"brief":{"type":"string"}},"required":["title","brief"],"additionalProperties":false}),
        },
        ToolDef {
            name: "rename_meeting".into(),
            description: "Give this meeting a short descriptive title.".into(),
            input_schema: json!({"type":"object","properties":{"title":{"type":"string"}},"required":["title"],"additionalProperties":false}),
        },
    ]
}

pub fn final_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "outcome": { "type": "string", "enum": ["filed", "named", "wait"] },
            "reason": { "type": "string" }
        },
        "required": ["outcome", "reason"],
        "additionalProperties": false
    })
}

pub(super) const SYSTEM_PROMPT: &str = "You file a recorded conversation (a meeting or call, interview, debate, class or talk, or a broadcast the user listened to) into the user's projects, using the tools.
Always write generated content in English, regardless of the language of the transcript, input, or existing content. Preserve proper nouns and verbatim quotes in their original form. This includes meeting titles, new project titles and briefs, tool arguments, and the final reason.
1. Call list_projects. Call read_project on any project that might match.
2. If one project clearly fits, call assign_meeting with its id. Prefer an existing project.
3. If none fits and the transcript makes the conversation's subject clear, call create_project with a short title and a one-paragraph brief describing the subject, people, and goals heard so far.
4. Call rename_meeting with a short descriptive title (under 60 characters) in English.
5. If the transcript does not yet show what the conversation is about, do not file or rename; answer outcome \"wait\".
Finish with JSON {\"outcome\": \"filed\" | \"wait\", \"reason\": \"...\"}.";

pub(super) const TITLE_PROMPT: &str = "You name a recorded conversation (a meeting or call, interview, debate, class or talk, or a broadcast the user listened to). Its project is already chosen.
Always write generated content in English, regardless of the language of the transcript, input, or existing content. Preserve proper nouns and verbatim quotes in their original form. This includes the meeting title, tool arguments, and the final reason.
1. If the transcript shows what the conversation is about, call rename_meeting with a short descriptive title (under 60 characters) in English, naming the subject rather than the format (\"Chagos deal and Iran strikes\", not \"Parliament session\").
2. If it does not yet, do not rename; answer outcome \"wait\".
Finish with JSON {\"outcome\": \"named\" | \"wait\", \"reason\": \"...\"}.";

/// User message: title, notes, and the transcript so far. Over budget, the
/// oldest lines are dropped.
pub fn user_prompt(title: &str, context: &str, lines: &[TranscriptLine]) -> String {
    let mut kept: Vec<String> = Vec::new();
    let mut used = 0;
    for l in lines.iter().rev() {
        let s = format!("[{}] {}", l.speaker, l.text);
        used += s.chars().count() + 1;
        if used > TRANSCRIPT_MAX_CHARS {
            break;
        }
        kept.push(s);
    }
    kept.reverse();
    let notes = if context.trim().is_empty() {
        "(none)"
    } else {
        context.trim()
    };
    format!(
        "## Conversation title\n{title}\n\n## Conversation notes\n{notes}\n\n## Transcript so far\n{}",
        kept.join("\n")
    )
}

fn excerpt(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// 1..=`TITLE_MAX_CHARS` characters after trimming.
fn clean_title(v: &Value) -> Result<String, String> {
    let t = v.as_str().ok_or("title is required")?.trim();
    if t.is_empty() {
        return Err("title cannot be empty".into());
    }
    if t.chars().count() > TITLE_MAX_CHARS {
        return Err(format!("title is longer than {TITLE_MAX_CHARS} characters"));
    }
    Ok(t.to_string())
}

struct Filed {
    project_id: i64,
    created: bool,
    brief: Option<String>,
}

#[derive(Default)]
struct RunState {
    filed: Option<Filed>,
    renamed: bool,
}

/// One filing run's tool handler state.
pub struct FilingContext {
    store: ProjectStore,
    meeting_id: i64,
    /// Title and creation time when the run started.
    start_title: String,
    created_at: i64,
    state: Mutex<RunState>,
}

impl FilingContext {
    pub fn new(store: ProjectStore, meeting_id: i64, start_title: String, created_at: i64) -> Self {
        Self {
            store,
            meeting_id,
            start_title,
            created_at,
            state: Mutex::new(RunState::default()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, RunState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn handle(&self, call: &ToolCall) -> String {
        match self.handle_inner(call) {
            Ok(v) => v.to_string(),
            Err(e) => json!({ "error": e }).to_string(),
        }
    }

    fn handle_inner(&self, call: &ToolCall) -> Result<Value, String> {
        let a = &call.arguments;
        let e = |e: anyhow::Error| e.to_string();
        match call.name.as_str() {
            "list_projects" => {
                let projects = self.store.list_projects().map_err(e)?;
                let meetings = self.store.list_meetings(None).map_err(e)?;
                Ok(json!({ "projects": projects.iter().map(|p| json!({
                    "id": p.id,
                    "title": p.title,
                    "brief_excerpt": excerpt(&p.brief, BRIEF_EXCERPT_CHARS),
                    "meetings": meetings.iter().filter(|m| m.project_id == Some(p.id)).count(),
                    "updated_at": p.updated_at,
                })).collect::<Vec<_>>() }))
            }
            "read_project" => {
                let pid = a["project_id"].as_i64().ok_or("project_id is required")?;
                let p = self
                    .store
                    .get_project(pid)
                    .map_err(e)?
                    .ok_or("project not found")?;
                let sources = self.store.list_sources(pid).map_err(e)?;
                let meetings = self.store.list_meetings(Some(pid)).map_err(e)?;
                Ok(json!({
                    "id": p.id,
                    "title": p.title,
                    "brief": p.brief,
                    "sources": sources.iter().map(|s| json!({ "title": s.title, "digest": s.digest })).collect::<Vec<_>>(),
                    "recent_meetings": meetings.iter().take(3).map(|m| json!({ "title": m.title, "rolling_summary": m.rolling_summary })).collect::<Vec<_>>(),
                }))
            }
            "assign_meeting" => {
                let pid = a["project_id"].as_i64().ok_or("project_id is required")?;
                let mut st = self.lock();
                if st.filed.is_some() {
                    return Err("meeting already filed".into());
                }
                let p = self
                    .store
                    .get_project(pid)
                    .map_err(e)?
                    .ok_or("project not found")?;
                if !self
                    .store
                    .file_meeting_ai(self.meeting_id, pid)
                    .map_err(e)?
                {
                    return Err("meeting was filed by the user".into());
                }
                st.filed = Some(Filed {
                    project_id: pid,
                    created: false,
                    brief: None,
                });
                Ok(json!({ "ok": true, "project_title": p.title }))
            }
            "create_project" => {
                let title = clean_title(&a["title"])?;
                let brief = excerpt(a["brief"].as_str().unwrap_or("").trim(), BRIEF_MAX_CHARS);
                let mut st = self.lock();
                if st.filed.is_some() {
                    return Err("meeting already filed".into());
                }
                let Some((p, created)) = self
                    .store
                    .create_project_and_file_ai(self.meeting_id, &title, &brief)
                    .map_err(e)?
                else {
                    return Err("meeting was filed by the user".into());
                };
                st.filed = Some(Filed {
                    project_id: p.id,
                    created,
                    brief: created.then_some(brief),
                });
                Ok(json!({ "ok": true, "project_id": p.id, "reused": !created }))
            }
            "rename_meeting" => {
                let title = clean_title(&a["title"])?;
                let mut st = self.lock();
                if st.renamed {
                    return Err("meeting already renamed".into());
                }
                // Only an untouched auto title may be replaced.
                if !is_auto_title(&self.start_title, self.created_at) {
                    return Err("meeting title was set by the user".into());
                }
                if !self
                    .store
                    .rename_meeting_ai(self.meeting_id, &title, &self.start_title)
                    .map_err(e)?
                {
                    return Err("meeting was filed by the user".into());
                }
                st.renamed = true;
                Ok(json!({ "ok": true }))
            }
            other => Err(format!("unknown tool: {other}")),
        }
    }

    /// Whether this run renamed the meeting.
    fn renamed(&self) -> bool {
        self.lock().renamed
    }

    /// Payload for what this run filed, if anything.
    fn outcome(&self) -> Option<MeetingFiledPayload> {
        let st = self.lock();
        let f = st.filed.as_ref()?;
        let project_title = self
            .store
            .get_project(f.project_id)
            .ok()
            .flatten()
            .map(|p| p.title);
        let meeting_title = self
            .store
            .get_meeting(self.meeting_id)
            .ok()
            .flatten()
            .map(|m| m.title)
            .unwrap_or_default();
        Some(MeetingFiledPayload {
            meeting_id: self.meeting_id,
            project_id: Some(f.project_id),
            project_title,
            meeting_title,
            by: FILED_BY_AI.into(),
            created_project: f.created,
            created_brief: f.brief.clone(),
        })
    }
}

#[derive(Debug)]
pub enum FilingError {
    ToolsRejected,
    Other(String),
}

pub struct LlmFiler {
    app: AppHandle,
    clients: ToolClientCache,
}

impl LlmFiler {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            clients: ToolClientCache::default(),
        }
    }

    /// Tell the main window about a title the assistant set without filing.
    fn notify_titled(&self, store: &ProjectStore, meeting_id: i64) {
        let Ok(Some(m)) = store.get_meeting(meeting_id) else {
            return;
        };
        info!("filing: named meeting {meeting_id} \"{}\"", m.title);
        let payload = MeetingTitledPayload {
            meeting_id,
            title: m.title,
        };
        if let Err(e) = payload.emit(&self.app) {
            warn!("emit meeting titled: {e}");
        }
    }

    /// One filing run. `Ok(Some)` when this run filed the meeting, even if
    /// the loop failed afterwards (the write is already in the store).
    pub fn run(
        &self,
        store: &ProjectStore,
        meeting_id: i64,
    ) -> Result<Option<MeetingFiledPayload>, FilingError> {
        let settings = crate::settings::get_settings(&self.app);
        let provider = settings
            .llm_provider()
            .cloned()
            .ok_or_else(|| FilingError::Other("no provider configured".into()))?;
        let model = settings
            .llm_model()
            .ok_or_else(|| FilingError::Other("no model configured".into()))?;
        let api_key = settings.llm_api_key();
        let client = self
            .clients
            .get(&provider, &api_key)
            .map_err(FilingError::Other)?;
        let meeting = store
            .get_meeting(meeting_id)
            .map_err(|e| FilingError::Other(e.to_string()))?
            .ok_or_else(|| FilingError::Other("meeting gone".into()))?;
        if !eligible(&meeting) {
            return Ok(None);
        }
        // A meeting whose project is chosen only gets named.
        let filing = needs_filing(&meeting);
        let (system, defs) = if filing {
            (SYSTEM_PROMPT, tool_defs())
        } else {
            (TITLE_PROMPT, title_tool_defs())
        };
        let lines = store
            .lines_in_range(meeting_id, None, None, 10_000)
            .map_err(|e| FilingError::Other(e.to_string()))?;
        let ctx = FilingContext::new(
            store.clone_handle(),
            meeting_id,
            meeting.title.clone(),
            meeting.created_at,
        );
        let handler = |c: &ToolCall| ctx.handle(c);
        let mut usage = llm_client::LlmUsage::default();
        let fut = llm_client::send_with_tools_with_client(
            &client,
            &provider,
            &model,
            system.to_string(),
            user_prompt(&meeting.title, &meeting.context, &lines),
            &defs,
            final_schema(),
            MAX_TOOL_CALLS,
            &handler,
            &mut usage,
        );
        let result =
            tauri::async_runtime::block_on(async move { tokio::time::timeout(BUDGET, fut).await });
        let ok = matches!(result, Ok(Ok(_))) || ctx.outcome().is_some() || ctx.renamed();
        if let Err(e) = store.record_usage(meeting_id, Purpose::Filing, &model, ok, &usage) {
            warn!("filing: record usage: {e:#}");
        }
        if let Some(filed) = ctx.outcome() {
            return Ok(Some(filed));
        }
        if ctx.renamed() {
            self.notify_titled(store, meeting_id);
        }
        match result {
            Err(_) => Err(FilingError::Other("filing timed out".into())),
            Ok(Err(ToolLoopError::ToolsRejected(_))) => Err(FilingError::ToolsRejected),
            Ok(Err(ToolLoopError::Other(e))) => Err(FilingError::Other(e)),
            Ok(Ok(text)) => {
                info!("filing: meeting {meeting_id} not filed yet: {text}");
                Ok(None)
            }
        }
    }
}

/// Runs the filer and records a tools rejection. Callers check eligibility
/// and record the attempt first.
pub fn attempt(
    filer: &LlmFiler,
    store: &ProjectStore,
    tracker: &FilingTracker,
    meeting_id: i64,
) -> Option<MeetingFiledPayload> {
    match filer.run(store, meeting_id) {
        Ok(filed) => filed,
        Err(FilingError::ToolsRejected) => {
            warn!("filing: provider rejected tools; meeting {meeting_id} stays unfiled");
            tracker.mark_unavailable(meeting_id);
            None
        }
        Err(FilingError::Other(e)) => {
            warn!("filing: meeting {meeting_id} attempt failed: {e}");
            None
        }
    }
}

/// The attempt made when a meeting ends, if it is still unfiled, long
/// enough, and the assistant is ready. Forgets the meeting's attempts.
pub fn final_attempt(
    filer: &LlmFiler,
    store: &ProjectStore,
    tracker: &FilingTracker,
    app: &AppHandle,
    meeting_id: i64,
) {
    let due = (|| {
        if readiness(&crate::settings::get_settings(app)) != Readiness::Ready {
            return false;
        }
        let Ok(Some(m)) = store.get_meeting(meeting_id) else {
            return false;
        };
        let lines = store.count_meeting_lines(meeting_id).unwrap_or(0);
        eligible(&m) && tracker.final_due(meeting_id, lines)
    })();
    if due {
        if let Some(p) = attempt(filer, store, tracker, meeting_id) {
            if let Ok(Some(stored)) = store.get_meeting(meeting_id) {
                if still_ai_filed(&stored, &p) {
                    notify(app, &p);
                }
            }
        }
    }
    tracker.forget(meeting_id);
}

/// Whether the stored meeting still carries the assistant's filing `payload`
/// (the user did not move it while the run was going).
pub fn still_ai_filed(stored: &Meeting, payload: &MeetingFiledPayload) -> bool {
    stored.filed_by.as_deref() == Some(FILED_BY_AI) && stored.project_id == payload.project_id
}

/// Tells the main window and shows the floating card. No beep: filing is
/// not a nudge.
pub fn notify(app: &AppHandle, payload: &MeetingFiledPayload) {
    if let Err(e) = payload.clone().emit(app) {
        warn!("emit meeting filed: {e}");
    }
    crate::intervention::show_filed(app, payload);
}
