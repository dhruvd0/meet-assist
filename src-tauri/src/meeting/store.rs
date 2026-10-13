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
        let conn = self.conn()?;
        conn.execute("DELETE FROM projects WHERE id = ?1", params![id])?;
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
}
