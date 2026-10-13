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
            tokenize = 'unicode61'
        );",
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

/// Turn free text into a safe FTS5 MATCH expression: each whitespace token
/// becomes a quoted phrase (inner quotes doubled), joined by implicit AND.
/// Raw FTS operators are deliberately not exposed; queries come from users
/// and the model, not from code.
fn fts_query(input: &str) -> String {
    input
        .split_whitespace()
        .map(|tok| format!("\"{}\"", tok.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
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
    pub fn search(&self, project_id: i64, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let q = fts_query(query);
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT c.source_id, s.title, c.path, c.ord,
                    snippet(source_chunks, 3, '[', ']', '…', 24) AS snip
             FROM source_chunks c
             JOIN sources s ON s.id = c.source_id
             WHERE s.project_id = ?1 AND source_chunks MATCH ?2
             ORDER BY bm25(source_chunks)
             LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![project_id, q, limit as i64], |row| {
            Ok(SearchHit {
                source_id: row.get(0)?,
                source_title: row.get(1)?,
                path: row.get(2)?,
                ord: row.get(3)?,
                snippet: row.get(4)?,
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
}
