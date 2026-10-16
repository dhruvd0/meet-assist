# Meetings and Live Transcript Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A user starts a meeting inside a project (or a quick meeting without one), speaks, and sees a live transcript of their own microphone appear line by line. They can pause, resume, end, and later continue the meeting as a new session. No system audio, no LLM, no interventions.

**Architecture:** Three new tables in `meet.db` (`meetings`, `sessions`, `transcript_lines`). A pure `Segmenter` turns the existing VAD's speech/noise stream into utterances. `MeetingCapture` opens its own `handy_recorder::Recorder<MeetingSink>` on the configured microphone and pushes utterances on a channel. `MeetingManager` (Tauri state) owns the `Idle → Live ⇄ Paused → Ended` state machine, a transcriber thread that calls the existing `TranscriptionManager::transcribe`, and emits typed events. Commands in `commands/meeting.rs`. Frontend: `meetingStore`, a Meetings section (second sidebar entry), meeting detail with transcript thread, and a meetings list inside project detail.

**Tech Stack:** Rust (rusqlite, handy-recorder 0.1, existing Silero/Earshot VAD via `CaptureVad`, tauri-specta), React 18, Zustand, i18next, Tailwind.

**Spec:** `docs/superpowers/specs/2026-10-13-meeting-assistant-design.md` sections 2 (entities, flows, screens), 3 (`mod.rs`, `capture.rs`, `transcriber.rs`, store, `commands/meeting.rs`, failure handling), 4 (frontend), 6 (testing). Observer, interventions, summaries, system audio are later plans.

## Global Constraints

- Toolchain, before every cargo or bun command:
  ```bash
  export PATH="$HOME/.cargo/bin:$HOME/.bun/bin:$PATH"
  export CMAKE_POLICY_VERSION_MINIMUM=3.5
  export SDKROOT=/Users/dhruv/Desktop/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX26.5.sdk
  ```
  cargo from `src-tauri/`, bun from the repo root. Disk is tight (about 3 GB free): no `cargo build`, no `bun run build` except where a task says so; `cargo test --lib meeting::`, `cargo check`, `cargo clippy --tests`, `cargo fmt` are fine. If a command fails with "No space left on device", stop and report BLOCKED.
- Rust tests: `cargo test --lib meeting::` from `src-tauri/`; unit tests use `tempfile::tempdir()`. No `unwrap` in non-test production paths.
- `src/bindings.ts` is regenerated only by `cargo run -- --list-models` from `src-tauri/` (Task 6). Never hand-edit it.
- Frontend checks: `bunx tsc --noEmit`, `bun run lint`, `bun run check:translations` all exit 0. New strings under `meetings.*` and `sidebar.meetings` in `src/i18n/locales/en/translation.json`, copied verbatim into the 25 other locales (CI gate).
- Reuse `components/ui` primitives; no new primitives; keep the existing design language.
- Speaker is always `me` in this plan (microphone only). Status values exactly `live`, `paused`, `ended`.
- Timestamps are Unix milliseconds (`chrono::Utc::now().timestamp_millis()`), matching `projects`.
- Audio is 16 kHz mono f32 everywhere. Utterance segmentation defaults: end after 700 ms of non-speech, minimum utterance 300 ms, hard cut at 30 s.
- Meeting capture refuses to start while dictation is recording (`AudioRecordingManager::is_recording()`); dictation is not blocked during a meeting (two recorders on one device are allowed by handy-recorder).
- Transcribing an utterance that returns empty or whitespace text stores no line.
- Commit after every task with a conventional prefix and the trailer `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.

## Review Focus

1. A user who quits the app mid-meeting expects to find the meeting resumable, not stuck "live". Pinned in Task 1 (`mark_live_sessions_paused` test) and Task 6 (called at startup).
2. A user who presses Start while a dictation hotkey is held expects a clear error, not two recordings fighting. Pinned in Task 4 (manager refuses when `is_recording` closure returns true).
3. A user whose model is unloaded (idle timeout) expects the next utterance to still transcribe after a short delay, not an error line. Pinned in Task 4 (transcriber calls `initiate_model_load` before each transcribe; test with a fake that fails once then succeeds).
4. A user who pauses expects no new lines to appear until resume, and the same session to continue. Pinned in Task 4 (pause stops capture; `resume` keeps `session_id`).
5. A user who deletes a project expects its meetings and transcript to go too. Pinned in Task 1 (cascade test through `delete_project`).

---

### Task 1: Meeting tables in ProjectStore

**Files:**

- Modify: `src-tauri/src/meeting/store.rs`

**Interfaces:**

- Produces (all on `impl ProjectStore`):
  - `pub enum MeetingStatus { Live, Paused, Ended }` serde lowercase, `as_str`/`parse`, derives `Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type`
  - `pub struct Meeting { pub id: i64, pub project_id: Option<i64>, pub title: String, pub created_at: i64, pub updated_at: i64, pub status: MeetingStatus, pub context: String, pub rolling_summary: Option<String> }`
  - `pub struct Session { pub id: i64, pub meeting_id: i64, pub started_at: i64, pub ended_at: Option<i64>, pub summary: Option<String> }`
  - `pub struct TranscriptLine { pub id: i64, pub session_id: i64, pub ts: i64, pub speaker: String, pub text: String }`
  - `pub struct MeetingDetail { pub meeting: Meeting, pub sessions: Vec<Session>, pub lines: Vec<TranscriptLine> }`
  - `create_meeting(&self, project_id: Option<i64>, title: &str) -> Result<Meeting>` (status `live`)
  - `list_meetings(&self, project_id: Option<i64>) -> Result<Vec<Meeting>>` (None lists all; ordered `updated_at DESC`)
  - `get_meeting(&self, id) -> Result<Option<Meeting>>`
  - `get_meeting_detail(&self, id) -> Result<Option<MeetingDetail>>` (sessions by `started_at ASC`, lines by `ts ASC, id ASC`)
  - `set_meeting_status(&self, id, MeetingStatus) -> Result<()>` (bumps `updated_at`)
  - `update_meeting_title(&self, id, &str)`, `update_meeting_context(&self, id, &str)` (bump `updated_at`)
  - `delete_meeting(&self, id) -> Result<()>` (lines, sessions, meeting in one transaction)
  - `create_session(&self, meeting_id) -> Result<Session>`
  - `end_session(&self, session_id) -> Result<()>` (sets `ended_at` now)
  - `append_line(&self, session_id, speaker: &str, text: &str) -> Result<TranscriptLine>` (bumps the meeting's `updated_at`)
  - `mark_live_sessions_paused(&self) -> Result<usize>` (meetings with status `live` become `paused`; their open sessions keep `ended_at NULL`)
  - `delete_project` now also deletes that project's meetings, sessions, and lines

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `store.rs`:

```rust
    #[test]
    fn create_meeting_is_live_and_listed_by_project() {
        let (_d, s) = store();
        let p = s.create_project("P").unwrap();
        let m = s.create_meeting(Some(p.id), "Standup").unwrap();
        assert_eq!(m.status, MeetingStatus::Live);
        let q = s.create_meeting(None, "Quick").unwrap();
        assert_eq!(s.list_meetings(Some(p.id)).unwrap().iter().map(|x| x.id).collect::<Vec<_>>(), vec![m.id]);
        let all = s.list_meetings(None).unwrap();
        assert!(all.iter().any(|x| x.id == q.id) && all.iter().any(|x| x.id == m.id));
    }

    #[test]
    fn session_lines_and_detail_ordering() {
        let (_d, s) = store();
        let m = s.create_meeting(None, "M").unwrap();
        let sess = s.create_session(m.id).unwrap();
        let a = s.append_line(sess.id, "me", "first").unwrap();
        let b = s.append_line(sess.id, "me", "second").unwrap();
        let d = s.get_meeting_detail(m.id).unwrap().unwrap();
        assert_eq!(d.sessions.len(), 1);
        assert_eq!(d.lines.iter().map(|l| l.id).collect::<Vec<_>>(), vec![a.id, b.id]);
        assert!(d.meeting.updated_at >= m.updated_at);
        s.end_session(sess.id).unwrap();
        let d = s.get_meeting_detail(m.id).unwrap().unwrap();
        assert!(d.sessions[0].ended_at.is_some());
    }

    #[test]
    fn status_title_context_updates() {
        let (_d, s) = store();
        let m = s.create_meeting(None, "M").unwrap();
        s.set_meeting_status(m.id, MeetingStatus::Paused).unwrap();
        s.update_meeting_title(m.id, "Renamed").unwrap();
        s.update_meeting_context(m.id, "agenda").unwrap();
        let got = s.get_meeting(m.id).unwrap().unwrap();
        assert_eq!(got.status, MeetingStatus::Paused);
        assert_eq!(got.title, "Renamed");
        assert_eq!(got.context, "agenda");
    }

    #[test]
    fn mark_live_sessions_paused_only_touches_live() {
        let (_d, s) = store();
        let live = s.create_meeting(None, "L").unwrap();
        let ended = s.create_meeting(None, "E").unwrap();
        s.set_meeting_status(ended.id, MeetingStatus::Ended).unwrap();
        assert_eq!(s.mark_live_sessions_paused().unwrap(), 1);
        assert_eq!(s.get_meeting(live.id).unwrap().unwrap().status, MeetingStatus::Paused);
        assert_eq!(s.get_meeting(ended.id).unwrap().unwrap().status, MeetingStatus::Ended);
    }

    #[test]
    fn delete_meeting_and_delete_project_cascade_lines() {
        let (_d, s) = store();
        let p = s.create_project("P").unwrap();
        let m1 = s.create_meeting(Some(p.id), "A").unwrap();
        let s1 = s.create_session(m1.id).unwrap();
        s.append_line(s1.id, "me", "x").unwrap();
        let m2 = s.create_meeting(Some(p.id), "B").unwrap();
        let s2 = s.create_session(m2.id).unwrap();
        s.append_line(s2.id, "me", "y").unwrap();
        s.delete_meeting(m1.id).unwrap();
        assert!(s.get_meeting_detail(m1.id).unwrap().is_none());
        s.delete_project(p.id).unwrap();
        assert!(s.get_meeting(m2.id).unwrap().is_none());
        let conn = rusqlite::Connection::open(_d.path().join("meet.db")).unwrap();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM transcript_lines", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib meeting::store 2>&1 | tail -5`
Expected: compile error, `create_meeting` not found.

- [ ] **Step 3: Implement**

Append a third migration to `MIGRATIONS`:

```rust
    M::up(
        "CREATE TABLE meetings (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id INTEGER REFERENCES projects(id) ON DELETE SET NULL,
            title TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            status TEXT NOT NULL DEFAULT 'live',
            context TEXT NOT NULL DEFAULT '',
            rolling_summary TEXT
        );
        CREATE INDEX meetings_project_idx ON meetings(project_id);
        CREATE TABLE sessions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            meeting_id INTEGER NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
            started_at INTEGER NOT NULL,
            ended_at INTEGER,
            summary TEXT
        );
        CREATE INDEX sessions_meeting_idx ON sessions(meeting_id);
        CREATE TABLE transcript_lines (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
            ts INTEGER NOT NULL,
            speaker TEXT NOT NULL,
            text TEXT NOT NULL
        );
        CREATE INDEX transcript_lines_session_idx ON transcript_lines(session_id);",
    ),
```

Types:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum MeetingStatus {
    Live,
    Paused,
    Ended,
}

impl MeetingStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            MeetingStatus::Live => "live",
            MeetingStatus::Paused => "paused",
            MeetingStatus::Ended => "ended",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "live" => Some(Self::Live),
            "paused" => Some(Self::Paused),
            "ended" => Some(Self::Ended),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct Meeting {
    pub id: i64,
    pub project_id: Option<i64>,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub status: MeetingStatus,
    pub context: String,
    pub rolling_summary: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct Session {
    pub id: i64,
    pub meeting_id: i64,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub summary: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct TranscriptLine {
    pub id: i64,
    pub session_id: i64,
    pub ts: i64,
    pub speaker: String,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct MeetingDetail {
    pub meeting: Meeting,
    pub sessions: Vec<Session>,
    pub lines: Vec<TranscriptLine>,
}

const MEETING_COLS: &str =
    "id, project_id, title, created_at, updated_at, status, context, rolling_summary";
```

Methods (inside `impl ProjectStore`):

```rust
    fn map_meeting(row: &rusqlite::Row<'_>) -> rusqlite::Result<Meeting> {
        let status: String = row.get("status")?;
        Ok(Meeting {
            id: row.get("id")?,
            project_id: row.get("project_id")?,
            title: row.get("title")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
            status: MeetingStatus::parse(&status).unwrap_or(MeetingStatus::Ended),
            context: row.get("context")?,
            rolling_summary: row.get("rolling_summary")?,
        })
    }

    fn map_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<Session> {
        Ok(Session {
            id: row.get("id")?,
            meeting_id: row.get("meeting_id")?,
            started_at: row.get("started_at")?,
            ended_at: row.get("ended_at")?,
            summary: row.get("summary")?,
        })
    }

    fn map_line(row: &rusqlite::Row<'_>) -> rusqlite::Result<TranscriptLine> {
        Ok(TranscriptLine {
            id: row.get("id")?,
            session_id: row.get("session_id")?,
            ts: row.get("ts")?,
            speaker: row.get("speaker")?,
            text: row.get("text")?,
        })
    }

    fn touch_meeting(conn: &Connection, id: i64) -> Result<()> {
        let changed = conn.execute(
            "UPDATE meetings SET updated_at = ?1 WHERE id = ?2",
            params![now(), id],
        )?;
        if changed == 0 {
            return Err(anyhow!("meeting {} not found", id));
        }
        Ok(())
    }

    pub fn create_meeting(&self, project_id: Option<i64>, title: &str) -> Result<Meeting> {
        let conn = self.conn()?;
        let ts = now();
        conn.execute(
            "INSERT INTO meetings (project_id, title, created_at, updated_at, status, context)
             VALUES (?1, ?2, ?3, ?3, 'live', '')",
            params![project_id, title, ts],
        )?;
        let id = conn.last_insert_rowid();
        if let Some(pid) = project_id {
            Self::touch_project(&conn, pid)?;
        }
        Ok(Meeting {
            id,
            project_id,
            title: title.to_string(),
            created_at: ts,
            updated_at: ts,
            status: MeetingStatus::Live,
            context: String::new(),
            rolling_summary: None,
        })
    }

    pub fn list_meetings(&self, project_id: Option<i64>) -> Result<Vec<Meeting>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {MEETING_COLS} FROM meetings
             WHERE (?1 IS NULL OR project_id = ?1)
             ORDER BY updated_at DESC, id DESC"
        ))?;
        let rows = stmt.query_map(params![project_id], Self::map_meeting)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn get_meeting(&self, id: i64) -> Result<Option<Meeting>> {
        let conn = self.conn()?;
        Ok(conn
            .query_row(
                &format!("SELECT {MEETING_COLS} FROM meetings WHERE id = ?1"),
                params![id],
                Self::map_meeting,
            )
            .optional()?)
    }

    pub fn get_meeting_detail(&self, id: i64) -> Result<Option<MeetingDetail>> {
        let Some(meeting) = self.get_meeting(id)? else {
            return Ok(None);
        };
        let conn = self.conn()?;
        let sessions = {
            let mut stmt = conn.prepare(
                "SELECT id, meeting_id, started_at, ended_at, summary FROM sessions
                 WHERE meeting_id = ?1 ORDER BY started_at ASC, id ASC",
            )?;
            let rows = stmt.query_map(params![id], Self::map_session)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let lines = {
            let mut stmt = conn.prepare(
                "SELECT l.id, l.session_id, l.ts, l.speaker, l.text FROM transcript_lines l
                 JOIN sessions s ON s.id = l.session_id
                 WHERE s.meeting_id = ?1 ORDER BY l.ts ASC, l.id ASC",
            )?;
            let rows = stmt.query_map(params![id], Self::map_line)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        Ok(Some(MeetingDetail { meeting, sessions, lines }))
    }

    pub fn set_meeting_status(&self, id: i64, status: MeetingStatus) -> Result<()> {
        let conn = self.conn()?;
        let changed = conn.execute(
            "UPDATE meetings SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status.as_str(), now(), id],
        )?;
        if changed == 0 {
            return Err(anyhow!("meeting {} not found", id));
        }
        Ok(())
    }

    pub fn update_meeting_title(&self, id: i64, title: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute("UPDATE meetings SET title = ?1 WHERE id = ?2", params![title, id])?;
        Self::touch_meeting(&conn, id)
    }

    pub fn update_meeting_context(&self, id: i64, context: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute("UPDATE meetings SET context = ?1 WHERE id = ?2", params![context, id])?;
        Self::touch_meeting(&conn, id)
    }

    pub fn delete_meeting(&self, id: i64) -> Result<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM transcript_lines WHERE session_id IN (SELECT id FROM sessions WHERE meeting_id = ?1)",
            params![id],
        )?;
        tx.execute("DELETE FROM sessions WHERE meeting_id = ?1", params![id])?;
        tx.execute("DELETE FROM meetings WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(())
    }

    pub fn create_session(&self, meeting_id: i64) -> Result<Session> {
        let conn = self.conn()?;
        let ts = now();
        conn.execute(
            "INSERT INTO sessions (meeting_id, started_at) VALUES (?1, ?2)",
            params![meeting_id, ts],
        )?;
        let id = conn.last_insert_rowid();
        Self::touch_meeting(&conn, meeting_id)?;
        Ok(Session { id, meeting_id, started_at: ts, ended_at: None, summary: None })
    }

    pub fn end_session(&self, session_id: i64) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE sessions SET ended_at = ?1 WHERE id = ?2 AND ended_at IS NULL",
            params![now(), session_id],
        )?;
        Ok(())
    }

    pub fn append_line(&self, session_id: i64, speaker: &str, text: &str) -> Result<TranscriptLine> {
        let conn = self.conn()?;
        let ts = now();
        conn.execute(
            "INSERT INTO transcript_lines (session_id, ts, speaker, text) VALUES (?1, ?2, ?3, ?4)",
            params![session_id, ts, speaker, text],
        )?;
        let id = conn.last_insert_rowid();
        let meeting_id: i64 = conn.query_row(
            "SELECT meeting_id FROM sessions WHERE id = ?1",
            params![session_id],
            |r| r.get(0),
        )?;
        Self::touch_meeting(&conn, meeting_id)?;
        Ok(TranscriptLine { id, session_id, ts, speaker: speaker.to_string(), text: text.to_string() })
    }

    pub fn mark_live_sessions_paused(&self) -> Result<usize> {
        let conn = self.conn()?;
        Ok(conn.execute(
            "UPDATE meetings SET status = 'paused', updated_at = ?1 WHERE status = 'live'",
            params![now()],
        )?)
    }
```

Extend `delete_project` (inside its transaction, before deleting sources):

```rust
        tx.execute(
            "DELETE FROM transcript_lines WHERE session_id IN
               (SELECT s.id FROM sessions s JOIN meetings m ON m.id = s.meeting_id WHERE m.project_id = ?1)",
            params![id],
        )?;
        tx.execute(
            "DELETE FROM sessions WHERE meeting_id IN (SELECT id FROM meetings WHERE project_id = ?1)",
            params![id],
        )?;
        tx.execute("DELETE FROM meetings WHERE project_id = ?1", params![id])?;
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib meeting::store 2>&1 | tail -3`
Expected: all pass (previous 14 plus 5 new).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/meeting/store.rs
git commit -m "feat: add meetings, sessions, and transcript lines to the store"
```

---

### Task 2: Utterance segmenter

**Files:**

- Create: `src-tauri/src/meeting/segment.rs`
- Modify: `src-tauri/src/meeting/mod.rs` (add `pub mod segment;`)

**Interfaces:**

- Produces:
  - `pub struct SegmenterConfig { pub sample_rate: usize, pub frame_samples: usize, pub end_silence_ms: u64, pub min_utterance_ms: u64, pub max_utterance_ms: u64 }` with `SegmenterConfig::defaults(frame_samples) -> Self` using 16000 / 700 / 300 / 30000.
  - `pub struct Segmenter { .. }` with `Segmenter::new(cfg: SegmenterConfig) -> Self`, `push(&mut self, speech: Option<&[f32]>) -> Option<Vec<f32>>` (call once per VAD frame; `Some(samples)` means the VAD passed speech for this frame, possibly aggregated prefill; `None` means noise), and `flush(&mut self) -> Option<Vec<f32>>` (emit whatever is buffered if it meets the minimum).

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> SegmenterConfig {
        // 16 kHz, 320-sample frames (20 ms): end after 700 ms = 35 noise frames,
        // min 300 ms = 4800 samples, max 30 s.
        SegmenterConfig::defaults(320)
    }

    fn speech_frame() -> Vec<f32> {
        vec![0.1; 320]
    }

    #[test]
    fn emits_after_end_silence() {
        let mut seg = Segmenter::new(cfg());
        for _ in 0..20 {
            assert!(seg.push(Some(&speech_frame())).is_none());
        }
        for _ in 0..34 {
            assert!(seg.push(None).is_none());
        }
        let out = seg.push(None).expect("utterance after 35 noise frames");
        assert_eq!(out.len(), 20 * 320);
    }

    #[test]
    fn drops_utterances_shorter_than_min() {
        let mut seg = Segmenter::new(cfg());
        for _ in 0..5 {
            seg.push(Some(&speech_frame()));
        }
        for _ in 0..40 {
            if let Some(_) = seg.push(None) {
                panic!("5 frames (100 ms) must be dropped");
            }
        }
    }

    #[test]
    fn hard_cuts_at_max_length() {
        let mut c = cfg();
        c.max_utterance_ms = 1000; // 50 frames
        let mut seg = Segmenter::new(c);
        let mut emitted = None;
        for i in 0..60 {
            if let Some(out) = seg.push(Some(&speech_frame())) {
                emitted = Some((i, out.len()));
                break;
            }
        }
        let (at, len) = emitted.expect("hard cut");
        assert_eq!(at, 49);
        assert_eq!(len, 50 * 320);
    }

    #[test]
    fn flush_emits_buffered_speech() {
        let mut seg = Segmenter::new(cfg());
        for _ in 0..20 {
            seg.push(Some(&speech_frame()));
        }
        assert_eq!(seg.flush().unwrap().len(), 20 * 320);
        assert!(seg.flush().is_none());
    }

    #[test]
    fn noise_before_speech_does_not_count() {
        let mut seg = Segmenter::new(cfg());
        for _ in 0..100 {
            assert!(seg.push(None).is_none());
        }
        for _ in 0..20 {
            seg.push(Some(&speech_frame()));
        }
        for _ in 0..34 {
            seg.push(None);
        }
        assert!(seg.push(None).is_some());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib meeting::segment 2>&1 | tail -3`
Expected: compile error.

- [ ] **Step 3: Implement**

```rust
//! Turns the VAD's per-frame speech/noise stream into utterances. Pure and
//! unit-tested; knows nothing about microphones or models.

pub struct SegmenterConfig {
    pub sample_rate: usize,
    pub frame_samples: usize,
    pub end_silence_ms: u64,
    pub min_utterance_ms: u64,
    pub max_utterance_ms: u64,
}

impl SegmenterConfig {
    pub fn defaults(frame_samples: usize) -> Self {
        Self {
            sample_rate: 16_000,
            frame_samples,
            end_silence_ms: 700,
            min_utterance_ms: 300,
            max_utterance_ms: 30_000,
        }
    }

    fn ms_to_samples(&self, ms: u64) -> usize {
        (self.sample_rate as u64 * ms / 1000) as usize
    }

    fn ms_to_frames(&self, ms: u64) -> usize {
        let frame_ms = (self.frame_samples as u64 * 1000) / self.sample_rate as u64;
        (ms / frame_ms.max(1)) as usize
    }
}

pub struct Segmenter {
    end_silence_frames: usize,
    min_samples: usize,
    max_samples: usize,
    buffer: Vec<f32>,
    silence_run: usize,
}

impl Segmenter {
    pub fn new(cfg: SegmenterConfig) -> Self {
        Self {
            end_silence_frames: cfg.ms_to_frames(cfg.end_silence_ms).max(1),
            min_samples: cfg.ms_to_samples(cfg.min_utterance_ms),
            max_samples: cfg.ms_to_samples(cfg.max_utterance_ms).max(cfg.frame_samples),
            buffer: Vec::new(),
            silence_run: 0,
        }
    }

    /// Feed one VAD frame. `Some(samples)` = speech passed by the VAD (may be
    /// longer than one frame when the VAD prepends prefill); `None` = noise.
    pub fn push(&mut self, speech: Option<&[f32]>) -> Option<Vec<f32>> {
        match speech {
            Some(samples) => {
                self.silence_run = 0;
                self.buffer.extend_from_slice(samples);
                if self.buffer.len() >= self.max_samples {
                    return self.take();
                }
                None
            }
            None => {
                if self.buffer.is_empty() {
                    return None;
                }
                self.silence_run += 1;
                if self.silence_run >= self.end_silence_frames {
                    return self.take();
                }
                None
            }
        }
    }

    pub fn flush(&mut self) -> Option<Vec<f32>> {
        self.take()
    }

    fn take(&mut self) -> Option<Vec<f32>> {
        self.silence_run = 0;
        let out = std::mem::take(&mut self.buffer);
        if out.len() >= self.min_samples {
            Some(out)
        } else {
            None
        }
    }
}
```

Note on `hard_cuts_at_max_length`: with `max_utterance_ms = 1000` the cap is 16000 samples = 50 frames of 320; the 50th push (index 49) reaches it. If the test disagrees by one frame, the `>=` is the contract; fix the implementation, not the test.

- [ ] **Step 4: Run tests**

Run: `cargo test --lib meeting::segment 2>&1 | tail -3`
Expected: `5 passed`.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/meeting/segment.rs src-tauri/src/meeting/mod.rs
git commit -m "feat: add utterance segmenter over VAD frames"
```

---

### Task 3: Microphone capture with VAD

**Files:**

- Modify: `src-tauri/src/audio_toolkit/audio/sink.rs` (add two methods on `CaptureVad`)
- Modify: `src-tauri/src/managers/audio.rs` (`create_capture_vad` and `microphone_config` become `pub(crate)`)
- Create: `src-tauri/src/meeting/capture.rs`
- Modify: `src-tauri/src/meeting/mod.rs` (add `pub mod capture;`)

**Interfaces:**

- Consumes: `Segmenter`, `SegmenterConfig`; `CaptureVad` (`frame_samples()`); `handy_recorder::{Recorder, RecorderConfig, Sink, AudioChunk}`; `crate::managers::audio::{create_capture_vad, microphone_config}`; `crate::settings::AppSettings` fields `selected_microphone`, `selected_channel`, `vad_backend`.
- Produces:
  - On `CaptureVad`: `pub fn prepare_offline(&mut self)` (sets hangover to the offline value and resets the detector) and `pub fn push_frame(&mut self, frame: &[f32]) -> anyhow::Result<Option<Vec<f32>>>` (Some = speech samples, None = noise).
  - `pub struct Utterance { pub samples: Vec<f32>, pub captured_at: i64 }`
  - `pub struct MeetingSink { .. }` implementing `handy_recorder::Sink`; `MeetingSink::new(vad: CaptureVad, tx: mpsc::Sender<Utterance>) -> Self`
  - `pub struct MeetingCapture { .. }` with `MeetingCapture::start(settings: &AppSettings, app: &tauri::AppHandle, tx: mpsc::Sender<Utterance>) -> anyhow::Result<Self>` and `stop(self)` (stops and closes the recorder; flushes a trailing utterance).

- [ ] **Step 1: Write the failing tests**

In `capture.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_toolkit::vad::{VadFrame, VoiceActivityDetector};
    use handy_recorder::AudioChunk;
    use std::sync::mpsc;

    /// Detector that calls a frame speech when its mean absolute amplitude
    /// exceeds 0.05, with no smoothing. 320-sample frames.
    struct LoudVad;
    impl VoiceActivityDetector for LoudVad {
        fn push_frame<'a>(&'a mut self, frame: &'a [f32]) -> anyhow::Result<VadFrame<'a>> {
            let mean = frame.iter().map(|s| s.abs()).sum::<f32>() / frame.len().max(1) as f32;
            Ok(if mean > 0.05 { VadFrame::Speech(frame) } else { VadFrame::Noise })
        }
        fn frame_samples(&self) -> usize { 320 }
        fn set_hangover_frames(&mut self, _frames: usize) {}
        fn reset(&mut self) {}
    }

    fn chunk(samples: &[f32]) -> AudioChunk<'_> {
        AudioChunk { samples, sample_rate: 16_000, channels: 1, valid_frames: samples.len() }
    }

    #[test]
    fn sink_emits_an_utterance_after_speech_then_silence() {
        let vad = CaptureVad::new(Box::new(LoudVad), 0, 0);
        let (tx, rx) = mpsc::channel();
        let mut sink = MeetingSink::new(vad, tx);
        let loud = vec![0.2f32; 320];
        let quiet = vec![0.0f32; 320];
        for _ in 0..25 {
            sink.process_chunk(chunk(&loud));
        }
        for _ in 0..40 {
            sink.process_chunk(chunk(&quiet));
        }
        let utt = rx.try_recv().expect("one utterance");
        assert_eq!(utt.samples.len(), 25 * 320);
        assert!(utt.captured_at > 0);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn sink_ignores_padding_beyond_valid_frames() {
        let vad = CaptureVad::new(Box::new(LoudVad), 0, 0);
        let (tx, rx) = mpsc::channel();
        let mut sink = MeetingSink::new(vad, tx);
        let mut partial = vec![0.2f32; 320];
        for s in partial.iter_mut().skip(100) {
            *s = 0.0;
        }
        // Only 100 valid frames: must not be pushed as a full frame.
        sink.process_chunk(AudioChunk { samples: &partial, sample_rate: 16_000, channels: 1, valid_frames: 100 });
        assert!(rx.try_recv().is_err());
    }
}
```

If the `VoiceActivityDetector` trait has more required methods than the four used above (check `src-tauri/src/audio_toolkit/vad/mod.rs:33` onward; `tail_report` has a default per the explorer report, but verify), implement them minimally in `LoudVad`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib meeting::capture 2>&1 | tail -3`
Expected: compile error.

- [ ] **Step 3: Implement**

`sink.rs`, add to `impl CaptureVad` (after `frame_samples`):

```rust
    /// Configure the detector for offline-style hangover and clear state.
    /// Used by meeting capture, which drives the detector frame by frame.
    pub fn prepare_offline(&mut self) {
        self.detector.set_hangover_frames(self.offline_hangover_frames);
        self.detector.reset();
    }

    /// Push one frame and return the speech samples the detector passes
    /// (prefill plus current plus hangover may make this longer than one
    /// frame), or `None` for noise.
    pub fn push_frame(&mut self, frame: &[f32]) -> anyhow::Result<Option<Vec<f32>>> {
        Ok(match self.detector.push_frame(frame)? {
            VadFrame::Speech(samples) => Some(samples.to_vec()),
            VadFrame::Noise => None,
        })
    }
```

Ensure `VadFrame` is imported in `sink.rs` (it is already used there; check the `use` lines).

`managers/audio.rs`: change `fn create_capture_vad(` to `pub(crate) fn create_capture_vad(` and `fn microphone_config(` to `pub(crate) fn microphone_config(`. No other change.

`meeting/capture.rs`:

```rust
//! Microphone capture for meetings: its own handy-recorder stream, the shared
//! VAD, and the segmenter. Emits utterances on a channel; owns no model.

use crate::audio_toolkit::audio::CaptureVad;
use crate::managers::audio::{create_capture_vad, microphone_config};
use crate::meeting::segment::{Segmenter, SegmenterConfig};
use crate::settings::AppSettings;
use anyhow::{anyhow, Context, Result};
use handy_recorder::{AudioChunk, Recorder, Sink};
use log::{info, warn};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

pub struct Utterance {
    pub samples: Vec<f32>,
    pub captured_at: i64,
}

pub struct MeetingSink {
    vad: CaptureVad,
    segmenter: Segmenter,
    tx: mpsc::Sender<Utterance>,
}

impl MeetingSink {
    pub fn new(mut vad: CaptureVad, tx: mpsc::Sender<Utterance>) -> Self {
        vad.prepare_offline();
        let segmenter = Segmenter::new(SegmenterConfig::defaults(vad.frame_samples()));
        Self { vad, segmenter, tx }
    }

    fn emit(&self, samples: Vec<f32>) {
        let utt = Utterance { samples, captured_at: chrono::Utc::now().timestamp_millis() };
        if self.tx.send(utt).is_err() {
            warn!("meeting capture: utterance receiver is gone");
        }
    }

    /// Flush a trailing utterance when capture stops.
    pub fn finish(mut self) {
        if let Some(samples) = self.segmenter.flush() {
            self.emit(samples);
        }
    }
}

impl Sink for MeetingSink {
    fn process_chunk(&mut self, chunk: AudioChunk<'_>) {
        let frame_len = self.vad.frame_samples();
        if chunk.valid_frames < frame_len {
            // Final zero-padded chunk; never feed padding to the detector.
            return;
        }
        let frame = &chunk.samples[..frame_len];
        let speech = match self.vad.push_frame(frame) {
            Ok(s) => s,
            Err(e) => {
                warn!("meeting capture: VAD error: {e:#}");
                None
            }
        };
        if let Some(samples) = self.segmenter.push(speech.as_deref()) {
            self.emit(samples);
        }
    }
}

pub struct MeetingCapture {
    recorder: Recorder<MeetingSink>,
    failed: Arc<AtomicBool>,
}

impl MeetingCapture {
    /// Open the configured microphone and start delivering utterances to `tx`.
    pub fn start(
        settings: &AppSettings,
        app: &tauri::AppHandle,
        tx: mpsc::Sender<Utterance>,
    ) -> Result<Self> {
        let vad = create_capture_vad(settings.vad_backend, app).context("create VAD")?;
        let frame = vad.frame_samples();
        let config = microphone_config(
            settings.selected_microphone.clone(),
            settings.selected_channel,
            frame,
        );
        let failed = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&failed);
        let recorder = Recorder::open_with_failure_handler(config, move |e| {
            warn!("meeting capture: recorder failed: {e}");
            flag.store(true, Ordering::Release);
        })
        .map_err(|e| anyhow!("open microphone: {e}"))?;
        let sink = MeetingSink::new(vad, tx);
        recorder
            .start(sink)
            .map_err(|e| anyhow!("start microphone: {}", e.error))?;
        info!("meeting capture started ({} samples/frame)", frame);
        Ok(Self { recorder, failed })
    }

    pub fn has_failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }

    /// Stop the stream, flush the trailing utterance, close the device.
    pub fn stop(self) {
        match self.recorder.stop() {
            Ok(stopped) => stopped.sink.finish(),
            Err(e) => warn!("meeting capture: stop failed: {e}"),
        }
        if let Err(e) = self.recorder.close() {
            warn!("meeting capture: close failed: {e}");
        }
    }
}
```

Check `handy_recorder::StartError` field names (`error`, `sink`) in `~/.cargo/registry/src/*/handy-recorder-0.1.0/src/lib.rs` and `Stopped { sink, .. }`; adapt names if they differ. `settings.vad_backend` is `VadBackend` (Copy); if it is not `Copy`, clone it.

- [ ] **Step 4: Run tests and check**

Run: `cargo test --lib meeting::capture 2>&1 | tail -3` then `cargo check 2>&1 | tail -2`
Expected: `2 passed`; check finishes (dead-code warnings for `MeetingCapture` are acceptable until Task 4).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/audio_toolkit/audio/sink.rs src-tauri/src/managers/audio.rs src-tauri/src/meeting/capture.rs src-tauri/src/meeting/mod.rs
git commit -m "feat: add meeting microphone capture over the shared VAD"
```

---

### Task 4: MeetingManager state machine and transcriber thread

**Files:**

- Create: `src-tauri/src/meeting/session.rs`
- Modify: `src-tauri/src/meeting/mod.rs` (add `pub mod session;`)

**Interfaces:**

- Consumes: `ProjectStore` meeting methods (Task 1), `MeetingCapture`, `Utterance` (Task 3).
- Produces:
  - `pub trait Transcriber: Send + Sync { fn transcribe(&self, samples: Vec<f32>) -> anyhow::Result<String>; }`
  - `pub struct ModelTranscriber(pub Arc<TranscriptionManager>)` implementing it as `self.0.initiate_model_load(); self.0.transcribe(samples)`.
  - `pub trait CaptureSource: Send + Sync { fn start(&self, tx: mpsc::Sender<Utterance>) -> anyhow::Result<Box<dyn CaptureHandle>>; }` and `pub trait CaptureHandle: Send { fn stop(self: Box<Self>); }`. Production impl `MicCapture { app: AppHandle }` reads settings and calls `MeetingCapture::start`.
  - `pub struct MeetingStatePayload { pub meeting_id: Option<i64>, pub session_id: Option<i64>, pub status: Option<MeetingStatus> }` and `pub struct TranscriptLinePayload { pub meeting_id: i64, pub line: TranscriptLine }`, both `tauri_specta::Event`.
  - `pub struct MeetingManager { .. }` with:
    - `new(app: &AppHandle, store: Arc<ProjectStore>, tm: Arc<TranscriptionManager>, rm: Arc<AudioRecordingManager>) -> Self`
    - `new_for_test(store: Arc<ProjectStore>, transcriber: Arc<dyn Transcriber>, capture: Arc<dyn CaptureSource>, dictation_recording: Arc<AtomicBool>) -> Self`
    - `start(&self, project_id: Option<i64>, meeting_id: Option<i64>) -> Result<Meeting>` (creates meeting if `meeting_id` is None with auto title; creates a session; refuses if a session is already active or dictation is recording; sets status `live`; starts capture; spawns transcriber thread)
    - `pause(&self) -> Result<()>`, `resume(&self) -> Result<()>`, `end(&self) -> Result<()>`
    - `state(&self) -> MeetingStatePayload`
    - `ingest(&self, utt: Utterance) -> Result<Option<TranscriptLine>>` (transcribe, drop blank, store, emit)

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::meeting::store::ProjectStore;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Mutex;

    struct FakeTranscriber {
        calls: AtomicUsize,
        fail_first: bool,
    }
    impl Transcriber for FakeTranscriber {
        fn transcribe(&self, samples: Vec<f32>) -> anyhow::Result<String> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail_first && n == 0 {
                return Err(anyhow::anyhow!("Model is not loaded for transcription."));
            }
            Ok(if samples.is_empty() { String::new() } else { format!("heard {} samples", samples.len()) })
        }
    }

    struct FakeCapture {
        senders: Mutex<Vec<mpsc::Sender<Utterance>>>,
        stops: AtomicUsize,
    }
    struct FakeHandle(Arc<FakeCapture>);
    impl CaptureHandle for FakeHandle {
        fn stop(self: Box<Self>) {
            self.0.stops.fetch_add(1, Ordering::SeqCst);
        }
    }
    impl CaptureSource for FakeCapture {
        fn start(&self, tx: mpsc::Sender<Utterance>) -> anyhow::Result<Box<dyn CaptureHandle>> {
            self.senders.lock().unwrap().push(tx);
            Ok(Box::new(FakeHandle(Arc::new(FakeCapture { senders: Mutex::new(vec![]), stops: AtomicUsize::new(0) }))))
        }
    }

    fn mgr(fail_first: bool) -> (tempfile::TempDir, MeetingManager, Arc<FakeCapture>, Arc<AtomicBool>) {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(ProjectStore::open(dir.path().join("meet.db")).unwrap());
        let cap = Arc::new(FakeCapture { senders: Mutex::new(vec![]), stops: AtomicUsize::new(0) });
        let dict = Arc::new(AtomicBool::new(false));
        let m = MeetingManager::new_for_test(
            store,
            Arc::new(FakeTranscriber { calls: AtomicUsize::new(0), fail_first }),
            cap.clone(),
            dict.clone(),
        );
        (dir, m, cap, dict)
    }

    fn wait_lines(m: &MeetingManager, meeting_id: i64, n: usize) -> Vec<TranscriptLine> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let d = m.store().get_meeting_detail(meeting_id).unwrap().unwrap();
            if d.lines.len() >= n {
                return d.lines;
            }
            assert!(std::time::Instant::now() < deadline, "timed out waiting for {} lines", n);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    #[test]
    fn start_creates_live_meeting_and_session_then_lines_arrive() {
        let (_d, m, cap, _dict) = mgr(false);
        let meeting = m.start(None, None).unwrap();
        assert_eq!(meeting.status, MeetingStatus::Live);
        let st = m.state();
        assert_eq!(st.meeting_id, Some(meeting.id));
        assert!(st.session_id.is_some());
        let tx = cap.senders.lock().unwrap()[0].clone();
        tx.send(Utterance { samples: vec![0.1; 4800], captured_at: 1 }).unwrap();
        tx.send(Utterance { samples: vec![], captured_at: 2 }).unwrap(); // blank text, dropped
        tx.send(Utterance { samples: vec![0.1; 1600], captured_at: 3 }).unwrap();
        let lines = wait_lines(&m, meeting.id, 2);
        assert_eq!(lines[0].text, "heard 4800 samples");
        assert_eq!(lines[1].text, "heard 1600 samples");
        assert!(lines.iter().all(|l| l.speaker == "me"));
    }

    #[test]
    fn start_refuses_while_dictation_recording_or_already_live() {
        let (_d, m, _cap, dict) = mgr(false);
        dict.store(true, Ordering::SeqCst);
        assert!(m.start(None, None).is_err());
        dict.store(false, Ordering::SeqCst);
        m.start(None, None).unwrap();
        assert!(m.start(None, None).is_err());
    }

    #[test]
    fn pause_resume_keep_session_and_end_closes_it() {
        let (_d, m, cap, _dict) = mgr(false);
        let meeting = m.start(None, None).unwrap();
        let session_id = m.state().session_id.unwrap();
        m.pause().unwrap();
        assert_eq!(m.state().status, Some(MeetingStatus::Paused));
        assert_eq!(m.store().get_meeting(meeting.id).unwrap().unwrap().status, MeetingStatus::Paused);
        m.resume().unwrap();
        assert_eq!(m.state().session_id, Some(session_id));
        assert_eq!(cap.senders.lock().unwrap().len(), 2, "capture restarted on resume");
        m.end().unwrap();
        let d = m.store().get_meeting_detail(meeting.id).unwrap().unwrap();
        assert_eq!(d.meeting.status, MeetingStatus::Ended);
        assert!(d.sessions[0].ended_at.is_some());
        assert_eq!(m.state().meeting_id, None);
    }

    #[test]
    fn continue_ended_meeting_opens_second_session() {
        let (_d, m, _cap, _dict) = mgr(false);
        let meeting = m.start(Some(1).filter(|_| false), None).unwrap();
        m.end().unwrap();
        let again = m.start(None, Some(meeting.id)).unwrap();
        assert_eq!(again.id, meeting.id);
        let d = m.store().get_meeting_detail(meeting.id).unwrap().unwrap();
        assert_eq!(d.sessions.len(), 2);
        assert_eq!(d.meeting.status, MeetingStatus::Live);
    }

    #[test]
    fn transcriber_error_is_retried_once_then_line_stored() {
        let (_d, m, cap, _dict) = mgr(true);
        let meeting = m.start(None, None).unwrap();
        let tx = cap.senders.lock().unwrap()[0].clone();
        tx.send(Utterance { samples: vec![0.1; 4800], captured_at: 1 }).unwrap();
        let lines = wait_lines(&m, meeting.id, 1);
        assert_eq!(lines[0].text, "heard 4800 samples");
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib meeting::session 2>&1 | tail -3`
Expected: compile error.

- [ ] **Step 3: Implement**

```rust
//! Meeting session manager: Idle -> Live <-> Paused -> Ended. Owns capture
//! lifetime and the transcriber thread; emits state and transcript events.

use crate::managers::audio::AudioRecordingManager;
use crate::managers::transcription::TranscriptionManager;
use crate::meeting::capture::{MeetingCapture, Utterance};
use crate::meeting::store::{Meeting, MeetingStatus, ProjectStore, TranscriptLine};
use anyhow::{anyhow, Result};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use tauri::AppHandle;
use tauri_specta::Event;

pub trait Transcriber: Send + Sync {
    fn transcribe(&self, samples: Vec<f32>) -> Result<String>;
}

pub struct ModelTranscriber(pub Arc<TranscriptionManager>);

impl Transcriber for ModelTranscriber {
    fn transcribe(&self, samples: Vec<f32>) -> Result<String> {
        // Idle unload may have dropped the model between utterances; the load
        // is a no-op when it is already warm, and transcribe waits on it.
        self.0.initiate_model_load();
        self.0.transcribe(samples)
    }
}

pub trait CaptureHandle: Send {
    fn stop(self: Box<Self>);
}

pub trait CaptureSource: Send + Sync {
    fn start(&self, tx: mpsc::Sender<Utterance>) -> Result<Box<dyn CaptureHandle>>;
}

struct MicCapture {
    app: AppHandle,
}

impl CaptureHandle for MeetingCapture {
    fn stop(self: Box<Self>) {
        MeetingCapture::stop(*self);
    }
}

impl CaptureSource for MicCapture {
    fn start(&self, tx: mpsc::Sender<Utterance>) -> Result<Box<dyn CaptureHandle>> {
        let settings = crate::settings::get_settings(&self.app);
        Ok(Box::new(MeetingCapture::start(&settings, &self.app, tx)?))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct MeetingStatePayload {
    pub meeting_id: Option<i64>,
    pub session_id: Option<i64>,
    pub status: Option<MeetingStatus>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct TranscriptLinePayload {
    pub meeting_id: i64,
    pub line: TranscriptLine,
}

struct Active {
    meeting_id: i64,
    session_id: i64,
    status: MeetingStatus,
    capture: Option<Box<dyn CaptureHandle>>,
}

pub struct MeetingManager {
    store: Arc<ProjectStore>,
    transcriber: Arc<dyn Transcriber>,
    capture: Arc<dyn CaptureSource>,
    dictation_recording: Arc<dyn Fn() -> bool + Send + Sync>,
    active: Mutex<Option<Active>>,
    app: Option<AppHandle>,
}

impl MeetingManager {
    pub fn new(
        app: &AppHandle,
        store: Arc<ProjectStore>,
        tm: Arc<TranscriptionManager>,
        rm: Arc<AudioRecordingManager>,
    ) -> Self {
        Self {
            store,
            transcriber: Arc::new(ModelTranscriber(tm)),
            capture: Arc::new(MicCapture { app: app.clone() }),
            dictation_recording: Arc::new(move || rm.is_recording()),
            active: Mutex::new(None),
            app: Some(app.clone()),
        }
    }

    pub fn new_for_test(
        store: Arc<ProjectStore>,
        transcriber: Arc<dyn Transcriber>,
        capture: Arc<dyn CaptureSource>,
        dictation_recording: Arc<AtomicBool>,
    ) -> Self {
        Self {
            store,
            transcriber,
            capture,
            dictation_recording: Arc::new(move || dictation_recording.load(Ordering::SeqCst)),
            active: Mutex::new(None),
            app: None,
        }
    }

    pub fn store(&self) -> &ProjectStore {
        &self.store
    }

    fn lock_active(&self) -> std::sync::MutexGuard<'_, Option<Active>> {
        self.active.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn state(&self) -> MeetingStatePayload {
        match self.lock_active().as_ref() {
            Some(a) => MeetingStatePayload {
                meeting_id: Some(a.meeting_id),
                session_id: Some(a.session_id),
                status: Some(a.status),
            },
            None => MeetingStatePayload { meeting_id: None, session_id: None, status: None },
        }
    }

    fn emit_state(&self) {
        if let Some(app) = &self.app {
            if let Err(e) = self.state().emit(app) {
                error!("emit meeting state: {e}");
            }
        }
    }

    fn start_capture_and_worker(&self, meeting_id: i64, session_id: i64) -> Result<Box<dyn CaptureHandle>> {
        let (tx, rx) = mpsc::channel::<Utterance>();
        let handle = self.capture.start(tx)?;
        let store = Arc::clone(&self.store);
        let transcriber = Arc::clone(&self.transcriber);
        let app = self.app.clone();
        std::thread::Builder::new()
            .name("meet-transcriber".into())
            .spawn(move || {
                for utt in rx {
                    match Self::transcribe_and_store(&store, transcriber.as_ref(), session_id, utt) {
                        Ok(Some(line)) => {
                            if let Some(app) = &app {
                                if let Err(e) = (TranscriptLinePayload { meeting_id, line }).emit(app) {
                                    error!("emit transcript line: {e}");
                                }
                            }
                        }
                        Ok(None) => {}
                        Err(e) => warn!("transcribe utterance: {e:#}"),
                    }
                }
                info!("meet-transcriber for session {} exited", session_id);
            })?;
        Ok(handle)
    }

    fn transcribe_and_store(
        store: &ProjectStore,
        transcriber: &dyn Transcriber,
        session_id: i64,
        utt: Utterance,
    ) -> Result<Option<TranscriptLine>> {
        let samples = utt.samples;
        let text = match transcriber.transcribe(samples.clone()) {
            Ok(t) => t,
            Err(first) => {
                warn!("transcribe failed once ({first:#}); retrying");
                std::thread::sleep(std::time::Duration::from_millis(500));
                transcriber.transcribe(samples)?
            }
        };
        let text = text.trim();
        if text.is_empty() {
            return Ok(None);
        }
        Ok(Some(store.append_line(session_id, "me", text)?))
    }

    /// Public for tests and for a future system-audio path; the worker uses
    /// the same function.
    pub fn ingest(&self, utt: Utterance) -> Result<Option<TranscriptLine>> {
        let session_id = self
            .lock_active()
            .as_ref()
            .map(|a| a.session_id)
            .ok_or_else(|| anyhow!("no active meeting"))?;
        Self::transcribe_and_store(&self.store, self.transcriber.as_ref(), session_id, utt)
    }

    fn auto_title() -> String {
        chrono::Local::now().format("Meeting %b %-d %H:%M").to_string()
    }

    pub fn start(&self, project_id: Option<i64>, meeting_id: Option<i64>) -> Result<Meeting> {
        if (self.dictation_recording)() {
            return Err(anyhow!("Dictation is recording; stop it before starting a meeting"));
        }
        let mut guard = self.lock_active();
        if guard.is_some() {
            return Err(anyhow!("A meeting is already running"));
        }
        let meeting = match meeting_id {
            Some(id) => {
                let m = self.store.get_meeting(id)?.ok_or_else(|| anyhow!("meeting {} not found", id))?;
                self.store.set_meeting_status(id, MeetingStatus::Live)?;
                Meeting { status: MeetingStatus::Live, ..m }
            }
            None => self.store.create_meeting(project_id, &Self::auto_title())?,
        };
        let session = self.store.create_session(meeting.id)?;
        let capture = match self.start_capture_and_worker(meeting.id, session.id) {
            Ok(c) => c,
            Err(e) => {
                self.store.end_session(session.id)?;
                self.store.set_meeting_status(meeting.id, MeetingStatus::Paused)?;
                return Err(e);
            }
        };
        *guard = Some(Active {
            meeting_id: meeting.id,
            session_id: session.id,
            status: MeetingStatus::Live,
            capture: Some(capture),
        });
        drop(guard);
        self.emit_state();
        Ok(meeting)
    }

    pub fn pause(&self) -> Result<()> {
        let mut guard = self.lock_active();
        let a = guard.as_mut().ok_or_else(|| anyhow!("no active meeting"))?;
        if a.status != MeetingStatus::Live {
            return Err(anyhow!("meeting is not live"));
        }
        if let Some(c) = a.capture.take() {
            c.stop();
        }
        a.status = MeetingStatus::Paused;
        self.store.set_meeting_status(a.meeting_id, MeetingStatus::Paused)?;
        drop(guard);
        self.emit_state();
        Ok(())
    }

    pub fn resume(&self) -> Result<()> {
        if (self.dictation_recording)() {
            return Err(anyhow!("Dictation is recording; stop it before resuming"));
        }
        let mut guard = self.lock_active();
        let a = guard.as_mut().ok_or_else(|| anyhow!("no active meeting"))?;
        if a.status != MeetingStatus::Paused {
            return Err(anyhow!("meeting is not paused"));
        }
        let capture = self.start_capture_and_worker(a.meeting_id, a.session_id)?;
        a.capture = Some(capture);
        a.status = MeetingStatus::Live;
        self.store.set_meeting_status(a.meeting_id, MeetingStatus::Live)?;
        drop(guard);
        self.emit_state();
        Ok(())
    }

    pub fn end(&self) -> Result<()> {
        let mut guard = self.lock_active();
        let a = guard.take().ok_or_else(|| anyhow!("no active meeting"))?;
        if let Some(c) = a.capture {
            c.stop();
        }
        self.store.end_session(a.session_id)?;
        self.store.set_meeting_status(a.meeting_id, MeetingStatus::Ended)?;
        drop(guard);
        self.emit_state();
        Ok(())
    }
}
```

Notes: `resume` reuses the same `session_id` and spawns a fresh worker thread; the previous worker exits when its channel sender (owned by the stopped capture) drops. `capture.stop()` is called while holding the lock; `MeetingCapture::stop` is quick (stop plus close). The test `continue_ended_meeting_opens_second_session` passes `None` for project_id (the `.filter` expression evaluates to `None`); keep it as written.

- [ ] **Step 4: Run tests and check**

Run: `cargo test --lib meeting::session 2>&1 | tail -3` then `cargo check 2>&1 | grep -E "^(warning|error)" | grep -i meeting | head`
Expected: `5 passed`; only dead-code warnings (constructor unused until Task 6).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/meeting/session.rs src-tauri/src/meeting/mod.rs
git commit -m "feat: add MeetingManager with live/paused/ended sessions and a transcriber thread"
```

---

### Task 5: Startup recovery and manager wiring

**Files:**

- Modify: `src-tauri/src/meeting/manager.rs` (expose the shared `Arc<ProjectStore>`)
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**

- Produces: `ProjectManager::store_arc(&self) -> Arc<ProjectStore>`; `MeetingManager` managed as `Arc<MeetingManager>`; at startup `store.mark_live_sessions_paused()` runs once and logs the count; events `MeetingStatePayload` and `TranscriptLinePayload` in `collect_events!`.

- [ ] **Step 1: Add `store_arc`**

In `manager.rs` `impl ProjectManager`:

```rust
    pub fn store_arc(&self) -> Arc<ProjectStore> {
        Arc::clone(&self.store)
    }
```

- [ ] **Step 2: Wire in `lib.rs`**

Right after `app_handle.manage(project_manager)` (added by the previous plan; find `ProjectManager::new`), add:

```rust
    let meeting_store = project_manager.store_arc();
    match meeting_store.mark_live_sessions_paused() {
        Ok(n) if n > 0 => log::info!("recovered {} meeting(s) left live by a previous run", n),
        Ok(_) => {}
        Err(e) => log::error!("meeting recovery failed: {e:#}"),
    }
    let meeting_manager = Arc::new(meeting::session::MeetingManager::new(
        app_handle,
        meeting_store,
        transcription_manager.clone(),
        recording_manager.clone(),
    ));
    app_handle.manage(meeting_manager);
```

Place this where `transcription_manager` and `recording_manager` (both `Arc<..>`) are in scope, which is the same block where `project_manager` is created (see the `HistoryManager::new` neighbourhood). If `project_manager` was moved into `manage`, bind `let project_manager = Arc::new(...)` first and call `app_handle.manage(project_manager.clone())`.

Add to `collect_events![ ... ]`:

```rust
            meeting::session::MeetingStatePayload,
            meeting::session::TranscriptLinePayload,
```

- [ ] **Step 3: Check**

Run: `cargo check 2>&1 | grep -E "^(warning|error)" | grep -i "meeting" | head`
Expected: no errors; at most dead-code warnings for `ingest`.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/meeting/manager.rs src-tauri/src/lib.rs
git commit -m "feat: wire MeetingManager into app state with startup recovery"
```

---

### Task 6: Meeting commands and bindings

**Files:**

- Create: `src-tauri/src/commands/meeting.rs`
- Modify: `src-tauri/src/commands/mod.rs` (`pub mod meeting;`)
- Modify: `src-tauri/src/lib.rs` (`collect_commands!`)
- Regenerate: `src/bindings.ts`

**Interfaces:**

- Produces commands: `list_meetings(project_id: Option<i64>)`, `get_meeting_detail(id)`, `start_meeting(project_id: Option<i64>, meeting_id: Option<i64>) -> Meeting`, `pause_meeting()`, `resume_meeting()`, `end_meeting()`, `meeting_state() -> MeetingStatePayload`, `update_meeting_title(id, title)`, `update_meeting_context(id, context)`, `delete_meeting(id)` (refuses if that meeting is active).

- [ ] **Step 1: Write the commands**

```rust
use crate::meeting::session::{MeetingManager, MeetingStatePayload};
use crate::meeting::store::{Meeting, MeetingDetail};
use std::sync::Arc;
use tauri::State;

type Mm<'a> = State<'a, Arc<MeetingManager>>;

fn err(e: anyhow::Error) -> String {
    format!("{e:#}")
}

#[tauri::command]
#[specta::specta]
pub fn list_meetings(mm: Mm<'_>, project_id: Option<i64>) -> Result<Vec<Meeting>, String> {
    mm.store().list_meetings(project_id).map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn get_meeting_detail(mm: Mm<'_>, id: i64) -> Result<Option<MeetingDetail>, String> {
    mm.store().get_meeting_detail(id).map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn start_meeting(
    mm: Mm<'_>,
    project_id: Option<i64>,
    meeting_id: Option<i64>,
) -> Result<Meeting, String> {
    mm.start(project_id, meeting_id).map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn pause_meeting(mm: Mm<'_>) -> Result<(), String> {
    mm.pause().map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn resume_meeting(mm: Mm<'_>) -> Result<(), String> {
    mm.resume().map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn end_meeting(mm: Mm<'_>) -> Result<(), String> {
    mm.end().map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn meeting_state(mm: Mm<'_>) -> MeetingStatePayload {
    mm.state()
}

#[tauri::command]
#[specta::specta]
pub fn update_meeting_title(mm: Mm<'_>, id: i64, title: String) -> Result<(), String> {
    let t = title.trim();
    if t.is_empty() {
        return Err("Title cannot be empty".into());
    }
    mm.store().update_meeting_title(id, t).map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn update_meeting_context(mm: Mm<'_>, id: i64, context: String) -> Result<(), String> {
    mm.store().update_meeting_context(id, &context).map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn delete_meeting(mm: Mm<'_>, id: i64) -> Result<(), String> {
    if mm.state().meeting_id == Some(id) {
        return Err("End the meeting before deleting it".into());
    }
    mm.store().delete_meeting(id).map_err(err)
}
```

- [ ] **Step 2: Register**

`commands/mod.rs`: add `pub mod meeting;` (alphabetical). `lib.rs` `collect_commands![`: add the ten `commands::meeting::*` entries after the `commands::project::*` block.

- [ ] **Step 3: Regenerate bindings**

Run from `src-tauri/`: `df -h / | tail -1` first; if free space is under 2 GB, stop and report BLOCKED. Then `cargo run -- --list-models > /tmp/listmodels.log 2>&1; tail -2 /tmp/listmodels.log`.
Expected: exit 0. Then from repo root: `grep -c "startMeeting\|getMeetingDetail\|meetingStatePayload\|transcriptLinePayload\|MeetingStatus" src/bindings.ts`
Expected: at least 5.

- [ ] **Step 4: Typecheck**

Run: `bunx tsc --noEmit && echo ok`
Expected: `ok`.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands/meeting.rs src-tauri/src/commands/mod.rs src-tauri/src/lib.rs src/bindings.ts
git commit -m "feat: expose meeting commands and events to the frontend"
```

---

### Task 7: Meeting store and i18n

**Files:**

- Create: `src/stores/meetingStore.ts`
- Modify: `src/i18n/locales/en/translation.json` and all 25 other locale files

**Interfaces:**

- Consumes: `commands.listMeetings`, `getMeetingDetail`, `startMeeting`, `pauseMeeting`, `resumeMeeting`, `endMeeting`, `meetingState`, `updateMeetingTitle`, `updateMeetingContext`, `deleteMeeting`; `events.meetingStatePayload`, `events.transcriptLinePayload`; types `Meeting`, `MeetingDetail`, `TranscriptLine`, `MeetingStatePayload`.
- Produces: `useMeetingStore` with state `{ meetings: Meeting[]; activeMeetingId: number | null; detail: MeetingDetail | null; live: MeetingStatePayload; loading: boolean }` and actions `loadMeetings(projectId?: number | null)`, `openMeeting(id)`, `closeMeeting()`, `startMeeting(projectId: number | null, meetingId: number | null)`, `pause()`, `resume()`, `end()`, `refreshLive()`, `renameMeeting(id, title)`, `saveContext(id, context)`, `deleteMeeting(id)`, `subscribe()`.

- [ ] **Step 1: i18n keys**

In `en/translation.json` add `"meetings": "Meetings"` to `sidebar` right after `"projects"`, and a top-level `meetings` object after `projects`:

```json
  "meetings": {
    "title": "Meetings",
    "description": "Live transcript of your microphone. Start a meeting, speak, and lines appear here.",
    "empty": "No meetings yet. Start one from a project or with the button above.",
    "quickStart": "Start quick meeting",
    "newInProject": "New meeting",
    "continue": "Continue",
    "open": "Open",
    "delete": "Delete meeting",
    "deleteConfirm": "Delete this meeting and its transcript?",
    "back": "All meetings",
    "rename": "Rename",
    "status": {
      "live": "Live",
      "paused": "Paused",
      "ended": "Ended"
    },
    "controls": {
      "pause": "Pause",
      "resume": "Resume",
      "end": "End meeting",
      "listening": "Listening…"
    },
    "context": {
      "title": "Meeting context",
      "description": "Agenda or notes for this meeting only. Sent to the assistant alongside the project brief.",
      "placeholder": "What is this meeting about?"
    },
    "transcript": {
      "title": "Transcript",
      "empty": "Nothing transcribed yet. Say something.",
      "session": "Session {{n}}",
      "me": "You"
    },
    "errors": {
      "load": "Could not load meetings.",
      "start": "Could not start the meeting.",
      "control": "Could not change the meeting state.",
      "save": "Could not save.",
      "delete": "Could not delete the meeting."
    }
  }
```

Copy `sidebar.meetings` and the `meetings` object into all 25 other locales at the same positions with a throwaway script (delete it before committing). `bun run check:translations` must pass.

- [ ] **Step 2: Write the store**

```ts
import { create } from "zustand";
import { toast } from "sonner";
import i18n from "@/i18n";
import {
  commands,
  events,
  type Meeting,
  type MeetingDetail,
  type MeetingStatePayload,
} from "@/bindings";

interface MeetingStore {
  meetings: Meeting[];
  activeMeetingId: number | null;
  detail: MeetingDetail | null;
  live: MeetingStatePayload;
  loading: boolean;

  loadMeetings: (projectId?: number | null) => Promise<void>;
  openMeeting: (id: number) => Promise<void>;
  closeMeeting: () => void;
  startMeeting: (
    projectId: number | null,
    meetingId: number | null,
  ) => Promise<Meeting | null>;
  pause: () => Promise<void>;
  resume: () => Promise<void>;
  end: () => Promise<void>;
  refreshLive: () => Promise<void>;
  renameMeeting: (id: number, title: string) => Promise<void>;
  saveContext: (id: number, context: string) => Promise<void>;
  deleteMeeting: (id: number) => Promise<void>;
  subscribe: () => Promise<() => void>;
}

const t = (key: string) => i18n.t(key);
const idle: MeetingStatePayload = {
  meeting_id: null,
  session_id: null,
  status: null,
};

export const useMeetingStore = create<MeetingStore>((set, get) => ({
  meetings: [],
  activeMeetingId: null,
  detail: null,
  live: idle,
  loading: false,

  loadMeetings: async (projectId = null) => {
    set({ loading: true });
    try {
      const res = await commands.listMeetings(projectId);
      if (res.status === "ok") set({ meetings: res.data });
      else toast.error(t("meetings.errors.load"));
    } finally {
      set({ loading: false });
    }
  },

  openMeeting: async (id) => {
    set({ activeMeetingId: id, detail: null });
    const res = await commands.getMeetingDetail(id);
    if (res.status === "ok" && get().activeMeetingId === id)
      set({ detail: res.data });
  },

  closeMeeting: () => set({ activeMeetingId: null, detail: null }),

  startMeeting: async (projectId, meetingId) => {
    const res = await commands.startMeeting(projectId, meetingId);
    if (res.status !== "ok") {
      toast.error(`${t("meetings.errors.start")} ${res.error}`);
      return null;
    }
    await get().refreshLive();
    await get().openMeeting(res.data.id);
    return res.data;
  },

  pause: async () => {
    const res = await commands.pauseMeeting();
    if (res.status !== "ok") toast.error(t("meetings.errors.control"));
    await get().refreshLive();
  },

  resume: async () => {
    const res = await commands.resumeMeeting();
    if (res.status !== "ok")
      toast.error(`${t("meetings.errors.control")} ${res.error}`);
    await get().refreshLive();
  },

  end: async () => {
    const res = await commands.endMeeting();
    if (res.status !== "ok") toast.error(t("meetings.errors.control"));
    await get().refreshLive();
    const id = get().activeMeetingId;
    if (id !== null) await get().openMeeting(id);
  },

  refreshLive: async () => {
    const live = await commands.meetingState();
    set({ live });
    const id = get().activeMeetingId;
    if (id !== null && live.meeting_id === id && live.status) {
      set((s) =>
        s.detail
          ? {
              detail: {
                ...s.detail,
                meeting: { ...s.detail.meeting, status: live.status! },
              },
            }
          : s,
      );
    }
  },

  renameMeeting: async (id, title) => {
    const res = await commands.updateMeetingTitle(id, title);
    if (res.status !== "ok") {
      toast.error(t("meetings.errors.save"));
      return;
    }
    set((s) => ({
      meetings: s.meetings.map((m) => (m.id === id ? { ...m, title } : m)),
      detail:
        s.detail && s.detail.meeting.id === id
          ? { ...s.detail, meeting: { ...s.detail.meeting, title } }
          : s.detail,
    }));
  },

  saveContext: async (id, context) => {
    const res = await commands.updateMeetingContext(id, context);
    if (res.status !== "ok") toast.error(t("meetings.errors.save"));
  },

  deleteMeeting: async (id) => {
    const res = await commands.deleteMeeting(id);
    if (res.status !== "ok") {
      toast.error(`${t("meetings.errors.delete")} ${res.error}`);
      return;
    }
    set((s) => ({
      meetings: s.meetings.filter((m) => m.id !== id),
      activeMeetingId: s.activeMeetingId === id ? null : s.activeMeetingId,
      detail: s.activeMeetingId === id ? null : s.detail,
    }));
  },

  subscribe: async () => {
    const un1 = await events.meetingStatePayload.listen((e) => {
      set({ live: e.payload });
      const id = get().activeMeetingId;
      if (id !== null && e.payload.meeting_id === id && e.payload.status) {
        set((s) =>
          s.detail
            ? {
                detail: {
                  ...s.detail,
                  meeting: { ...s.detail.meeting, status: e.payload.status! },
                },
              }
            : s,
        );
      }
    });
    const un2 = await events.transcriptLinePayload.listen((e) => {
      set((s) => {
        if (s.activeMeetingId !== e.payload.meeting_id || !s.detail) return s;
        if (s.detail.lines.some((l) => l.id === e.payload.line.id)) return s;
        return {
          detail: { ...s.detail, lines: [...s.detail.lines, e.payload.line] },
        };
      });
    });
    return () => {
      un1();
      un2();
    };
  },
}));
```

Check `src/bindings.ts` for the exact parameter order of `listMeetings` and `startMeeting` and the field name casing on `MeetingStatePayload` before finalising.

- [ ] **Step 3: Verify**

Run: `bunx tsc --noEmit && bun run lint 2>&1 | tail -2 && bun run check:translations 2>&1 | tail -1 && bunx prettier --check src/stores/meetingStore.ts "src/i18n/locales/**/translation.json" 2>&1 | tail -1`
Expected: all clean.

- [ ] **Step 4: Commit**

```bash
git add src/stores/meetingStore.ts src/i18n/locales
git commit -m "feat: add meeting store and i18n strings"
```

---

### Task 8: Meetings UI and project meetings list

**Files:**

- Create: `src/components/meetings/MeetingsSection.tsx`, `MeetingList.tsx`, `MeetingDetail.tsx`, `MeetingControls.tsx`, `TranscriptThread.tsx`, `ContextEditor.tsx`, `ProjectMeetings.tsx`
- Modify: `src/components/projects/ProjectDetail.tsx` (render `ProjectMeetings` under `SourceList`; render `MeetingDetail` instead of the project when a meeting is open from here)
- Modify: `src/components/Sidebar.tsx` (`meetings` entry second, icon `Mic` from lucide)

**Interfaces:**

- Consumes: `useMeetingStore`, `useProjectStore`, `components/ui`.
- Produces: `MeetingsSection` for the sidebar; `ProjectMeetings` for project detail.

- [ ] **Step 1: MeetingControls**

```tsx
import React from "react";
import { useTranslation } from "react-i18next";
import { Pause, Play, Square } from "lucide-react";
import { useMeetingStore } from "@/stores/meetingStore";
import Badge from "../ui/Badge";
import { Button } from "../ui/Button";

export const MeetingControls: React.FC<{ meetingId: number }> = ({
  meetingId,
}) => {
  const { t } = useTranslation();
  const live = useMeetingStore((s) => s.live);
  const pause = useMeetingStore((s) => s.pause);
  const resume = useMeetingStore((s) => s.resume);
  const end = useMeetingStore((s) => s.end);
  const startMeeting = useMeetingStore((s) => s.startMeeting);
  const detail = useMeetingStore((s) => s.detail);

  const isThis = live.meeting_id === meetingId;
  const status = isThis ? live.status : (detail?.meeting.status ?? null);
  const otherRunning = live.meeting_id !== null && !isThis;

  if (!isThis) {
    return (
      <div className="flex items-center gap-2">
        {status && (
          <Badge variant="secondary">{t(`meetings.status.${status}`)}</Badge>
        )}
        <Button
          size="sm"
          disabled={otherRunning}
          onClick={() =>
            startMeeting(detail?.meeting.project_id ?? null, meetingId)
          }
        >
          <Play className="w-4 h-4 inline mr-1" />
          {t("meetings.continue")}
        </Button>
      </div>
    );
  }

  return (
    <div className="flex items-center gap-2">
      <Badge variant={status === "live" ? "success" : "secondary"}>
        {status === "live"
          ? t("meetings.controls.listening")
          : t(`meetings.status.${status ?? "paused"}`)}
      </Badge>
      {status === "live" ? (
        <Button size="sm" variant="secondary" onClick={pause}>
          <Pause className="w-4 h-4 inline mr-1" />
          {t("meetings.controls.pause")}
        </Button>
      ) : (
        <Button size="sm" variant="secondary" onClick={resume}>
          <Play className="w-4 h-4 inline mr-1" />
          {t("meetings.controls.resume")}
        </Button>
      )}
      <Button size="sm" variant="danger" onClick={end}>
        <Square className="w-4 h-4 inline mr-1" />
        {t("meetings.controls.end")}
      </Button>
    </div>
  );
};
```

- [ ] **Step 2: TranscriptThread**

```tsx
import React, { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import type { MeetingDetail } from "@/bindings";
import { SettingsGroup } from "../ui/SettingsGroup";
import { formatDateTime } from "@/utils/dateFormat";

export const TranscriptThread: React.FC<{
  detail: MeetingDetail;
  autoscroll: boolean;
}> = ({ detail, autoscroll }) => {
  const { t, i18n } = useTranslation();
  const endRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (autoscroll) endRef.current?.scrollIntoView({ block: "end" });
  }, [detail.lines.length, autoscroll]);

  const sessionIndex = new Map(detail.sessions.map((s, i) => [s.id, i + 1]));

  return (
    <SettingsGroup title={t("meetings.transcript.title")}>
      {detail.lines.length === 0 ? (
        <p className="p-4 text-sm text-mid-gray">
          {t("meetings.transcript.empty")}
        </p>
      ) : (
        <div className="p-4 space-y-3 max-h-[50vh] overflow-y-auto">
          {detail.sessions.map((s) => {
            const lines = detail.lines.filter((l) => l.session_id === s.id);
            if (lines.length === 0) return null;
            return (
              <div key={s.id} className="space-y-1">
                <div className="text-xs font-medium text-mid-gray uppercase tracking-wide">
                  {t("meetings.transcript.session", {
                    n: sessionIndex.get(s.id),
                  })}
                  {" · "}
                  {formatDateTime(
                    String(Math.floor(s.started_at / 1000)),
                    i18n.language,
                  )}
                </div>
                {lines.map((l) => (
                  <div key={l.id} className="flex gap-2 text-sm">
                    <span className="shrink-0 text-logo-primary font-semibold">
                      {t("meetings.transcript.me")}
                    </span>
                    <span className="text-text">{l.text}</span>
                  </div>
                ))}
              </div>
            );
          })}
          <div ref={endRef} />
        </div>
      )}
    </SettingsGroup>
  );
};
```

- [ ] **Step 3: ContextEditor**

Same shape as `projects/BriefEditor.tsx` (debounced save, sync on id change, flush on unmount) but backed by `useMeetingStore().saveContext` and keys `meetings.context.*`. Copy BriefEditor, rename to `ContextEditor` with props `{ meetingId: number; context: string }`.

- [ ] **Step 4: MeetingDetail**

```tsx
import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowLeft, Pencil } from "lucide-react";
import { useMeetingStore } from "@/stores/meetingStore";
import { Button } from "../ui/Button";
import { Input } from "../ui/Input";
import { ContextEditor } from "./ContextEditor";
import { MeetingControls } from "./MeetingControls";
import { TranscriptThread } from "./TranscriptThread";

export const MeetingDetail: React.FC<{ onBack: () => void }> = ({ onBack }) => {
  const { t } = useTranslation();
  const detail = useMeetingStore((s) => s.detail);
  const live = useMeetingStore((s) => s.live);
  const renameMeeting = useMeetingStore((s) => s.renameMeeting);
  const [editing, setEditing] = useState(false);
  const [title, setTitle] = useState("");

  if (!detail)
    return <p className="p-4 text-sm text-mid-gray">{t("common.loading")}</p>;
  const m = detail.meeting;

  const commit = () => {
    const next = title.trim();
    if (next && next !== m.title) renameMeeting(m.id, next);
    setEditing(false);
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center gap-2">
        <Button
          variant="ghost"
          size="sm"
          onClick={onBack}
          title={t("meetings.back")}
        >
          <ArrowLeft className="w-4 h-4" />
        </Button>
        {editing ? (
          <Input
            variant="compact"
            className="flex-1"
            value={title}
            autoFocus
            onChange={(e) => setTitle(e.target.value)}
            onBlur={commit}
            onKeyDown={(e) => {
              if (e.key === "Enter") commit();
              if (e.key === "Escape") setEditing(false);
            }}
          />
        ) : (
          <h1 className="flex-1 text-lg font-semibold text-text truncate">
            {m.title}
          </h1>
        )}
        <Button
          variant="ghost"
          size="sm"
          title={t("meetings.rename")}
          onClick={() => {
            setTitle(m.title);
            setEditing(true);
          }}
        >
          <Pencil className="w-4 h-4" />
        </Button>
        <MeetingControls meetingId={m.id} />
      </div>
      <ContextEditor meetingId={m.id} context={m.context} />
      <TranscriptThread
        detail={detail}
        autoscroll={live.meeting_id === m.id && live.status === "live"}
      />
    </div>
  );
};
```

- [ ] **Step 5: MeetingList and MeetingsSection**

`MeetingList.tsx`: cards for `meetings` (title, status badge, relative `updated_at` via `formatRelativeTime(String(Math.floor(m.updated_at / 1000)), i18n.language)`), Open button (`openMeeting`), Delete (`ask` confirm then `deleteMeeting`, disabled when `live.meeting_id === m.id`), and a top button "Start quick meeting" calling `startMeeting(null, null)` (disabled when any meeting is live). Empty state `meetings.empty`.

`MeetingsSection.tsx`:

```tsx
import React, { useEffect } from "react";
import { useMeetingStore } from "@/stores/meetingStore";
import { MeetingList } from "./MeetingList";
import { MeetingDetail } from "./MeetingDetail";

export const MeetingsSection: React.FC = () => {
  const activeMeetingId = useMeetingStore((s) => s.activeMeetingId);
  const loadMeetings = useMeetingStore((s) => s.loadMeetings);
  const refreshLive = useMeetingStore((s) => s.refreshLive);
  const subscribe = useMeetingStore((s) => s.subscribe);
  const closeMeeting = useMeetingStore((s) => s.closeMeeting);
  const openMeeting = useMeetingStore((s) => s.openMeeting);

  useEffect(() => {
    loadMeetings(null);
    refreshLive();
    const id = useMeetingStore.getState().activeMeetingId;
    if (id !== null) openMeeting(id);
    const unlisten = subscribe();
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [loadMeetings, refreshLive, subscribe, openMeeting]);

  return activeMeetingId === null ? (
    <MeetingList />
  ) : (
    <MeetingDetail
      onBack={() => {
        closeMeeting();
        loadMeetings(null);
      }}
    />
  );
};
```

- [ ] **Step 6: ProjectMeetings and ProjectDetail**

`ProjectMeetings.tsx`: a `SettingsGroup` titled `meetings.title` listing `meetings` filtered by `project_id === projectId` (call `loadMeetings(projectId)` on mount and after start/end), each row with title, status badge, and Continue/Open; a "New meeting" button (`startMeeting(projectId, null)`, disabled while a meeting is live). It also subscribes to meeting events on mount like `MeetingsSection` so the badges update.

`ProjectDetail.tsx`: import `useMeetingStore` and `MeetingDetail`; if `useMeetingStore((s) => s.activeMeetingId) !== null`, render `<MeetingDetail onBack={() => { closeMeeting(); loadMeetings(project.id); }} />` instead of the project view; otherwise append `<ProjectMeetings projectId={project.id} />` after `<SourceList />`.

- [ ] **Step 7: Sidebar**

In `Sidebar.tsx` add after `projects`:

```ts
  meetings: {
    labelKey: "sidebar.meetings",
    icon: Mic,
    component: MeetingsSection,
    enabled: () => true,
  },
```

with `Mic` imported from `lucide-react` and `MeetingsSection` from `./meetings/MeetingsSection`.

- [ ] **Step 8: Verify**

Run: `bunx tsc --noEmit && bun run lint 2>&1 | tail -2 && bunx prettier --check "src/components/meetings/*.tsx" src/components/projects/ProjectDetail.tsx src/components/Sidebar.tsx 2>&1 | tail -1`
Expected: all clean. Manual smoke (controller runs it; implementer skips if the GUI cannot be driven): Meetings tab second; Start quick meeting; speak; a line appears within a few seconds; Pause stops new lines; Resume continues; End sets Ended; Continue starts session 2; project detail shows the meeting under the project.

- [ ] **Step 9: Commit**

```bash
git add src/components/meetings src/components/projects/ProjectDetail.tsx src/components/Sidebar.tsx
git commit -m "feat: add Meetings section with live transcript and project meetings list"
```

---

### Task 9: Format, clippy, docs

**Files:**

- Modify: `docs/ARCHITECTURE.md`, `docs/superpowers/specs/2026-10-13-meeting-assistant-design.md` (module name note)

- [ ] **Step 1: Format and lint**

`bun run format 2>&1 | tail -1`; revert any unrelated reformat with `git checkout -- <file>`. From `src-tauri/`: `cargo clippy --tests 2>&1 | grep -E "^(warning|error)" -A3 | grep -E "meeting|commands/meeting" | head`; fix anything in scope.

- [ ] **Step 2: Tests**

From `src-tauri/`: `cargo test --lib 2>&1 | grep "test result" | tail -1` (all pass). Repo root: `bunx tsc --noEmit && bun run lint && bun run check:translations`.

- [ ] **Step 3: Docs**

`docs/ARCHITECTURE.md`: in the "Existing (CASE)" backend table add rows for `meeting/segment.rs`, `meeting/capture.rs`, `meeting/session.rs` (`MeetingManager`), `commands/meeting.rs`; remove the planned rows for `meeting/mod.rs` (manager), `meeting/capture.rs`, `meeting/transcriber.rs` and note that `session.rs` holds the state machine and the transcriber thread; store row lists the three new tables; events list adds `meeting-state-payload` and `transcript-line-payload`; frontend existing table adds `stores/meetingStore.ts`, `components/meetings/*`; delivery order marks step 3 done. In the spec's section 3 table, add one line under `mod.rs`: "Implemented as `session.rs` (manager and transcriber thread) plus `segment.rs`; event names are the tauri-specta derived ones."

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "chore: format, clippy, and docs for meetings and transcript"
```
