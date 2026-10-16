# Projects and Sources Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Users can create a project, write a brief, add sources (pasted text, local file, local folder, link, image), see each source index in the background, and search the indexed text. No LLM, no meetings yet.

**Architecture:** New `src-tauri/src/meeting/` module with a pure-rusqlite `ProjectStore` (migrations, CRUD, FTS5 search), a `chunk` helper, an `indexer` that turns each source kind into chunks, and a `ProjectManager` Tauri state that runs indexing on a thread and emits `project:source-status`. Tauri commands in `commands/project.rs`. Frontend: Zustand `projectStore`, `components/projects/` screens, "Projects" as first sidebar section. Reuses `components/ui`, `rusqlite_migration`, `tauri-plugin-dialog`, `reqwest`.

**Tech Stack:** Rust (rusqlite bundled with FTS5, rusqlite_migration, `ignore` crate for gitignore walks, `html2text`, reqwest, tauri-specta), React 18, Zustand 5, i18next, Tailwind.

**Spec:** `docs/superpowers/specs/2026-10-13-meeting-assistant-design.md` sections 2a, 3 (store, indexer, commands/project.rs), 4 (projectStore, components/projects), 6.

## Global Constraints

- Toolchain on this machine, before every cargo or bun command:
  ```bash
  export PATH="$HOME/.cargo/bin:$HOME/.bun/bin:$PATH"
  export CMAKE_POLICY_VERSION_MINIMUM=3.5
  export SDKROOT=/Users/dhruv/Desktop/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX26.5.sdk
  ```
  `SDKROOT` is required: without it the linker picks the Command Line Tools 27.0 SDK whose `libSystem.tbd` this Xcode's `ld` rejects (`unknown architecture arm64e.x1`). Run cargo from `src-tauri/`.
- Rust tests: `cargo test --lib meeting::` from `src-tauri/`. Unit tests use `tempfile::tempdir()` (add `tempfile` as a dev-dependency in Task 1).
- Regenerate `src/bindings.ts` with `cargo run -- --list-models` from `src-tauri/` (debug build exports bindings before the GUI starts). Never hand-edit `bindings.ts`.
- Frontend checks: `bun run lint` and `bunx tsc --noEmit` from repo root. Both must exit 0.
- All user-facing strings go through i18next under `projects.*` and `sidebar.projects` in `src/i18n/locales/en/translation.json`. ESLint `i18next/no-literal-string` enforces this.
- Reuse `components/ui` primitives (`Button`, `Input`, `Textarea`, `Dialog`, `Badge`, `SettingsGroup`). No new primitives. Keep the existing design language.
- Folder indexing: respect `.gitignore`, skip files over 524288 bytes, skip non-UTF-8 files, chunk at 1500 characters.
- Source status values are exactly `pending`, `ready`, `failed`.
- Image digests are user captions in this plan. Model-written digests are Plan 6.
- Commit after every task with a conventional prefix. No `unwrap` in non-test production paths.

## Review Focus

1. A folder source pointed at a large repo (node_modules present, no .gitignore) should finish indexing in bounded time and skip binaries rather than hang or error. Pinned in Task 5 (binary skip test, size cap test).
2. A link that returns non-HTML (PDF, JSON) or a 404 should mark the source `failed` with a readable error, not crash the indexer thread. Pinned in Task 6.
3. Deleting a project must remove its sources, chunks, and copied image files, or the FTS table and app-data directory leak. Pinned in Task 3 (cascade test) and Task 7 (image file removal).
4. Re-indexing a source must replace its chunks, not append, or search returns duplicates. Pinned in Task 3.
5. Two sources added back to back must both reach `ready`; the indexing thread must not drop the second job. Pinned in Task 7.

---

### Task 1: Dependencies and module skeleton

**Files:**

- Modify: `src-tauri/Cargo.toml`
- Create: `src-tauri/src/meeting/mod.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod meeting;` next to the other `mod` lines near the top)

**Interfaces:**

- Produces: crate modules `meeting::store`, `meeting::chunk`, `meeting::indexer`, `meeting::manager` (files created in later tasks; declare them as they appear).

- [ ] **Step 1: Add dependencies**

Run from `src-tauri/`:

```bash
cargo add ignore@0.4 html2text@0.15
cargo add --dev tempfile@3
```

Expected: `Cargo.toml` gains `ignore`, `html2text` under `[dependencies]` and `tempfile` under `[dev-dependencies]`. If `html2text@0.15` does not resolve, use `cargo add html2text` for the latest and note the version in the commit message.

- [ ] **Step 2: Create module file**

`src-tauri/src/meeting/mod.rs`:

```rust
//! Meeting assistant subsystem: projects, sources, and (later) meetings.
//! See docs/ARCHITECTURE.md "Planned (meeting subsystem)".

pub mod chunk;
pub mod indexer;
pub mod manager;
pub mod store;
```

Create empty placeholders so the crate compiles now; later tasks fill them:

`src-tauri/src/meeting/chunk.rs`, `indexer.rs`, `manager.rs`, `store.rs` each containing only:

```rust
// Filled in by a later task of docs/superpowers/plans/2026-10-13-projects-and-sources.md
```

- [ ] **Step 3: Register module in lib.rs**

Find the block of `mod` declarations in `src-tauri/src/lib.rs` (e.g. `mod managers;`) and add:

```rust
mod meeting;
```

- [ ] **Step 4: Verify it compiles**

Run from `src-tauri/`: `cargo check 2>&1 | tail -3`
Expected: `Finished` with no errors (warnings about unused modules are fine).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/meeting src-tauri/src/lib.rs
git commit -m "chore: add meeting module skeleton and indexing dependencies"
```

---

### Task 2: ProjectStore with projects table

**Files:**

- Create: `src-tauri/src/meeting/store.rs` (replace placeholder)

**Interfaces:**

- Produces:
  - `pub struct ProjectStore { db_path: PathBuf }`, `ProjectStore::open(db_path: PathBuf) -> anyhow::Result<ProjectStore>` (creates parent dir, runs migrations)
  - `pub struct Project { pub id: i64, pub title: String, pub brief: String, pub created_at: i64, pub updated_at: i64 }` deriving `Clone, Debug, Serialize, Deserialize, specta::Type`
  - `create_project(&self, title: &str) -> Result<Project>`
  - `list_projects(&self) -> Result<Vec<Project>>` ordered by `updated_at DESC`
  - `get_project(&self, id: i64) -> Result<Option<Project>>`
  - `update_project_title(&self, id: i64, title: &str) -> Result<()>`
  - `update_project_brief(&self, id: i64, brief: &str) -> Result<()>`
  - `delete_project(&self, id: i64) -> Result<()>`

- [ ] **Step 1: Write the failing tests**

Append to `src-tauri/src/meeting/store.rs`:

```rust
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
        s.update_project_brief(a.id, "touched").unwrap();
        let list = s.list_projects().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, a.id, "updated project sorts first");
        assert_eq!(list[1].id, b.id);
        assert_eq!(list[0].brief, "touched");
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
}
```

- [ ] **Step 2: Run to verify failure**

Run from `src-tauri/`: `cargo test --lib meeting::store 2>&1 | tail -5`
Expected: compile error, `ProjectStore` not found.

- [ ] **Step 3: Implement**

Replace the placeholder comment at the top of `store.rs` with:

```rust
//! SQLite store for projects, sources, and searchable chunks.
//! Pure rusqlite; no Tauri types, so it is unit-testable with a temp dir.

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension};
use rusqlite_migration::{Migrations, M};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::PathBuf;

static MIGRATIONS: &[M] = &[M::up(
    "CREATE TABLE projects (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        title TEXT NOT NULL,
        brief TEXT NOT NULL DEFAULT '',
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );",
)];

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
        let conn = self.conn()?;
        conn.execute("DELETE FROM projects WHERE id = ?1", params![id])?;
        Ok(())
    }
}
```

- [ ] **Step 4: Run tests**

Run from `src-tauri/`: `cargo test --lib meeting::store 2>&1 | tail -5`
Expected: `test result: ok. 3 passed`.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/meeting/store.rs
git commit -m "feat: add ProjectStore with projects table and migrations"
```

---

### Task 3: Sources and FTS5 chunks in ProjectStore

**Files:**

- Modify: `src-tauri/src/meeting/store.rs`

**Interfaces:**

- Produces:
  - `pub enum SourceKind { Text, File, Folder, Link, Image }` with `#[serde(rename_all = "lowercase")]`, deriving `Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Type`; `SourceKind::as_str(&self) -> &'static str`, `SourceKind::parse(&str) -> Option<SourceKind>`
  - `pub enum SourceStatus { Pending, Ready, Failed }` same derives and serde; `as_str`, `parse`
  - `pub struct Source { pub id: i64, pub project_id: i64, pub kind: SourceKind, pub title: String, pub locator: String, pub added_at: i64, pub indexed_at: Option<i64>, pub status: SourceStatus, pub error: Option<String>, pub digest: Option<String> }`
  - `pub struct SearchHit { pub source_id: i64, pub source_title: String, pub path: String, pub ord: i64, pub snippet: String }`
  - `add_source(&self, project_id, kind, title, locator) -> Result<Source>` (status `pending`)
  - `list_sources(&self, project_id) -> Result<Vec<Source>>` ordered by `added_at ASC`
  - `get_source(&self, id) -> Result<Option<Source>>`
  - `set_source_status(&self, id, status, error: Option<&str>) -> Result<()>` (sets `indexed_at` when `ready`)
  - `set_source_digest(&self, id, digest: &str) -> Result<()>`
  - `replace_chunks(&self, source_id, chunks: &[(String, String)]) -> Result<usize>` where each tuple is `(path, content)`; deletes old chunks first, assigns `ord` by position
  - `search(&self, project_id, query: &str, limit: usize) -> Result<Vec<SearchHit>>` FTS5 `MATCH` with `snippet()`
  - `read_chunks(&self, source_id, path: Option<&str>, offset: usize, limit: usize) -> Result<Vec<(String, i64, String)>>` returning `(path, ord, content)`
  - `remove_source(&self, id) -> Result<()>`
  - `delete_project` now cascades to sources and chunks

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `store.rs`:

```rust
    #[test]
    fn add_source_starts_pending_and_lists_in_order() {
        let (_d, s) = store();
        let p = s.create_project("P").unwrap();
        let a = s.add_source(p.id, SourceKind::Text, "Notes", "").unwrap();
        let b = s.add_source(p.id, SourceKind::Link, "Docs", "https://example.com").unwrap();
        assert_eq!(a.status, SourceStatus::Pending);
        let list = s.list_sources(p.id).unwrap();
        assert_eq!(list.iter().map(|x| x.id).collect::<Vec<_>>(), vec![a.id, b.id]);
    }

    #[test]
    fn status_ready_sets_indexed_at_and_failed_keeps_error() {
        let (_d, s) = store();
        let p = s.create_project("P").unwrap();
        let src = s.add_source(p.id, SourceKind::Text, "N", "").unwrap();
        s.set_source_status(src.id, SourceStatus::Ready, None).unwrap();
        let got = s.get_source(src.id).unwrap().unwrap();
        assert!(got.indexed_at.is_some());
        s.set_source_status(src.id, SourceStatus::Failed, Some("boom")).unwrap();
        let got = s.get_source(src.id).unwrap().unwrap();
        assert_eq!(got.error.as_deref(), Some("boom"));
    }

    #[test]
    fn replace_chunks_replaces_not_appends() {
        let (_d, s) = store();
        let p = s.create_project("P").unwrap();
        let src = s.add_source(p.id, SourceKind::Text, "N", "").unwrap();
        s.replace_chunks(src.id, &[("a".into(), "alpha one".into()), ("a".into(), "alpha two".into())]).unwrap();
        s.replace_chunks(src.id, &[("a".into(), "alpha three".into())]).unwrap();
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
        s.replace_chunks(s1.id, &[("spec.md".into(), "The retry budget is five attempts.".into())]).unwrap();
        s.replace_chunks(s2.id, &[("x".into(), "retry budget elsewhere".into())]).unwrap();
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
        s.replace_chunks(src.id, &[("a".into(), "needle".into())]).unwrap();
        s.delete_project(p.id).unwrap();
        assert!(s.get_source(src.id).unwrap().is_none());
        assert!(s.read_chunks(src.id, None, 0, 10).unwrap().is_empty());
    }

    #[test]
    fn remove_source_removes_chunks() {
        let (_d, s) = store();
        let p = s.create_project("P").unwrap();
        let src = s.add_source(p.id, SourceKind::Text, "N", "").unwrap();
        s.replace_chunks(src.id, &[("a".into(), "needle".into())]).unwrap();
        s.remove_source(src.id).unwrap();
        assert!(s.search(p.id, "needle", 10).unwrap().is_empty());
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib meeting::store 2>&1 | tail -5`
Expected: compile errors for `SourceKind`, `add_source`, etc.

- [ ] **Step 3: Implement**

Add a second migration to `MIGRATIONS` (keep the first unchanged):

```rust
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
```

Add types and methods:

```rust
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
        let indexed_at = if status == SourceStatus::Ready { Some(now()) } else { None };
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

    /// Full-text search scoped to one project. Query syntax errors yield an
    /// empty result instead of an error so a stray quote from the model or
    /// user never surfaces as a failure.
    pub fn search(&self, project_id: i64, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
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
        let rows = stmt.query_map(params![project_id, query, limit as i64], |row| {
            Ok(SearchHit {
                source_id: row.get(0)?,
                source_title: row.get(1)?,
                path: row.get(2)?,
                ord: row.get(3)?,
                snippet: row.get(4)?,
            })
        });
        match rows {
            Ok(rows) => Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?),
            Err(rusqlite::Error::SqliteFailure(_, Some(msg))) if msg.contains("fts5") => Ok(Vec::new()),
            Err(e) => Err(e.into()),
        }
    }

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

    pub fn remove_source(&self, id: i64) -> Result<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM source_chunks WHERE source_id = ?1", params![id])?;
        tx.execute("DELETE FROM sources WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(())
    }
}
```

Replace `delete_project` so chunks are removed too (FTS tables do not honour foreign keys):

```rust
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
```

Note on the bad-syntax test: if the FTS error message does not contain `fts5` on this SQLite build, run the test, read the actual message, and match on it (e.g. `"syntax error"`); record the string in the commit message.

- [ ] **Step 4: Run tests**

Run: `cargo test --lib meeting::store 2>&1 | tail -5`
Expected: `test result: ok. 10 passed`.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/meeting/store.rs
git commit -m "feat: add sources and FTS5 chunk search to ProjectStore"
```

---

### Task 4: Chunking helper

**Files:**

- Create: `src-tauri/src/meeting/chunk.rs` (replace placeholder)

**Interfaces:**

- Produces: `pub const MAX_CHUNK_CHARS: usize = 1500;` and `pub fn chunk_text(text: &str, max_chars: usize) -> Vec<String>`. Splits on blank lines first, then single newlines, then hard-splits at `max_chars` on a char boundary. Trims each chunk; drops empty chunks.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_is_one_chunk() {
        assert_eq!(chunk_text("hello world", 100), vec!["hello world"]);
    }

    #[test]
    fn splits_on_paragraphs_under_limit() {
        let text = "para one\n\npara two\n\npara three";
        let chunks = chunk_text(text, 20);
        assert_eq!(chunks, vec!["para one", "para two", "para three"]);
    }

    #[test]
    fn packs_paragraphs_until_limit() {
        let text = "aaaa\n\nbbbb\n\ncccc";
        let chunks = chunk_text(text, 11);
        assert_eq!(chunks, vec!["aaaa\n\nbbbb", "cccc"]);
    }

    #[test]
    fn hard_splits_long_paragraph_on_char_boundary() {
        let text = "é".repeat(10);
        let chunks = chunk_text(&text, 4);
        assert_eq!(chunks.len(), 3);
        assert!(chunks.iter().all(|c| c.chars().count() <= 4));
    }

    #[test]
    fn drops_empty_chunks() {
        assert!(chunk_text("\n\n   \n\n", 10).is_empty());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib meeting::chunk 2>&1 | tail -3`
Expected: compile error, `chunk_text` not found.

- [ ] **Step 3: Implement**

```rust
//! Text chunking for the source index. Prefers paragraph boundaries, then
//! lines, then hard splits, so search snippets stay readable.

pub const MAX_CHUNK_CHARS: usize = 1500;

pub fn chunk_text(text: &str, max_chars: usize) -> Vec<String> {
    let max_chars = max_chars.max(1);
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();

    for para in text.split("\n\n") {
        let para = para.trim();
        if para.is_empty() {
            continue;
        }
        let para_len = para.chars().count();
        if para_len > max_chars {
            flush(&mut out, &mut current);
            for piece in hard_split(para, max_chars) {
                out.push(piece);
            }
            continue;
        }
        let cur_len = current.chars().count();
        let joined_len = if current.is_empty() { para_len } else { cur_len + 2 + para_len };
        if joined_len > max_chars {
            flush(&mut out, &mut current);
        }
        if !current.is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(para);
    }
    flush(&mut out, &mut current);
    out
}

fn flush(out: &mut Vec<String>, current: &mut String) {
    let trimmed = current.trim();
    if !trimmed.is_empty() {
        out.push(trimmed.to_string());
    }
    current.clear();
}

fn hard_split(text: &str, max_chars: usize) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut buf = String::new();
    for line in text.split('\n') {
        for ch in line.chars() {
            if buf.chars().count() >= max_chars {
                pieces.push(std::mem::take(&mut buf));
            }
            buf.push(ch);
        }
        if buf.chars().count() >= max_chars {
            pieces.push(std::mem::take(&mut buf));
        } else if !buf.is_empty() {
            buf.push('\n');
        }
    }
    let tail = buf.trim().to_string();
    if !tail.is_empty() {
        pieces.push(tail);
    }
    pieces.into_iter().map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect()
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib meeting::chunk 2>&1 | tail -3`
Expected: `5 passed`.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/meeting/chunk.rs
git commit -m "feat: add paragraph-aware text chunking for source indexing"
```

---

### Task 5: Indexer for text, file, and folder sources

**Files:**

- Create: `src-tauri/src/meeting/indexer.rs` (replace placeholder)

**Interfaces:**

- Consumes: `chunk::chunk_text`, `chunk::MAX_CHUNK_CHARS`.
- Produces:
  - `pub const MAX_FILE_BYTES: u64 = 524_288;`
  - `pub fn chunks_for_text(text: &str) -> Vec<(String, String)>` path `""`
  - `pub fn chunks_for_file(path: &Path) -> anyhow::Result<Vec<(String, String)>>` path = file name; errors on size cap or non-UTF-8
  - `pub fn chunks_for_folder(root: &Path) -> anyhow::Result<Vec<(String, String)>>` path = relative path with `/` separators; skips oversize, binary (non-UTF-8), hidden dirs, and `.gitignore`d entries; returns chunks sorted by path for determinism
  - `pub fn is_probably_binary(bytes: &[u8]) -> bool` (NUL byte in first 8 KB)

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn text_source_yields_chunks_with_empty_path() {
        let chunks = chunks_for_text("one\n\ntwo");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].0, "");
    }

    #[test]
    fn file_source_uses_file_name_as_path() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("notes.md");
        fs::write(&f, "hello").unwrap();
        let chunks = chunks_for_file(&f).unwrap();
        assert_eq!(chunks[0].0, "notes.md");
        assert_eq!(chunks[0].1, "hello");
    }

    #[test]
    fn file_over_size_cap_is_error() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("big.txt");
        fs::write(&f, vec![b'a'; (MAX_FILE_BYTES + 1) as usize]).unwrap();
        assert!(chunks_for_file(&f).is_err());
    }

    #[test]
    fn folder_respects_gitignore_and_skips_binary_and_hidden() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join(".gitignore"), "ignored/\n*.log\n").unwrap();
        fs::create_dir_all(root.join("ignored")).unwrap();
        fs::write(root.join("ignored/secret.txt"), "should not index").unwrap();
        fs::write(root.join("app.log"), "log line").unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(root.join("blob.bin"), b"\x00\x01\x02binary").unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join(".git/config"), "[core]").unwrap();
        fs::write(root.join("README.md"), "readme").unwrap();

        let chunks = chunks_for_folder(root).unwrap();
        let paths: Vec<&str> = chunks.iter().map(|c| c.0.as_str()).collect();
        assert_eq!(paths, vec!["README.md", "src/main.rs"]);
    }

    #[test]
    fn folder_skips_oversize_files_without_failing() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("big.txt"), vec![b'a'; (MAX_FILE_BYTES + 1) as usize]).unwrap();
        fs::write(dir.path().join("small.txt"), "ok").unwrap();
        let chunks = chunks_for_folder(dir.path()).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].0, "small.txt");
    }

    #[test]
    fn binary_detection() {
        assert!(is_probably_binary(b"abc\x00def"));
        assert!(!is_probably_binary("plain text é".as_bytes()));
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib meeting::indexer 2>&1 | tail -3`
Expected: compile error.

- [ ] **Step 3: Implement**

```rust
//! Turns a source into searchable chunks. Pure functions over the
//! filesystem and network so the store and manager stay thin.

use crate::meeting::chunk::{chunk_text, MAX_CHUNK_CHARS};
use anyhow::{anyhow, Context, Result};
use std::path::Path;

pub const MAX_FILE_BYTES: u64 = 524_288;

pub fn is_probably_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8192).any(|b| *b == 0)
}

pub fn chunks_for_text(text: &str) -> Vec<(String, String)> {
    chunk_text(text, MAX_CHUNK_CHARS)
        .into_iter()
        .map(|c| (String::new(), c))
        .collect()
}

fn read_text_file(path: &Path) -> Result<String> {
    let meta = std::fs::metadata(path).with_context(|| format!("stat {}", path.display()))?;
    if meta.len() > MAX_FILE_BYTES {
        return Err(anyhow!(
            "{} is {} bytes, over the {} byte limit",
            path.display(),
            meta.len(),
            MAX_FILE_BYTES
        ));
    }
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    if is_probably_binary(&bytes) {
        return Err(anyhow!("{} looks binary", path.display()));
    }
    String::from_utf8(bytes).map_err(|_| anyhow!("{} is not UTF-8", path.display()))
}

pub fn chunks_for_file(path: &Path) -> Result<Vec<(String, String)>> {
    let text = read_text_file(path)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    Ok(chunk_text(&text, MAX_CHUNK_CHARS)
        .into_iter()
        .map(|c| (name.clone(), c))
        .collect())
}

pub fn chunks_for_folder(root: &Path) -> Result<Vec<(String, String)>> {
    if !root.is_dir() {
        return Err(anyhow!("{} is not a directory", root.display()));
    }
    let mut out: Vec<(String, String)> = Vec::new();
    let walker = ignore::WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .build();
    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                log::debug!("skip walk entry: {}", e);
                continue;
            }
        };
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        let text = match read_text_file(path) {
            Ok(t) => t,
            Err(e) => {
                log::debug!("skip {}: {}", path.display(), e);
                continue;
            }
        };
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("/");
        for c in chunk_text(&text, MAX_CHUNK_CHARS) {
            out.push((rel.clone(), c));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}
```

Note: `.sort_by` on path is stable, so chunk order within a file is preserved.

- [ ] **Step 4: Run tests**

Run: `cargo test --lib meeting::indexer 2>&1 | tail -3`
Expected: `6 passed`.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/meeting/indexer.rs
git commit -m "feat: index text, file, and gitignore-aware folder sources into chunks"
```

---

### Task 6: Indexer for link and image sources

**Files:**

- Modify: `src-tauri/src/meeting/indexer.rs`

**Interfaces:**

- Produces:
  - `pub fn html_to_chunks(html: &str) -> Vec<(String, String)>` path `""`
  - `pub async fn chunks_for_link(url: &str) -> anyhow::Result<Vec<(String, String)>>`: GET with 20 s timeout, requires 2xx and a `text/html` or `text/plain` content type; HTML goes through `html2text`, plain text through `chunks_for_text`
  - `pub fn copy_image_into(source_path: &Path, dest_dir: &Path, source_id: i64) -> anyhow::Result<PathBuf>` copies to `dest_dir/<source_id>.<ext>`
  - `pub fn chunks_for_caption(caption: &str) -> Vec<(String, String)>` same as text

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module:

```rust
    #[test]
    fn html_is_stripped_to_text_chunks() {
        let html = "<html><body><h1>Title</h1><p>Body <b>bold</b> text.</p><script>var x=1;</script></body></html>";
        let chunks = html_to_chunks(html);
        let joined = chunks.iter().map(|c| c.1.as_str()).collect::<Vec<_>>().join("\n");
        assert!(joined.contains("Title"));
        assert!(joined.contains("bold"));
        assert!(!joined.contains("var x"));
    }

    #[tokio::test]
    async fn link_to_unreachable_host_is_error_not_panic() {
        let res = chunks_for_link("http://127.0.0.1:9/nothing").await;
        assert!(res.is_err());
    }

    #[test]
    fn image_copy_uses_source_id_and_extension() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("pic.PNG");
        fs::write(&src, b"\x89PNG").unwrap();
        let dest = dir.path().join("images");
        let out = copy_image_into(&src, &dest, 42).unwrap();
        assert_eq!(out.file_name().unwrap().to_string_lossy(), "42.png");
        assert!(out.exists());
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib meeting::indexer 2>&1 | tail -3`
Expected: compile error.

- [ ] **Step 3: Implement**

Add to `indexer.rs`:

```rust
use std::path::PathBuf;
use std::time::Duration;

pub fn html_to_chunks(html: &str) -> Vec<(String, String)> {
    let text = html2text::from_read(html.as_bytes(), 100).unwrap_or_default();
    chunks_for_text(&text)
}

pub async fn chunks_for_link(url: &str) -> Result<Vec<(String, String)>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent("CASE/0.1")
        .build()?;
    let resp = client.get(url).send().await.with_context(|| format!("fetch {}", url))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(anyhow!("{} returned HTTP {}", url, status.as_u16()));
    }
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let body = resp.text().await.with_context(|| format!("read body of {}", url))?;
    if content_type.contains("text/html") || content_type.contains("application/xhtml") {
        Ok(html_to_chunks(&body))
    } else if content_type.starts_with("text/") || content_type.is_empty() {
        Ok(chunks_for_text(&body))
    } else {
        Err(anyhow!(
            "{} is {}, only HTML and plain text links are supported",
            url,
            content_type
        ))
    }
}

pub fn copy_image_into(source_path: &Path, dest_dir: &Path, source_id: i64) -> Result<PathBuf> {
    std::fs::create_dir_all(dest_dir)?;
    let ext = source_path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_else(|| "img".to_string());
    let dest = dest_dir.join(format!("{}.{}", source_id, ext));
    std::fs::copy(source_path, &dest)
        .with_context(|| format!("copy {} to {}", source_path.display(), dest.display()))?;
    Ok(dest)
}

pub fn chunks_for_caption(caption: &str) -> Vec<(String, String)> {
    chunks_for_text(caption)
}
```

If `html2text::from_read` in the resolved version returns `String` rather than `Result<String, _>`, drop the `.unwrap_or_default()`; follow the compiler.

- [ ] **Step 4: Run tests**

Run: `cargo test --lib meeting::indexer 2>&1 | tail -3`
Expected: `9 passed`.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/meeting/indexer.rs
git commit -m "feat: index link and image sources"
```

---

### Task 7: ProjectManager with background indexing and status events

**Files:**

- Create: `src-tauri/src/meeting/manager.rs` (replace placeholder)
- Modify: `src-tauri/src/lib.rs` (manage state in the same place `HistoryManager` is managed, around line 208-217; add event to `collect_events!`)

**Interfaces:**

- Consumes: `ProjectStore`, indexer functions.
- Produces:
  - `pub struct SourceStatusPayload { pub source: Source }` deriving `Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event`; event name defaults to `source-status-payload` (tauri-specta derives it); the frontend subscribes via `events.sourceStatusPayload`.
  - `pub struct ProjectManager { store: Arc<ProjectStore>, images_dir: PathBuf, tx: Mutex<Option<mpsc::Sender<i64>>>, app: Option<AppHandle> }`
  - `ProjectManager::new(app: &AppHandle) -> Result<Self>` opens `app_data_dir/meet.db`, `images_dir = app_data_dir/sources/images`, spawns worker thread
  - `ProjectManager::new_for_test(db_path, images_dir) -> Result<Self>` no AppHandle, no event emit
  - `store(&self) -> &ProjectStore`
  - `enqueue_index(&self, source_id: i64)` pushes to worker
  - `index_source_now(&self, source_id: i64) -> Result<()>` synchronous indexing used by the worker and by tests
  - `add_and_index(&self, project_id, kind, title, locator) -> Result<Source>` adds, copies image if kind is Image (locator becomes copied path), enqueues
  - `remove_source(&self, id) -> Result<()>` deletes copied image file if any, then store removal
  - `delete_project(&self, id) -> Result<()>` removes every image file for the project's sources, then store delete

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::meeting::store::{SourceKind, SourceStatus};
    use std::fs;

    fn mgr() -> (tempfile::TempDir, ProjectManager) {
        let dir = tempfile::tempdir().unwrap();
        let m = ProjectManager::new_for_test(dir.path().join("meet.db"), dir.path().join("images")).unwrap();
        (dir, m)
    }

    #[test]
    fn text_source_indexes_to_ready_and_is_searchable() {
        let (_d, m) = mgr();
        let p = m.store().create_project("P").unwrap();
        let s = m.store().add_source(p.id, SourceKind::Text, "Notes", "retry budget is five").unwrap();
        m.index_source_now(s.id).unwrap();
        let got = m.store().get_source(s.id).unwrap().unwrap();
        assert_eq!(got.status, SourceStatus::Ready);
        assert_eq!(m.store().search(p.id, "retry", 5).unwrap().len(), 1);
    }

    #[test]
    fn missing_file_marks_failed_with_error() {
        let (_d, m) = mgr();
        let p = m.store().create_project("P").unwrap();
        let s = m.store().add_source(p.id, SourceKind::File, "Gone", "/definitely/not/here.txt").unwrap();
        let _ = m.index_source_now(s.id);
        let got = m.store().get_source(s.id).unwrap().unwrap();
        assert_eq!(got.status, SourceStatus::Failed);
        assert!(got.error.is_some());
    }

    #[test]
    fn two_queued_sources_both_reach_ready() {
        let (_d, m) = mgr();
        let p = m.store().create_project("P").unwrap();
        let a = m.add_and_index(p.id, SourceKind::Text, "A", "alpha").unwrap();
        let b = m.add_and_index(p.id, SourceKind::Text, "B", "beta").unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let sa = m.store().get_source(a.id).unwrap().unwrap().status;
            let sb = m.store().get_source(b.id).unwrap().unwrap().status;
            if sa == SourceStatus::Ready && sb == SourceStatus::Ready {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "indexing did not finish: {:?} {:?}", sa, sb);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    #[test]
    fn image_is_copied_and_removed_with_source() {
        let (d, m) = mgr();
        let p = m.store().create_project("P").unwrap();
        let pic = d.path().join("shot.png");
        fs::write(&pic, b"\x89PNG").unwrap();
        let s = m.add_and_index(p.id, SourceKind::Image, "Shot", pic.to_str().unwrap()).unwrap();
        let copied = std::path::PathBuf::from(&s.locator);
        assert!(copied.starts_with(d.path().join("images")));
        assert!(copied.exists());
        m.remove_source(s.id).unwrap();
        assert!(!copied.exists());
    }

    #[test]
    fn delete_project_removes_image_files() {
        let (d, m) = mgr();
        let p = m.store().create_project("P").unwrap();
        let pic = d.path().join("shot.jpg");
        fs::write(&pic, b"JFIF").unwrap();
        let s = m.add_and_index(p.id, SourceKind::Image, "Shot", pic.to_str().unwrap()).unwrap();
        let copied = std::path::PathBuf::from(&s.locator);
        m.delete_project(p.id).unwrap();
        assert!(!copied.exists());
        assert!(m.store().get_project(p.id).unwrap().is_none());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib meeting::manager 2>&1 | tail -3`
Expected: compile error.

- [ ] **Step 3: Implement**

```rust
//! Tauri-facing owner of the project store. Runs source indexing on a
//! worker thread and emits status events to the frontend.

use crate::meeting::indexer;
use crate::meeting::store::{ProjectStore, Source, SourceKind, SourceStatus};
use anyhow::{anyhow, Result};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use tauri::AppHandle;
use tauri_specta::Event;

#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct SourceStatusPayload {
    pub source: Source,
}

pub struct ProjectManager {
    store: Arc<ProjectStore>,
    images_dir: PathBuf,
    tx: Mutex<Option<mpsc::Sender<i64>>>,
    app: Option<AppHandle>,
}

impl ProjectManager {
    pub fn new(app: &AppHandle) -> Result<Self> {
        let data_dir = crate::portable::app_data_dir(app)?;
        Self::build(data_dir.join("meet.db"), data_dir.join("sources").join("images"), Some(app.clone()))
    }

    pub fn new_for_test(db_path: PathBuf, images_dir: PathBuf) -> Result<Self> {
        Self::build(db_path, images_dir, None)
    }

    fn build(db_path: PathBuf, images_dir: PathBuf, app: Option<AppHandle>) -> Result<Self> {
        let store = Arc::new(ProjectStore::open(db_path)?);
        std::fs::create_dir_all(&images_dir)?;
        let (tx, rx) = mpsc::channel::<i64>();
        let worker_store = Arc::clone(&store);
        let worker_images = images_dir.clone();
        let worker_app = app.clone();
        std::thread::Builder::new()
            .name("meet-indexer".into())
            .spawn(move || {
                let worker = ProjectManager {
                    store: worker_store,
                    images_dir: worker_images,
                    tx: Mutex::new(None),
                    app: worker_app,
                };
                for source_id in rx {
                    if let Err(e) = worker.index_source_now(source_id) {
                        warn!("indexing source {} failed: {}", source_id, e);
                    }
                }
            })?;
        Ok(Self { store, images_dir, tx: Mutex::new(Some(tx)), app })
    }

    pub fn store(&self) -> &ProjectStore {
        &self.store
    }

    fn emit_status(&self, source: &Source) {
        if let Some(app) = &self.app {
            if let Err(e) = (SourceStatusPayload { source: source.clone() }).emit(app) {
                error!("emit source status: {}", e);
            }
        }
    }

    fn set_status(&self, id: i64, status: SourceStatus, err: Option<&str>) -> Result<()> {
        self.store.set_source_status(id, status, err)?;
        if let Some(s) = self.store.get_source(id)? {
            self.emit_status(&s);
        }
        Ok(())
    }

    pub fn enqueue_index(&self, source_id: i64) {
        if let Some(tx) = self.tx.lock().ok().and_then(|g| g.clone()) {
            if tx.send(source_id).is_err() {
                error!("indexer worker is gone; source {} not queued", source_id);
            }
        }
    }

    /// Index one source synchronously. Any error marks the source failed
    /// with the error text and is also returned.
    pub fn index_source_now(&self, source_id: i64) -> Result<()> {
        let Some(src) = self.store.get_source(source_id)? else {
            return Err(anyhow!("source {} not found", source_id));
        };
        self.set_status(source_id, SourceStatus::Pending, None)?;
        let result: Result<Vec<(String, String)>> = match src.kind {
            SourceKind::Text => Ok(indexer::chunks_for_text(&src.locator)),
            SourceKind::File => indexer::chunks_for_file(Path::new(&src.locator)),
            SourceKind::Folder => indexer::chunks_for_folder(Path::new(&src.locator)),
            SourceKind::Link => tauri::async_runtime::block_on(indexer::chunks_for_link(&src.locator)),
            SourceKind::Image => Ok(indexer::chunks_for_caption(src.digest.as_deref().unwrap_or(""))),
        };
        match result {
            Ok(chunks) => {
                let n = self.store.replace_chunks(source_id, &chunks)?;
                info!("indexed source {} ({} chunks)", source_id, n);
                self.set_status(source_id, SourceStatus::Ready, None)
            }
            Err(e) => {
                let msg = e.to_string();
                self.set_status(source_id, SourceStatus::Failed, Some(&msg))?;
                Err(e)
            }
        }
    }

    pub fn add_and_index(
        &self,
        project_id: i64,
        kind: SourceKind,
        title: &str,
        locator: &str,
    ) -> Result<Source> {
        let mut source = self.store.add_source(project_id, kind, title, locator)?;
        if kind == SourceKind::Image {
            let copied = indexer::copy_image_into(Path::new(locator), &self.images_dir, source.id)?;
            let copied_str = copied.to_string_lossy().to_string();
            self.store.update_source_locator(source.id, &copied_str)?;
            source.locator = copied_str;
        }
        self.emit_status(&source);
        self.enqueue_index(source.id);
        Ok(source)
    }

    fn remove_image_file(&self, source: &Source) {
        if source.kind == SourceKind::Image {
            let p = Path::new(&source.locator);
            if p.starts_with(&self.images_dir) {
                if let Err(e) = std::fs::remove_file(p) {
                    warn!("remove image {}: {}", p.display(), e);
                }
            }
        }
    }

    pub fn remove_source(&self, id: i64) -> Result<()> {
        if let Some(src) = self.store.get_source(id)? {
            self.remove_image_file(&src);
        }
        self.store.remove_source(id)
    }

    pub fn delete_project(&self, id: i64) -> Result<()> {
        for src in self.store.list_sources(id)? {
            self.remove_image_file(&src);
        }
        self.store.delete_project(id)
    }
}
```

This needs one more store method. Add to `store.rs` `impl ProjectStore`:

```rust
    pub fn update_source_locator(&self, id: i64, locator: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE sources SET locator = ?1 WHERE id = ?2",
            params![locator, id],
        )?;
        Ok(())
    }
```

`tauri::async_runtime::block_on` requires the Tauri runtime to be initialised; in unit tests it is not. Make the Link arm test-safe:

```rust
            SourceKind::Link => {
                let url = src.locator.clone();
                match tokio::runtime::Handle::try_current() {
                    Ok(h) => tokio::task::block_in_place(|| h.block_on(indexer::chunks_for_link(&url))),
                    Err(_) => tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()?
                        .block_on(indexer::chunks_for_link(&url)),
                }
            }
```

The worker thread has no runtime, so it takes the `Err(_)` branch and builds a small one per link. Use this version, not the `tauri::async_runtime` one.

Register in `lib.rs` next to the history manager:

```rust
    let project_manager = Arc::new(
        meeting::manager::ProjectManager::new(app_handle)
            .expect("Failed to initialize project manager"),
    );
    app_handle.manage(project_manager);
```

and add `meeting::manager::SourceStatusPayload,` inside `collect_events![ ... ]`.

- [ ] **Step 4: Run tests and a full check**

Run: `cargo test --lib meeting:: 2>&1 | tail -3`
Expected: `25 passed` (10 store + 5 chunk + 9 indexer + 5 manager... adjust to actual; all pass, zero failed).

Run: `cargo check 2>&1 | tail -2`
Expected: `Finished`.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/meeting/manager.rs src-tauri/src/meeting/store.rs src-tauri/src/lib.rs
git commit -m "feat: add ProjectManager with background indexing and status events"
```

---

### Task 8: Tauri commands and regenerated bindings

**Files:**

- Create: `src-tauri/src/commands/project.rs`
- Modify: `src-tauri/src/commands/mod.rs` (add `pub mod project;`)
- Modify: `src-tauri/src/lib.rs` (add commands to `collect_commands!`)
- Regenerate: `src/bindings.ts`

**Interfaces:**

- Produces commands (names as the frontend will call them in camelCase): `list_projects`, `get_project`, `create_project(title)`, `update_project_title(id, title)`, `update_project_brief(id, brief)`, `delete_project(id)`, `list_sources(project_id)`, `add_source(project_id, kind, title, locator)`, `remove_source(id)`, `reindex_source(id)`, `set_source_caption(id, caption)` (sets digest then reindexes), `search_sources(project_id, query, limit)`, `pick_source_path(kind)` (opens native file or folder picker, returns `Option<String>`).

- [ ] **Step 1: Write the commands**

`src-tauri/src/commands/project.rs`:

```rust
use crate::meeting::manager::ProjectManager;
use crate::meeting::store::{Project, SearchHit, Source, SourceKind};
use std::sync::Arc;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

type Pm<'a> = State<'a, Arc<ProjectManager>>;

fn err(e: anyhow::Error) -> String {
    e.to_string()
}

#[tauri::command]
#[specta::specta]
pub fn list_projects(pm: Pm<'_>) -> Result<Vec<Project>, String> {
    pm.store().list_projects().map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn get_project(pm: Pm<'_>, id: i64) -> Result<Option<Project>, String> {
    pm.store().get_project(id).map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn create_project(pm: Pm<'_>, title: String) -> Result<Project, String> {
    pm.store().create_project(title.trim()).map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn update_project_title(pm: Pm<'_>, id: i64, title: String) -> Result<(), String> {
    pm.store().update_project_title(id, title.trim()).map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn update_project_brief(pm: Pm<'_>, id: i64, brief: String) -> Result<(), String> {
    pm.store().update_project_brief(id, &brief).map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn delete_project(pm: Pm<'_>, id: i64) -> Result<(), String> {
    pm.delete_project(id).map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn list_sources(pm: Pm<'_>, project_id: i64) -> Result<Vec<Source>, String> {
    pm.store().list_sources(project_id).map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn add_source(
    pm: Pm<'_>,
    project_id: i64,
    kind: SourceKind,
    title: String,
    locator: String,
) -> Result<Source, String> {
    pm.add_and_index(project_id, kind, title.trim(), &locator).map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn remove_source(pm: Pm<'_>, id: i64) -> Result<(), String> {
    pm.remove_source(id).map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn reindex_source(pm: Pm<'_>, id: i64) -> Result<(), String> {
    pm.enqueue_index(id);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn set_source_caption(pm: Pm<'_>, id: i64, caption: String) -> Result<(), String> {
    pm.store().set_source_digest(id, caption.trim()).map_err(err)?;
    pm.enqueue_index(id);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn search_sources(
    pm: Pm<'_>,
    project_id: i64,
    query: String,
    limit: Option<u32>,
) -> Result<Vec<SearchHit>, String> {
    pm.store()
        .search(project_id, &query, limit.unwrap_or(8) as usize)
        .map_err(err)
}

/// Native picker. `kind` is `file`, `folder`, or `image`.
#[tauri::command]
#[specta::specta]
pub async fn pick_source_path(app: AppHandle, kind: SourceKind) -> Result<Option<String>, String> {
    let dialog = app.dialog().file();
    let picked = match kind {
        SourceKind::Folder => dialog.blocking_pick_folder(),
        SourceKind::Image => dialog
            .add_filter("Images", &["png", "jpg", "jpeg", "gif", "webp", "bmp"])
            .blocking_pick_file(),
        _ => dialog.blocking_pick_file(),
    };
    Ok(picked.and_then(|p| p.as_path().map(|p| p.to_string_lossy().to_string())))
}
```

Verified against `tauri-plugin-dialog 2.6.0`: `add_filter(name, &[&str]) -> Self`, `blocking_pick_file(self) -> Option<FilePath>`, `blocking_pick_folder(self) -> Option<FilePath>`, and `FilePath::as_path(&self) -> Option<&Path>` (re-exported from `tauri-plugin-fs`).

- [ ] **Step 2: Register**

`commands/mod.rs`: add `pub mod project;`.

`lib.rs` `collect_commands![ ... ]`: add

```rust
            commands::project::list_projects,
            commands::project::get_project,
            commands::project::create_project,
            commands::project::update_project_title,
            commands::project::update_project_brief,
            commands::project::delete_project,
            commands::project::list_sources,
            commands::project::add_source,
            commands::project::remove_source,
            commands::project::reindex_source,
            commands::project::set_source_caption,
            commands::project::search_sources,
            commands::project::pick_source_path,
```

- [ ] **Step 3: Build and regenerate bindings**

Run from `src-tauri/`: `cargo run -- --list-models 2>&1 | tail -3`
Expected: the command prints the model list (or an empty list) and exits 0. Then:

Run from repo root: `grep -c "listProjects\|addSource\|sourceStatusPayload\|searchSources\|pickSourcePath" src/bindings.ts`
Expected: at least 5.

- [ ] **Step 4: Frontend still typechecks**

Run from repo root: `bunx tsc --noEmit && echo ok`
Expected: `ok`.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands/project.rs src-tauri/src/commands/mod.rs src-tauri/src/lib.rs src/bindings.ts
git commit -m "feat: expose project and source commands to the frontend"
```

---

### Task 9: Zustand project store and i18n keys

**Files:**

- Create: `src/stores/projectStore.ts`
- Modify: `src/i18n/locales/en/translation.json` (add `sidebar.projects` and a `projects` block)

**Interfaces:**

- Consumes: `commands.*` and `events.sourceStatusPayload` from `@/bindings`; types `Project`, `Source`, `SourceKind`, `SearchHit`.
- Produces: `useProjectStore` with state `{ projects: Project[]; activeProjectId: number | null; sources: Source[]; loading: boolean }` and actions `loadProjects()`, `createProject(title)`, `openProject(id)`, `closeProject()`, `renameProject(id, title)`, `saveBrief(id, brief)`, `deleteProject(id)`, `loadSources(projectId)`, `addSource(projectId, kind, title, locator)`, `removeSource(id)`, `reindexSource(id)`, `setCaption(id, caption)`, `subscribe()` returning an unlisten promise.

- [ ] **Step 1: Add i18n keys**

In `src/i18n/locales/en/translation.json`, under `"sidebar"` add `"projects": "Projects"` as the first key. Add a top-level `"projects"` object:

```json
  "projects": {
    "title": "Projects",
    "description": "A project holds a brief and sources the assistant can search during meetings.",
    "new": "New project",
    "newPlaceholder": "Project name",
    "create": "Create",
    "empty": "No projects yet. Create one and add the notes, files, and links the assistant should know.",
    "open": "Open",
    "delete": "Delete project",
    "deleteConfirm": "Delete this project, its sources, and its meetings?",
    "back": "All projects",
    "rename": "Rename",
    "sourcesCount_one": "{{count}} source",
    "sourcesCount_other": "{{count}} sources",
    "brief": {
      "title": "Brief",
      "description": "Always sent to the assistant. Keep it short: what this project is, who is involved, what matters.",
      "placeholder": "What is this project about?",
      "saved": "Brief saved"
    },
    "sources": {
      "title": "Sources",
      "description": "Indexed locally. The assistant searches these on its own when the conversation needs them.",
      "add": "Add source",
      "empty": "No sources yet.",
      "remove": "Remove source",
      "reindex": "Re-index",
      "caption": "Caption",
      "captionPlaceholder": "Describe this image for the assistant",
      "saveCaption": "Save caption",
      "status": {
        "pending": "Indexing",
        "ready": "Ready",
        "failed": "Failed"
      },
      "kind": {
        "text": "Text",
        "file": "File",
        "folder": "Folder",
        "link": "Link",
        "image": "Image"
      },
      "dialog": {
        "title": "Add source",
        "kind": "Type",
        "titleLabel": "Title",
        "titlePlaceholder": "Short name for this source",
        "textLabel": "Text",
        "textPlaceholder": "Paste notes, an agenda, anything useful",
        "linkLabel": "URL",
        "linkPlaceholder": "https://",
        "pathLabel": "Path",
        "pick": "Choose",
        "add": "Add",
        "cancel": "Cancel",
        "close": "Close"
      },
      "errors": {
        "add": "Could not add source.",
        "remove": "Could not remove source."
      }
    },
    "errors": {
      "load": "Could not load projects.",
      "create": "Could not create project.",
      "delete": "Could not delete project.",
      "save": "Could not save."
    }
  }
```

- [ ] **Step 2: Write the store**

`src/stores/projectStore.ts`:

```ts
import { create } from "zustand";
import { toast } from "sonner";
import i18n from "@/i18n";
import {
  commands,
  events,
  type Project,
  type Source,
  type SourceKind,
} from "@/bindings";

interface ProjectStore {
  projects: Project[];
  activeProjectId: number | null;
  sources: Source[];
  loading: boolean;

  loadProjects: () => Promise<void>;
  createProject: (title: string) => Promise<Project | null>;
  openProject: (id: number) => Promise<void>;
  closeProject: () => void;
  renameProject: (id: number, title: string) => Promise<void>;
  saveBrief: (id: number, brief: string) => Promise<void>;
  deleteProject: (id: number) => Promise<void>;
  loadSources: (projectId: number) => Promise<void>;
  addSource: (
    projectId: number,
    kind: SourceKind,
    title: string,
    locator: string,
  ) => Promise<Source | null>;
  removeSource: (id: number) => Promise<void>;
  reindexSource: (id: number) => Promise<void>;
  setCaption: (id: number, caption: string) => Promise<void>;
  subscribe: () => Promise<() => void>;
}

const t = (key: string) => i18n.t(key);

export const useProjectStore = create<ProjectStore>((set, get) => ({
  projects: [],
  activeProjectId: null,
  sources: [],
  loading: false,

  loadProjects: async () => {
    set({ loading: true });
    const res = await commands.listProjects();
    if (res.status === "ok") {
      set({ projects: res.data, loading: false });
    } else {
      set({ loading: false });
      toast.error(t("projects.errors.load"));
    }
  },

  createProject: async (title) => {
    const res = await commands.createProject(title);
    if (res.status !== "ok") {
      toast.error(t("projects.errors.create"));
      return null;
    }
    set((s) => ({ projects: [res.data, ...s.projects] }));
    return res.data;
  },

  openProject: async (id) => {
    set({ activeProjectId: id, sources: [] });
    await get().loadSources(id);
  },

  closeProject: () => set({ activeProjectId: null, sources: [] }),

  renameProject: async (id, title) => {
    const res = await commands.updateProjectTitle(id, title);
    if (res.status !== "ok") {
      toast.error(t("projects.errors.save"));
      return;
    }
    set((s) => ({
      projects: s.projects.map((p) => (p.id === id ? { ...p, title } : p)),
    }));
  },

  saveBrief: async (id, brief) => {
    const res = await commands.updateProjectBrief(id, brief);
    if (res.status !== "ok") {
      toast.error(t("projects.errors.save"));
      return;
    }
    set((s) => ({
      projects: s.projects.map((p) => (p.id === id ? { ...p, brief } : p)),
    }));
  },

  deleteProject: async (id) => {
    const res = await commands.deleteProject(id);
    if (res.status !== "ok") {
      toast.error(t("projects.errors.delete"));
      return;
    }
    set((s) => ({
      projects: s.projects.filter((p) => p.id !== id),
      activeProjectId: s.activeProjectId === id ? null : s.activeProjectId,
      sources: s.activeProjectId === id ? [] : s.sources,
    }));
  },

  loadSources: async (projectId) => {
    const res = await commands.listSources(projectId);
    if (res.status === "ok" && get().activeProjectId === projectId) {
      set({ sources: res.data });
    }
  },

  addSource: async (projectId, kind, title, locator) => {
    const res = await commands.addSource(projectId, kind, title, locator);
    if (res.status !== "ok") {
      toast.error(t("projects.sources.errors.add"));
      return null;
    }
    set((s) => ({ sources: [...s.sources, res.data] }));
    return res.data;
  },

  removeSource: async (id) => {
    const res = await commands.removeSource(id);
    if (res.status !== "ok") {
      toast.error(t("projects.sources.errors.remove"));
      return;
    }
    set((s) => ({ sources: s.sources.filter((x) => x.id !== id) }));
  },

  reindexSource: async (id) => {
    await commands.reindexSource(id);
  },

  setCaption: async (id, caption) => {
    const res = await commands.setSourceCaption(id, caption);
    if (res.status !== "ok") {
      toast.error(t("projects.errors.save"));
      return;
    }
    set((s) => ({
      sources: s.sources.map((x) =>
        x.id === id ? { ...x, digest: caption } : x,
      ),
    }));
  },

  subscribe: async () => {
    const unlisten = await events.sourceStatusPayload.listen((event) => {
      const updated = event.payload.source;
      set((s) => {
        if (s.activeProjectId !== updated.project_id) return s;
        const exists = s.sources.some((x) => x.id === updated.id);
        return {
          sources: exists
            ? s.sources.map((x) => (x.id === updated.id ? updated : x))
            : [...s.sources, updated],
        };
      });
    });
    return unlisten;
  },
}));
```

`src/i18n/index.ts:137` has `export default i18n`, matching the import above.

- [ ] **Step 3: Typecheck and lint**

Run: `bunx tsc --noEmit && bun run lint 2>&1 | tail -3`
Expected: both exit 0.

- [ ] **Step 4: Commit**

```bash
git add src/stores/projectStore.ts src/i18n/locales/en/translation.json
git commit -m "feat: add project store and i18n strings for projects UI"
```

---

### Task 10: Projects UI and sidebar entry

**Files:**

- Create: `src/components/projects/ProjectList.tsx`
- Create: `src/components/projects/ProjectDetail.tsx`
- Create: `src/components/projects/BriefEditor.tsx`
- Create: `src/components/projects/SourceList.tsx`
- Create: `src/components/projects/AddSourceDialog.tsx`
- Create: `src/components/projects/ProjectsSection.tsx` (switches list vs detail)
- Modify: `src/components/Sidebar.tsx` (add `projects` as the first entry of `SECTIONS_CONFIG`)
- Modify: `src/App.tsx` (default section becomes `"projects"`)

**Interfaces:**

- Consumes: `useProjectStore`, `commands.pickSourcePath`, `components/ui`.
- Produces: `ProjectsSection` component used by the sidebar config.

- [ ] **Step 1: ProjectsSection**

```tsx
import React, { useEffect } from "react";
import { useProjectStore } from "@/stores/projectStore";
import { ProjectList } from "./ProjectList";
import { ProjectDetail } from "./ProjectDetail";

export const ProjectsSection: React.FC = () => {
  const activeProjectId = useProjectStore((s) => s.activeProjectId);
  const loadProjects = useProjectStore((s) => s.loadProjects);
  const subscribe = useProjectStore((s) => s.subscribe);

  useEffect(() => {
    loadProjects();
    const unlisten = subscribe();
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [loadProjects, subscribe]);

  return activeProjectId === null ? <ProjectList /> : <ProjectDetail />;
};
```

- [ ] **Step 2: ProjectList**

```tsx
import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { FolderKanban, Plus, Trash2 } from "lucide-react";
import { useProjectStore } from "@/stores/projectStore";
import { Button } from "../ui/Button";
import { Input } from "../ui/Input";
import { SettingsGroup } from "../ui/SettingsGroup";
import { formatRelativeTime } from "@/utils/dateFormat";

export const ProjectList: React.FC = () => {
  const { t, i18n } = useTranslation();
  const projects = useProjectStore((s) => s.projects);
  const loading = useProjectStore((s) => s.loading);
  const createProject = useProjectStore((s) => s.createProject);
  const openProject = useProjectStore((s) => s.openProject);
  const deleteProject = useProjectStore((s) => s.deleteProject);
  const [title, setTitle] = useState("");

  const submit = async () => {
    const name = title.trim();
    if (!name) return;
    const created = await createProject(name);
    if (created) {
      setTitle("");
      openProject(created.id);
    }
  };

  return (
    <div className="space-y-6">
      <SettingsGroup
        title={t("projects.title")}
        description={t("projects.description")}
      >
        <div className="flex items-center gap-2 p-4">
          <Input
            variant="compact"
            className="flex-1"
            placeholder={t("projects.newPlaceholder")}
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") submit();
            }}
          />
          <Button size="sm" onClick={submit} disabled={!title.trim()}>
            <Plus className="w-4 h-4 inline mr-1" />
            {t("projects.create")}
          </Button>
        </div>
      </SettingsGroup>

      <SettingsGroup>
        {loading && projects.length === 0 ? (
          <p className="p-4 text-sm text-mid-gray">{t("common.loading")}</p>
        ) : projects.length === 0 ? (
          <p className="p-4 text-sm text-mid-gray">{t("projects.empty")}</p>
        ) : (
          projects.map((p) => (
            <div key={p.id} className="flex items-center gap-3 p-4">
              <FolderKanban className="w-5 h-5 text-logo-primary shrink-0" />
              <button
                className="flex-1 text-left cursor-pointer"
                onClick={() => openProject(p.id)}
              >
                <div className="text-sm font-semibold text-text">{p.title}</div>
                <div className="text-xs text-mid-gray">
                  {formatRelativeTime(
                    String(Math.floor(p.updated_at / 1000)),
                    i18n.language,
                  )}
                </div>
              </button>
              <Button
                variant="danger-ghost"
                size="sm"
                title={t("projects.delete")}
                onClick={() => {
                  if (window.confirm(t("projects.deleteConfirm"))) {
                    deleteProject(p.id);
                  }
                }}
              >
                <Trash2 className="w-4 h-4" />
              </Button>
            </div>
          ))
        )}
      </SettingsGroup>
    </div>
  );
};
```

`formatRelativeTime(timestamp: string, locale: string)` expects Unix seconds as a string (see `src/utils/dateFormat.ts:65`); `updated_at` is milliseconds, hence the division.

- [ ] **Step 3: BriefEditor**

```tsx
import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useProjectStore } from "@/stores/projectStore";
import { SettingsGroup } from "../ui/SettingsGroup";
import { Textarea } from "../ui/Textarea";

export const BriefEditor: React.FC<{ projectId: number; brief: string }> = ({
  projectId,
  brief,
}) => {
  const { t } = useTranslation();
  const saveBrief = useProjectStore((s) => s.saveBrief);
  const [value, setValue] = useState(brief);
  const timer = useRef<number | null>(null);

  useEffect(() => setValue(brief), [brief, projectId]);

  const onChange = (next: string) => {
    setValue(next);
    if (timer.current) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => saveBrief(projectId, next), 600);
  };

  return (
    <SettingsGroup
      title={t("projects.brief.title")}
      description={t("projects.brief.description")}
    >
      <div className="p-4">
        <Textarea
          className="w-full"
          placeholder={t("projects.brief.placeholder")}
          value={value}
          onChange={(e) => onChange(e.target.value)}
        />
      </div>
    </SettingsGroup>
  );
};
```

- [ ] **Step 4: AddSourceDialog**

```tsx
import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type SourceKind } from "@/bindings";
import { useProjectStore } from "@/stores/projectStore";
import { Button } from "../ui/Button";
import { Dialog } from "../ui/Dialog";
import { Input } from "../ui/Input";
import { Textarea } from "../ui/Textarea";

const KINDS: SourceKind[] = ["text", "file", "folder", "link", "image"];

export const AddSourceDialog: React.FC<{
  projectId: number;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}> = ({ projectId, open, onOpenChange }) => {
  const { t } = useTranslation();
  const addSource = useProjectStore((s) => s.addSource);
  const [kind, setKind] = useState<SourceKind>("text");
  const [title, setTitle] = useState("");
  const [locator, setLocator] = useState("");
  const [busy, setBusy] = useState(false);

  const reset = () => {
    setKind("text");
    setTitle("");
    setLocator("");
  };

  const pick = async () => {
    const res = await commands.pickSourcePath(kind);
    if (res.status === "ok" && res.data) {
      setLocator(res.data);
      if (!title) setTitle(res.data.split(/[\\/]/).pop() ?? "");
    }
  };

  const submit = async () => {
    if (!locator.trim()) return;
    setBusy(true);
    const created = await addSource(
      projectId,
      kind,
      title.trim() || t(`projects.sources.kind.${kind}`),
      locator,
    );
    setBusy(false);
    if (created) {
      reset();
      onOpenChange(false);
    }
  };

  const needsPicker = kind === "file" || kind === "folder" || kind === "image";

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o) reset();
        onOpenChange(o);
      }}
      title={t("projects.sources.dialog.title")}
      closeLabel={t("projects.sources.dialog.close")}
      footer={
        <div className="flex justify-end gap-2">
          <Button
            variant="secondary"
            size="sm"
            onClick={() => onOpenChange(false)}
          >
            {t("projects.sources.dialog.cancel")}
          </Button>
          <Button size="sm" onClick={submit} disabled={busy || !locator.trim()}>
            {t("projects.sources.dialog.add")}
          </Button>
        </div>
      }
    >
      <div className="space-y-3">
        <label className="block text-xs text-mid-gray">
          {t("projects.sources.dialog.kind")}
          <div className="flex gap-2 mt-1 flex-wrap">
            {KINDS.map((k) => (
              <Button
                key={k}
                size="sm"
                variant={k === kind ? "primary-soft" : "secondary"}
                onClick={() => {
                  setKind(k);
                  setLocator("");
                }}
              >
                {t(`projects.sources.kind.${k}`)}
              </Button>
            ))}
          </div>
        </label>

        <label className="block text-xs text-mid-gray">
          {t("projects.sources.dialog.titleLabel")}
          <Input
            variant="compact"
            className="w-full mt-1"
            placeholder={t("projects.sources.dialog.titlePlaceholder")}
            value={title}
            onChange={(e) => setTitle(e.target.value)}
          />
        </label>

        {kind === "text" && (
          <label className="block text-xs text-mid-gray">
            {t("projects.sources.dialog.textLabel")}
            <Textarea
              variant="compact"
              className="w-full mt-1"
              placeholder={t("projects.sources.dialog.textPlaceholder")}
              value={locator}
              onChange={(e) => setLocator(e.target.value)}
            />
          </label>
        )}

        {kind === "link" && (
          <label className="block text-xs text-mid-gray">
            {t("projects.sources.dialog.linkLabel")}
            <Input
              variant="compact"
              className="w-full mt-1"
              placeholder={t("projects.sources.dialog.linkPlaceholder")}
              value={locator}
              onChange={(e) => setLocator(e.target.value)}
            />
          </label>
        )}

        {needsPicker && (
          <div className="text-xs text-mid-gray">
            {t("projects.sources.dialog.pathLabel")}
            <div className="flex items-center gap-2 mt-1">
              <Input
                variant="compact"
                className="flex-1"
                value={locator}
                readOnly
              />
              <Button size="sm" variant="secondary" onClick={pick}>
                {t("projects.sources.dialog.pick")}
              </Button>
            </div>
          </div>
        )}
      </div>
    </Dialog>
  );
};
```

- [ ] **Step 5: SourceList**

```tsx
import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  FileText,
  Folder,
  Image as ImageIcon,
  Link as LinkIcon,
  Plus,
  RefreshCw,
  Trash2,
  Type,
} from "lucide-react";
import type { Source, SourceKind } from "@/bindings";
import { useProjectStore } from "@/stores/projectStore";
import Badge from "../ui/Badge";
import { Button } from "../ui/Button";
import { Input } from "../ui/Input";
import { SettingsGroup } from "../ui/SettingsGroup";
import { AddSourceDialog } from "./AddSourceDialog";

const KIND_ICON: Record<
  SourceKind,
  React.ComponentType<{ className?: string }>
> = {
  text: Type,
  file: FileText,
  folder: Folder,
  link: LinkIcon,
  image: ImageIcon,
};

const statusVariant = (s: Source["status"]) =>
  s === "ready" ? "success" : s === "failed" ? "primary" : "secondary";

const CaptionRow: React.FC<{ source: Source }> = ({ source }) => {
  const { t } = useTranslation();
  const setCaption = useProjectStore((s) => s.setCaption);
  const [value, setValue] = useState(source.digest ?? "");
  return (
    <div className="flex items-center gap-2 mt-2">
      <Input
        variant="compact"
        className="flex-1"
        placeholder={t("projects.sources.captionPlaceholder")}
        value={value}
        onChange={(e) => setValue(e.target.value)}
      />
      <Button
        size="sm"
        variant="secondary"
        onClick={() => setCaption(source.id, value)}
      >
        {t("projects.sources.saveCaption")}
      </Button>
    </div>
  );
};

export const SourceList: React.FC<{ projectId: number }> = ({ projectId }) => {
  const { t } = useTranslation();
  const sources = useProjectStore((s) => s.sources);
  const removeSource = useProjectStore((s) => s.removeSource);
  const reindexSource = useProjectStore((s) => s.reindexSource);
  const [open, setOpen] = useState(false);

  return (
    <SettingsGroup
      title={t("projects.sources.title")}
      description={t("projects.sources.description")}
    >
      <div className="flex justify-end p-3">
        <Button size="sm" onClick={() => setOpen(true)}>
          <Plus className="w-4 h-4 inline mr-1" />
          {t("projects.sources.add")}
        </Button>
      </div>
      {sources.length === 0 ? (
        <p className="p-4 text-sm text-mid-gray">
          {t("projects.sources.empty")}
        </p>
      ) : (
        sources.map((s) => {
          const Icon = KIND_ICON[s.kind];
          return (
            <div key={s.id} className="p-4">
              <div className="flex items-center gap-3">
                <Icon className="w-4 h-4 text-logo-primary shrink-0" />
                <div className="flex-1 min-w-0">
                  <div className="text-sm font-semibold text-text truncate">
                    {s.title}
                  </div>
                  <div className="text-xs text-mid-gray truncate">
                    {s.kind === "text"
                      ? t("projects.sources.kind.text")
                      : s.locator}
                  </div>
                  {s.status === "failed" && s.error && (
                    <div className="text-xs text-red-500 mt-1">{s.error}</div>
                  )}
                </div>
                <Badge variant={statusVariant(s.status)}>
                  {t(`projects.sources.status.${s.status}`)}
                </Badge>
                <Button
                  variant="ghost"
                  size="sm"
                  title={t("projects.sources.reindex")}
                  onClick={() => reindexSource(s.id)}
                >
                  <RefreshCw className="w-4 h-4" />
                </Button>
                <Button
                  variant="danger-ghost"
                  size="sm"
                  title={t("projects.sources.remove")}
                  onClick={() => removeSource(s.id)}
                >
                  <Trash2 className="w-4 h-4" />
                </Button>
              </div>
              {s.kind === "image" && <CaptionRow source={s} />}
            </div>
          );
        })
      )}
      <AddSourceDialog
        projectId={projectId}
        open={open}
        onOpenChange={setOpen}
      />
    </SettingsGroup>
  );
};
```

`Badge` is imported as a default export elsewhere (`src/components/onboarding/ModelCard.tsx:23`), matching the import above.

- [ ] **Step 6: ProjectDetail**

```tsx
import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowLeft, Pencil } from "lucide-react";
import { useProjectStore } from "@/stores/projectStore";
import { Button } from "../ui/Button";
import { Input } from "../ui/Input";
import { BriefEditor } from "./BriefEditor";
import { SourceList } from "./SourceList";

export const ProjectDetail: React.FC = () => {
  const { t } = useTranslation();
  const activeProjectId = useProjectStore((s) => s.activeProjectId);
  const project = useProjectStore((s) =>
    s.projects.find((p) => p.id === s.activeProjectId),
  );
  const closeProject = useProjectStore((s) => s.closeProject);
  const renameProject = useProjectStore((s) => s.renameProject);
  const [editing, setEditing] = useState(false);
  const [title, setTitle] = useState(project?.title ?? "");

  if (activeProjectId === null || !project) return null;

  const commit = () => {
    const next = title.trim();
    if (next && next !== project.title) renameProject(project.id, next);
    setEditing(false);
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center gap-2">
        <Button
          variant="ghost"
          size="sm"
          onClick={closeProject}
          title={t("projects.back")}
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
            {project.title}
          </h1>
        )}
        <Button
          variant="ghost"
          size="sm"
          title={t("projects.rename")}
          onClick={() => {
            setTitle(project.title);
            setEditing(true);
          }}
        >
          <Pencil className="w-4 h-4" />
        </Button>
      </div>
      <BriefEditor projectId={project.id} brief={project.brief} />
      <SourceList projectId={project.id} />
    </div>
  );
};
```

- [ ] **Step 7: Sidebar and default section**

In `src/components/Sidebar.tsx`, import `FolderKanban` from `lucide-react` and `ProjectsSection` from `./projects/ProjectsSection`, and add as the FIRST entry of `SECTIONS_CONFIG`:

```ts
  projects: {
    labelKey: "sidebar.projects",
    icon: FolderKanban,
    component: ProjectsSection,
    enabled: () => true,
  },
```

`SectionConfig.icon` expects a component taking `IconProps` (`width`, `height`, `className`); lucide icons satisfy that, as `History` and `Cpu` already do.

In `src/App.tsx` change the initial state `useState<SidebarSection>("general")` to `useState<SidebarSection>("projects")`.

- [ ] **Step 8: Verify**

Run from repo root:

```bash
bunx tsc --noEmit && bun run lint 2>&1 | tail -3 && bun run build 2>&1 | tail -2
```

Expected: all exit 0; `vite build` prints `built in`.

Then a manual smoke in the real app (do this once; it is the only end-to-end proof in this plan):

```bash
cd src-tauri && cargo run 2>&1 | grep -i "project\|indexed\|error" &
```

In the window: Projects is the first sidebar item and the landing page. Create a project, type a brief, add a text source and a folder source pointing at this repo. Both reach "Ready". Close the app (`Cmd+Q`) and stop the background cargo process.

- [ ] **Step 9: Commit**

```bash
git add src/components/projects src/components/Sidebar.tsx src/App.tsx
git commit -m "feat: add Projects screens as the default landing section"
```

---

### Task 11: Format, clippy, and docs touch

**Files:**

- Modify: `docs/ARCHITECTURE.md` (mark store, indexer, projects UI rows as existing)

- [ ] **Step 1: Format and lint everything**

```bash
bun run format 2>&1 | tail -1
cd src-tauri && cargo clippy --tests 2>&1 | grep -E "^(warning|error).*meeting" | head
```

Expected: `cargo clippy` prints no warnings in `meeting/` or `commands/project.rs`. Fix any it prints.

- [ ] **Step 2: Full Rust test run**

Run from `src-tauri/`: `cargo test --lib 2>&1 | tail -3`
Expected: `test result: ok`, zero failed. If a pre-existing test fails outside `meeting::`, report it by name in the final message and do not fix it here.

- [ ] **Step 3: Docs**

In `docs/ARCHITECTURE.md`, move these rows from the "Planned" tables to the "Existing" tables (keep wording): `meeting/store.rs`, `meeting/indexer.rs`, `commands/project.rs`, `stores/projectStore.ts`, `components/projects/ProjectList.tsx`, `components/projects/ProjectDetail.tsx`. Add a row for `meeting/manager.rs` ("`ProjectManager`: Tauri state, background indexing thread, `source-status-payload` event") and `meeting/chunk.rs` ("paragraph-aware chunking"). Update the Delivery order to mark step 2 done.

Run: `bunx prettier --check docs/ARCHITECTURE.md`
Expected: passes.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "chore: format, clippy, and architecture doc update for projects"
```
