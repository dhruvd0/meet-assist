# Quick-Start Meeting and AI Filing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The shortcut starts an unfiled meeting; once there is enough transcript the assistant files it into a project (existing or new) and renames it through a tool loop, and the user can change or undo that from a floating card or the meeting header.

**Architecture:** A new `meeting/filing.rs` owns the filing prompt, its own tool set, a per-meeting attempt tracker, and `LlmFiler`, which runs `llm_client::send_with_tools_with_client`. All writes go through conditional store methods so a manual move always wins. `MeetingManager` owns the filer; the observer loop triggers live attempts and `end()` triggers one final attempt on the summary thread. The result is a `MeetingFiledPayload` event plus a "filed" variant of the existing floating card.

**Tech Stack:** Rust (Tauri 2, rusqlite + rusqlite_migration, tauri-specta), React + TypeScript (zustand, react-select wrapper `components/ui/Select`), i18next.

**Spec:** [docs/superpowers/specs/2026-10-15-quick-start-meeting-filing-design.md](../specs/2026-10-15-quick-start-meeting-filing-design.md)

## Global Constraints

- Branch: `feat/quick-start-filing` (already created; the spec is committed on it).
- Toolchain: `export PATH="$HOME/.cargo/bin:$HOME/.bun/bin:$PATH"`. Cargo commands run from `src-tauri/`, bun from the repo root.
- Never hand-edit `src/bindings.ts`. Regenerate it with `cargo run -- --list-models` from `src-tauri/` (a debug build exports bindings before the GUI starts).
- Tests: only the three store tests in Task 1. No UI, end-to-end, or other unit tests (minimal test policy).
- Filing thresholds, copied from the spec: live attempt needs ≥ 20 lines (`FILING_MIN_LINES`), at most 3 live attempts (`FILING_MAX_ATTEMPTS`), ≥ 10 new lines between attempts (`FILING_RETRY_LINES`); final attempt needs ≥ 5 lines (`FILING_FINAL_MIN_LINES`).
- Tool loop: at most 8 tool calls, 30 s budget. Titles 1–80 chars; created-project brief ≤ 2000 chars; `list_projects` brief excerpt 300 chars.
- `filed_by` values: `NULL`, `'ai'`, `'user'`. A meeting is eligible for filing only when `project_id IS NULL AND filed_by IS NULL`.
- Filing runs only under the observer gates: `meeting_assistant_enabled` and `observer::readiness(..) == Readiness::Ready`. No beep for filing.
- All user-facing strings via i18next; new keys under `meetings.filing.*` in `src/i18n/locales/en/translation.json` only.
- Rust: no `unwrap` outside tests; tool handlers return `{"error": ...}` JSON and never panic.
- Commits: conventional prefixes, message says why, ends with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. **User moves the meeting while a filing run is in flight.** Expected: the user's choice stands; the assistant files and renames nothing. Pinned by `manual_move_beats_ai_filing` (Task 1); the handler maps a refused write to `"meeting was filed by the user"` (Task 2).
2. **The model calls `assign_meeting` twice, or `assign_meeting` then `create_project`.** Expected: only the first write applies; no stray empty project. Enforced in `FilingContext` (Task 2) and the rollback in `create_project_and_file_ai` (Task 1 test asserts no leftover project).
3. **The loop times out or errors after the meeting was already filed.** Expected: the user still gets the card and the active cache is updated. `LlmFiler::run` reads the outcome from the context before looking at the loop error (Task 2).
4. **Undo after the user already used the created project** (added a meeting or source, or edited the brief). Expected: the project is kept. Pinned by `undo_filing_deletes_only_an_untouched_created_project` (Task 1).
5. **Provider without tool support.** Expected: no more attempts for that meeting, including the final one; the meeting stays unfiled with the picker. `FilingTracker::mark_unavailable` (Task 2), checked in `live_due` and `final_due`.

---

## File Structure

| File                                               | Change     | Responsibility                                                                                                |
| -------------------------------------------------- | ---------- | ------------------------------------------------------------------------------------------------------------- |
| `src-tauri/src/meeting/store.rs`                   | modify     | `filed_by` migration and column; conditional filing writes; move; undo; line count; 3 tests                   |
| `src-tauri/src/llm_client.rs`                      | modify     | `ToolClientCache` (moved out of `LlmJudge`)                                                                   |
| `src-tauri/src/meeting/observer.rs`                | modify     | use `ToolClientCache`; call `mm.maybe_file_live` before the tick                                              |
| `src-tauri/src/meeting/filing.rs`                  | create     | prompt, tools, `FilingContext`, `FilingTracker`, `LlmFiler`, `MeetingFiledPayload`, `notify`, `final_attempt` |
| `src-tauri/src/meeting/mod.rs`                     | modify     | `pub mod filing;`                                                                                             |
| `src-tauri/src/intervention.rs`                    | modify     | `show_filed`, shared `present` helper                                                                         |
| `src-tauri/src/meeting/session.rs`                 | modify     | own the filer and tracker; `set_active_project`; `maybe_file_live`; final attempt on end                      |
| `src-tauri/src/meeting/toggle.rs`                  | modify     | always start unfiled                                                                                          |
| `src-tauri/src/commands/meeting.rs`                | modify     | `move_meeting`, `undo_filing` commands                                                                        |
| `src-tauri/src/lib.rs`                             | modify     | register commands and `MeetingFiledPayload`                                                                   |
| `src/bindings.ts`                                  | regenerate | —                                                                                                             |
| `src/intervention/InterventionCard.tsx`            | modify     | nudge or filed card                                                                                           |
| `src/stores/meetingStore.ts`                       | modify     | `moveMeeting`; `meetingFiledPayload` listener                                                                 |
| `src/components/meetings/MeetingProjectPicker.tsx` | create     | project picker plus "Filed by assistant" badge                                                                |
| `src/components/meetings/MeetingDetail.tsx`        | modify     | render the picker                                                                                             |
| `src/components/meetings/MeetingList.tsx`          | modify     | project / Unfiled label per row                                                                               |
| `src/i18n/locales/en/translation.json`             | modify     | `meetings.filing.*`                                                                                           |
| `docs/ARCHITECTURE.md`, `AGENTS.md`                | modify     | toggle behaviour, filing module, flow                                                                         |

---

### Task 1: Store support for filing

**Files:**

- Modify: `src-tauri/src/meeting/store.rs` (migrations list at top; `Meeting` struct ~line 604; `MEETING_COLS` ~line 654; `map_meeting`; `create_meeting`; new methods after `delete_meeting`; tests at bottom)

**Interfaces:**

- Consumes: nothing new.
- Produces:
  - `Meeting.filed_by: Option<String>`
  - `pub const FILED_BY_AI: &str = "ai"; pub const FILED_BY_USER: &str = "user";`
  - `ProjectStore::count_meeting_lines(&self, meeting_id: i64) -> Result<i64>`
  - `ProjectStore::file_meeting_ai(&self, meeting_id: i64, project_id: i64) -> Result<bool>`
  - `ProjectStore::create_project_and_file_ai(&self, meeting_id: i64, title: &str, brief: &str) -> Result<Option<(Project, bool)>>` (bool = created)
  - `ProjectStore::rename_meeting_ai(&self, meeting_id: i64, title: &str) -> Result<bool>`
  - `ProjectStore::move_meeting(&self, meeting_id: i64, project_id: Option<i64>) -> Result<()>`
  - `ProjectStore::undo_filing(&self, meeting_id: i64, created_project_id: Option<i64>, created_brief: Option<&str>) -> Result<bool>` (bool = project deleted)

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `store.rs`:

```rust
    #[test]
    fn filed_by_migration_keeps_existing_meetings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("meet.db");
        {
            // A database as it was before the `filed_by` migration.
            let mut conn = Connection::open(&path).unwrap();
            Migrations::new(MIGRATIONS[..MIGRATIONS.len() - 1].to_vec())
                .to_latest(&mut conn)
                .unwrap();
            conn.execute(
                "INSERT INTO projects (title, brief, created_at, updated_at) VALUES ('P', '', 1, 1)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO meetings (project_id, title, created_at, updated_at) VALUES (1, 'M', 1, 1)",
                [],
            )
            .unwrap();
        }
        let s = ProjectStore::open(path).unwrap();
        let meetings = s.list_meetings(None).unwrap();
        assert_eq!(meetings.len(), 1);
        assert_eq!(meetings[0].project_id, Some(1));
        assert_eq!(meetings[0].filed_by, None);
    }

    #[test]
    fn manual_move_beats_ai_filing() {
        let (_d, s) = store();
        let a = s.create_project("A").unwrap();
        let b = s.create_project("B").unwrap();
        let m = s.create_meeting(None, "M").unwrap();
        s.move_meeting(m.id, Some(b.id)).unwrap();
        assert!(!s.file_meeting_ai(m.id, a.id).unwrap());
        assert!(s
            .create_project_and_file_ai(m.id, "New", "brief")
            .unwrap()
            .is_none());
        assert!(!s.rename_meeting_ai(m.id, "AI title").unwrap());
        let got = s.get_meeting(m.id).unwrap().unwrap();
        assert_eq!(got.project_id, Some(b.id));
        assert_eq!(got.filed_by.as_deref(), Some(FILED_BY_USER));
        assert_eq!(got.title, "M");
        assert_eq!(
            s.list_projects().unwrap().len(),
            2,
            "a refused create leaves no project behind"
        );
        // Moving back to Unfiled is a user choice too; the assistant stays out.
        s.move_meeting(m.id, None).unwrap();
        assert!(!s.file_meeting_ai(m.id, a.id).unwrap());
    }

    #[test]
    fn undo_filing_deletes_only_an_untouched_created_project() {
        let (_d, s) = store();
        let m1 = s.create_meeting(None, "M1").unwrap();
        let (p1, created) = s
            .create_project_and_file_ai(m1.id, "Acme", "Pilot")
            .unwrap()
            .unwrap();
        assert!(created);
        assert!(s.undo_filing(m1.id, Some(p1.id), Some("Pilot")).unwrap());
        assert!(s.get_project(p1.id).unwrap().is_none());
        let got = s.get_meeting(m1.id).unwrap().unwrap();
        assert_eq!(got.project_id, None);
        assert_eq!(got.filed_by.as_deref(), Some(FILED_BY_USER));

        // A project that gained another meeting is kept.
        let m2 = s.create_meeting(None, "M2").unwrap();
        let (p2, _) = s
            .create_project_and_file_ai(m2.id, "Beta", "B")
            .unwrap()
            .unwrap();
        s.create_meeting(Some(p2.id), "Other").unwrap();
        assert!(!s.undo_filing(m2.id, Some(p2.id), Some("B")).unwrap());
        assert!(s.get_project(p2.id).unwrap().is_some());

        // An edited brief keeps it too.
        let m3 = s.create_meeting(None, "M3").unwrap();
        let (p3, _) = s
            .create_project_and_file_ai(m3.id, "Gamma", "G")
            .unwrap()
            .unwrap();
        s.update_project_brief(p3.id, "G, edited").unwrap();
        assert!(!s.undo_filing(m3.id, Some(p3.id), Some("G")).unwrap());
        assert!(s.get_project(p3.id).unwrap().is_some());

        // Create reuses a project whose title matches case-insensitively.
        let m4 = s.create_meeting(None, "M4").unwrap();
        let (p4, created4) = s
            .create_project_and_file_ai(m4.id, "gamma", "x")
            .unwrap()
            .unwrap();
        assert_eq!(p4.id, p3.id);
        assert!(!created4);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run (from `src-tauri/`): `cargo test --lib meeting::store 2>&1 | tail -20`
Expected: compile errors, e.g. "no field `filed_by` on type `Meeting`" and "no method named `move_meeting`".

- [ ] **Step 3: Add the migration, column, and mapping**

Append to the end of the `MIGRATIONS` array (after the `interventions` migration):

```rust
    M::up("ALTER TABLE meetings ADD COLUMN filed_by TEXT;"),
```

In `pub struct Meeting`, after `rolling_summary`:

```rust
    /// Who chose the project: `None` (nobody yet), `"ai"`, or `"user"`.
    pub filed_by: Option<String>,
```

Above `const MEETING_COLS`, add:

```rust
pub const FILED_BY_AI: &str = "ai";
pub const FILED_BY_USER: &str = "user";
```

Replace `MEETING_COLS`:

```rust
const MEETING_COLS: &str =
    "id, project_id, title, created_at, updated_at, status, context, rolling_summary, filed_by";
```

In `map_meeting`, after `rolling_summary: row.get("rolling_summary")?,` add `filed_by: row.get("filed_by")?,`. In `create_meeting`'s returned `Meeting { .. }`, after `rolling_summary: None,` add `filed_by: None,`. Run `cargo check 2>&1 | grep -n "missing field" ` and add `filed_by: None` to any other `Meeting { .. }` literal it reports.

- [ ] **Step 4: Add the store methods**

Add after `delete_meeting` in the same `impl ProjectStore` block:

```rust
    /// Transcript lines across all sessions of a meeting.
    pub fn count_meeting_lines(&self, meeting_id: i64) -> Result<i64> {
        let conn = self.conn()?;
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM transcript_lines l JOIN sessions s ON s.id = l.session_id
             WHERE s.meeting_id = ?1",
            params![meeting_id],
            |r| r.get(0),
        )?)
    }

    /// Files an unfiled meeting into `project_id` for the assistant. Returns
    /// `false` when the meeting is no longer eligible (it has a project, or
    /// the user chose one), so a manual move always wins.
    pub fn file_meeting_ai(&self, meeting_id: i64, project_id: i64) -> Result<bool> {
        let conn = self.conn()?;
        Self::file_meeting_ai_on(&conn, meeting_id, project_id)
    }

    fn file_meeting_ai_on(conn: &Connection, meeting_id: i64, project_id: i64) -> Result<bool> {
        let changed = conn.execute(
            "UPDATE meetings SET project_id = ?1, filed_by = 'ai', updated_at = ?2
             WHERE id = ?3 AND project_id IS NULL AND filed_by IS NULL
               AND EXISTS (SELECT 1 FROM projects WHERE id = ?1)",
            params![project_id, now(), meeting_id],
        )?;
        if changed == 1 {
            Self::touch_project(conn, project_id)?;
        }
        Ok(changed == 1)
    }

    /// Files the meeting into the project titled `title` (trimmed,
    /// case-insensitive), creating it with `brief` if none exists. `None`
    /// when the meeting is no longer eligible; a project created here is
    /// rolled back in that case. The bool is whether the project was created.
    pub fn create_project_and_file_ai(
        &self,
        meeting_id: i64,
        title: &str,
        brief: &str,
    ) -> Result<Option<(Project, bool)>> {
        let mut conn = self.conn()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing = tx
            .query_row(
                "SELECT id, title, brief, created_at, updated_at FROM projects
                 WHERE lower(trim(title)) = lower(trim(?1))
                 ORDER BY updated_at DESC, id DESC LIMIT 1",
                params![title],
                Self::map_project,
            )
            .optional()?;
        let (project, created) = match existing {
            Some(p) => (p, false),
            None => {
                let ts = now();
                tx.execute(
                    "INSERT INTO projects (title, brief, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
                    params![title, brief, ts],
                )?;
                let project = Project {
                    id: tx.last_insert_rowid(),
                    title: title.to_string(),
                    brief: brief.to_string(),
                    created_at: ts,
                    updated_at: ts,
                };
                (project, true)
            }
        };
        if !Self::file_meeting_ai_on(&tx, meeting_id, project.id)? {
            // Dropping `tx` rolls back the insert.
            return Ok(None);
        }
        tx.commit()?;
        Ok(Some((project, created)))
    }

    /// Renames the meeting for the assistant unless the user chose its
    /// project. Returns whether the title changed.
    pub fn rename_meeting_ai(&self, meeting_id: i64, title: &str) -> Result<bool> {
        let conn = self.conn()?;
        let changed = conn.execute(
            "UPDATE meetings SET title = ?1, updated_at = ?2
             WHERE id = ?3 AND (filed_by IS NULL OR filed_by = 'ai')",
            params![title, now(), meeting_id],
        )?;
        Ok(changed == 1)
    }

    /// The user's choice of project (`None` = Unfiled). Marks the meeting
    /// `filed_by = 'user'` so the assistant never re-files it.
    pub fn move_meeting(&self, meeting_id: i64, project_id: Option<i64>) -> Result<()> {
        let conn = self.conn()?;
        Self::move_meeting_on(&conn, meeting_id, project_id)
    }

    fn move_meeting_on(conn: &Connection, meeting_id: i64, project_id: Option<i64>) -> Result<()> {
        let changed = conn.execute(
            "UPDATE meetings SET project_id = ?1, filed_by = 'user', updated_at = ?2 WHERE id = ?3",
            params![project_id, now(), meeting_id],
        )?;
        if changed == 0 {
            return Err(anyhow!("meeting {} not found", meeting_id));
        }
        if let Some(pid) = project_id {
            Self::touch_project(conn, pid)?;
        }
        Ok(())
    }

    /// Undo of an assistant filing: the meeting becomes Unfiled (by the
    /// user), and `created_project_id` is deleted only if it is still
    /// untouched: no meetings, no sources, brief equal to `created_brief`.
    /// Returns whether the project was deleted.
    pub fn undo_filing(
        &self,
        meeting_id: i64,
        created_project_id: Option<i64>,
        created_brief: Option<&str>,
    ) -> Result<bool> {
        let mut conn = self.conn()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::move_meeting_on(&tx, meeting_id, None)?;
        let mut deleted = false;
        if let (Some(pid), Some(brief)) = (created_project_id, created_brief) {
            let n = tx.execute(
                "DELETE FROM projects WHERE id = ?1 AND brief = ?2
                   AND NOT EXISTS (SELECT 1 FROM meetings WHERE project_id = ?1)
                   AND NOT EXISTS (SELECT 1 FROM sources WHERE project_id = ?1)",
                params![pid, brief],
            )?;
            deleted = n == 1;
        }
        tx.commit()?;
        Ok(deleted)
    }
```

Note: SQLite `lower()` folds ASCII only; non-ASCII titles match exactly. Acceptable for dedupe.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib meeting::store 2>&1 | tail -15`
Expected: all store tests PASS (the 4 existing plus the 3 new).

- [ ] **Step 6: Clippy and commit**

Run: `cargo clippy --tests 2>&1 | grep -E "^(warning|error)" | head` (expected: no new warnings from `store.rs`), then `cargo fmt -- src/meeting/store.rs`.

```bash
git add src-tauri/src/meeting/store.rs
git commit -m "feat: let the store record who filed a meeting

The assistant may only file meetings nobody has filed; a user move must
always win, and undo must not destroy a project the user has started using.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Filing module, shared tool client, filed card backend

**Files:**

- Modify: `src-tauri/src/llm_client.rs` (add `ToolClientCache` next to `create_tool_client`, ~line 564)
- Modify: `src-tauri/src/meeting/observer.rs` (`LlmJudge` uses the cache)
- Create: `src-tauri/src/meeting/filing.rs`
- Modify: `src-tauri/src/meeting/mod.rs`
- Modify: `src-tauri/src/intervention.rs` (`show` → shared `present`; add `show_filed`)
- Modify: `src-tauri/src/lib.rs` (register `MeetingFiledPayload` in `collect_events!`)

**Interfaces:**

- Consumes (Task 1): `file_meeting_ai`, `create_project_and_file_ai`, `rename_meeting_ai`, `count_meeting_lines`, `Meeting.filed_by`.
- Produces:
  - `llm_client::ToolClientCache` with `fn get(&self, provider: &LlmProvider, api_key: &str) -> Result<reqwest::Client, String>`; `Default`.
  - `filing::MeetingFiledPayload { meeting_id: i64, project_id: Option<i64>, project_title: Option<String>, meeting_title: String, by: String, created_project: bool, created_brief: Option<String> }` (tauri-specta event; TS `events.meetingFiledPayload`).
  - `filing::eligible(m: &Meeting) -> bool`
  - `filing::FilingTracker` (`Default`) with `live_due(&self, meeting_id: i64, lines: i64) -> bool`, `final_due(&self, meeting_id: i64, lines: i64) -> bool`, `record_attempt(&self, meeting_id: i64, lines: i64)`, `mark_unavailable(&self, meeting_id: i64)`, `forget(&self, meeting_id: i64)`.
  - `filing::LlmFiler::new(app: AppHandle) -> Self`; `filing::attempt(filer: &LlmFiler, store: &ProjectStore, tracker: &FilingTracker, meeting_id: i64) -> Option<MeetingFiledPayload>`.
  - `filing::final_attempt(filer: &LlmFiler, store: &ProjectStore, tracker: &FilingTracker, app: &AppHandle, meeting_id: i64)`.
  - `filing::notify(app: &AppHandle, payload: &MeetingFiledPayload)` — emits the event and shows the card.
  - `intervention::show_filed(app: &AppHandle, payload: &MeetingFiledPayload)`; window event name `"intervention:filed"`.

- [ ] **Step 1: Move the tool client cache into `llm_client`**

In `llm_client.rs`, after `create_tool_client`, add:

```rust
/// One reusable tool-loop HTTP client, rebuilt when the provider, base URL,
/// or API key changes.
#[derive(Default)]
pub struct ToolClientCache(std::sync::Mutex<Option<(String, reqwest::Client)>>);

impl ToolClientCache {
    pub fn get(&self, provider: &LlmProvider, api_key: &str) -> Result<reqwest::Client, String> {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        api_key.hash(&mut h);
        let key = format!("{}|{}|{:x}", provider.id, provider.base_url, h.finish());
        let mut slot = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((k, c)) = slot.as_ref() {
            if *k == key {
                return Ok(c.clone());
            }
        }
        let c = create_tool_client(provider, api_key)?;
        *slot = Some((key, c.clone()));
        Ok(c)
    }
}
```

In `observer.rs`, change `LlmJudge`:

```rust
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
```

Delete `LlmJudge::client_for` and, in `judge`, replace `self.client_for(&provider, &api_key)` with `self.clients.get(&provider, &api_key)`. Remove `Mutex` from the `std::sync` import if it is now unused (`cargo check` will say).

- [ ] **Step 2: Add `present` and `show_filed` to `intervention.rs`**

Replace the body of `pub fn show` and add `show_filed` plus a private `present`:

```rust
/// Shows the card for `iv` (from meeting `meeting_id`) without taking focus.
pub fn show(app: &AppHandle, meeting_id: i64, iv: &Intervention) {
    present(app, "intervention:show", card_payload(meeting_id, iv));
}

/// Shows the "filed by assistant" card without taking focus.
pub fn show_filed(app: &AppHandle, payload: &crate::meeting::filing::MeetingFiledPayload) {
    let v = serde_json::to_value(payload).unwrap_or_else(|_| serde_json::json!({}));
    present(app, "intervention:filed", v);
}

/// Positions the card, sends it `event` with `payload`, and shows it.
fn present(app: &AppHandle, event: &'static str, payload: serde_json::Value) {
    let handle = app.clone();
    INTERVENTION_GENERATION.fetch_add(1, Ordering::SeqCst);
    let _ = app.run_on_main_thread(move || {
        let Some(window) = handle.get_webview_window(LABEL) else {
            log::warn!("Intervention window missing; cannot show card");
            return;
        };
        position_top_right(&handle, &window);
        let _ = window.emit(event, &payload);
        #[cfg(target_os = "macos")]
        match handle.get_webview_panel(LABEL) {
            Ok(panel) => panel.show(),
            Err(_) => {
                let _ = window.show();
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = window.show();
    });
}
```

- [ ] **Step 3: Create `src-tauri/src/meeting/filing.rs`**

```rust
//! Assistant filing: files an unfiled meeting into a project (existing or
//! new) and gives it a real title, through a tool loop. The model chooses;
//! Rust enforces the rules (one filing per run, a user move always wins).

use crate::llm_client::{self, ToolCall, ToolClientCache, ToolDef, ToolLoopError};
use crate::meeting::observer::{readiness, Readiness};
use crate::meeting::store::{Meeting, ProjectStore, TranscriptLine, FILED_BY_AI};
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
pub fn eligible(m: &Meeting) -> bool {
    m.project_id.is_none() && m.filed_by.is_none()
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
            "outcome": { "type": "string", "enum": ["filed", "wait"] },
            "reason": { "type": "string" }
        },
        "required": ["outcome", "reason"],
        "additionalProperties": false
    })
}

const SYSTEM_PROMPT: &str = "You file a meeting recording into the user's projects, using the tools.
1. Call list_projects. Call read_project on any project that might match.
2. If one project clearly fits, call assign_meeting with its id. Prefer an existing project.
3. If none fits and the transcript makes the meeting's subject clear, call create_project with a short title and a one-paragraph brief describing the subject, people, and goals heard so far.
4. Call rename_meeting with a short descriptive title (under 60 characters) in the transcript's language.
5. If the transcript does not yet show what the meeting is about, do not file or rename; answer outcome \"wait\".
Finish with JSON {\"outcome\": \"filed\" | \"wait\", \"reason\": \"...\"}.";

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
        "## Meeting title\n{title}\n\n## Meeting notes\n{notes}\n\n## Transcript so far\n{}",
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
    state: Mutex<RunState>,
}

impl FilingContext {
    pub fn new(store: ProjectStore, meeting_id: i64) -> Self {
        Self {
            store,
            meeting_id,
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
                if !self.store.file_meeting_ai(self.meeting_id, pid).map_err(e)? {
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
                if !self.store.rename_meeting_ai(self.meeting_id, &title).map_err(e)? {
                    return Err("meeting was filed by the user".into());
                }
                st.renamed = true;
                Ok(json!({ "ok": true }))
            }
            other => Err(format!("unknown tool: {other}")),
        }
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
        let lines = store
            .lines_in_range(meeting_id, None, None, 10_000)
            .map_err(|e| FilingError::Other(e.to_string()))?;
        let ctx = FilingContext::new(store.clone_handle(), meeting_id);
        let handler = |c: &ToolCall| ctx.handle(c);
        let defs = tool_defs();
        let fut = llm_client::send_with_tools_with_client(
            &client,
            &provider,
            &model,
            SYSTEM_PROMPT.to_string(),
            user_prompt(&meeting.title, &meeting.context, &lines),
            &defs,
            final_schema(),
            MAX_TOOL_CALLS,
            &handler,
        );
        let result =
            tauri::async_runtime::block_on(async move { tokio::time::timeout(BUDGET, fut).await });
        if let Some(filed) = ctx.outcome() {
            return Ok(Some(filed));
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
            notify(app, &p);
        }
    }
    tracker.forget(meeting_id);
}

/// Tells the main window and shows the floating card. No beep: filing is
/// not a nudge.
pub fn notify(app: &AppHandle, payload: &MeetingFiledPayload) {
    if let Err(e) = payload.clone().emit(app) {
        warn!("emit meeting filed: {e}");
    }
    crate::intervention::show_filed(app, payload);
}
```

In `src-tauri/src/meeting/mod.rs`, add `pub mod filing;` (alphabetical, after `pub mod chunk;`).

In `lib.rs`, inside `collect_events![ ... ]`, add `meeting::filing::MeetingFiledPayload,` after `meeting::observer::ObserverStatusPayload,`.

If `readiness` or `Readiness` are not `pub` in `observer.rs`, they are (`pub fn readiness`, `pub enum Readiness`); no change needed.

- [ ] **Step 4: Check it compiles**

Run (from `src-tauri/`): `cargo check 2>&1 | tail -5`
Expected: compiles. Warnings about unused `LlmFiler`, `attempt`, `final_attempt`, `notify`, `FilingTracker` are expected until Task 3; nothing else.

- [ ] **Step 5: Commit**

```bash
cargo fmt -- src/meeting/filing.rs src/meeting/observer.rs src/llm_client.rs src/intervention.rs src/lib.rs src/meeting/mod.rs
git add src-tauri/src/meeting/filing.rs src-tauri/src/meeting/mod.rs src-tauri/src/meeting/observer.rs src-tauri/src/llm_client.rs src-tauri/src/intervention.rs src-tauri/src/lib.rs
git commit -m "feat: add the assistant filing loop and its tools

Filing needs write tools the observer must never see, so it gets its own
tool set and context; the store's conditional writes keep the user in charge.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Wire filing into meetings, the observer, and the shortcut

**Files:**

- Modify: `src-tauri/src/meeting/session.rs` (`MeetingManager` fields and `new`; `spawn_summary`/`spawn_summaries`; `end`; new methods)
- Modify: `src-tauri/src/meeting/observer.rs` (`spawn_observer` loop)
- Modify: `src-tauri/src/meeting/toggle.rs`

**Interfaces:**

- Consumes (Task 2): `filing::{LlmFiler, FilingTracker, eligible, attempt, final_attempt, notify}`; (Task 1): `count_meeting_lines`.
- Produces:
  - `MeetingManager::set_active_project(&self, meeting_id: i64, project_id: Option<i64>)`
  - `MeetingManager::maybe_file_live(&self, meeting_id: i64)`

- [ ] **Step 1: Give `MeetingManager` the filer and tracker**

In `pub struct MeetingManager`, after `summary_lock`:

```rust
    /// Files unfiled meetings into projects; `None` without an app handle.
    filer: Option<Arc<crate::meeting::filing::LlmFiler>>,
    filing: Arc<crate::meeting::filing::FilingTracker>,
```

In `MeetingManager::new`, after `summary_lock: Arc::new(Mutex::new(())),`:

```rust
            filer: Some(Arc::new(crate::meeting::filing::LlmFiler::new(app.clone()))),
            filing: Arc::new(crate::meeting::filing::FilingTracker::default()),
```

- [ ] **Step 2: Add `set_active_project` and `maybe_file_live`**

Add after `active_project_id`:

```rust
    /// Points the running meeting's cached project at `project_id` after it
    /// was filed or moved. No-op for any other meeting.
    pub fn set_active_project(&self, meeting_id: i64, project_id: Option<i64>) {
        if let Some(a) = self.lock_active().as_mut() {
            if a.meeting_id == meeting_id {
                a.project_id = project_id;
            }
        }
    }

    /// Live filing attempt for the running meeting when one is due. The
    /// observer calls this before its tick, under the same gates.
    pub fn maybe_file_live(&self, meeting_id: i64) {
        use crate::meeting::filing;
        let Some(filer) = &self.filer else {
            return;
        };
        let Ok(Some(meeting)) = self.store.get_meeting(meeting_id) else {
            return;
        };
        if !filing::eligible(&meeting) {
            return;
        }
        let lines = match self.store.count_meeting_lines(meeting_id) {
            Ok(n) => n,
            Err(e) => {
                warn!("filing: count lines failed: {e:#}");
                return;
            }
        };
        if !self.filing.live_due(meeting_id, lines) {
            return;
        }
        self.filing.record_attempt(meeting_id, lines);
        if let Some(filed) = filing::attempt(filer, &self.store, &self.filing, meeting_id) {
            self.set_active_project(meeting_id, filed.project_id);
            if let Some(app) = &self.app {
                filing::notify(app, &filed);
            }
        }
    }
```

- [ ] **Step 3: Final attempt when a meeting ends**

Change `spawn_summaries` to take the meeting to file, and update its callers:

```rust
    pub fn spawn_summary(&self, session_id: i64) {
        self.spawn_summaries(vec![session_id], None, None);
    }

    /// Summarize sessions one after another (the rolling summary is a chain,
    /// so they must not run concurrently) on one background thread.
    /// `pending` is a transcriber still finishing queued utterances; the
    /// thread joins it first so the summary sees every line. `file_meeting`
    /// gets one last filing attempt before the summaries, so they see its
    /// project.
    fn spawn_summaries(
        &self,
        session_ids: Vec<i64>,
        pending: Option<JoinHandle<()>>,
        file_meeting: Option<i64>,
    ) {
```

Inside, next to `let summary_lock = Arc::clone(&self.summary_lock);` add:

```rust
        let filer = self.filer.clone();
        let filing = Arc::clone(&self.filing);
```

In the thread closure, right after the `if let Some(worker) = pending { ... }` block and before `for session_id in session_ids {`, add:

```rust
                if let (Some(filer), Some(app), Some(meeting_id)) =
                    (filer.as_ref(), app.as_ref(), file_meeting)
                {
                    crate::meeting::filing::final_attempt(
                        filer, &store, &filing, app, meeting_id,
                    );
                }
```

In `end()`, change `self.spawn_summaries(vec![session_id], leftover);` to `self.spawn_summaries(vec![session_id], leftover, Some(meeting_id));`. Run `cargo check` and update any other `spawn_summaries(` call it reports by adding `, None` as the third argument.

The existing `cloud_allowed` early return in `spawn_summaries` already skips the final attempt when the assistant is off, which is the intended gate.

- [ ] **Step 4: Trigger live filing from the observer**

In `observer.rs` `spawn_observer`, just before `let now = chrono::Utc::now().timestamp_millis();`:

```rust
                if let Some(mid) = st.meeting_id {
                    // Files the meeting first, so this tick's judge already
                    // sees the project's brief and sources.
                    mm.maybe_file_live(mid);
                }
```

- [ ] **Step 5: Start every shortcut meeting unfiled**

Replace `src-tauri/src/meeting/toggle.rs` from the `/// Ends the running meeting` doc comment to the end of the file with:

```rust
/// Ends the running meeting (live or paused), otherwise starts a new,
/// unfiled one. The assistant files it into a project once it has enough
/// transcript (see `meeting::filing`).
pub fn toggle_meeting(app: &AppHandle) {
    let Some(mm) = app.try_state::<Arc<MeetingManager>>() else {
        error!("toggle_meeting: MeetingManager not initialised");
        return;
    };
    let mm = mm.inner().clone();
    let app = app.clone();
    // Start/end do blocking capture and store work; keep it off the shortcut
    // and main threads.
    std::thread::spawn(move || {
        if mm.state().meeting_id.is_some() {
            if let Err(e) = mm.end() {
                error!("toggle_meeting: end failed: {e:#}");
            }
            return;
        }
        match mm.start(None, None) {
            Ok(m) => info!("toggle_meeting: started meeting {}", m.id),
            Err(e) => {
                // A double press can race another start; the meeting the
                // user wanted is running, so there is nothing to report.
                if mm.state().meeting_id.is_some() {
                    info!("toggle_meeting: start skipped, a meeting is already running");
                    return;
                }
                error!("toggle_meeting: start failed: {e:#}");
                crate::show_main_window(&app);
                let _ = app.emit("meeting-toggle-error", format!("{e:#}"));
            }
        }
    });
}
```

`last_used_project` is deleted. The `use tauri::{AppHandle, Emitter, Manager};` import stays.

- [ ] **Step 6: Check, lint, test, commit**

Run (from `src-tauri/`): `cargo check 2>&1 | tail -3` (no warnings from `filing.rs` now), `cargo clippy --tests 2>&1 | grep -E "^(warning|error)" | head`, `cargo test --lib meeting::store 2>&1 | tail -3`.
Expected: clean clippy; store tests PASS.

```bash
cargo fmt -- src/meeting/session.rs src/meeting/observer.rs src/meeting/toggle.rs
git add src-tauri/src/meeting/session.rs src-tauri/src/meeting/observer.rs src-tauri/src/meeting/toggle.rs
git commit -m "feat: start shortcut meetings unfiled and let the assistant file them

The shortcut should record at once; filing live gives the observer project
context mid-meeting, and a last attempt at the end catches short meetings.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Move and undo commands, bindings

**Files:**

- Modify: `src-tauri/src/commands/meeting.rs`
- Modify: `src-tauri/src/lib.rs` (`collect_commands!`)
- Regenerate: `src/bindings.ts`

**Interfaces:**

- Consumes: `ProjectStore::{move_meeting, undo_filing}` (Task 1); `MeetingManager::set_active_project` (Task 3); `filing::MeetingFiledPayload` (Task 2).
- Produces (TS): `commands.moveMeeting(meetingId: number, projectId: number | null)`, `commands.undoFiling(meetingId: number, createdProjectId: number | null, createdBrief: string | null)`, `events.meetingFiledPayload`, `Meeting.filed_by: string | null`. Card window invokes `"undo_filing"` with `{ meetingId, createdProjectId, createdBrief }`.

- [ ] **Step 1: Add the commands**

In `commands/meeting.rs`, add `use crate::meeting::filing::MeetingFiledPayload;` to the imports, then after `delete_meeting`:

```rust
/// The user's choice of project for a meeting (`None` = Unfiled).
#[tauri::command]
#[specta::specta]
pub fn move_meeting(
    app: tauri::AppHandle,
    mm: Mm<'_>,
    meeting_id: i64,
    project_id: Option<i64>,
) -> Result<(), String> {
    mm.store().move_meeting(meeting_id, project_id).map_err(err)?;
    mm.set_active_project(meeting_id, project_id);
    emit_user_filed(&app, &mm, meeting_id, project_id);
    Ok(())
}

/// Undo from the filed card: back to Unfiled, and delete the project the
/// assistant created if nobody has used it yet.
#[tauri::command]
#[specta::specta]
pub fn undo_filing(
    app: tauri::AppHandle,
    mm: Mm<'_>,
    meeting_id: i64,
    created_project_id: Option<i64>,
    created_brief: Option<String>,
) -> Result<(), String> {
    mm.store()
        .undo_filing(meeting_id, created_project_id, created_brief.as_deref())
        .map_err(err)?;
    mm.set_active_project(meeting_id, None);
    emit_user_filed(&app, &mm, meeting_id, None);
    crate::intervention::hide(&app);
    Ok(())
}

fn emit_user_filed(
    app: &tauri::AppHandle,
    mm: &MeetingManager,
    meeting_id: i64,
    project_id: Option<i64>,
) {
    let project_title = project_id
        .and_then(|p| mm.store().get_project(p).ok().flatten())
        .map(|p| p.title);
    let meeting_title = mm
        .store()
        .get_meeting(meeting_id)
        .ok()
        .flatten()
        .map(|m| m.title)
        .unwrap_or_default();
    let payload = MeetingFiledPayload {
        meeting_id,
        project_id,
        project_title,
        meeting_title,
        by: crate::meeting::store::FILED_BY_USER.into(),
        created_project: false,
        created_brief: None,
    };
    if let Err(e) = payload.emit(app) {
        log::warn!("emit meeting filed: {e}");
    }
}
```

In `lib.rs` `collect_commands![...]`, after `commands::meeting::delete_meeting,` add:

```rust
            commands::meeting::move_meeting,
            commands::meeting::undo_filing,
```

- [ ] **Step 2: Regenerate bindings and verify**

Run (from `src-tauri/`): `cargo run -- --list-models > /tmp/lm.log 2>&1; tail -2 /tmp/lm.log` (exit 0).
Then from the repo root: `grep -c "moveMeeting\|undoFiling\|meetingFiledPayload\|filed_by" src/bindings.ts`
Expected: at least 4.

- [ ] **Step 3: Clippy and commit**

Run: `cargo clippy --tests 2>&1 | grep -E "^(warning|error)" | head` (clean).

```bash
cargo fmt -- src/commands/meeting.rs src/lib.rs
git add src-tauri/src/commands/meeting.rs src-tauri/src/lib.rs src/bindings.ts
git commit -m "feat: let the user move a meeting or undo the assistant's filing

Auto-filing is only acceptable if the user can change it in one click.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Filed card

**Files:**

- Modify: `src/intervention/InterventionCard.tsx` (full replacement below)
- Modify: `src/i18n/locales/en/translation.json` (`meetings.filing`)

**Interfaces:**

- Consumes: window events `"intervention:show"` (nudge, unchanged) and `"intervention:filed"` (payload = `MeetingFiledPayload` JSON, snake_case); commands `focus_meeting { meetingId }`, `undo_filing { meetingId, createdProjectId, createdBrief }`, `hide_intervention_window`, `dismiss_intervention { id }`.
- Produces: i18n keys `meetings.filing.{project,unfiled,byAssistant,pending,pickProject}` and `meetings.filing.card.{title,filedInto,created,change,undo}` (Task 6 uses the first five).

- [ ] **Step 1: Add the translation keys**

In `src/i18n/locales/en/translation.json`, inside the `"meetings"` object, after the `"intervention": { ... }` block, add:

```json
    "filing": {
      "project": "Project",
      "unfiled": "Unfiled",
      "byAssistant": "Filed by assistant",
      "pending": "Assistant will file this soon",
      "pickProject": "Unfiled · pick a project",
      "card": {
        "title": "Filed by assistant",
        "filedInto": "Filed into {{project}} as “{{title}}”",
        "created": "Created {{project}} and filed “{{title}}”",
        "change": "Change",
        "undo": "Undo"
      }
    },
```

- [ ] **Step 2: Replace `src/intervention/InterventionCard.tsx`**

```tsx
import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import {
  FolderInput,
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
  /** Added by the backend so the card can open its meeting. */
  meeting_id: number;
};

/** `MeetingFiledPayload` from the backend. */
type Filed = {
  meeting_id: number;
  project_id: number | null;
  project_title: string | null;
  meeting_title: string;
  by: string;
  created_project: boolean;
  created_brief: string | null;
};

type Card =
  | { type: "nudge"; iv: Intervention }
  | { type: "filed"; filed: Filed };

const ICON: Record<string, React.ComponentType<{ className?: string }>> = {
  loop: Repeat,
  stuck: MessageCircleQuestion,
  info: Info,
  idea: Lightbulb,
};

const AUTO_HIDE_MS = 30_000;

export const InterventionCard: React.FC = () => {
  const { t } = useTranslation();
  const [card, setCard] = useState<Card | null>(null);
  const hovered = useRef(false);
  const timer = useRef<number | null>(null);

  const hide = async () => {
    if (timer.current) window.clearTimeout(timer.current);
    hovered.current = false;
    try {
      await invoke("hide_intervention_window");
    } catch (err) {
      console.error("hide_intervention_window failed", err);
    }
  };

  const arm = () => {
    if (timer.current) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      if (hovered.current) arm();
      else hide();
    }, AUTO_HIDE_MS);
  };

  useEffect(() => {
    const show = (next: Card) => {
      hovered.current = false;
      setCard(next);
      arm();
    };
    const unNudge = listen<Intervention>("intervention:show", (e) =>
      show({ type: "nudge", iv: e.payload }),
    );
    const unFiled = listen<Filed>("intervention:filed", (e) =>
      show({ type: "filed", filed: e.payload }),
    );
    unNudge.catch(console.error);
    unFiled.catch(console.error);
    return () => {
      unNudge.then((f) => f()).catch(console.error);
      unFiled.then((f) => f()).catch(console.error);
      if (timer.current) window.clearTimeout(timer.current);
    };
  }, []);

  if (!card) return null;

  const meetingId =
    card.type === "nudge" ? card.iv.meeting_id : card.filed.meeting_id;
  const Icon =
    card.type === "nudge" ? (ICON[card.iv.kind] ?? Info) : FolderInput;
  const label =
    card.type === "nudge"
      ? t(`meetings.intervention.kind.${card.iv.kind}`)
      : t("meetings.filing.card.title");
  const body =
    card.type === "nudge"
      ? card.iv.text
      : t(
          card.filed.created_project
            ? "meetings.filing.card.created"
            : "meetings.filing.card.filedInto",
          {
            project: card.filed.project_title ?? "",
            title: card.filed.meeting_title,
          },
        );

  const onDismiss = async () => {
    if (card.type === "nudge") {
      try {
        await invoke("dismiss_intervention", { id: card.iv.id });
      } catch (err) {
        console.error("dismiss_intervention failed", err);
      }
    }
    await hide();
  };

  const openMeeting = async () => {
    try {
      await invoke("focus_meeting", { meetingId });
    } catch (err) {
      console.error("focus_meeting failed", err);
      await hide();
    }
  };

  const undo = async (filed: Filed) => {
    try {
      await invoke("undo_filing", {
        meetingId: filed.meeting_id,
        createdProjectId: filed.created_project ? filed.project_id : null,
        createdBrief: filed.created_brief,
      });
    } catch (err) {
      console.error("undo_filing failed", err);
      await hide();
    }
  };

  const action =
    "text-xs font-medium text-logo-primary hover:underline cursor-pointer";

  return (
    <div
      className="h-screen w-screen p-3 bg-background text-text rounded-xl border border-mid-gray/30 shadow-lg flex flex-col gap-2"
      onMouseEnter={() => (hovered.current = true)}
      onMouseLeave={() => (hovered.current = false)}
    >
      <div className="flex items-center gap-2">
        <Icon className="w-4 h-4 text-logo-primary shrink-0" />
        <span className="text-xs font-medium uppercase tracking-wide text-mid-gray flex-1">
          {label}
        </span>
        <button
          type="button"
          aria-label={t("meetings.intervention.dismiss")}
          className="text-mid-gray hover:text-text cursor-pointer"
          title={t("meetings.intervention.dismiss")}
          onClick={onDismiss}
        >
          <X className="w-4 h-4" aria-hidden="true" />
        </button>
      </div>
      <p className="text-sm leading-snug flex-1 overflow-hidden">{body}</p>
      <div className="flex justify-end gap-3">
        {card.type === "filed" ? (
          <>
            <button
              type="button"
              className={action}
              onClick={() => undo(card.filed)}
            >
              {t("meetings.filing.card.undo")}
            </button>
            <button type="button" className={action} onClick={openMeeting}>
              {t("meetings.filing.card.change")}
            </button>
          </>
        ) : (
          <button type="button" className={action} onClick={openMeeting}>
            {t("meetings.intervention.open")}
          </button>
        )}
      </div>
    </div>
  );
};
```

- [ ] **Step 3: Typecheck, lint, commit**

Run (repo root): `bunx tsc --noEmit 2>&1 | tail -5` and `bun run lint 2>&1 | tail -5`.
Expected: no errors.

```bash
bunx prettier --write src/intervention/InterventionCard.tsx src/i18n/locales/en/translation.json
git add src/intervention/InterventionCard.tsx src/i18n/locales/en/translation.json
git commit -m "feat: show the assistant's filing on the floating card with Undo

The user needs to see where the meeting went without opening the app, and
to reverse it in one click.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Project picker and Unfiled labels in the main window

**Files:**

- Modify: `src/stores/meetingStore.ts`
- Create: `src/components/meetings/MeetingProjectPicker.tsx`
- Modify: `src/components/meetings/MeetingDetail.tsx`
- Modify: `src/components/meetings/MeetingList.tsx`

**Interfaces:**

- Consumes: `commands.moveMeeting`, `events.meetingFiledPayload`, `Meeting.filed_by` (Task 4); i18n keys from Task 5; `useProjectStore` (`projects`, `loadProjects`); `Select` (named export, `value: string | null`, `options: SelectOption[]`, `onChange(value: string | null)`); `Badge` (default export, variants `primary|success|secondary`).
- Produces: `useMeetingStore().moveMeeting(id: number, projectId: number | null): Promise<void>`.

- [ ] **Step 1: Store action and listener**

In `src/stores/meetingStore.ts`:

1. Add `import { useProjectStore } from "./projectStore";` after the existing imports.
2. In the store's type, after `renameMeeting: ...`, add:

```ts
moveMeeting: (id: number, projectId: number | null) => Promise<void>;
```

3. In the implementation, after the `renameMeeting` action, add:

```ts
  moveMeeting: async (id, projectId) => {
    const res = await commands.moveMeeting(id, projectId);
    if (res.status !== "ok")
      toast.error(`${t("meetings.errors.save")} ${res.error}`);
    // State updates arrive through the meetingFiledPayload event.
  },
```

4. In `subscribe`, after the `un7` listener, add:

```ts
const un8 = await events.meetingFiledPayload.listen((e) => {
  const { meeting_id, project_id, meeting_title, by } = e.payload;
  const patch = { project_id, title: meeting_title, filed_by: by };
  set((s) => ({
    meetings: s.meetings.map((m) =>
      m.id === meeting_id ? { ...m, ...patch } : m,
    ),
    detail:
      s.detail && s.detail.meeting.id === meeting_id
        ? { ...s.detail, meeting: { ...s.detail.meeting, ...patch } }
        : s.detail,
  }));
  // A project may have been created or deleted.
  useProjectStore.getState().loadProjects();
});
```

and add `un8();` to the returned cleanup function after `un7();`.

- [ ] **Step 2: Create `src/components/meetings/MeetingProjectPicker.tsx`**

```tsx
import React, { useEffect } from "react";
import { useTranslation } from "react-i18next";
import type { Meeting } from "@/bindings";
import { useMeetingStore } from "@/stores/meetingStore";
import { useProjectStore } from "@/stores/projectStore";
import Badge from "../ui/Badge";
import { Select } from "../ui/Select";

const UNFILED = "unfiled";

/** The meeting's project, changeable by the user; marks assistant filings. */
export const MeetingProjectPicker: React.FC<{ meeting: Meeting }> = ({
  meeting,
}) => {
  const { t } = useTranslation();
  const projects = useProjectStore((s) => s.projects);
  const loadProjects = useProjectStore((s) => s.loadProjects);
  const moveMeeting = useMeetingStore((s) => s.moveMeeting);

  useEffect(() => {
    loadProjects();
  }, [loadProjects]);

  const options = [
    { value: UNFILED, label: t("meetings.filing.unfiled") },
    ...projects.map((p) => ({ value: String(p.id), label: p.title })),
  ];

  return (
    <div className="flex items-center gap-2">
      <span className="text-xs font-medium text-mid-gray uppercase tracking-wide">
        {t("meetings.filing.project")}
      </span>
      <Select
        className="w-64"
        value={
          meeting.project_id === null ? UNFILED : String(meeting.project_id)
        }
        options={options}
        onChange={(value) => {
          const next =
            value === null || value === UNFILED ? null : Number(value);
          if (next !== meeting.project_id) moveMeeting(meeting.id, next);
        }}
      />
      {meeting.filed_by === "ai" && (
        <Badge variant="secondary">{t("meetings.filing.byAssistant")}</Badge>
      )}
    </div>
  );
};
```

- [ ] **Step 3: Render the picker in `MeetingDetail.tsx`**

Add `import { MeetingProjectPicker } from "./MeetingProjectPicker";` with the other local imports. Directly after the closing `</div>` of the header row (the `flex items-center gap-2` div that ends with `<MeetingControls meetingId={m.id} />`), add:

```tsx
<MeetingProjectPicker meeting={m} />
```

- [ ] **Step 4: Project or Unfiled label in `MeetingList.tsx`**

Add imports: `import { useEffect } from "react";` (merge into the existing `import React from "react";` as `import React, { useEffect } from "react";`) and `import { useProjectStore } from "@/stores/projectStore";`.

In the component body, after `const deleteMeeting = ...`, add:

```tsx
const observer = useMeetingStore((s) => s.observer);
const projects = useProjectStore((s) => s.projects);
const loadProjects = useProjectStore((s) => s.loadProjects);

useEffect(() => {
  loadProjects();
}, [loadProjects]);

const projectLabel = (m: (typeof meetings)[number]) => {
  if (m.project_id !== null)
    return projects.find((p) => p.id === m.project_id)?.title ?? "";
  if (live.meeting_id === m.id && m.filed_by === null)
    return observer.readiness === "ready"
      ? t("meetings.filing.pending")
      : t("meetings.filing.pickProject");
  return t("meetings.filing.unfiled");
};
```

In each row, directly after the title `<div className="text-sm font-semibold text-text">{m.title}</div>`, add:

```tsx
<div className="text-xs text-mid-gray">{projectLabel(m)}</div>
```

- [ ] **Step 5: Typecheck, lint, format, commit**

Run (repo root): `bunx tsc --noEmit 2>&1 | tail -5`, `bun run lint 2>&1 | tail -5`, `bun run format:check 2>&1 | tail -5`.
Expected: no errors (run `bun run format:frontend` if formatting fails, then recheck).

```bash
git add src/stores/meetingStore.ts src/components/meetings/MeetingProjectPicker.tsx src/components/meetings/MeetingDetail.tsx src/components/meetings/MeetingList.tsx
git commit -m "feat: show each meeting's project and let the user change it

Unfiled meetings were invisible as such, and a wrong filing had no fix in
the main window.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: Docs

**Files:**

- Modify: `docs/ARCHITECTURE.md` (toggle paragraph ~line 27; meeting module table ~line 59)
- Modify: `AGENTS.md` (Meeting Subsystem module list; Application Flow step 3)

- [ ] **Step 1: ARCHITECTURE.md**

Replace the sentence "It ends a running (live or paused) meeting; otherwise it starts one in the most recently used project; with no project it opens the window on Projects." with:

"It ends a running (live or paused) meeting; otherwise it starts a new, unfiled one. Once the meeting has 20 transcript lines, the observer loop runs `meeting::filing`: a tool loop (`list_projects`, `read_project`, `assign_meeting`, `create_project`, `rename_meeting`) that files the meeting into a project and renames it. It retries up to 3 times as the transcript grows and makes one last attempt when the meeting ends. Writes are conditional on `meetings.filed_by IS NULL`, so a user move (`move_meeting`, `filed_by = 'user'`) always wins. A filing shows on the floating card with Change and Undo."

In the meeting module table, add a row after `meeting/toggle.rs` (keep the table's column padding style):

```markdown
| `meeting/filing.rs` | Assistant filing of unfiled meetings: prompt, filing tools, attempt tracker, `LlmFiler`, `MeetingFiledPayload` |
```

Update the `meeting/toggle.rs` row text to: "`toggle_meeting`: the single entry point for shortcut, tray, CLI, and SIGUSR2; starts an unfiled meeting".

- [ ] **Step 2: AGENTS.md**

In "Meeting Subsystem", after "`toggle.rs` (`toggle_meeting`, the one entry point for shortcut, tray, CLI, and SIGUSR2)," insert "`filing.rs` (assistant files unfiled meetings into projects),". In "Application Flow", replace step 3 with:

"3. **Meeting:** The shortcut toggles a meeting (starts an unfiled one, or ends the running one); the assistant files it into a project once it knows what it is about"

- [ ] **Step 3: Commit**

```bash
git add docs/ARCHITECTURE.md AGENTS.md
git commit -m "docs: describe unfiled shortcut meetings and assistant filing

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Final verification (after all tasks)

From `src-tauri/`: `cargo clippy --tests 2>&1 | grep -E "^(warning|error)" | head` (clean), `cargo test --lib 2>&1 | tail -3` (all pass). From the repo root: `bun run build 2>&1 | tail -3`, `bun run lint`, `bun run format:check`.

Manual checks for the human (from the spec):

1. Shortcut with zero projects starts a meeting; after about 20 lines it is filed into a new project; the card shows "Created …".
2. With existing projects, a meeting about one of them is filed into it and renamed.
3. Undo on the card returns the meeting to Unfiled, deletes the just-created empty project, and the assistant does not file it again.
4. Moving the meeting with the picker during a live meeting stops filing; the observer uses the new project's brief.
5. Assistant off: the meeting stays Unfiled and shows "Unfiled · pick a project".
6. A short meeting (under 5 lines) ended early stays Unfiled.
