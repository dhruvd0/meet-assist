//! SQLite store for projects, sources, and searchable chunks.
//! Pure rusqlite; no Tauri types, so it is unit-testable with a temp dir.

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension};
use rusqlite_migration::{Migrations, M};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::PathBuf;

static MIGRATIONS: &[M] = &[
    M::up(
        "CREATE TABLE projects (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        title TEXT NOT NULL,
        brief TEXT NOT NULL DEFAULT '',
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );",
    ),
    M::up(
        "CREATE TABLE sources (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            kind TEXT NOT NULL,
            title TEXT NOT NULL,
            locator TEXT NOT NULL,
            added_at INTEGER NOT NULL,
            indexed_at INTEGER,
            status TEXT NOT NULL DEFAULT 'pending',
            error TEXT,
            digest TEXT
        );
        CREATE INDEX sources_project_idx ON sources(project_id);
        CREATE VIRTUAL TABLE source_chunks USING fts5(
            source_id UNINDEXED,
            path UNINDEXED,
            ord UNINDEXED,
            content,
            tokenize = 'porter unicode61'
        );",
    ),
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
];

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct Project {
    pub id: i64,
    pub title: String,
    pub brief: String,
    pub created_at: i64,
    pub updated_at: i64,
}

pub struct ProjectStore {
    db_path: PathBuf,
}

fn fts_tokens(input: &str) -> Vec<String> {
    input
        .split_whitespace()
        .map(|tok| format!("\"{}\"", tok.replace('"', "\"\"")))
        .collect()
}

/// Turn free text into a safe FTS5 MATCH expression: each whitespace token
/// becomes a quoted phrase (inner quotes doubled), joined by implicit AND.
/// Raw FTS operators are deliberately not exposed; queries come from users
/// and the model, not from code.
fn fts_query(input: &str) -> String {
    fts_tokens(input).join(" ")
}

/// Same sanitization as `fts_query`, but tokens joined with OR. Used as a
/// fallback when the AND form matches nothing.
fn fts_query_any(input: &str) -> String {
    fts_tokens(input).join(" OR ")
}

fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

impl ProjectStore {
    pub fn open(db_path: PathBuf) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let store = Self { db_path };
        let mut conn = store.conn()?;
        let migrations = Migrations::new(MIGRATIONS.to_vec());
        #[cfg(debug_assertions)]
        migrations.validate().expect("Invalid project migrations");
        migrations.to_latest(&mut conn)?;
        // WAL lets a long chunk write run without blocking a brief save.
        // The pragma returns the resulting mode; it is persistent per file.
        let _mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
        Ok(store)
    }

    fn conn(&self) -> Result<Connection> {
        let conn = Connection::open(&self.db_path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        Ok(conn)
    }

    fn map_project(row: &rusqlite::Row<'_>) -> rusqlite::Result<Project> {
        Ok(Project {
            id: row.get("id")?,
            title: row.get("title")?,
            brief: row.get("brief")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
        })
    }

    pub fn create_project(&self, title: &str) -> Result<Project> {
        let conn = self.conn()?;
        let ts = now();
        conn.execute(
            "INSERT INTO projects (title, brief, created_at, updated_at) VALUES (?1, '', ?2, ?2)",
            params![title, ts],
        )?;
        Ok(Project {
            id: conn.last_insert_rowid(),
            title: title.to_string(),
            brief: String::new(),
            created_at: ts,
            updated_at: ts,
        })
    }

    pub fn list_projects(&self) -> Result<Vec<Project>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, title, brief, created_at, updated_at FROM projects ORDER BY updated_at DESC, id DESC",
        )?;
        let rows = stmt.query_map([], Self::map_project)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn get_project(&self, id: i64) -> Result<Option<Project>> {
        let conn = self.conn()?;
        Ok(conn
            .query_row(
                "SELECT id, title, brief, created_at, updated_at FROM projects WHERE id = ?1",
                params![id],
                Self::map_project,
            )
            .optional()?)
    }

    fn touch_project(conn: &Connection, id: i64) -> Result<()> {
        let changed = conn.execute(
            "UPDATE projects SET updated_at = ?1 WHERE id = ?2",
            params![now(), id],
        )?;
        if changed == 0 {
            return Err(anyhow!("project {} not found", id));
        }
        Ok(())
    }

    pub fn update_project_title(&self, id: i64, title: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE projects SET title = ?1 WHERE id = ?2",
            params![title, id],
        )?;
        Self::touch_project(&conn, id)
    }

    pub fn update_project_brief(&self, id: i64, brief: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE projects SET brief = ?1 WHERE id = ?2",
            params![brief, id],
        )?;
        Self::touch_project(&conn, id)
    }

    pub fn delete_project(&self, id: i64) -> Result<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM source_chunks WHERE source_id IN (SELECT id FROM sources WHERE project_id = ?1)",
            params![id],
        )?;
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
        tx.execute("DELETE FROM sources WHERE project_id = ?1", params![id])?;
        tx.execute("DELETE FROM projects WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Text,
    File,
    Folder,
    Link,
    Image,
}

impl SourceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SourceKind::Text => "text",
            SourceKind::File => "file",
            SourceKind::Folder => "folder",
            SourceKind::Link => "link",
            SourceKind::Image => "image",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "text" => Some(Self::Text),
            "file" => Some(Self::File),
            "folder" => Some(Self::Folder),
            "link" => Some(Self::Link),
            "image" => Some(Self::Image),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum SourceStatus {
    Pending,
    Ready,
    Failed,
}

impl SourceStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SourceStatus::Pending => "pending",
            SourceStatus::Ready => "ready",
            SourceStatus::Failed => "failed",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "ready" => Some(Self::Ready),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct Source {
    pub id: i64,
    pub project_id: i64,
    pub kind: SourceKind,
    pub title: String,
    pub locator: String,
    pub added_at: i64,
    pub indexed_at: Option<i64>,
    pub status: SourceStatus,
    pub error: Option<String>,
    pub digest: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct SearchHit {
    pub source_id: i64,
    pub source_title: String,
    pub path: String,
    pub ord: i64,
    pub snippet: String,
    /// Raw SQLite FTS5 `bm25(source_chunks)` value. Lower (more negative)
    /// is a better match; hits are returned best first.
    pub score: f64,
}

const SOURCE_COLS: &str =
    "id, project_id, kind, title, locator, added_at, indexed_at, status, error, digest";

impl ProjectStore {
    fn map_source(row: &rusqlite::Row<'_>) -> rusqlite::Result<Source> {
        let kind: String = row.get("kind")?;
        let status: String = row.get("status")?;
        Ok(Source {
            id: row.get("id")?,
            project_id: row.get("project_id")?,
            kind: SourceKind::parse(&kind).unwrap_or(SourceKind::Text),
            title: row.get("title")?,
            locator: row.get("locator")?,
            added_at: row.get("added_at")?,
            indexed_at: row.get("indexed_at")?,
            status: SourceStatus::parse(&status).unwrap_or(SourceStatus::Failed),
            error: row.get("error")?,
            digest: row.get("digest")?,
        })
    }

    pub fn add_source(
        &self,
        project_id: i64,
        kind: SourceKind,
        title: &str,
        locator: &str,
    ) -> Result<Source> {
        let conn = self.conn()?;
        let ts = now();
        conn.execute(
            "INSERT INTO sources (project_id, kind, title, locator, added_at, status)
             VALUES (?1, ?2, ?3, ?4, ?5, 'pending')",
            params![project_id, kind.as_str(), title, locator, ts],
        )?;
        let id = conn.last_insert_rowid();
        Self::touch_project(&conn, project_id)?;
        Ok(Source {
            id,
            project_id,
            kind,
            title: title.to_string(),
            locator: locator.to_string(),
            added_at: ts,
            indexed_at: None,
            status: SourceStatus::Pending,
            error: None,
            digest: None,
        })
    }

    /// Ids of every source still `pending`, oldest first. Used at startup
    /// to resume indexing that was interrupted by a quit or crash.
    pub fn list_pending_source_ids(&self) -> Result<Vec<i64>> {
        let conn = self.conn()?;
        let mut stmt =
            conn.prepare("SELECT id FROM sources WHERE status = 'pending' ORDER BY id ASC")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<i64>>>()?)
    }

    pub fn list_sources(&self, project_id: i64) -> Result<Vec<Source>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {SOURCE_COLS} FROM sources WHERE project_id = ?1 ORDER BY added_at ASC, id ASC"
        ))?;
        let rows = stmt.query_map(params![project_id], Self::map_source)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn get_source(&self, id: i64) -> Result<Option<Source>> {
        let conn = self.conn()?;
        Ok(conn
            .query_row(
                &format!("SELECT {SOURCE_COLS} FROM sources WHERE id = ?1"),
                params![id],
                Self::map_source,
            )
            .optional()?)
    }

    pub fn set_source_status(
        &self,
        id: i64,
        status: SourceStatus,
        error: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn()?;
        let indexed_at = if status == SourceStatus::Ready {
            Some(now())
        } else {
            None
        };
        conn.execute(
            "UPDATE sources SET status = ?1, error = ?2,
             indexed_at = COALESCE(?3, indexed_at) WHERE id = ?4",
            params![status.as_str(), error, indexed_at, id],
        )?;
        Ok(())
    }

    pub fn set_source_digest(&self, id: i64, digest: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE sources SET digest = ?1 WHERE id = ?2",
            params![digest, id],
        )?;
        Ok(())
    }

    pub fn replace_chunks(&self, source_id: i64, chunks: &[(String, String)]) -> Result<usize> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        // The source may have been removed while it was being indexed;
        // inserting then would leave orphan FTS rows nothing ever deletes.
        let exists = tx
            .query_row(
                "SELECT 1 FROM sources WHERE id = ?1",
                params![source_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            return Ok(0);
        }
        tx.execute(
            "DELETE FROM source_chunks WHERE source_id = ?1",
            params![source_id],
        )?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO source_chunks (source_id, path, ord, content) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for (ord, (path, content)) in chunks.iter().enumerate() {
                stmt.execute(params![source_id, path, ord as i64, content])?;
            }
        }
        tx.commit()?;
        Ok(chunks.len())
    }

    /// Full-text search scoped to one project. The query is sanitized via
    /// `fts_query`, so any input is valid FTS5 and real errors propagate.
    /// All tokens must match first; if that finds nothing and the query has
    /// more than one token, any-token (OR) matches are returned instead.
    /// Terms are porter-stemmed by the tokenizer ("retries" matches "retry").
    pub fn search(&self, project_id: i64, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let q = fts_query(query);
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.conn()?;
        let hits = Self::run_search(&conn, project_id, &q, limit)?;
        if hits.is_empty() && query.split_whitespace().nth(1).is_some() {
            return Self::run_search(&conn, project_id, &fts_query_any(query), limit);
        }
        Ok(hits)
    }

    fn run_search(
        conn: &Connection,
        project_id: i64,
        q: &str,
        limit: usize,
    ) -> Result<Vec<SearchHit>> {
        let mut stmt = conn.prepare(
            "SELECT c.source_id, s.title, c.path, c.ord,
                    snippet(source_chunks, 3, '[', ']', '…', 24) AS snip,
                    bm25(source_chunks) AS score
             FROM source_chunks c
             JOIN sources s ON s.id = c.source_id
             WHERE s.project_id = ?1 AND source_chunks MATCH ?2
             ORDER BY score
             LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![project_id, q, limit as i64], |row| {
            Ok(SearchHit {
                source_id: row.get(0)?,
                source_title: row.get(1)?,
                path: row.get(2)?,
                ord: row.get(3)?,
                snippet: row.get(4)?,
                score: row.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Not yet called outside tests; reserved for the tool-calling step.
    #[allow(dead_code)]
    pub fn read_chunks(
        &self,
        source_id: i64,
        path: Option<&str>,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<(String, i64, String)>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT path, ord, content FROM source_chunks
             WHERE source_id = ?1 AND (?2 IS NULL OR path = ?2)
             ORDER BY ord LIMIT ?3 OFFSET ?4",
        )?;
        let rows = stmt.query_map(
            params![source_id, path, limit as i64, offset as i64],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn update_source_locator(&self, id: i64, locator: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE sources SET locator = ?1 WHERE id = ?2",
            params![locator, id],
        )?;
        Ok(())
    }

    pub fn remove_source(&self, id: i64) -> Result<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM source_chunks WHERE source_id = ?1",
            params![id],
        )?;
        tx.execute("DELETE FROM sources WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(())
    }
}

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

impl ProjectStore {
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
        Ok(Some(MeetingDetail {
            meeting,
            sessions,
            lines,
        }))
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
        conn.execute(
            "UPDATE meetings SET title = ?1 WHERE id = ?2",
            params![title, id],
        )?;
        Self::touch_meeting(&conn, id)
    }

    pub fn update_meeting_context(&self, id: i64, context: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE meetings SET context = ?1 WHERE id = ?2",
            params![context, id],
        )?;
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
        Ok(Session {
            id,
            meeting_id,
            started_at: ts,
            ended_at: None,
            summary: None,
        })
    }

    pub fn end_session(&self, session_id: i64) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE sessions SET ended_at = ?1 WHERE id = ?2 AND ended_at IS NULL",
            params![now(), session_id],
        )?;
        Ok(())
    }

    pub fn append_line(
        &self,
        session_id: i64,
        speaker: &str,
        text: &str,
    ) -> Result<TranscriptLine> {
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
        Ok(TranscriptLine {
            id,
            session_id,
            ts,
            speaker: speaker.to_string(),
            text: text.to_string(),
        })
    }

    pub fn mark_live_sessions_paused(&self) -> Result<usize> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        let ts = now();
        // A crash or quit leaves open sessions behind; close them so they do
        // not stay open forever.
        tx.execute(
            "UPDATE sessions SET ended_at = ?1 WHERE ended_at IS NULL AND meeting_id IN (SELECT id FROM meetings WHERE status = 'live')",
            params![ts],
        )?;
        let flipped = tx.execute(
            "UPDATE meetings SET status = 'paused', updated_at = ?1 WHERE status = 'live'",
            params![ts],
        )?;
        tx.commit()?;
        Ok(flipped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, ProjectStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(dir.path().join("meet.db")).unwrap();
        (dir, store)
    }

    #[test]
    fn create_and_list_projects_newest_first() {
        let (_d, s) = store();
        let a = s.create_project("Alpha").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let b = s.create_project("Beta").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        s.update_project_brief(a.id, "touched").unwrap();
        let list = s.list_projects().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, a.id, "updated project sorts first");
        assert_eq!(list[1].id, b.id);
        assert_eq!(list[0].brief, "touched");
        assert!(list[0].updated_at > list[1].updated_at);
    }

    #[test]
    fn get_missing_project_is_none() {
        let (_d, s) = store();
        assert!(s.get_project(999).unwrap().is_none());
    }

    #[test]
    fn update_title_and_delete() {
        let (_d, s) = store();
        let p = s.create_project("Old").unwrap();
        s.update_project_title(p.id, "New").unwrap();
        assert_eq!(s.get_project(p.id).unwrap().unwrap().title, "New");
        s.delete_project(p.id).unwrap();
        assert!(s.get_project(p.id).unwrap().is_none());
    }

    #[test]
    fn add_source_starts_pending_and_lists_in_order() {
        let (_d, s) = store();
        let p = s.create_project("P").unwrap();
        let a = s.add_source(p.id, SourceKind::Text, "Notes", "").unwrap();
        let b = s
            .add_source(p.id, SourceKind::Link, "Docs", "https://example.com")
            .unwrap();
        assert_eq!(a.status, SourceStatus::Pending);
        let list = s.list_sources(p.id).unwrap();
        assert_eq!(
            list.iter().map(|x| x.id).collect::<Vec<_>>(),
            vec![a.id, b.id]
        );
    }

    #[test]
    fn status_ready_sets_indexed_at_and_failed_keeps_error() {
        let (_d, s) = store();
        let p = s.create_project("P").unwrap();
        let src = s.add_source(p.id, SourceKind::Text, "N", "").unwrap();
        s.set_source_status(src.id, SourceStatus::Ready, None)
            .unwrap();
        let got = s.get_source(src.id).unwrap().unwrap();
        assert!(got.indexed_at.is_some());
        s.set_source_status(src.id, SourceStatus::Failed, Some("boom"))
            .unwrap();
        let got = s.get_source(src.id).unwrap().unwrap();
        assert_eq!(got.error.as_deref(), Some("boom"));
    }

    #[test]
    fn replace_chunks_replaces_not_appends() {
        let (_d, s) = store();
        let p = s.create_project("P").unwrap();
        let src = s.add_source(p.id, SourceKind::Text, "N", "").unwrap();
        s.replace_chunks(
            src.id,
            &[
                ("a".into(), "alpha one".into()),
                ("a".into(), "alpha two".into()),
            ],
        )
        .unwrap();
        s.replace_chunks(src.id, &[("a".into(), "alpha three".into())])
            .unwrap();
        let chunks = s.read_chunks(src.id, None, 0, 50).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].2, "alpha three");
    }

    #[test]
    fn search_finds_chunk_and_scopes_to_project() {
        let (_d, s) = store();
        let p1 = s.create_project("P1").unwrap();
        let p2 = s.create_project("P2").unwrap();
        let s1 = s.add_source(p1.id, SourceKind::Text, "Spec", "").unwrap();
        let s2 = s.add_source(p2.id, SourceKind::Text, "Other", "").unwrap();
        s.replace_chunks(
            s1.id,
            &[(
                "spec.md".into(),
                "The retry budget is five attempts.".into(),
            )],
        )
        .unwrap();
        s.replace_chunks(s2.id, &[("x".into(), "retry budget elsewhere".into())])
            .unwrap();
        let hits = s.search(p1.id, "retry budget", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].source_id, s1.id);
        assert_eq!(hits[0].path, "spec.md");
        assert!(hits[0].snippet.contains("retry"));
    }

    #[test]
    fn search_with_bad_fts_syntax_returns_empty_not_error() {
        let (_d, s) = store();
        let p = s.create_project("P").unwrap();
        let hits = s.search(p.id, "\"unbalanced", 10).unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn delete_project_cascades_sources_and_chunks() {
        let (_d, s) = store();
        let p = s.create_project("P").unwrap();
        let src = s.add_source(p.id, SourceKind::Text, "N", "").unwrap();
        s.replace_chunks(src.id, &[("a".into(), "needle".into())])
            .unwrap();
        s.delete_project(p.id).unwrap();
        assert!(s.get_source(src.id).unwrap().is_none());
        assert!(s.read_chunks(src.id, None, 0, 10).unwrap().is_empty());
    }

    #[test]
    fn remove_source_removes_chunks() {
        let (_d, s) = store();
        let p = s.create_project("P").unwrap();
        let src = s.add_source(p.id, SourceKind::Text, "N", "").unwrap();
        s.replace_chunks(src.id, &[("a".into(), "needle".into())])
            .unwrap();
        s.remove_source(src.id).unwrap();
        assert!(s.search(p.id, "needle", 10).unwrap().is_empty());
        assert!(s.read_chunks(src.id, None, 0, 10).unwrap().is_empty());
    }

    #[test]
    fn replace_chunks_after_remove_inserts_nothing() {
        let (_d, s) = store();
        let p = s.create_project("P").unwrap();
        let src = s.add_source(p.id, SourceKind::Text, "N", "").unwrap();
        s.replace_chunks(src.id, &[("a".into(), "needle".into())])
            .unwrap();
        s.remove_source(src.id).unwrap();
        let n = s
            .replace_chunks(src.id, &[("a".into(), "late needle".into())])
            .unwrap();
        assert_eq!(n, 0);
        assert!(s.read_chunks(src.id, None, 0, 10).unwrap().is_empty());
    }

    #[test]
    fn search_matches_stemmed_terms() {
        let (_d, s) = store();
        let p = s.create_project("P").unwrap();
        let src = s.add_source(p.id, SourceKind::Text, "N", "").unwrap();
        s.replace_chunks(src.id, &[("a".into(), "we retry once then give up".into())])
            .unwrap();
        let hits = s.search(p.id, "retries", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].score.is_finite());
    }

    #[test]
    fn search_falls_back_to_or_when_and_misses() {
        let (_d, s) = store();
        let p = s.create_project("P").unwrap();
        let src = s.add_source(p.id, SourceKind::Text, "Spec", "").unwrap();
        s.replace_chunks(
            src.id,
            &[(
                "spec.md".into(),
                "The retry budget is five attempts.".into(),
            )],
        )
        .unwrap();
        let hits = s.search(p.id, "budget elephant", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].source_id, src.id);
        assert!(s.search(p.id, "elephant", 10).unwrap().is_empty());
    }

    #[test]
    fn open_enables_wal() {
        let (_d, s) = store();
        let conn = s.conn().unwrap();
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode.to_ascii_lowercase(), "wal");
    }

    #[test]
    fn search_handles_punctuation_tokens_without_error() {
        let (_d, s) = store();
        let p = s.create_project("P").unwrap();
        let src = s.add_source(p.id, SourceKind::Text, "N", "").unwrap();
        s.replace_chunks(
            src.id,
            &[(
                "a".into(),
                "schedule a follow-up with e.g. the C++ team".into(),
            )],
        )
        .unwrap();
        assert_eq!(s.search(p.id, "follow-up", 10).unwrap().len(), 1);
        assert_eq!(s.search(p.id, "foo:bar", 10).unwrap().len(), 0);
        assert_eq!(s.search(p.id, "   ", 10).unwrap().len(), 0);
    }

    #[test]
    fn create_meeting_is_live_and_listed_by_project() {
        let (_d, s) = store();
        let p = s.create_project("P").unwrap();
        let m = s.create_meeting(Some(p.id), "Standup").unwrap();
        assert_eq!(m.status, MeetingStatus::Live);
        let q = s.create_meeting(None, "Quick").unwrap();
        assert_eq!(
            s.list_meetings(Some(p.id))
                .unwrap()
                .iter()
                .map(|x| x.id)
                .collect::<Vec<_>>(),
            vec![m.id]
        );
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
        assert_eq!(
            d.lines.iter().map(|l| l.id).collect::<Vec<_>>(),
            vec![a.id, b.id]
        );
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
        s.set_meeting_status(ended.id, MeetingStatus::Ended)
            .unwrap();
        let live_session = s.create_session(live.id).unwrap();
        let ended_session = s.create_session(ended.id).unwrap();
        assert_eq!(s.mark_live_sessions_paused().unwrap(), 1);
        let detail = s.get_meeting_detail(live.id).unwrap().unwrap();
        let ls = detail
            .sessions
            .iter()
            .find(|x| x.id == live_session.id)
            .unwrap();
        assert!(ls.ended_at.is_some(), "open session of live meeting closed");
        let detail = s.get_meeting_detail(ended.id).unwrap().unwrap();
        let es = detail
            .sessions
            .iter()
            .find(|x| x.id == ended_session.id)
            .unwrap();
        assert!(
            es.ended_at.is_none(),
            "session of non-live meeting untouched"
        );
        assert_eq!(
            s.get_meeting(live.id).unwrap().unwrap().status,
            MeetingStatus::Paused
        );
        assert_eq!(
            s.get_meeting(ended.id).unwrap().unwrap().status,
            MeetingStatus::Ended
        );
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
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM transcript_lines", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }
}
