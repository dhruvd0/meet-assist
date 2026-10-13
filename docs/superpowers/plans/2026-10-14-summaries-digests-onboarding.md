# Summaries, Image Digests, and Onboarding Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When a meeting ends, the assistant writes a session summary and updates the meeting's rolling summary. Image sources get a model-written digest when the provider can see images. New users meet one onboarding step that checks the assistant provider. Meeting cards and project meeting rows show summaries.

**Architecture:** One provider-agnostic `complete_json` helper in `llm_client.rs` built on the existing tool loop with zero tools (works for OpenAI and Anthropic formats), plus one `describe_image` helper that sends a base64 image in each format's content shape. `meeting/summary.rs` holds the prompts, a `Summarizer` trait (fake in tests), and `summarize_session` / `merge_rolling`. `MeetingManager::end` and `end_idle` spawn a summary job after closing the session; `ProjectManager` runs image digests in the indexer worker. Frontend: summaries rendered in meeting detail and lists; an onboarding "Assistant" step inserted after the model step.

**Tech Stack:** Rust (reqwest, serde_json, base64 via `base64` crate if present else `data-encoding`; check `Cargo.lock`), React 18, Zustand, i18next.

**Spec:** `docs/superpowers/specs/2026-10-13-meeting-assistant-design.md` sections 2 (summary field, onboarding), 2a (image digest at add time), 3 (`summary.rs`), 5 (summary prompts), 8 step 6.

## Parallel lanes

| Lane                | Tasks     | Files                                                                                                                                                        |
| ------------------- | --------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| A                   | 1         | `src-tauri/src/llm_client.rs`                                                                                                                                |
| B                   | 2         | `src-tauri/src/meeting/store.rs`                                                                                                                             |
| C (after A, B)      | 3 then 4  | `src-tauri/src/meeting/summary.rs`, `meeting/mod.rs`, `meeting/session.rs`, `lib.rs`; then `meeting/manager.rs`, `meeting/indexer.rs`, `commands/project.rs` |
| D (parallel with C) | 5a        | `src/i18n/locales/**`, `src/components/onboarding/AssistantOnboarding.tsx` (new)                                                                             |
| E (after C, D)      | 5b then 6 | `src/App.tsx`, `src/components/meetings/*`, `src/components/projects/ProjectMeetings.tsx`, `src/stores/meetingStore.ts`, bindings regen; then docs           |

Rules as in the previous plan: disjoint files per lane, per-file formatting, path-scoped commits, retry on `index.lock`, wait and retry when another lane's file breaks the crate build.

## Global Constraints

- Toolchain exports as before (`PATH`, `CMAKE_POLICY_VERSION_MINIMUM=3.5`, `SDKROOT=...MacOSX26.5.sdk`); cargo from `src-tauri/`, bun from repo root; `cargo run -- --list-models` only in Task 5b; never hand-edit `src/bindings.ts`; no `bun run build`.
- No `unwrap` in non-test production paths. Rust tests use `tempfile::tempdir()`.
- Session summary prompt output schema exactly: `{ "decisions": string[], "open_questions": string[], "action_items": string[], "recap": string }`; stored on `sessions.summary` as JSON text. Rolling summary is plain text capped at about 300 words (prompt-enforced; hard cap 2500 characters on store).
- Summaries run in a background thread after the session is closed; failure is logged and leaves `summary` NULL; UI shows "No summary" with a Retry button that calls `summarize_session(session_id)`.
- Image digest: at add time, after the copy, if `settings.meeting_provider()` exists and `meeting_vision_enabled` (new bool setting, default true) the indexer calls `describe_image`; on success the digest is stored and indexed; on failure the digest stays empty and the UI caption path still works. Digest length cap 1200 characters.
- Onboarding step "Assistant": shown to new users after the model step; lets them pick a provider from `post_process_providers`, enter an API key, pick or type a model, and press "Test" which calls `fetch_post_process_models`; "Skip" is always available; marks `onboarding_completed` as today.
- Strings under `meetings.summary.*`, `onboarding.assistant.*`, `projects.sources.digest*` in en, copied into 25 locales.
- Commit each task with a conventional prefix and the trailer `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.

## Review Focus

1. Ending a meeting must never block the UI on the summary call; the session closes immediately and the summary arrives later via event. Pinned in Task 3 (end returns before the summary job; event `session-summary-payload`).
2. A provider without vision must not break image sources; the caption path still works and no error toast storms. Pinned in Task 4.
3. A rolling summary must not grow without bound across many sessions. Pinned in Task 2 (hard cap) and Task 3 (merge prompt instruction).
4. The onboarding step must be skippable and must not trap returning users. Pinned in Task 5b.
5. Anthropic-format providers must get summaries too (the old schema helper was OpenAI-only). Pinned in Task 1.

---

### Task 1: Provider-agnostic JSON completion and image description (Lane A)

**Files:**

- Modify: `src-tauri/src/llm_client.rs`

**Interfaces:**

- Produces:
  - `pub async fn complete_json(provider, api_key: String, model: &str, system_prompt: String, user_content: String, schema: Value) -> Result<Value, String>`: calls `send_with_tools` with no tools and `max_tool_calls = 0`, strips fences, parses JSON.
  - `pub async fn describe_image(provider, api_key: String, model: &str, prompt: String, mime: &str, bytes: &[u8]) -> Result<String, String>`: one request; OpenAI format uses `content: [{"type":"text",...},{"type":"image_url","image_url":{"url":"data:<mime>;base64,<...>"}}]`; Anthropic format uses `content: [{"type":"image","source":{"type":"base64","media_type":<mime>,"data":<...>}},{"type":"text",...}]` with `max_tokens` 1024; returns the text answer. A 400/422 whose body mentions `image` or `vision` or `content` maps to `Err("vision_unsupported")` (exact string) so callers can fall back quietly.
  - Pure helpers for tests: `openai_image_message(prompt, mime, b64) -> Value`, `anthropic_image_message(prompt, mime, b64) -> Value`, `strip_json_fences(&str) -> &str`, `is_vision_rejection(status: u16, body: &str) -> bool`.

- [ ] **Step 1: Write the failing tests**

````rust
#[cfg(test)]
mod json_and_vision_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn image_messages_have_provider_shapes() {
        let o = openai_image_message("Describe", "image/png", "AAAA");
        assert_eq!(o["role"], "user");
        assert_eq!(o["content"][0]["type"], "text");
        assert_eq!(o["content"][1]["type"], "image_url");
        assert_eq!(o["content"][1]["image_url"]["url"], "data:image/png;base64,AAAA");
        let a = anthropic_image_message("Describe", "image/png", "AAAA");
        assert_eq!(a["content"][0]["type"], "image");
        assert_eq!(a["content"][0]["source"]["media_type"], "image/png");
        assert_eq!(a["content"][0]["source"]["data"], "AAAA");
        assert_eq!(a["content"][1]["type"], "text");
    }

    #[test]
    fn fences_are_stripped() {
        assert_eq!(strip_json_fences("```json\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(strip_json_fences("  {\"a\":1}  "), "{\"a\":1}");
        assert_eq!(strip_json_fences("prefix text {\"a\":1} suffix"), "{\"a\":1}");
    }

    #[test]
    fn vision_rejection_detection() {
        assert!(is_vision_rejection(400, "This model does not support image input"));
        assert!(is_vision_rejection(422, "invalid content type: image_url"));
        assert!(!is_vision_rejection(401, "image"));
        assert!(!is_vision_rejection(400, "rate limit"));
    }

    #[test]
    fn mime_from_extension_covers_common_images() {
        assert_eq!(mime_for_extension("PNG"), Some("image/png"));
        assert_eq!(mime_for_extension("jpg"), Some("image/jpeg"));
        assert_eq!(mime_for_extension("jpeg"), Some("image/jpeg"));
        assert_eq!(mime_for_extension("webp"), Some("image/webp"));
        assert_eq!(mime_for_extension("gif"), Some("image/gif"));
        assert_eq!(mime_for_extension("bmp"), None);
        let _ = json!({});
    }
}
````

- [ ] **Step 2: RED** — `cargo test --lib llm_client::json_and_vision_tests` fails to compile.

- [ ] **Step 3: Implement**

````rust
pub fn strip_json_fences(text: &str) -> &str {
    let t = text.trim();
    let t = t.strip_prefix("```json").or_else(|| t.strip_prefix("```")).unwrap_or(t);
    let t = t.strip_suffix("```").unwrap_or(t).trim();
    // Fall back to the outermost braces if the model wrapped the JSON in prose.
    match (t.find('{'), t.rfind('}')) {
        (Some(s), Some(e)) if e > s => &t[s..=e],
        _ => t,
    }
}

pub async fn complete_json(
    provider: &PostProcessProvider,
    api_key: String,
    model: &str,
    system_prompt: String,
    user_content: String,
    schema: Value,
) -> Result<Value, String> {
    let handler = |_c: &ToolCall| String::new();
    let text = send_with_tools(provider, api_key, model, system_prompt, user_content, &[], schema, 0, &handler)
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_str::<Value>(strip_json_fences(&text)).map_err(|e| format!("json parse: {e}"))
}

pub fn mime_for_extension(ext: &str) -> Option<&'static str> {
    match ext.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        _ => None,
    }
}

pub fn is_vision_rejection(status: u16, body: &str) -> bool {
    let b = body.to_ascii_lowercase();
    (status == 400 || status == 422) && (b.contains("image") || b.contains("vision") || b.contains("content"))
}

pub fn openai_image_message(prompt: &str, mime: &str, b64: &str) -> Value {
    serde_json::json!({ "role": "user", "content": [
        { "type": "text", "text": prompt },
        { "type": "image_url", "image_url": { "url": format!("data:{mime};base64,{b64}") } }
    ]})
}

pub fn anthropic_image_message(prompt: &str, mime: &str, b64: &str) -> Value {
    serde_json::json!({ "role": "user", "content": [
        { "type": "image", "source": { "type": "base64", "media_type": mime, "data": b64 } },
        { "type": "text", "text": prompt }
    ]})
}

pub async fn describe_image(
    provider: &PostProcessProvider,
    api_key: String,
    model: &str,
    prompt: String,
    mime: &str,
    bytes: &[u8],
) -> Result<String, String> {
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    let client = create_client(provider, &api_key)?;
    let base = provider.base_url.trim_end_matches('/');
    let anthropic = provider.api_format == PostProcessApiFormat::Anthropic;
    let (url, body) = if anthropic {
        (format!("{base}/messages"), serde_json::json!({
            "model": model, "max_tokens": 1024,
            "messages": [anthropic_image_message(&prompt, mime, &b64)]
        }))
    } else {
        (format!("{base}/chat/completions"), serde_json::json!({
            "model": model, "stream": false,
            "messages": [openai_image_message(&prompt, mime, &b64)]
        }))
    };
    let resp = client.post(&url).json(&body).send().await.map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    let text = resp.text().await.map_err(|e| e.to_string())?;
    if !(200..300).contains(&status) {
        if is_vision_rejection(status, &text) {
            return Err("vision_unsupported".into());
        }
        return Err(format!("HTTP {status}: {text}"));
    }
    let json: Value = serde_json::from_str(&text).map_err(|e| format!("bad json: {e}"))?;
    let (answer, _) = if anthropic { parse_anthropic_tool_calls(&json) } else { parse_openai_tool_calls(&json) };
    answer.map(|s| s.trim().to_string()).ok_or_else(|| "empty answer".into())
}
````

If `base64` is not in `Cargo.lock`, run `cargo add base64@0.22` from `src-tauri/` and include `Cargo.toml`/`Cargo.lock` in the commit. If it is present as a transitive dependency, `cargo add` still makes it direct; do that.

- [ ] **Step 4: GREEN** — `cargo test --lib llm_client::` all pass. Commit `feat: add provider-agnostic JSON completion and image description to the LLM client` (paths: `src-tauri/src/llm_client.rs`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`).

---

### Task 2: Summary columns and setters (Lane B)

**Files:**

- Modify: `src-tauri/src/meeting/store.rs`

**Interfaces:**

- Produces: `set_session_summary(&self, session_id, summary_json: &str) -> Result<()>`, `set_rolling_summary(&self, meeting_id, text: &str) -> Result<()>` (truncates to 2500 chars on a char boundary, bumps `updated_at`), `get_session(&self, session_id) -> Result<Option<Session>>`, `session_lines(&self, session_id) -> Result<Vec<TranscriptLine>>` (ts asc), `sessions_without_summary(&self, meeting_id) -> Result<Vec<Session>>` (ended sessions with NULL summary and at least one line). No new migration needed (columns exist).

- [ ] **Step 1: Tests**

```rust
    #[test]
    fn summary_setters_and_queries() {
        let (_d, s) = store();
        let m = s.create_meeting(None, "M").unwrap();
        let sess = s.create_session(m.id).unwrap();
        s.append_line(sess.id, "me", "hello", 1000).unwrap();
        s.end_session(sess.id).unwrap();
        assert_eq!(s.sessions_without_summary(m.id).unwrap().len(), 1);
        s.set_session_summary(sess.id, "{\"recap\":\"hi\"}").unwrap();
        assert_eq!(s.get_session(sess.id).unwrap().unwrap().summary.as_deref(), Some("{\"recap\":\"hi\"}"));
        assert!(s.sessions_without_summary(m.id).unwrap().is_empty());
        let long = "é".repeat(3000);
        s.set_rolling_summary(m.id, &long).unwrap();
        let got = s.get_meeting(m.id).unwrap().unwrap().rolling_summary.unwrap();
        assert!(got.chars().count() <= 2500);
        assert_eq!(s.session_lines(sess.id).unwrap()[0].text, "hello");
    }

    #[test]
    fn empty_sessions_are_not_summary_candidates() {
        let (_d, s) = store();
        let m = s.create_meeting(None, "M").unwrap();
        let sess = s.create_session(m.id).unwrap();
        s.end_session(sess.id).unwrap();
        assert!(s.sessions_without_summary(m.id).unwrap().is_empty());
    }
```

- [ ] **Step 2: RED**, **Step 3: Implement** (straightforward SQL; `sessions_without_summary`: `SELECT ... FROM sessions s WHERE s.meeting_id = ?1 AND s.ended_at IS NOT NULL AND s.summary IS NULL AND EXISTS (SELECT 1 FROM transcript_lines l WHERE l.session_id = s.id) ORDER BY s.started_at`), **Step 4: GREEN** `cargo test --lib meeting::store`. Commit `feat: add session and rolling summary setters` (path `src-tauri/src/meeting/store.rs`).

---

### Task 3: Summarizer and end-of-meeting job (Lane C)

**Files:**

- Create: `src-tauri/src/meeting/summary.rs`
- Modify: `src-tauri/src/meeting/mod.rs`, `src-tauri/src/meeting/session.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/commands/meeting.rs`

**Interfaces:**

- Produces:
  - `pub struct SessionSummary { pub decisions: Vec<String>, pub open_questions: Vec<String>, pub action_items: Vec<String>, pub recap: String }` (Serialize, Deserialize, Type)
  - `pub fn session_summary_schema() -> Value`; `pub fn session_prompt(title: &str, context: &str, lines: &[TranscriptLine]) -> (String, String)` (system, user); `pub fn rolling_prompt(previous: Option<&str>, new_session: &SessionSummary) -> (String, String)`; `pub fn format_session_summary_for_rolling(s: &SessionSummary) -> String`
  - `pub trait Summarizer: Send + Sync { fn summarize_session(&self, title: &str, context: &str, lines: &[TranscriptLine]) -> Result<SessionSummary>; fn merge_rolling(&self, previous: Option<&str>, new_session: &SessionSummary) -> Result<String>; }`
  - `pub struct LlmSummarizer { pub app: AppHandle }` using `complete_json` (blocking via `tauri::async_runtime::block_on`) with the meeting provider; rolling merge via `complete_json` with schema `{ "rolling_summary": string }`.
  - `pub fn run_summary_job(store: &ProjectStore, summarizer: &dyn Summarizer, session_id: i64) -> Result<Option<SessionSummary>>`: loads session, meeting, lines; returns `Ok(None)` if no lines; summarizes; stores; merges rolling (previous = meeting.rolling_summary); stores capped.
  - Event `SessionSummaryPayload { meeting_id: i64, session_id: i64, summary: SessionSummary, rolling_summary: String }`.
  - `MeetingManager` gains `summarizer: Option<Arc<dyn Summarizer>>` (None in `new_for_test` unless provided via `with_summarizer`); after `end` closes the session (and in `end_idle` for each session returned by `sessions_without_summary`), it spawns a thread `meet-summary` that runs `run_summary_job` and emits the event; test with a fake summarizer asserts the summary lands and `end()` returns before the fake (which sleeps 200 ms) finishes.
  - Command `summarize_session(session_id) -> Result<(), String>` (spawns the same job; used by the UI Retry).
  - `lib.rs`: pass `Arc::new(LlmSummarizer { app })` into the manager; register the event; register the command.

- [ ] **Step 1: Tests** (fake summarizer; `run_summary_job` stores both summaries; empty session returns `Ok(None)`; `end()` returns within 50 ms while the fake sleeps 200 ms; rolling merge receives the previous rolling text; `session_prompt` contains title, context, and lines in order; `format_session_summary_for_rolling` renders the three lists).
- [ ] **Step 2: RED**, **Step 3: Implement** (prompts per spec section 5: decisions, open questions, action items, three-line recap; rolling merge instruction "merge previous rolling summary with the new session summary, keep under 300 words, keep decisions and open questions, drop resolved questions"), **Step 4: GREEN** `cargo test --lib meeting::`; `cargo check`. Commit `feat: summarize sessions on end and maintain a rolling meeting summary`.

---

### Task 4: Image digests at add time (Lane C, after Task 3)

**Files:**

- Modify: `src-tauri/src/settings.rs` (add `meeting_vision_enabled: bool` default true with command `change_meeting_vision_setting`), `src-tauri/src/shortcut/mod.rs`, `src-tauri/src/lib.rs` (command), `src/stores/settingsStore.ts` (updater; typecheck deferred to 5b)
- Modify: `src-tauri/src/meeting/manager.rs`, `src-tauri/src/meeting/indexer.rs`

**Interfaces:**

- `indexer.rs`: `pub fn image_digest_prompt() -> String` ("Describe this image for someone who cannot see it, in at most 120 words: what it shows, any text in it, and why it might matter in a meeting."); `pub fn read_image_for_vision(path: &Path) -> Result<(&'static str, Vec<u8>)>` (mime via `llm_client::mime_for_extension`, size cap 4 MB, error otherwise).
- `manager.rs`: a `Describer` trait `{ fn describe(&self, mime: &str, bytes: &[u8]) -> Result<String, String> }` with real impl calling `llm_client::describe_image` (blocking) and `ProjectManager::new_for_test` taking `Option<Arc<dyn Describer>>`; in `index_inner` for `SourceKind::Image`: if digest is empty and a describer exists and `settings.meeting_vision_enabled`, call it; on `Ok(text)` store `set_source_digest(id, text truncated to 1200 chars)` and index it; on `Err("vision_unsupported")` log info once per manager (AtomicBool) and fall through; other errors warn. Caption path unchanged (a user caption overwrites the digest and reindexes).
- Tests: fake describer returns "A whiteboard with three boxes"; adding an image yields that digest and a searchable chunk; a describer returning `vision_unsupported` leaves the digest empty and status `ready`; a user caption after a model digest wins.

Commit `feat: write image digests with a vision-capable provider`.

---

### Task 5a: Onboarding step component and strings (Lane D)

**Files:**

- Create: `src/components/onboarding/AssistantOnboarding.tsx`
- Modify: `src/i18n/locales/en/translation.json` and 25 locales

**Interfaces:**

- Props `{ onComplete: () => void }`. Uses `usePostProcessProviderState()` for provider/model/API key fields (reuse `ProviderSelect`, `ApiKeyField`, `ModelSelect` from `src/components/settings/PostProcessingSettingsApi/` if their props allow standalone use; else render the equivalent with `Dropdown`/`Input`), a "Test connection" button calling `commands.fetchPostProcessModels(selectedProviderId)` and showing success/failure inline, "Continue" (enabled after a successful test or when a model is set) and "Skip". It writes nothing new itself; the shared hook persists the post-processing provider settings, which the meeting provider falls back to.
- Strings: `onboarding.assistant.{title,description,test,testing,success,failure,continue,skip,hint}`; `meetings.summary.{title,decisions,openQuestions,actionItems,recap,rolling,none,retry,generating}`; `projects.sources.{digestLabel,digestByModel}`.

Verify: `bunx eslint src/components/onboarding/AssistantOnboarding.tsx`, prettier, `bun run check:translations`. Commit `feat: add assistant onboarding step and summary strings`.

---

### Task 5b: Wire onboarding, summaries UI, bindings (Lane E)

**Files:**

- Modify: `src/App.tsx` (`OnboardingStep` gains `"assistant"`; new users go `accessibility → model → assistant → done`; returning users skip it), `src/stores/meetingStore.ts` (listen `sessionSummaryPayload`, update `detail.sessions[].summary` and `detail.meeting.rolling_summary`; `summarize(sessionId)` action), `src/components/meetings/SessionSummary.tsx` (new; renders parsed JSON summary with the three lists and recap, "No summary" plus Retry when NULL on an ended session), `MeetingDetail.tsx` (rolling summary block under the header when present; `SessionSummary` under each session segment in `TranscriptThread` or below it), `MeetingList.tsx` and `projects/ProjectMeetings.tsx` (first line of `rolling_summary` under the title), `src/components/projects/SourceList.tsx` (show digest text for images with a "by model" badge when no caption typed; keep caption row).
- Regenerate bindings: `cargo run -- --list-models` (for `summarize_session`, `SessionSummaryPayload`, `meeting_vision_enabled`, `changeMeetingVisionSetting`).

Verify: `bunx tsc --noEmit`, `bun run lint`, prettier. Commit `feat: show session and rolling summaries and add the assistant onboarding step`.

---

### Task 6: Format, clippy, docs (Lane E, last)

`bun run format` (revert unrelated), `cargo clippy --tests` clean in scope, `cargo test --lib`, `tsc`, lint, `check:translations`. `docs/ARCHITECTURE.md`: rows for `meeting/summary.rs`, `onboarding/AssistantOnboarding.tsx`, `meetings/SessionSummary.tsx`; `llm_client.rs` row mentions JSON completion and vision; events add `session-summary-payload`; delivery order marks step 6 done; VISION.md roadmap marks steps 1 to 6 done except step 5 if still pending. Commit `chore: format, clippy, and docs for summaries and onboarding`.

Manual smoke: end a meeting with a few lines; within seconds the detail shows a session summary and a rolling summary; Continue then End again: rolling summary merges. Add a PNG source with a vision-capable provider: digest appears without typing a caption. Fresh app data dir: onboarding shows the Assistant step after the model step; Skip works.
