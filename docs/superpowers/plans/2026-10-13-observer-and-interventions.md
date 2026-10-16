# Observer and Interventions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** While a meeting is live, every N seconds the assistant reads the recent transcript plus a small base prompt, can call MCP-shaped tools to search project sources and past transcript, and answers one JSON question: intervene or not. On intervene the app beeps and shows a floating card to the user only; the intervention is stored and shown in the meeting timeline.

**Architecture:** `llm_client` gains a tool-calling loop for both OpenAI and Anthropic formats (`send_with_tools`). `meeting/tools.rs` defines five tools and executes them against `ProjectStore`. `meeting/observer.rs` runs a timer thread that polls `MeetingManager::state()`, builds the base prompt, runs the judge (tool loop with a final JSON schema), applies cooldown and dedupe, stores an `Intervention`, beeps, and emits `intervention-payload`. A new `intervention` webview window (Rust `intervention.rs`, frontend `src/intervention/`) shows the card. Settings add provider, sensitivity, interval, beep.

**Tech Stack:** Rust (reqwest, serde_json, rusqlite, tauri-specta), React 18, Zustand, i18next.

**Spec:** `docs/superpowers/specs/2026-10-13-meeting-assistant-design.md` sections 2 (intervention flow, settings), 3 (`observer.rs`, `tools.rs`, store), 3a (tool loop, limits), 4 (`intervention/` window, settings components), 5 (prompting), 6.

## Parallel lanes

Implementers may run concurrently ONLY in the lanes below. Each lane touches disjoint files, formats only its own files (`cargo fmt -- <file>`, `bunx prettier --write <file>`), and commits with explicit paths (`git add <files>`), never `git add -A`. Cargo's build lock serializes test runs; that is fine.

| Lane                | Tasks            | Files                                                                                                                                  |
| ------------------- | ---------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| A                   | 1                | `src-tauri/src/llm_client.rs`                                                                                                          |
| B                   | 2 then 3         | `src-tauri/src/meeting/store.rs`, then `src-tauri/src/meeting/tools.rs` + `meeting/mod.rs`                                             |
| C                   | 4                | `src-tauri/src/settings.rs`, `src-tauri/src/shortcut/mod.rs`, `src/stores/settingsStore.ts`                                            |
| D (after A, B, C)   | 5                | `src-tauri/src/meeting/observer.rs`, `meeting/session.rs`, `meeting/mod.rs`, `src-tauri/src/audio_feedback.rs`, `src-tauri/src/lib.rs` |
| E (parallel with D) | 7a               | `src/intervention/*`, `vite.config.ts`, `src/i18n/locales/**`                                                                          |
| F (after D, E)      | 6 then 7b then 8 | `src-tauri/src/intervention.rs`, `commands/meeting.rs`, `lib.rs`, bindings; then `src/components/**`; then docs                        |

## Global Constraints

- Toolchain before every cargo or bun command:
  ```bash
  export PATH="$HOME/.cargo/bin:$HOME/.bun/bin:$PATH"
  export CMAKE_POLICY_VERSION_MINIMUM=3.5
  export SDKROOT=/Users/dhruv/Desktop/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX26.5.sdk
  ```
  cargo from `src-tauri/`, bun from the repo root. `cargo test --lib <module>`, `cargo check`, `cargo clippy --tests`, `cargo fmt -- <file>` allowed; `cargo run -- --list-models` only where a task says (regenerates `src/bindings.ts`); no `bun run build`.
- Never hand-edit `src/bindings.ts`.
- No `unwrap` in non-test production paths. Rust tests use `tempfile::tempdir()`.
- Observer limits exactly: default interval 20 s, cooldown 60 s, max 5 tool calls per tick, 15 s wall budget, tool results capped at 4096 bytes, transcript window last 180 s, dedupe against the last 3 interventions' text (case-insensitive, trimmed).
- Intervention kinds exactly `loop`, `stuck`, `info`, `idea`. Final answer schema exactly `{ "intervene": bool, "kind": "loop"|"stuck"|"info"|"idea"|null, "text": string|null, "reason": string }`.
- Provider rejection of tools (HTTP 400/422 mentioning tools, or any error on the first tooled request) falls back to a no-tools judge call for that tick and sets a `tools_unavailable` flag reported in `ObserverStatusPayload`.
- Beep reuses the feedback-sound player with the bundled `resources/pop_start.wav`; gated by the new `meeting_beep_enabled` setting, not by `audio_feedback`.
- Strings under `meetings.observer.*`, `meetings.intervention.*`, `settings.meeting.*` in en, copied into the 25 other locales (CI gate).
- Reuse `components/ui`; keep the design language. Commit each task with a conventional prefix and the trailer `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.

## Review Focus

1. A provider that does not support tool calling (older OpenAI-compatible servers, some local models) must still produce verdicts, not a wall of errors. Pinned in Task 1 (fallback test) and Task 5 (observer uses `tools_unavailable`).
2. The model answering with text instead of JSON, or malformed JSON, must not crash the observer thread; the tick is skipped. Pinned in Task 5.
3. The same nudge must not repeat every tick; cooldown and dedupe. Pinned in Task 5.
4. An intervention card must never steal focus from the call app and must disappear on its own. Pinned in Task 6 (window flags) and Task 7a (auto-hide timer).
5. Deleting a meeting removes its interventions. Pinned in Task 2.

---

### Task 1: Tool calling in `llm_client` (Lane A)

**Files:**

- Modify: `src-tauri/src/llm_client.rs`

**Interfaces:**

- Produces:
  - `pub struct ToolDef { pub name: String, pub description: String, pub input_schema: serde_json::Value }`
  - `pub struct ToolCall { pub id: String, pub name: String, pub arguments: serde_json::Value }`
  - `pub enum ToolLoopError { ToolsRejected(String), Other(String) }` implementing `Display`
  - `pub async fn send_with_tools(provider: &PostProcessProvider, api_key: String, model: &str, system_prompt: String, user_content: String, tools: &[ToolDef], final_schema: serde_json::Value, max_tool_calls: usize, handler: &(dyn Fn(&ToolCall) -> String + Sync)) -> Result<String, ToolLoopError>` returning the final assistant text (expected JSON).
  - Pure helpers (pub, for tests): `openai_tool_request(model, messages: &[Value], tools: &[ToolDef], final_schema: Option<&Value>) -> Value`, `parse_openai_tool_calls(resp: &Value) -> (Option<String>, Vec<ToolCall>)`, `anthropic_tool_request(model, system: &str, messages: &[Value], tools: &[ToolDef]) -> Value`, `parse_anthropic_tool_calls(resp: &Value) -> (Option<String>, Vec<ToolCall>)`.

- [ ] **Step 1: Write the failing tests** (append to the existing `#[cfg(test)] mod tests` or create one)

```rust
#[cfg(test)]
mod tool_tests {
    use super::*;
    use serde_json::json;

    fn tool() -> ToolDef {
        ToolDef {
            name: "search_context".into(),
            description: "Search project sources".into(),
            input_schema: json!({"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}),
        }
    }

    #[test]
    fn openai_request_carries_tools_and_schema() {
        let msgs = vec![json!({"role":"user","content":"hi"})];
        let schema = json!({"type":"object"});
        let req = openai_tool_request("m", &msgs, &[tool()], Some(&schema));
        assert_eq!(req["tools"][0]["type"], "function");
        assert_eq!(req["tools"][0]["function"]["name"], "search_context");
        assert_eq!(req["tools"][0]["function"]["parameters"]["required"][0], "query");
        assert_eq!(req["response_format"]["type"], "json_schema");
        assert_eq!(req["stream"], false);
        let req2 = openai_tool_request("m", &msgs, &[], None);
        assert!(req2.get("tools").is_none());
        assert!(req2.get("response_format").is_none());
    }

    #[test]
    fn openai_response_tool_calls_parsed() {
        let resp = json!({"choices":[{"message":{"role":"assistant","content":null,
            "tool_calls":[{"id":"c1","type":"function","function":{"name":"search_context","arguments":"{\"query\":\"retry\"}"}}]},
            "finish_reason":"tool_calls"}]});
        let (text, calls) = parse_openai_tool_calls(&resp);
        assert!(text.is_none());
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "c1");
        assert_eq!(calls[0].arguments["query"], "retry");
        let resp2 = json!({"choices":[{"message":{"role":"assistant","content":"{\"intervene\":false}"},"finish_reason":"stop"}]});
        let (text, calls) = parse_openai_tool_calls(&resp2);
        assert_eq!(text.as_deref(), Some("{\"intervene\":false}"));
        assert!(calls.is_empty());
    }

    #[test]
    fn anthropic_request_and_response_shapes() {
        let msgs = vec![json!({"role":"user","content":"hi"})];
        let req = anthropic_tool_request("m", "sys", &msgs, &[tool()]);
        assert_eq!(req["system"], "sys");
        assert_eq!(req["tools"][0]["name"], "search_context");
        assert!(req["tools"][0]["input_schema"].is_object());
        assert_eq!(req["max_tokens"], 4096);
        let resp = json!({"content":[{"type":"text","text":"thinking"},{"type":"tool_use","id":"t1","name":"search_context","input":{"query":"x"}}],"stop_reason":"tool_use"});
        let (text, calls) = parse_anthropic_tool_calls(&resp);
        assert_eq!(text.as_deref(), Some("thinking"));
        assert_eq!(calls[0].id, "t1");
        assert_eq!(calls[0].arguments["query"], "x");
    }

    #[test]
    fn tool_result_messages_have_provider_shape() {
        let call = ToolCall { id: "c1".into(), name: "search_context".into(), arguments: json!({}) };
        let o = openai_tool_result_message(&call, "RESULT");
        assert_eq!(o["role"], "tool");
        assert_eq!(o["tool_call_id"], "c1");
        assert_eq!(o["content"], "RESULT");
        let a = anthropic_tool_result_message(&[(call.clone(), "RESULT".to_string())]);
        assert_eq!(a["role"], "user");
        assert_eq!(a["content"][0]["type"], "tool_result");
        assert_eq!(a["content"][0]["tool_use_id"], "c1");
    }

    #[test]
    fn tools_rejected_detection() {
        assert!(is_tools_rejection(400, "Unrecognized request argument supplied: tools"));
        assert!(is_tools_rejection(422, "tools is not supported"));
        assert!(!is_tools_rejection(400, "invalid api key"));
        assert!(!is_tools_rejection(500, "tools"));
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib llm_client::tool_tests 2>&1 | tail -3`
Expected: compile error.

- [ ] **Step 3: Implement**

Add near the top of `llm_client.rs`:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug)]
pub enum ToolLoopError {
    /// The endpoint rejected the `tools` field; caller should retry without tools.
    ToolsRejected(String),
    Other(String),
}

impl std::fmt::Display for ToolLoopError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolLoopError::ToolsRejected(s) => write!(f, "tools rejected: {s}"),
            ToolLoopError::Other(s) => write!(f, "{s}"),
        }
    }
}

pub fn is_tools_rejection(status: u16, body: &str) -> bool {
    (status == 400 || status == 422) && body.to_ascii_lowercase().contains("tool")
}

pub fn openai_tool_request(
    model: &str,
    messages: &[Value],
    tools: &[ToolDef],
    final_schema: Option<&Value>,
) -> Value {
    let mut req = serde_json::json!({ "model": model, "messages": messages, "stream": false });
    if !tools.is_empty() {
        req["tools"] = Value::Array(
            tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "type": "function",
                        "function": { "name": t.name, "description": t.description, "parameters": t.input_schema }
                    })
                })
                .collect(),
        );
    }
    if let Some(schema) = final_schema {
        req["response_format"] = serde_json::json!({
            "type": "json_schema",
            "json_schema": { "name": "observer_verdict", "strict": true, "schema": schema }
        });
    }
    req
}

pub fn parse_openai_tool_calls(resp: &Value) -> (Option<String>, Vec<ToolCall>) {
    let msg = &resp["choices"][0]["message"];
    let text = msg["content"].as_str().map(|s| s.to_string()).filter(|s| !s.is_empty());
    let calls = msg["tool_calls"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|c| {
                    let id = c["id"].as_str()?.to_string();
                    let name = c["function"]["name"].as_str()?.to_string();
                    let raw = c["function"]["arguments"].as_str().unwrap_or("{}");
                    let arguments = serde_json::from_str(raw).unwrap_or(Value::Object(Default::default()));
                    Some(ToolCall { id, name, arguments })
                })
                .collect()
        })
        .unwrap_or_default();
    (text, calls)
}

pub fn openai_tool_result_message(call: &ToolCall, result: &str) -> Value {
    serde_json::json!({ "role": "tool", "tool_call_id": call.id, "content": result })
}

pub fn openai_assistant_tool_call_message(calls: &[ToolCall], text: Option<&str>) -> Value {
    serde_json::json!({
        "role": "assistant",
        "content": text,
        "tool_calls": calls.iter().map(|c| serde_json::json!({
            "id": c.id, "type": "function",
            "function": { "name": c.name, "arguments": c.arguments.to_string() }
        })).collect::<Vec<_>>()
    })
}

pub fn anthropic_tool_request(model: &str, system: &str, messages: &[Value], tools: &[ToolDef]) -> Value {
    let mut req = serde_json::json!({
        "model": model, "max_tokens": ANTHROPIC_MAX_TOKENS, "system": system, "messages": messages
    });
    if !tools.is_empty() {
        req["tools"] = Value::Array(
            tools
                .iter()
                .map(|t| serde_json::json!({ "name": t.name, "description": t.description, "input_schema": t.input_schema }))
                .collect(),
        );
    }
    req
}

pub fn parse_anthropic_tool_calls(resp: &Value) -> (Option<String>, Vec<ToolCall>) {
    let mut text = String::new();
    let mut calls = Vec::new();
    if let Some(blocks) = resp["content"].as_array() {
        for b in blocks {
            match b["type"].as_str() {
                Some("text") => text.push_str(b["text"].as_str().unwrap_or("")),
                Some("tool_use") => calls.push(ToolCall {
                    id: b["id"].as_str().unwrap_or("").to_string(),
                    name: b["name"].as_str().unwrap_or("").to_string(),
                    arguments: b["input"].clone(),
                }),
                _ => {}
            }
        }
    }
    (if text.is_empty() { None } else { Some(text) }, calls)
}

pub fn anthropic_assistant_tool_use_message(calls: &[ToolCall], text: Option<&str>) -> Value {
    let mut content: Vec<Value> = Vec::new();
    if let Some(t) = text {
        content.push(serde_json::json!({ "type": "text", "text": t }));
    }
    for c in calls {
        content.push(serde_json::json!({ "type": "tool_use", "id": c.id, "name": c.name, "input": c.arguments }));
    }
    serde_json::json!({ "role": "assistant", "content": content })
}

pub fn anthropic_tool_result_message(results: &[(ToolCall, String)]) -> Value {
    serde_json::json!({
        "role": "user",
        "content": results.iter().map(|(c, r)| serde_json::json!({
            "type": "tool_result", "tool_use_id": c.id, "content": r
        })).collect::<Vec<_>>()
    })
}
```

The loop:

```rust
/// Run a tool-calling conversation until the model answers without tool
/// calls, or `max_tool_calls` is reached (then one final no-tools request
/// demanding the answer). Returns the final assistant text.
pub async fn send_with_tools(
    provider: &PostProcessProvider,
    api_key: String,
    model: &str,
    system_prompt: String,
    user_content: String,
    tools: &[ToolDef],
    final_schema: Value,
    max_tool_calls: usize,
    handler: &(dyn Fn(&ToolCall) -> String + Sync),
) -> Result<String, ToolLoopError> {
    let client = create_client(provider, &api_key).map_err(ToolLoopError::Other)?;
    let base = provider.base_url.trim_end_matches('/');
    let anthropic = provider.api_format == PostProcessApiFormat::Anthropic;
    let url = if anthropic { format!("{base}/messages") } else { format!("{base}/chat/completions") };

    let mut messages: Vec<Value> = Vec::new();
    if !anthropic {
        messages.push(serde_json::json!({ "role": "system", "content": system_prompt }));
    }
    messages.push(serde_json::json!({ "role": "user", "content": user_content }));

    let mut calls_made = 0usize;
    let mut first_request = true;
    loop {
        let tools_now: &[ToolDef] = if calls_made >= max_tool_calls { &[] } else { tools };
        if calls_made >= max_tool_calls {
            messages.push(serde_json::json!({ "role": "user", "content": "Tool budget exhausted. Answer now with the final JSON only." }));
        }
        let body = if anthropic {
            anthropic_tool_request(model, &system_prompt, &messages, tools_now)
        } else {
            openai_tool_request(model, &messages, tools_now, if tools_now.is_empty() { Some(&final_schema) } else { None })
        };
        let resp = client.post(&url).json(&body).send().await.map_err(|e| ToolLoopError::Other(e.to_string()))?;
        let status = resp.status().as_u16();
        let text = resp.text().await.map_err(|e| ToolLoopError::Other(e.to_string()))?;
        if !(200..300).contains(&status) {
            if first_request && !tools_now.is_empty() && is_tools_rejection(status, &text) {
                return Err(ToolLoopError::ToolsRejected(text));
            }
            return Err(ToolLoopError::Other(format!("HTTP {status}: {text}")));
        }
        first_request = false;
        let json: Value = serde_json::from_str(&text).map_err(|e| ToolLoopError::Other(format!("bad json: {e}")))?;
        let (answer, calls) = if anthropic { parse_anthropic_tool_calls(&json) } else { parse_openai_tool_calls(&json) };
        if calls.is_empty() {
            return answer.ok_or_else(|| ToolLoopError::Other("empty answer".into()));
        }
        let mut results = Vec::new();
        for c in &calls {
            if calls_made >= max_tool_calls {
                results.push((c.clone(), "Tool budget exhausted; answer now.".to_string()));
                continue;
            }
            calls_made += 1;
            let mut r = handler(c);
            if r.len() > 4096 {
                r.truncate(4096);
                r.push_str("\n[truncated]");
            }
            results.push((c.clone(), r));
        }
        if anthropic {
            messages.push(anthropic_assistant_tool_use_message(&calls, answer.as_deref()));
            messages.push(anthropic_tool_result_message(&results));
        } else {
            messages.push(openai_assistant_tool_call_message(&calls, answer.as_deref()));
            for (c, r) in &results {
                messages.push(openai_tool_result_message(c, r));
            }
        }
    }
}
```

Keep `response_format` off while tools are offered (several providers reject the combination); the schema is applied only on the final, tool-less request. For Anthropic the schema is conveyed by the system prompt (Task 5 includes the schema text in the prompt).

- [ ] **Step 4: Run tests**

Run: `cargo test --lib llm_client:: 2>&1 | tail -3`
Expected: all pass (5 new plus any existing).

- [ ] **Step 5: Commit (paths only)**

```bash
cargo fmt -- src/llm_client.rs
git add src-tauri/src/llm_client.rs
git commit -m "feat: add tool-calling loop to the LLM client for OpenAI and Anthropic formats"
```

---

### Task 2: Interventions table and transcript queries (Lane B)

**Files:**

- Modify: `src-tauri/src/meeting/store.rs`

**Interfaces:**

- Produces:
  - Migration 5: `interventions(id, session_id FK, ts, kind, text, reason, dismissed INTEGER DEFAULT 0)` with index on `session_id`.
  - `pub struct Intervention { pub id: i64, pub session_id: i64, pub ts: i64, pub kind: String, pub text: String, pub reason: String, pub dismissed: bool }` (specta Type)
  - `MeetingDetail` gains `pub interventions: Vec<Intervention>` (ordered `ts ASC`).
  - `add_intervention(&self, session_id, kind: &str, text: &str, reason: &str) -> Result<Intervention>`
  - `list_interventions(&self, meeting_id) -> Result<Vec<Intervention>>`
  - `recent_interventions(&self, meeting_id, limit: usize) -> Result<Vec<Intervention>>` (newest first)
  - `dismiss_intervention(&self, id) -> Result<()>`
  - `lines_since(&self, meeting_id, since_ts: i64) -> Result<Vec<TranscriptLine>>` (all sessions of the meeting, `ts >= since_ts`, ordered ts asc)
  - `lines_in_range(&self, meeting_id, from_ts: Option<i64>, to_ts: Option<i64>, limit: usize) -> Result<Vec<TranscriptLine>>`
  - `latest_line_ts(&self, meeting_id) -> Result<Option<i64>>`
  - `previous_meeting_summary(&self, project_id, exclude_meeting_id) -> Result<Option<(String, Option<String>)>>` returning `(title, rolling_summary)` of the most recently updated other meeting in the project
  - `delete_meeting` and `delete_project` also delete interventions.

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn interventions_crud_and_cascade() {
        let (_d, s) = store();
        let p = s.create_project("P").unwrap();
        let m = s.create_meeting(Some(p.id), "M").unwrap();
        let sess = s.create_session(m.id).unwrap();
        let a = s.add_intervention(sess.id, "loop", "You said that twice", "repeat").unwrap();
        let b = s.add_intervention(sess.id, "idea", "Try X", "untried").unwrap();
        assert!(!a.dismissed);
        let d = s.get_meeting_detail(m.id).unwrap().unwrap();
        assert_eq!(d.interventions.iter().map(|i| i.id).collect::<Vec<_>>(), vec![a.id, b.id]);
        s.dismiss_intervention(a.id).unwrap();
        assert!(s.list_interventions(m.id).unwrap()[0].dismissed);
        let recent = s.recent_interventions(m.id, 1).unwrap();
        assert_eq!(recent[0].id, b.id);
        s.delete_meeting(m.id).unwrap();
        let conn = rusqlite::Connection::open(_d.path().join("meet.db")).unwrap();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM interventions", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn line_window_queries() {
        let (_d, s) = store();
        let m = s.create_meeting(None, "M").unwrap();
        let s1 = s.create_session(m.id).unwrap();
        s.append_line(s1.id, "me", "one", 1000).unwrap();
        s.append_line(s1.id, "me", "two", 2000).unwrap();
        let s2 = s.create_session(m.id).unwrap();
        s.append_line(s2.id, "me", "three", 3000).unwrap();
        assert_eq!(s.latest_line_ts(m.id).unwrap(), Some(3000));
        let since = s.lines_since(m.id, 2000).unwrap();
        assert_eq!(since.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), vec!["two", "three"]);
        let range = s.lines_in_range(m.id, Some(1500), Some(2500), 10).unwrap();
        assert_eq!(range.len(), 1);
        assert_eq!(s.lines_in_range(m.id, None, None, 2).unwrap().len(), 2);
    }

    #[test]
    fn previous_meeting_summary_picks_other_meeting_in_project() {
        let (_d, s) = store();
        let p = s.create_project("P").unwrap();
        let older = s.create_meeting(Some(p.id), "Older").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let current = s.create_meeting(Some(p.id), "Current").unwrap();
        assert_eq!(s.previous_meeting_summary(p.id, current.id).unwrap().map(|x| x.0), Some("Older".to_string()));
        assert!(s.previous_meeting_summary(p.id, older.id).unwrap().map(|x| x.0) == Some("Current".to_string()));
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib meeting::store 2>&1 | tail -3`
Expected: compile error.

- [ ] **Step 3: Implement**

Migration 5 appended to `MIGRATIONS`:

```rust
    M::up(
        "CREATE TABLE interventions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
            ts INTEGER NOT NULL,
            kind TEXT NOT NULL,
            text TEXT NOT NULL,
            reason TEXT NOT NULL DEFAULT '',
            dismissed INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX interventions_session_idx ON interventions(session_id);",
    ),
```

Type and methods:

```rust
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct Intervention {
    pub id: i64,
    pub session_id: i64,
    pub ts: i64,
    pub kind: String,
    pub text: String,
    pub reason: String,
    pub dismissed: bool,
}

const INTERVENTION_COLS: &str = "i.id, i.session_id, i.ts, i.kind, i.text, i.reason, i.dismissed";

impl ProjectStore {
    fn map_intervention(row: &rusqlite::Row<'_>) -> rusqlite::Result<Intervention> {
        Ok(Intervention {
            id: row.get(0)?, session_id: row.get(1)?, ts: row.get(2)?, kind: row.get(3)?,
            text: row.get(4)?, reason: row.get(5)?, dismissed: row.get::<_, i64>(6)? != 0,
        })
    }

    pub fn add_intervention(&self, session_id: i64, kind: &str, text: &str, reason: &str) -> Result<Intervention> {
        let conn = self.conn()?;
        let ts = now();
        conn.execute(
            "INSERT INTO interventions (session_id, ts, kind, text, reason) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![session_id, ts, kind, text, reason],
        )?;
        Ok(Intervention { id: conn.last_insert_rowid(), session_id, ts, kind: kind.into(), text: text.into(), reason: reason.into(), dismissed: false })
    }

    pub fn list_interventions(&self, meeting_id: i64) -> Result<Vec<Intervention>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {INTERVENTION_COLS} FROM interventions i JOIN sessions s ON s.id = i.session_id
             WHERE s.meeting_id = ?1 ORDER BY i.ts ASC, i.id ASC"
        ))?;
        let rows = stmt.query_map(params![meeting_id], Self::map_intervention)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn recent_interventions(&self, meeting_id: i64, limit: usize) -> Result<Vec<Intervention>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {INTERVENTION_COLS} FROM interventions i JOIN sessions s ON s.id = i.session_id
             WHERE s.meeting_id = ?1 ORDER BY i.ts DESC, i.id DESC LIMIT ?2"
        ))?;
        let rows = stmt.query_map(params![meeting_id, limit as i64], Self::map_intervention)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn dismiss_intervention(&self, id: i64) -> Result<()> {
        let conn = self.conn()?;
        conn.execute("UPDATE interventions SET dismissed = 1 WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn lines_since(&self, meeting_id: i64, since_ts: i64) -> Result<Vec<TranscriptLine>> {
        self.lines_in_range(meeting_id, Some(since_ts), None, 10_000)
    }

    pub fn lines_in_range(&self, meeting_id: i64, from_ts: Option<i64>, to_ts: Option<i64>, limit: usize) -> Result<Vec<TranscriptLine>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT l.id, l.session_id, l.ts, l.speaker, l.text FROM transcript_lines l
             JOIN sessions s ON s.id = l.session_id
             WHERE s.meeting_id = ?1 AND (?2 IS NULL OR l.ts >= ?2) AND (?3 IS NULL OR l.ts <= ?3)
             ORDER BY l.ts ASC, l.id ASC LIMIT ?4",
        )?;
        let rows = stmt.query_map(params![meeting_id, from_ts, to_ts, limit as i64], Self::map_line)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn latest_line_ts(&self, meeting_id: i64) -> Result<Option<i64>> {
        let conn = self.conn()?;
        Ok(conn.query_row(
            "SELECT MAX(l.ts) FROM transcript_lines l JOIN sessions s ON s.id = l.session_id WHERE s.meeting_id = ?1",
            params![meeting_id],
            |r| r.get::<_, Option<i64>>(0),
        )?)
    }

    pub fn previous_meeting_summary(&self, project_id: i64, exclude_meeting_id: i64) -> Result<Option<(String, Option<String>)>> {
        let conn = self.conn()?;
        Ok(conn
            .query_row(
                "SELECT title, rolling_summary FROM meetings WHERE project_id = ?1 AND id != ?2
                 ORDER BY updated_at DESC, id DESC LIMIT 1",
                params![project_id, exclude_meeting_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    }
}
```

`get_meeting_detail`: add `interventions: self.list_interventions(id)?` to the struct (fetch after lines). `delete_meeting`: before deleting lines, `DELETE FROM interventions WHERE session_id IN (SELECT id FROM sessions WHERE meeting_id = ?1)`. `delete_project`: same with the project join. Keep `TransactionBehavior::Immediate`.

- [ ] **Step 4: Run tests and commit (paths only)**

Run: `cargo test --lib meeting::store 2>&1 | tail -3` (all pass).

```bash
cargo fmt -- src/meeting/store.rs
git add src-tauri/src/meeting/store.rs
git commit -m "feat: add interventions table and transcript window queries"
```

---

### Task 3: Tool definitions and handlers (Lane B, after Task 2)

**Files:**

- Create: `src-tauri/src/meeting/tools.rs`
- Modify: `src-tauri/src/meeting/mod.rs` (`pub mod tools;`)

**Interfaces:**

- Consumes: `ProjectStore` (`list_sources`, `search`, `read_chunks`, `lines_in_range`, `list_meetings`), `llm_client::{ToolDef, ToolCall}`.
- Produces:
  - `pub fn tool_defs() -> Vec<ToolDef>` with names `list_sources`, `search_context`, `read_source`, `read_transcript`, `list_meetings`.
  - `pub struct ToolContext { pub store: Arc<ProjectStore>, pub project_id: Option<i64>, pub meeting_id: i64 }`
  - `pub fn handle_tool(ctx: &ToolContext, call: &ToolCall) -> String` returning compact JSON text (never panics; errors are returned as `{"error": "..."}`).

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::meeting::store::{ProjectStore, SourceKind};
    use serde_json::{json, Value};

    fn ctx() -> (tempfile::TempDir, ToolContext) {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(ProjectStore::open(dir.path().join("meet.db")).unwrap());
        let p = store.create_project("P").unwrap();
        let src = store.add_source(p.id, SourceKind::Text, "Spec", "").unwrap();
        store.set_source_digest(src.id, "Spec digest").unwrap();
        store.replace_chunks(src.id, &[("".into(), "The retry budget is five attempts.".into())]).unwrap();
        let m = store.create_meeting(Some(p.id), "Standup").unwrap();
        let sess = store.create_session(m.id).unwrap();
        store.append_line(sess.id, "me", "we keep circling the retry budget", 1000).unwrap();
        (dir, ToolContext { store, project_id: Some(p.id), meeting_id: m.id })
    }

    fn call(name: &str, args: Value) -> ToolCall {
        ToolCall { id: "1".into(), name: name.into(), arguments: args }
    }

    #[test]
    fn defs_have_five_tools_with_schemas() {
        let defs = tool_defs();
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["list_sources", "search_context", "read_source", "read_transcript", "list_meetings"]);
        assert!(defs.iter().all(|d| d["type"] == "object" || d.input_schema["type"] == "object"));
    }

    #[test]
    fn list_sources_returns_digest_not_locator() {
        let (_d, c) = ctx();
        let out: Value = serde_json::from_str(&handle_tool(&c, &call("list_sources", json!({})))).unwrap();
        assert_eq!(out["sources"][0]["title"], "Spec");
        assert_eq!(out["sources"][0]["digest"], "Spec digest");
        assert!(out["sources"][0].get("locator").is_none());
    }

    #[test]
    fn search_and_read_source() {
        let (_d, c) = ctx();
        let out: Value = serde_json::from_str(&handle_tool(&c, &call("search_context", json!({"query":"retry budget"})))).unwrap();
        assert_eq!(out["hits"].as_array().unwrap().len(), 1);
        let sid = out["hits"][0]["source_id"].as_i64().unwrap();
        let out: Value = serde_json::from_str(&handle_tool(&c, &call("read_source", json!({"source_id": sid})))).unwrap();
        assert!(out["chunks"][0]["content"].as_str().unwrap().contains("five"));
    }

    #[test]
    fn read_transcript_and_list_meetings() {
        let (_d, c) = ctx();
        let out: Value = serde_json::from_str(&handle_tool(&c, &call("read_transcript", json!({})))).unwrap();
        assert_eq!(out["lines"][0]["text"], "we keep circling the retry budget");
        let out: Value = serde_json::from_str(&handle_tool(&c, &call("list_meetings", json!({})))).unwrap();
        assert_eq!(out["meetings"][0]["title"], "Standup");
    }

    #[test]
    fn unknown_tool_and_bad_args_return_error_json() {
        let (_d, c) = ctx();
        let out: Value = serde_json::from_str(&handle_tool(&c, &call("nope", json!({})))).unwrap();
        assert!(out["error"].as_str().unwrap().contains("unknown tool"));
        let out: Value = serde_json::from_str(&handle_tool(&c, &call("read_source", json!({})))).unwrap();
        assert!(out["error"].is_string());
    }

    #[test]
    fn quick_meeting_without_project_has_no_sources() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(ProjectStore::open(dir.path().join("meet.db")).unwrap());
        let m = store.create_meeting(None, "Quick").unwrap();
        let c = ToolContext { store, project_id: None, meeting_id: m.id };
        let out: Value = serde_json::from_str(&handle_tool(&c, &call("search_context", json!({"query":"x"})))).unwrap();
        assert_eq!(out["hits"].as_array().unwrap().len(), 0);
    }
}
```

Fix the odd assertion in `defs_have_five_tools_with_schemas` to `assert!(defs.iter().all(|d| d.input_schema["type"] == "object"));` (the `d["type"]` form does not compile; use the corrected line).

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib meeting::tools 2>&1 | tail -3`
Expected: compile error.

- [ ] **Step 3: Implement**

```rust
//! MCP-shaped tools the observer can call. One implementation, reusable by a
//! future MCP server. Every handler returns compact JSON text and never panics.

use crate::llm_client::{ToolCall, ToolDef};
use crate::meeting::store::ProjectStore;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct ToolContext {
    pub store: Arc<ProjectStore>,
    pub project_id: Option<i64>,
    pub meeting_id: i64,
}

pub fn tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "list_sources".into(),
            description: "List the project's sources with their short digests. Call this first when you need to know what material exists.".into(),
            input_schema: json!({"type":"object","properties":{},"additionalProperties":false}),
        },
        ToolDef {
            name: "search_context".into(),
            description: "Full-text search across all project sources. Returns snippets with source_id and path.".into(),
            input_schema: json!({"type":"object","properties":{"query":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":20}},"required":["query"],"additionalProperties":false}),
        },
        ToolDef {
            name: "read_source".into(),
            description: "Read chunks of one source. Use path to pick a file inside a folder source; offset and limit page through chunks.".into(),
            input_schema: json!({"type":"object","properties":{"source_id":{"type":"integer"},"path":{"type":"string"},"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":20}},"required":["source_id"],"additionalProperties":false}),
        },
        ToolDef {
            name: "read_transcript".into(),
            description: "Read transcript lines of this meeting (default) or another meeting in the project, optionally within a time range (unix ms).".into(),
            input_schema: json!({"type":"object","properties":{"meeting_id":{"type":"integer"},"from_ts":{"type":"integer"},"to_ts":{"type":"integer"},"limit":{"type":"integer","minimum":1,"maximum":200}},"additionalProperties":false}),
        },
        ToolDef {
            name: "list_meetings".into(),
            description: "List meetings in this project with dates and rolling summaries.".into(),
            input_schema: json!({"type":"object","properties":{},"additionalProperties":false}),
        },
    ]
}

fn err(msg: impl std::fmt::Display) -> String {
    json!({ "error": msg.to_string() }).to_string()
}

pub fn handle_tool(ctx: &ToolContext, call: &ToolCall) -> String {
    match handle_inner(ctx, call) {
        Ok(v) => v.to_string(),
        Err(e) => err(e),
    }
}

fn handle_inner(ctx: &ToolContext, call: &ToolCall) -> Result<Value, String> {
    let a = &call.arguments;
    match call.name.as_str() {
        "list_sources" => {
            let Some(pid) = ctx.project_id else { return Ok(json!({ "sources": [] })); };
            let sources = ctx.store.list_sources(pid).map_err(|e| e.to_string())?;
            Ok(json!({ "sources": sources.iter().map(|s| json!({
                "id": s.id, "kind": s.kind, "title": s.title, "status": s.status, "digest": s.digest
            })).collect::<Vec<_>>() }))
        }
        "search_context" => {
            let query = a["query"].as_str().ok_or("query is required")?;
            let limit = a["limit"].as_u64().unwrap_or(8).clamp(1, 20) as usize;
            let Some(pid) = ctx.project_id else { return Ok(json!({ "hits": [] })); };
            let hits = ctx.store.search(pid, query, limit).map_err(|e| e.to_string())?;
            Ok(json!({ "hits": hits.iter().map(|h| json!({
                "source_id": h.source_id, "source_title": h.source_title, "path": h.path, "ord": h.ord, "snippet": h.snippet, "score": h.score
            })).collect::<Vec<_>>() }))
        }
        "read_source" => {
            let sid = a["source_id"].as_i64().ok_or("source_id is required")?;
            let path = a["path"].as_str();
            let offset = a["offset"].as_u64().unwrap_or(0) as usize;
            let limit = a["limit"].as_u64().unwrap_or(5).clamp(1, 20) as usize;
            let chunks = ctx.store.read_chunks(sid, path, offset, limit).map_err(|e| e.to_string())?;
            Ok(json!({ "chunks": chunks.iter().map(|(p, o, c)| json!({ "path": p, "ord": o, "content": c })).collect::<Vec<_>>() }))
        }
        "read_transcript" => {
            let mid = a["meeting_id"].as_i64().unwrap_or(ctx.meeting_id);
            if mid != ctx.meeting_id {
                let m = ctx.store.get_meeting(mid).map_err(|e| e.to_string())?.ok_or("meeting not found")?;
                if m.project_id.is_none() || m.project_id != ctx.project_id {
                    return Err("meeting is not in this project".into());
                }
            }
            let limit = a["limit"].as_u64().unwrap_or(100).clamp(1, 200) as usize;
            let lines = ctx.store.lines_in_range(mid, a["from_ts"].as_i64(), a["to_ts"].as_i64(), limit).map_err(|e| e.to_string())?;
            Ok(json!({ "lines": lines.iter().map(|l| json!({ "ts": l.ts, "speaker": l.speaker, "text": l.text })).collect::<Vec<_>>() }))
        }
        "list_meetings" => {
            let Some(pid) = ctx.project_id else { return Ok(json!({ "meetings": [] })); };
            let meetings = ctx.store.list_meetings(Some(pid)).map_err(|e| e.to_string())?;
            Ok(json!({ "meetings": meetings.iter().map(|m| json!({
                "id": m.id, "title": m.title, "status": m.status, "created_at": m.created_at, "updated_at": m.updated_at, "rolling_summary": m.rolling_summary
            })).collect::<Vec<_>>() }))
        }
        other => Err(format!("unknown tool: {other}")),
    }
}
```

`SourceKind`/`SourceStatus`/`MeetingStatus` serialize via serde (lowercase) so `json!` embeds them.

- [ ] **Step 4: Run tests and commit (paths only)**

Run: `cargo test --lib meeting::tools 2>&1 | tail -3` (6 passed).

```bash
cargo fmt -- src/meeting/tools.rs src/meeting/mod.rs
git add src-tauri/src/meeting/tools.rs src-tauri/src/meeting/mod.rs
git commit -m "feat: add observer tools over project sources and transcripts"
```

---

### Task 4: Observer settings (Lane C)

**Files:**

- Modify: `src-tauri/src/settings.rs`
- Modify: `src-tauri/src/shortcut/mod.rs` (settings commands live here)
- Modify: `src-tauri/src/lib.rs` (`collect_commands!`: four new commands)
- Modify: `src/stores/settingsStore.ts` (`settingUpdaters` entries)

**Interfaces:**

- Produces on `AppSettings`:
  - `pub meeting_provider_id: String` (default `""`; empty means "same as post-processing provider"), `#[serde(default = "default_meeting_provider_id")]`
  - `pub meeting_sensitivity: MeetingSensitivity` with `pub enum MeetingSensitivity { Low, #[default] Medium, High }` (serde lowercase, specta Type)
  - `pub meeting_observer_interval_secs: u32` default 20 (clamped 10..=120 on write)
  - `pub meeting_beep_enabled: bool` default true
  - Helper `impl AppSettings { pub fn meeting_provider(&self) -> Option<&PostProcessProvider> }` resolving `meeting_provider_id` or falling back to the active post-process provider; `pub fn meeting_model(&self) -> Option<String>` from `post_process_models[provider.id]`; `pub fn meeting_api_key(&self) -> String` from `post_process_api_keys` (use whatever accessor the SecretMap type exposes; see how `change_post_process_api_key_setting` reads it).
- Commands (in `shortcut/mod.rs`, `#[tauri::command] #[specta::specta]`): `change_meeting_provider_setting(app, provider_id: String)`, `change_meeting_sensitivity_setting(app, sensitivity: MeetingSensitivity)`, `change_meeting_observer_interval_setting(app, secs: u32)`, `change_meeting_beep_setting(app, enabled: bool)`; each loads settings, mutates, `write_settings`.
- Frontend `settingUpdaters`: `meeting_provider_id`, `meeting_sensitivity`, `meeting_observer_interval_secs`, `meeting_beep_enabled` mapped to the four commands. (The TS entries will not typecheck until bindings are regenerated in Task 6; that is expected. Commit them anyway; Task 6 regenerates.)

- [ ] **Step 1: Write the failing test** (in `settings.rs` tests module, or create one)

```rust
    #[test]
    fn meeting_settings_defaults_and_provider_fallback() {
        let s = get_default_settings();
        assert_eq!(s.meeting_provider_id, "");
        assert_eq!(s.meeting_sensitivity, MeetingSensitivity::Medium);
        assert_eq!(s.meeting_observer_interval_secs, 20);
        assert!(s.meeting_beep_enabled);
        let active = s.active_post_process_provider().map(|p| p.id.clone());
        assert_eq!(s.meeting_provider().map(|p| p.id.clone()), active);
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib settings:: 2>&1 | tail -3`
Expected: compile error.

- [ ] **Step 3: Implement**

Follow the existing patterns exactly: `#[serde(default = "...")]` on each new field; default fns; entries in `get_default_settings()`; `#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type, Default)] #[serde(rename_all = "lowercase")] pub enum MeetingSensitivity`. No schema-version bump needed (serde defaults cover absent keys). Commands mirror `change_post_process_enabled_setting`. For the interval command clamp: `settings.meeting_observer_interval_secs = secs.clamp(10, 120)`.

- [ ] **Step 4: Verify and commit (paths only)**

Run: `cargo test --lib settings:: 2>&1 | tail -3` and `cargo check 2>&1 | tail -2`.

```bash
cargo fmt -- src/settings.rs src/shortcut/mod.rs src/lib.rs
git add src-tauri/src/settings.rs src-tauri/src/shortcut/mod.rs src-tauri/src/lib.rs src/stores/settingsStore.ts
git commit -m "feat: add meeting observer settings"
```

---

### Task 5: Observer (Lane D, after Tasks 1 to 4)

**Files:**

- Create: `src-tauri/src/meeting/observer.rs`
- Modify: `src-tauri/src/meeting/mod.rs` (`pub mod observer;`)
- Modify: `src-tauri/src/meeting/session.rs` (observer hooks: `is_live()` already exists; add `active_meeting_id()`, `active_session_id()`; keep `active_project_id()`)
- Modify: `src-tauri/src/audio_feedback.rs` (add `pub fn play_alert_sound(app: &AppHandle)`)
- Modify: `src-tauri/src/lib.rs` (spawn observer after `MeetingManager` is managed; register `InterventionPayload`, `ObserverStatusPayload` events)

**Interfaces:**

- Produces:
  - `pub struct Verdict { pub intervene: bool, pub kind: Option<String>, pub text: Option<String>, pub reason: String }` (Deserialize)
  - `pub fn verdict_schema() -> Value`
  - `pub struct JudgeInput { pub system: String, pub user: String }`
  - `pub trait Judge: Send + Sync { fn judge(&self, input: &JudgeInput, tools_ctx: &ToolContext, allow_tools: bool) -> Result<Verdict, JudgeError>; }` with `pub enum JudgeError { ToolsRejected, Other(String) }`
  - `pub struct LlmJudge { pub app: AppHandle }` implementing it with `llm_client::send_with_tools` (blocking via `tauri::async_runtime::block_on`); parses the text as `Verdict` (strip ```json fences if present).
  - `pub fn build_prompt(brief: &str, prev_meeting: Option<(&str, Option<&str>)>, context: &str, recent: &[Intervention], lines: &[TranscriptLine], sensitivity: MeetingSensitivity, tools_available: bool) -> JudgeInput`
  - `pub struct ObserverState { last_intervention_ts: i64, last_seen_line_ts: i64, consecutive_failures: u32, tools_unavailable: bool }`
  - `pub fn should_emit(verdict: &Verdict, now_ms: i64, state: &ObserverState, recent: &[Intervention]) -> bool` (intervene true, kind valid, text non-empty, cooldown 60 s, not equal (case-insensitive trimmed) to any of the last 3 texts)
  - `pub fn tick(store: &ProjectStore, judge: &dyn Judge, mm: &MeetingManager, settings: &AppSettings, state: &mut ObserverState, now_ms: i64) -> Result<Option<Intervention>>` doing one observation; emits nothing itself (the runner emits and beeps)
  - `pub fn spawn_observer(app: AppHandle, store: Arc<ProjectStore>, mm: Arc<MeetingManager>, judge: Arc<dyn Judge>)` thread named `meet-observer`: loop { sleep interval from settings; if `mm.is_live()` → `tick`; on `Some(iv)` → `audio_feedback::play_alert_sound` if enabled, emit `InterventionPayload { meeting_id, intervention }`, call `crate::intervention::show(&app, &iv)` (Task 6 provides; until then stub a `pub fn show` that only logs); emit `ObserverStatusPayload { meeting_id, tools_unavailable, consecutive_failures }` after each tick }.
  - Events: `InterventionPayload { meeting_id: i64, intervention: Intervention }`, `ObserverStatusPayload { meeting_id: Option<i64>, tools_unavailable: bool, consecutive_failures: u32 }`.

- [ ] **Step 1: Write the failing tests**

````rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::meeting::store::{Intervention, ProjectStore};
    use crate::meeting::session::{CaptureHandle, CaptureSource, MeetingManager, Transcriber};
    use crate::meeting::capture::Utterance;
    use std::sync::atomic::AtomicBool;
    use std::sync::{mpsc, Arc, Mutex};

    struct NoCapture;
    struct NoHandle;
    impl CaptureHandle for NoHandle { fn stop(self: Box<Self>) {} fn has_failed(&self) -> bool { false } }
    impl CaptureSource for NoCapture {
        fn start(&self, _tx: mpsc::Sender<Utterance>) -> anyhow::Result<Box<dyn CaptureHandle>> { Ok(Box::new(NoHandle)) }
    }
    struct NoTranscriber;
    impl Transcriber for NoTranscriber { fn transcribe(&self, _s: Vec<f32>) -> anyhow::Result<String> { Ok(String::new()) } }

    struct ScriptedJudge { verdicts: Mutex<Vec<Result<Verdict, JudgeError>>>, calls: Mutex<Vec<bool>> }
    impl Judge for ScriptedJudge {
        fn judge(&self, _i: &JudgeInput, _c: &ToolContext, allow_tools: bool) -> Result<Verdict, JudgeError> {
            self.calls.lock().unwrap().push(allow_tools);
            self.verdicts.lock().unwrap().remove(0)
        }
    }

    fn setup() -> (tempfile::TempDir, Arc<ProjectStore>, Arc<MeetingManager>, i64) {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(ProjectStore::open(dir.path().join("meet.db")).unwrap());
        let mm = Arc::new(MeetingManager::new_for_test(store.clone(), Arc::new(NoTranscriber), Arc::new(NoCapture), Arc::new(AtomicBool::new(false))));
        let m = mm.start(None, None).unwrap();
        let sid = mm.state().session_id.unwrap();
        store.append_line(sid, "me", "we keep going around on the retry budget", 1000).unwrap();
        (dir, store, mm, m.id)
    }

    fn v(intervene: bool, kind: &str, text: &str) -> Verdict {
        Verdict { intervene, kind: Some(kind.into()), text: Some(text.into()), reason: "r".into() }
    }

    #[test]
    fn prompt_contains_ordered_sections_and_schema() {
        let lines = vec![TranscriptLine { id: 1, session_id: 1, ts: 1000, speaker: "me".into(), text: "hello".into() }];
        let p = build_prompt("Project brief", Some(("Old meeting", Some("old summary"))), "agenda", &[], &lines, MeetingSensitivity::Low, true);
        let b = p.user.find("Project brief").unwrap();
        let o = p.user.find("old summary").unwrap();
        let a = p.user.find("agenda").unwrap();
        let h = p.user.find("hello").unwrap();
        assert!(b < o && o < a && a < h);
        assert!(p.system.contains("\"intervene\""));
        assert!(p.system.to_lowercase().contains("only on loops and explicit stuck") || p.system.to_lowercase().contains("low"));
    }

    #[test]
    fn should_emit_applies_cooldown_and_dedupe() {
        let state = ObserverState { last_intervention_ts: 100_000, last_seen_line_ts: 0, consecutive_failures: 0, tools_unavailable: false };
        let recent = vec![Intervention { id: 1, session_id: 1, ts: 100_000, kind: "loop".into(), text: "You said that twice".into(), reason: "".into(), dismissed: false }];
        assert!(!should_emit(&v(true, "loop", "new text"), 130_000, &state, &recent), "within 60 s cooldown");
        assert!(should_emit(&v(true, "loop", "new text"), 170_000, &state, &recent));
        assert!(!should_emit(&v(true, "loop", "  you said THAT twice "), 170_000, &state, &recent), "dedupe");
        assert!(!should_emit(&v(false, "loop", "x"), 170_000, &state, &recent));
        assert!(!should_emit(&v(true, "weird", "x"), 170_000, &state, &recent));
        assert!(!should_emit(&Verdict { intervene: true, kind: Some("idea".into()), text: None, reason: "".into() }, 170_000, &state, &recent));
    }

    #[test]
    fn tick_stores_intervention_and_skips_when_no_new_lines() {
        let (_d, store, mm, mid) = setup();
        let judge = ScriptedJudge { verdicts: Mutex::new(vec![Ok(v(true, "loop", "You are circling"))]), calls: Mutex::new(vec![]) };
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
        let judge = ScriptedJudge { verdicts: Mutex::new(vec![Err(JudgeError::ToolsRejected), Ok(v(false, "loop", ""))]), calls: Mutex::new(vec![]) };
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
        let judge = ScriptedJudge { verdicts: Mutex::new(vec![Err(JudgeError::Other("boom".into()))]), calls: Mutex::new(vec![]) };
        let settings = crate::settings::get_default_settings();
        let mut st = ObserverState::default();
        let out = tick(&store, &judge, &mm, &settings, &mut st, 200_000).unwrap();
        assert!(out.is_none());
        assert_eq!(st.consecutive_failures, 1);
    }

    #[test]
    fn parse_verdict_strips_fences() {
        let v: Verdict = parse_verdict("```json\n{\"intervene\":true,\"kind\":\"info\",\"text\":\"t\",\"reason\":\"r\"}\n```").unwrap();
        assert!(v.intervene);
        assert!(parse_verdict("not json").is_err());
    }
}
````

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib meeting::observer 2>&1 | tail -3`
Expected: compile error.

- [ ] **Step 3: Implement**

`observer.rs`:

````rust
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
    let t = t.strip_prefix("```json").or_else(|| t.strip_prefix("```")).unwrap_or(t);
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
    fn judge(&self, input: &JudgeInput, tools_ctx: &ToolContext, allow_tools: bool) -> Result<Verdict, JudgeError>;
}

pub struct LlmJudge {
    pub app: AppHandle,
}

impl Judge for LlmJudge {
    fn judge(&self, input: &JudgeInput, tools_ctx: &ToolContext, allow_tools: bool) -> Result<Verdict, JudgeError> {
        let settings = crate::settings::get_settings(&self.app);
        let provider = settings.meeting_provider().cloned().ok_or_else(|| JudgeError::Other("no provider configured".into()))?;
        let model = settings.meeting_model().ok_or_else(|| JudgeError::Other("no model configured".into()))?;
        let api_key = settings.meeting_api_key();
        let defs = if allow_tools { tool_defs() } else { Vec::new() };
        let handler = |c: &llm_client::ToolCall| handle_tool(tools_ctx, c);
        let fut = llm_client::send_with_tools(
            &provider, api_key, &model, input.system.clone(), input.user.clone(), &defs, verdict_schema(), MAX_TOOL_CALLS, &handler,
        );
        let budget = std::time::Duration::from_secs(15);
        let text = tauri::async_runtime::block_on(async move {
            tokio::time::timeout(budget, fut).await
        });
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
        MeetingSensitivity::Low => "Sensitivity LOW: intervene only on loops and explicit stuck moments.",
        MeetingSensitivity::Medium => "Sensitivity MEDIUM: intervene on loops, stuck moments, and clearly missing facts.",
        MeetingSensitivity::High => "Sensitivity HIGH: also offer ideas freely when an obvious alternative is untried.",
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
    user.push_str(if brief.trim().is_empty() { "(none)" } else { brief });
    user.push_str("\n\n## Previous meeting in this project\n");
    match prev_meeting {
        Some((title, Some(summary))) => user.push_str(&format!("{title}: {summary}")),
        Some((title, None)) => user.push_str(&format!("{title}: (no summary yet)")),
        None => user.push_str("(none)"),
    }
    user.push_str("\n\n## This meeting's context\n");
    user.push_str(if context.trim().is_empty() { "(none)" } else { context });
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

pub fn should_emit(verdict: &Verdict, now_ms: i64, state: &ObserverState, recent: &[Intervention]) -> bool {
    if !verdict.intervene {
        return false;
    }
    let Some(kind) = verdict.kind.as_deref() else { return false };
    if !VALID_KINDS.contains(&kind) {
        return false;
    }
    let Some(text) = verdict.text.as_deref().map(str::trim).filter(|t| !t.is_empty()) else { return false };
    if now_ms - state.last_intervention_ts < COOLDOWN_MS {
        return false;
    }
    let norm = text.to_lowercase();
    !recent.iter().take(DEDUPE_RECENT).any(|i| i.text.trim().to_lowercase() == norm)
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
    let (Some(meeting_id), Some(session_id)) = (st.meeting_id, st.session_id) else { return Ok(None) };
    let latest = store.latest_line_ts(meeting_id)?.unwrap_or(0);
    if latest <= state.last_seen_line_ts {
        return Ok(None);
    }
    state.last_seen_line_ts = latest;

    let meeting = store.get_meeting(meeting_id)?.ok_or_else(|| anyhow::anyhow!("meeting gone"))?;
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
    let ctx = ToolContext { store: Arc::new(store.clone_handle()), project_id: meeting.project_id, meeting_id };

    let verdict = match judge.judge(&input, &ctx, !state.tools_unavailable) {
        Ok(v) => v,
        Err(JudgeError::ToolsRejected) => {
            warn!("observer: provider rejected tools; falling back without tools");
            state.tools_unavailable = true;
            let input = build_prompt(&brief, prev.as_ref().map(|(t, s)| (t.as_str(), s.as_deref())), &meeting.context, &recent, &lines, settings.meeting_sensitivity, false);
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
````

`ToolContext` needs an `Arc<ProjectStore>`; `tick` receives `&ProjectStore`. Add to `store.rs` a cheap `pub fn clone_handle(&self) -> ProjectStore` that returns a new `ProjectStore { db_path: self.db_path.clone() }` (connections are per call, so this is safe). Alternatively change `tick` to take `Arc<ProjectStore>`; choose whichever keeps the tests above compiling with minimal edits (the tests pass `&store` where `store: Arc<ProjectStore>` derefs, so `&ProjectStore` works with `clone_handle`).

Events and runner:

```rust
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

pub fn spawn_observer(app: AppHandle, store: Arc<ProjectStore>, mm: Arc<MeetingManager>, judge: Arc<dyn Judge>) {
    let _ = std::thread::Builder::new().name("meet-observer".into()).spawn(move || {
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
                        let _ = (InterventionPayload { meeting_id: mid, intervention: iv.clone() }).emit(&app);
                    }
                    crate::intervention::show(&app, &iv);
                }
                Ok(None) => {}
                Err(e) => warn!("observer tick error: {e:#}"),
            }
            let _ = (ObserverStatusPayload { meeting_id: st.meeting_id, tools_unavailable: state.tools_unavailable, consecutive_failures: state.consecutive_failures }).emit(&app);
        }
    });
}
```

`audio_feedback.rs`: add

```rust
/// Short alert for assistant interventions. Not gated by `audio_feedback`;
/// the caller checks `meeting_beep_enabled`.
pub fn play_alert_sound(app: &AppHandle) {
    let app = app.clone();
    thread::spawn(move || {
        let path = match app.path().resolve("resources/pop_start.wav", tauri::path::BaseDirectory::Resource) {
            Ok(p) => p,
            Err(e) => { warn!("alert sound path: {e}"); return; }
        };
        if let Err(e) = play_sound_at_path(&app, &path) {
            warn!("alert sound: {e}");
        }
    });
}
```

(`tauri::Manager` must be in scope for `.path()`; check the file's imports.)

Create a stub `src-tauri/src/intervention.rs` with `pub fn show(app: &tauri::AppHandle, iv: &crate::meeting::store::Intervention) { log::info!("intervention: [{}] {}", iv.kind, iv.text); }` and `mod intervention;` in `lib.rs`; Task 6 replaces the body. `session.rs`: add `pub fn active_meeting_id(&self) -> Option<i64>` and `active_session_id` if not present (use `state()`).

`lib.rs`: after `app_handle.manage(meeting_manager.clone())` (bind `let meeting_manager = Arc::new(...)` and manage a clone), call `meeting::observer::spawn_observer(app_handle.clone(), meeting_store.clone(), meeting_manager.clone(), Arc::new(meeting::observer::LlmJudge { app: app_handle.clone() }))`. Add both events to `collect_events!`.

- [ ] **Step 4: Run tests, check, commit (paths only)**

Run: `cargo test --lib meeting::observer 2>&1 | tail -3` (6 passed); `cargo test --lib meeting:: 2>&1 | grep "test result"`; `cargo check 2>&1 | tail -2`.

```bash
cargo fmt -- src/meeting/observer.rs src/meeting/session.rs src/meeting/store.rs src/audio_feedback.rs src/intervention.rs src/lib.rs src/meeting/mod.rs
git add src-tauri/src/meeting/observer.rs src-tauri/src/meeting/mod.rs src-tauri/src/meeting/session.rs src-tauri/src/meeting/store.rs src-tauri/src/audio_feedback.rs src-tauri/src/intervention.rs src-tauri/src/lib.rs
git commit -m "feat: add observer loop that judges the live transcript and records interventions"
```

---

### Task 6: Intervention window and commands (Lane F, after Task 5)

**Files:**

- Modify: `src-tauri/src/intervention.rs` (real window)
- Modify: `src-tauri/src/commands/meeting.rs` (`list_interventions`, `dismiss_intervention`, `hide_intervention_window`)
- Modify: `src-tauri/src/lib.rs` (commands; create the window at setup next to `create_recording_overlay`)
- Regenerate: `src/bindings.ts`

**Interfaces:**

- Produces:
  - `pub fn create_intervention_window(app: &AppHandle)` building a `WebviewWindow` labelled `intervention` at `src/intervention/index.html`, `inner_size(380.0, 150.0)`, `decorations(false)`, `always_on_top(true)`, `skip_taskbar(true)`, `transparent(true)`, `focusable(false)`, `focused(false)`, `visible(false)`, `resizable(false)`, `shadow(true)`; macOS: additionally `visible_on_all_workspaces(true)` if the builder exposes it, else use the overlay's NSPanel pattern copied with `PanelLevel::Floating` and `no_activate(true)`. Position: top-right of the monitor under the cursor (reuse `overlay::get_monitor_with_cursor` if visible; else primary monitor), 16 px margins.
  - `pub fn show(app: &AppHandle, iv: &Intervention)`: emits `intervention:show` with the `Intervention` JSON to the window and shows it (on the main thread via `run_on_main_thread`), using a generation counter like the overlay so a late hide from a previous card cannot hide a new one.
  - `pub fn hide(app: &AppHandle)`.
  - Commands: `list_interventions(meeting_id) -> Vec<Intervention>`, `dismiss_intervention(id)`, `hide_intervention_window()`.

- [ ] **Step 1: Implement** (no unit tests; Tauri window code; verified by `cargo check` and the manual smoke)

Model the window code on the non-macOS `create_recording_overlay` body (same builder chain minus `closable(false)`), and on macOS reuse the NSPanel approach from `overlay.rs` with `PanelLevel::Floating`, `no_activate(true)`, `can_join_all_spaces()`. Store the `OVERLAY_SHOW_GENERATION`-style counter as `static INTERVENTION_GENERATION: AtomicU64`. `show`:

```rust
pub fn show(app: &AppHandle, iv: &Intervention) {
    let app = app.clone();
    let iv = iv.clone();
    let gen = INTERVENTION_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let _ = app.run_on_main_thread(move || {
        if let Some(w) = app.get_webview_window("intervention") {
            position_top_right(&app, &w);
            let _ = w.emit("intervention:show", &iv);
            let _ = w.show();
        }
        #[cfg(target_os = "macos")]
        { /* if using NSPanel: fetch panel by label and call show_and_make_key? no: show without activating */ }
        let _ = gen;
    });
}
```

Keep the frontend responsible for auto-hide: it calls `hide_intervention_window` after 30 s unless hovered (Task 7a).

Commands:

```rust
#[tauri::command]
#[specta::specta]
pub fn list_interventions(mm: Mm<'_>, meeting_id: i64) -> Result<Vec<Intervention>, String> {
    mm.store().list_interventions(meeting_id).map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn dismiss_intervention(mm: Mm<'_>, id: i64) -> Result<(), String> {
    mm.store().dismiss_intervention(id).map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn hide_intervention_window(app: tauri::AppHandle) {
    crate::intervention::hide(&app);
}
```

Register the three commands. In `lib.rs` setup, where `overlay::create_recording_overlay(&app_handle)` is called, add `intervention::create_intervention_window(&app_handle)`.

- [ ] **Step 2: Regenerate bindings and verify**

Run from `src-tauri/`: `cargo check 2>&1 | tail -2`, then `cargo run -- --list-models > /tmp/lm.log 2>&1; tail -2 /tmp/lm.log` (exit 0). From repo root: `grep -c "listInterventions\|dismissIntervention\|hideInterventionWindow\|interventionPayload\|observerStatusPayload\|MeetingSensitivity\|changeMeetingProviderSetting" src/bindings.ts` (at least 7); `bunx tsc --noEmit` (the Task 4 `settingUpdaters` entries now typecheck).

- [ ] **Step 3: Commit**

```bash
cargo fmt -- src/intervention.rs src/commands/meeting.rs src/lib.rs
git add src-tauri/src/intervention.rs src-tauri/src/commands/meeting.rs src-tauri/src/lib.rs src/bindings.ts
git commit -m "feat: add intervention window and intervention commands"
```

---

### Task 7a: Intervention card frontend and i18n (Lane E, parallel with Task 5)

**Files:**

- Create: `src/intervention/index.html`, `src/intervention/main.tsx`, `src/intervention/InterventionCard.tsx`
- Modify: `vite.config.ts` (add `intervention: resolve(__dirname, "src/intervention/index.html")`)
- Modify: `src/i18n/locales/en/translation.json` and the 25 other locales

**Interfaces:**

- Consumes: untyped event `intervention:show` with payload `{ id, session_id, ts, kind, text, reason, dismissed }`; `invoke("hide_intervention_window")` and `invoke("dismiss_intervention", { id })` via `@tauri-apps/api/core` `invoke` (typed bindings arrive in Task 6; use `invoke` here to stay independent).
- Produces: the card window UI.

- [ ] **Step 1: i18n**

Add under `meetings` in en:

```json
    "intervention": {
      "kind": { "loop": "Going in circles", "stuck": "Someone is stuck", "info": "Quick fact", "idea": "An idea" },
      "dismiss": "Dismiss",
      "open": "Open meeting",
      "timeline": "Assistant nudges",
      "empty": "No nudges yet.",
      "dismissed": "Dismissed"
    },
    "observer": {
      "toolsUnavailable": "This provider does not support tools; the assistant answers from the transcript only.",
      "failing": "The assistant has failed {{count}} times in a row. Check the provider settings."
    }
```

and a top-level `settings.meeting` block:

```json
    "meeting": {
      "title": "Meeting assistant",
      "description": "How the assistant watches live meetings.",
      "provider": { "label": "Provider", "description": "Which post-processing provider answers during meetings. Empty uses the post-processing default.", "default": "Same as post-processing" },
      "sensitivity": { "label": "Sensitivity", "description": "How eager the assistant is to speak up.", "low": "Low", "medium": "Medium", "high": "High" },
      "interval": { "label": "Check interval", "description": "Seconds between checks of the live transcript." },
      "beep": { "label": "Beep on nudge", "description": "Play a short sound when the assistant speaks up." }
    }
```

Copy into all 25 locales at the same positions (throwaway script, delete before commit). `bun run check:translations` 25/25.

- [ ] **Step 2: Window entry**

`src/intervention/index.html`: copy `src/overlay/index.html`, change the script to `/src/intervention/main.tsx` and the title to `CASE`. `main.tsx`: copy `src/overlay/main.tsx`, render `<InterventionCard />`.

`InterventionCard.tsx`:

```tsx
import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import {
  Lightbulb,
  MessageCircleQuestion,
  Repeat,
  Info,
  X,
} from "lucide-react";

type Intervention = {
  id: number;
  session_id: number;
  ts: number;
  kind: string;
  text: string;
  reason: string;
  dismissed: boolean;
};

const ICON: Record<string, React.ComponentType<{ className?: string }>> = {
  loop: Repeat,
  stuck: MessageCircleQuestion,
  info: Info,
  idea: Lightbulb,
};

const AUTO_HIDE_MS = 30_000;

export const InterventionCard: React.FC = () => {
  const { t } = useTranslation();
  const [iv, setIv] = useState<Intervention | null>(null);
  const hovered = useRef(false);
  const timer = useRef<number | null>(null);

  const hide = async () => {
    if (timer.current) window.clearTimeout(timer.current);
    await invoke("hide_intervention_window");
  };

  const arm = () => {
    if (timer.current) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      if (hovered.current) arm();
      else hide();
    }, AUTO_HIDE_MS);
  };

  useEffect(() => {
    const un = listen<Intervention>("intervention:show", (e) => {
      setIv(e.payload);
      arm();
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  if (!iv) return null;
  const Icon = ICON[iv.kind] ?? Info;
  return (
    <div
      className="h-screen w-screen p-3 bg-background text-text rounded-xl border border-mid-gray/30 shadow-lg flex flex-col gap-2"
      onMouseEnter={() => (hovered.current = true)}
      onMouseLeave={() => (hovered.current = false)}
    >
      <div className="flex items-center gap-2">
        <Icon className="w-4 h-4 text-logo-primary shrink-0" />
        <span className="text-xs font-medium uppercase tracking-wide text-mid-gray flex-1">
          {t(`meetings.intervention.kind.${iv.kind}`)}
        </span>
        <button
          className="text-mid-gray hover:text-text cursor-pointer"
          title={t("meetings.intervention.dismiss")}
          onClick={async () => {
            await invoke("dismiss_intervention", { id: iv.id });
            await hide();
          }}
        >
          <X className="w-4 h-4" />
        </button>
      </div>
      <p className="text-sm leading-snug">{iv.text}</p>
    </div>
  );
};
```

Check `src/overlay/RecordingOverlay.tsx` for how it imports Tailwind styles (likely `import "@/App.css"` or similar in `main.tsx`); mirror it so tokens resolve.

- [ ] **Step 3: Verify and commit (paths only)**

Run: `bunx tsc --noEmit && bun run lint && bun run check:translations 2>&1 | tail -1 && bunx prettier --check src/intervention vite.config.ts "src/i18n/locales/**/translation.json"`.

```bash
git add src/intervention vite.config.ts src/i18n/locales
git commit -m "feat: add intervention card window entry and observer strings"
```

---

### Task 7b: Timeline, observer status, and settings UI (Lane F, after Task 6)

**Files:**

- Create: `src/components/meetings/InterventionTimeline.tsx`
- Modify: `src/stores/meetingStore.ts` (interventions in `detail`, `observer` status, `dismiss`)
- Modify: `src/components/meetings/MeetingDetail.tsx` (render timeline, observer banner)
- Create: `src/components/settings/meeting/MeetingAssistantSettings.tsx`
- Modify: `src/components/settings/AdvancedSettings.tsx` (render the new group at the end)

**Interfaces:**

- Consumes: `events.interventionPayload`, `events.observerStatusPayload`, `commands.dismissIntervention`, `commands.changeMeeting*Setting`, `MeetingDetail.interventions`.

- [ ] **Step 1: Store**

`meetingStore`: add `observer: { tools_unavailable: boolean; consecutive_failures: number }` default `{ tools_unavailable: false, consecutive_failures: 0 }`; in `subscribe` also listen to `events.interventionPayload` (append to `detail.interventions` when `meeting_id` matches and id not present) and `events.observerStatusPayload` (set `observer` when `meeting_id === activeMeetingId` or `meeting_id === null`); add `dismiss(id)` calling `commands.dismissIntervention(id)` then marking `dismissed: true` locally. The returned unlisten calls all four.

- [ ] **Step 2: Timeline**

```tsx
import React from "react";
import { useTranslation } from "react-i18next";
import type { Intervention } from "@/bindings";
import { useMeetingStore } from "@/stores/meetingStore";
import Badge from "../ui/Badge";
import { Button } from "../ui/Button";
import { SettingsGroup } from "../ui/SettingsGroup";
import { formatDateTime } from "@/utils/dateFormat";

export const InterventionTimeline: React.FC<{ items: Intervention[] }> = ({
  items,
}) => {
  const { t, i18n } = useTranslation();
  const dismiss = useMeetingStore((s) => s.dismiss);
  return (
    <SettingsGroup title={t("meetings.intervention.timeline")}>
      {items.length === 0 ? (
        <p className="p-4 text-sm text-mid-gray">
          {t("meetings.intervention.empty")}
        </p>
      ) : (
        items.map((iv) => (
          <div key={iv.id} className="p-4 flex items-start gap-3">
            <Badge variant={iv.dismissed ? "secondary" : "primary"}>
              {t(`meetings.intervention.kind.${iv.kind}`)}
            </Badge>
            <div className="flex-1 min-w-0">
              <div className="text-sm text-text">{iv.text}</div>
              <div className="text-xs text-mid-gray">
                {formatDateTime(
                  String(Math.floor(iv.ts / 1000)),
                  i18n.language,
                )}
              </div>
            </div>
            {!iv.dismissed && (
              <Button size="sm" variant="ghost" onClick={() => dismiss(iv.id)}>
                {t("meetings.intervention.dismiss")}
              </Button>
            )}
          </div>
        ))
      )}
    </SettingsGroup>
  );
};
```

In `MeetingDetail`, render `<InterventionTimeline items={detail.interventions} />` after the transcript, and above the context editor show an `Alert` (from `components/ui/Alert`, check its props) with `meetings.observer.toolsUnavailable` when `observer.tools_unavailable`, and `meetings.observer.failing` with `{ count }` when `consecutive_failures >= 3`.

- [ ] **Step 3: Settings UI**

`MeetingAssistantSettings.tsx`: a `SettingsGroup` titled `settings.meeting.title` with four rows using `SettingContainer` + `Dropdown` / `ToggleSwitch` / `Input type=number` following `ModelUnloadTimeout.tsx` and `PostProcessingToggle.tsx`:

- Provider: `Dropdown` options = `[{ value: "", label: t("settings.meeting.provider.default") }, ...settings.post_process_providers.map(p => ({ value: p.id, label: p.label }))]`, writes `updateSetting("meeting_provider_id", v)`.
- Sensitivity: `Dropdown` low/medium/high, writes `meeting_sensitivity`.
- Interval: `Input` number 10..120, on blur writes `meeting_observer_interval_secs`.
- Beep: `ToggleSwitch` writes `meeting_beep_enabled`.
  Render it at the bottom of `AdvancedSettings.tsx`.

- [ ] **Step 4: Verify and commit**

Run: `bunx tsc --noEmit && bun run lint && bunx prettier --check "src/components/**/*.tsx" src/stores/meetingStore.ts`.

```bash
git add src/components src/stores/meetingStore.ts
git commit -m "feat: show assistant nudges in the meeting timeline and add observer settings"
```

---

### Task 8: Format, clippy, docs (Lane F, last)

- [ ] **Step 1:** `bun run format` (revert unrelated reformat); `cargo clippy --tests` clean for `meeting/`, `llm_client.rs`, `intervention.rs`, `commands/meeting.rs`; `cargo test --lib` all pass; `bunx tsc --noEmit`; `bun run lint`; `bun run check:translations`.
- [ ] **Step 2:** `docs/ARCHITECTURE.md`: existing rows for `meeting/observer.rs`, `meeting/tools.rs`, `intervention.rs`, `src/intervention/`, `components/meetings/InterventionTimeline.tsx`, `components/settings/meeting/`; store row adds `interventions`; events add `intervention-payload`, `observer-status-payload`; `llm_client.rs` row mentions tool calling; delivery order marks step 4 done. Spec section 3a: note that `response_format` is applied only on the final tool-less request and that Anthropic receives the schema in the system prompt.
- [ ] **Step 3:** Commit `chore: format, clippy, and docs for observer and interventions`.

Manual smoke (controller or user, on a running `tauri dev`): configure a provider with tool support (Anthropic or OpenAI) under post-processing, start a quick meeting, read aloud the same sentence three times with pauses; within two intervals a beep sounds and a card appears top-right without stealing focus; the card auto-hides after 30 s; the meeting detail shows the nudge in the timeline; Dismiss marks it dismissed. Switch the provider to one without tools (if available) and confirm the banner.
