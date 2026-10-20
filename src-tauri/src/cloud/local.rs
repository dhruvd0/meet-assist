//! SQLite side of cloud sync: the entity registry, reading the outbox,
//! acknowledging pushed rows, and applying pulled rows without echoing them
//! back into the outbox. Pure rusqlite, no network.

use anyhow::Result;
use rusqlite::{
    params, types::Value as SqlValue, Connection, OptionalExtension, TransactionBehavior,
};
use serde_json::{Map, Value};

#[derive(Clone, Copy)]
pub enum ColKind {
    Text,
    Int,
    Bool,
}

pub struct Col {
    pub name: &'static str,
    pub kind: ColKind,
}

/// A local integer foreign key that travels as the parent's uid.
pub struct Parent {
    pub column: &'static str,
    pub table: &'static str,
    pub nullable: bool,
}

pub struct Entity {
    /// Local and remote table name (they match).
    pub table: &'static str,
    pub parent: Option<Parent>,
    pub cols: &'static [Col],
    /// The local table has its own NOT NULL `updated_at`, filled from the remote one.
    pub local_updated_at: bool,
    /// A pulled change to this column resets `status` to `pending` so the
    /// indexer rebuilds local-only derived data.
    pub reindex_on: Option<&'static str>,
}

const fn text(name: &'static str) -> Col {
    Col {
        name,
        kind: ColKind::Text,
    }
}
const fn int(name: &'static str) -> Col {
    Col {
        name,
        kind: ColKind::Int,
    }
}

/// Parent-first: push and pull walk this order so children find their parents.
pub static ENTITIES: &[Entity] = &[
    Entity {
        table: "projects",
        parent: None,
        cols: &[text("title"), text("brief"), int("created_at")],
        local_updated_at: true,
        reindex_on: None,
    },
    Entity {
        table: "sources",
        parent: Some(Parent {
            column: "project_id",
            table: "projects",
            nullable: false,
        }),
        cols: &[
            text("kind"),
            text("title"),
            text("locator"),
            int("added_at"),
            text("digest"),
        ],
        local_updated_at: false,
        reindex_on: Some("locator"),
    },
    Entity {
        table: "meetings",
        parent: Some(Parent {
            column: "project_id",
            table: "projects",
            nullable: true,
        }),
        cols: &[
            text("title"),
            int("created_at"),
            text("status"),
            text("context"),
            text("rolling_summary"),
            text("filed_by"),
            text("input_mode"),
        ],
        local_updated_at: true,
        reindex_on: None,
    },
    Entity {
        table: "sessions",
        parent: Some(Parent {
            column: "meeting_id",
            table: "meetings",
            nullable: false,
        }),
        cols: &[int("started_at"), int("ended_at"), text("summary")],
        local_updated_at: false,
        reindex_on: None,
    },
    Entity {
        table: "transcript_lines",
        parent: Some(Parent {
            column: "session_id",
            table: "sessions",
            nullable: false,
        }),
        cols: &[int("ts"), text("speaker"), text("text")],
        local_updated_at: false,
        reindex_on: None,
    },
    Entity {
        table: "interventions",
        parent: Some(Parent {
            column: "session_id",
            table: "sessions",
            nullable: false,
        }),
        cols: &[
            int("ts"),
            text("kind"),
            text("text"),
            text("reason"),
            Col {
                name: "dismissed",
                kind: ColKind::Bool,
            },
        ],
        local_updated_at: false,
        reindex_on: None,
    },
];

/// One outbox entry ready to send, with the version read so the ack can tell
/// whether the row changed again while the push was in flight.
pub struct Outgoing {
    pub uid: String,
    pub version: i64,
    pub row: Value,
}

pub fn pending_count(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("SELECT COUNT(*) FROM sync_outbox", [], |r| r.get(0))?)
}

pub fn outbox_batch(conn: &Connection, e: &Entity, limit: usize) -> Result<Vec<Outgoing>> {
    let mut select = vec![
        "o.uid".to_string(),
        "o.deleted".into(),
        "o.changed_at".into(),
        "o.version".into(),
        "t.id".into(),
    ];
    select.extend(e.cols.iter().map(|c| format!("t.{}", c.name)));
    let parent_join = match &e.parent {
        Some(p) => {
            select.push("p.uid".into());
            format!("LEFT JOIN {} p ON p.id = t.{}", p.table, p.column)
        }
        None => String::new(),
    };
    let sql = format!(
        "SELECT {} FROM sync_outbox o LEFT JOIN {} t ON t.uid = o.uid {} \
         WHERE o.entity = ?1 ORDER BY o.changed_at LIMIT ?2",
        select.join(", "),
        e.table,
        parent_join
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![e.table, limit as i64], |r| {
        let uid: String = r.get(0)?;
        let deleted: bool = r.get::<_, i64>(1)? != 0;
        let changed_at: i64 = r.get(2)?;
        let version: i64 = r.get(3)?;
        let local_id: Option<i64> = r.get(4)?;
        let tombstone = deleted || local_id.is_none();
        let mut obj = Map::new();
        obj.insert("id".into(), Value::String(uid.clone()));
        obj.insert("updated_at".into(), Value::from(changed_at));
        obj.insert("deleted".into(), Value::Bool(tombstone));
        for (i, c) in e.cols.iter().enumerate() {
            let v = if tombstone {
                Value::Null
            } else {
                to_json(r.get(5 + i)?, c.kind)
            };
            obj.insert(c.name.into(), v);
        }
        if let Some(p) = &e.parent {
            let v: Option<String> = if tombstone {
                None
            } else {
                r.get(5 + e.cols.len())?
            };
            obj.insert(p.column.into(), v.map(Value::String).unwrap_or(Value::Null));
        }
        Ok(Outgoing {
            uid,
            version,
            row: Value::Object(obj),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn ack(conn: &mut Connection, e: &Entity, sent: &[Outgoing]) -> Result<()> {
    let tx = conn.transaction()?;
    {
        let mut stmt =
            tx.prepare("DELETE FROM sync_outbox WHERE entity = ?1 AND uid = ?2 AND version = ?3")?;
        for o in sent {
            stmt.execute(params![e.table, o.uid, o.version])?;
        }
    }
    tx.commit()?;
    Ok(())
}

pub struct ApplyOutcome {
    pub applied: usize,
    /// Index of the first row skipped because its parent is not here yet.
    pub first_blocked: Option<usize>,
}

/// Apply remote rows in one transaction with the outbox triggers muted.
pub fn apply_remote(conn: &mut Connection, e: &Entity, rows: &[Value]) -> Result<ApplyOutcome> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute("UPDATE sync_guard SET applying = 1", [])?;
    let upsert = upsert_sql(e);
    let mut applied = 0;
    let mut first_blocked = None;
    for (i, row) in rows.iter().enumerate() {
        let Some(uid) = row.get("id").and_then(Value::as_str) else {
            continue;
        };
        let local_pending: bool = tx
            .query_row(
                "SELECT 1 FROM sync_outbox WHERE entity = ?1 AND uid = ?2",
                params![e.table, uid],
                |_| Ok(true),
            )
            .optional()?
            .is_some();
        if local_pending {
            continue;
        }
        if row.get("deleted").and_then(Value::as_bool).unwrap_or(false) {
            applied += tx.execute(
                &format!("DELETE FROM {} WHERE uid = ?1", e.table),
                params![uid],
            )?;
            continue;
        }
        let mut values: Vec<SqlValue> = vec![SqlValue::Text(uid.to_string())];
        if let Some(p) = &e.parent {
            let parent_uid = row.get(p.column).and_then(Value::as_str);
            let parent_id: Option<i64> = match parent_uid {
                Some(pu) => tx
                    .query_row(
                        &format!("SELECT id FROM {} WHERE uid = ?1", p.table),
                        params![pu],
                        |r| r.get(0),
                    )
                    .optional()?,
                None => None,
            };
            match parent_id {
                Some(id) => values.push(SqlValue::Integer(id)),
                None if p.nullable && parent_uid.is_none() => values.push(SqlValue::Null),
                None => {
                    first_blocked.get_or_insert(i);
                    continue;
                }
            }
        }
        for c in e.cols {
            values.push(from_json(row.get(c.name).unwrap_or(&Value::Null), c.kind));
        }
        if e.local_updated_at {
            values.push(SqlValue::Integer(
                row.get("updated_at").and_then(Value::as_i64).unwrap_or(0),
            ));
        }
        applied += tx.execute(&upsert, rusqlite::params_from_iter(values))?;
    }
    tx.execute("UPDATE sync_guard SET applying = 0", [])?;
    tx.commit()?;
    Ok(ApplyOutcome {
        applied,
        first_blocked,
    })
}

fn upsert_sql(e: &Entity) -> String {
    let mut cols: Vec<&str> = vec!["uid"];
    if let Some(p) = &e.parent {
        cols.push(p.column);
    }
    cols.extend(e.cols.iter().map(|c| c.name));
    if e.local_updated_at {
        cols.push("updated_at");
    }
    let placeholders: Vec<String> = (1..=cols.len()).map(|i| format!("?{i}")).collect();
    let mut sets: Vec<String> = Vec::new();
    if let Some(field) = e.reindex_on {
        sets.push(format!(
            "status = CASE WHEN {t}.{f} IS NOT excluded.{f} THEN 'pending' ELSE {t}.status END",
            t = e.table,
            f = field
        ));
    }
    sets.extend(cols.iter().skip(1).map(|c| format!("{c} = excluded.{c}")));
    format!(
        "INSERT INTO {} ({}) VALUES ({}) ON CONFLICT(uid) DO UPDATE SET {}",
        e.table,
        cols.join(", "),
        placeholders.join(", "),
        sets.join(", ")
    )
}

fn to_json(v: SqlValue, kind: ColKind) -> Value {
    match (v, kind) {
        (SqlValue::Integer(i), ColKind::Bool) => Value::Bool(i != 0),
        (SqlValue::Integer(i), _) => Value::from(i),
        (SqlValue::Real(f), _) => Value::from(f),
        (SqlValue::Text(s), _) => Value::String(s),
        _ => Value::Null,
    }
}

fn from_json(v: &Value, kind: ColKind) -> SqlValue {
    match (v, kind) {
        (Value::Bool(b), _) => SqlValue::Integer(*b as i64),
        (Value::Number(n), ColKind::Int | ColKind::Bool) => {
            n.as_i64().map(SqlValue::Integer).unwrap_or(SqlValue::Null)
        }
        (Value::String(s), ColKind::Text) => SqlValue::Text(s.clone()),
        _ => SqlValue::Null,
    }
}

pub fn cursor(conn: &Connection, entity: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT synced_at FROM sync_cursor WHERE entity = ?1",
            params![entity],
            |r| r.get(0),
        )
        .optional()?)
}

pub fn set_cursor(conn: &Connection, entity: &str, synced_at: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_cursor (entity, synced_at) VALUES (?1, ?2) \
         ON CONFLICT(entity) DO UPDATE SET synced_at = excluded.synced_at",
        params![entity, synced_at],
    )?;
    Ok(())
}

pub fn meta(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT value FROM sync_meta WHERE key = ?1",
            params![key],
            |r| r.get(0),
        )
        .optional()?)
}

pub fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_meta (key, value) VALUES (?1, ?2) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meeting::store::ProjectStore;

    fn store() -> (tempfile::TempDir, ProjectStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(dir.path().join("meet.db")).unwrap();
        (dir, store)
    }

    fn entity(table: &str) -> &'static Entity {
        ENTITIES.iter().find(|e| e.table == table).unwrap()
    }

    fn outbox(conn: &Connection) -> Vec<(String, String, bool)> {
        let mut stmt = conn
            .prepare("SELECT entity, uid, deleted FROM sync_outbox ORDER BY entity, uid")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? != 0)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    #[test]
    fn local_writes_record_outbox_rows_including_cascades() {
        let (_d, s) = store();
        let p = s.create_project("P").unwrap();
        let m = s.create_meeting(Some(p.id), "M").unwrap();
        let sess = s.create_session(m.id).unwrap();
        s.append_line(sess.id, "me", "hello", 1).unwrap();
        let mut conn = s.conn().unwrap();
        let rows = outbox(&conn);
        for t in ["projects", "meetings", "sessions", "transcript_lines"] {
            assert!(
                rows.iter().any(|(e, _, d)| e == t && !d),
                "insert recorded for {t}"
            );
        }
        for e in ENTITIES {
            let batch = outbox_batch(&conn, e, 100).unwrap();
            ack(&mut conn, e, &batch).unwrap();
        }
        assert_eq!(pending_count(&conn).unwrap(), 0, "ack clears the outbox");

        s.update_project_title(p.id, "P2").unwrap();
        assert_eq!(outbox(&conn).len(), 1, "update recorded once");
        s.delete_meeting(m.id).unwrap();
        let rows = outbox(&conn);
        for t in ["meetings", "sessions", "transcript_lines"] {
            assert!(
                rows.iter().any(|(e, _, d)| e == t && *d),
                "cascaded delete recorded for {t}"
            );
        }
    }

    #[test]
    fn applying_remote_rows_does_not_echo_into_outbox() {
        let (_d, s) = store();
        let mut conn = s.conn().unwrap();
        let project = serde_json::json!({"id": "00000000-0000-4000-8000-000000000001", "updated_at": 5, "deleted": false, "title": "Remote", "brief": "b", "created_at": 1});
        let meeting = serde_json::json!({"id": "00000000-0000-4000-8000-000000000002", "updated_at": 5, "deleted": false, "project_id": "00000000-0000-4000-8000-000000000001", "title": "RM", "created_at": 1, "status": "ended", "context": "", "rolling_summary": null, "filed_by": null, "input_mode": "audio"});
        assert_eq!(
            apply_remote(&mut conn, entity("projects"), &[project])
                .unwrap()
                .applied,
            1
        );
        assert_eq!(
            apply_remote(&mut conn, entity("meetings"), &[meeting])
                .unwrap()
                .applied,
            1
        );
        assert_eq!(
            pending_count(&conn).unwrap(),
            0,
            "pulled rows stay out of the outbox"
        );
        let meetings = s.list_meetings(None).unwrap();
        assert_eq!(meetings.len(), 1);
        assert_eq!(meetings[0].title, "RM");
        assert!(
            meetings[0].project_id.is_some(),
            "parent uid mapped to local id"
        );

        let tombstone = serde_json::json!({"id": "00000000-0000-4000-8000-000000000002", "updated_at": 6, "deleted": true});
        apply_remote(&mut conn, entity("meetings"), &[tombstone]).unwrap();
        assert!(
            s.list_meetings(None).unwrap().is_empty(),
            "tombstone deletes locally"
        );
        assert_eq!(pending_count(&conn).unwrap(), 0);
    }

    #[test]
    fn stale_ack_keeps_a_row_changed_mid_push() {
        let (_d, s) = store();
        let p = s.create_project("P").unwrap();
        let mut conn = s.conn().unwrap();
        let batch = outbox_batch(&conn, entity("projects"), 10).unwrap();
        s.update_project_title(p.id, "changed while pushing")
            .unwrap();
        ack(&mut conn, entity("projects"), &batch).unwrap();
        assert_eq!(
            pending_count(&conn).unwrap(),
            1,
            "newer version survives the ack"
        );
    }

    #[test]
    fn pending_local_edit_wins_over_pulled_row() {
        let (_d, s) = store();
        let p = s.create_project("Local").unwrap();
        let mut conn = s.conn().unwrap();
        let uid: String = conn
            .query_row("SELECT uid FROM projects WHERE id = ?1", [p.id], |r| {
                r.get(0)
            })
            .unwrap();
        let remote = serde_json::json!({"id": uid, "updated_at": 9_999_999_999_999i64, "deleted": false, "title": "Remote", "brief": "", "created_at": 1});
        apply_remote(&mut conn, entity("projects"), &[remote]).unwrap();
        assert_eq!(s.get_project(p.id).unwrap().unwrap().title, "Local");
    }

    #[test]
    fn migration_backfills_uids_and_seeds_outbox() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("meet.db");
        {
            let mut conn = Connection::open(&path).unwrap();
            let pre_sync: Vec<_> = crate::meeting::store::migrations_before_sync();
            rusqlite_migration::Migrations::new(pre_sync)
                .to_latest(&mut conn)
                .unwrap();
            conn.execute("INSERT INTO projects (title, brief, created_at, updated_at) VALUES ('Old', '', 1, 1)", []).unwrap();
        }
        let s = ProjectStore::open(path).unwrap();
        let conn = s.conn().unwrap();
        let uid: Option<String> = conn
            .query_row("SELECT uid FROM projects", [], |r| r.get(0))
            .unwrap();
        let uid = uid.expect("existing row backfilled");
        assert_eq!(uid.len(), 36);
        assert_eq!(&uid[14..15], "4", "uuid v4 shape");
        assert_eq!(
            outbox(&conn),
            vec![("projects".into(), uid, false)],
            "history queued for first upload"
        );
    }

    #[test]
    fn pulled_source_with_new_locator_is_reindexed() {
        let (_d, s) = store();
        let p = s.create_project("P").unwrap();
        let src = s
            .add_source(p.id, crate::meeting::store::SourceKind::Text, "N", "old")
            .unwrap();
        s.set_source_status(src.id, crate::meeting::store::SourceStatus::Ready, None)
            .unwrap();
        let mut conn = s.conn().unwrap();
        for e in ENTITIES {
            let batch = outbox_batch(&conn, e, 100).unwrap();
            ack(&mut conn, e, &batch).unwrap();
        }
        let (uid, puid): (String, String) = conn
            .query_row(
                "SELECT s.uid, p.uid FROM sources s JOIN projects p ON p.id = s.project_id",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        let remote = serde_json::json!({"id": uid, "updated_at": 5, "deleted": false, "project_id": puid, "kind": "text", "title": "N", "locator": "new", "added_at": 1, "digest": null});
        apply_remote(&mut conn, entity("sources"), &[remote]).unwrap();
        let status: String = conn
            .query_row("SELECT status FROM sources", [], |r| r.get(0))
            .unwrap();
        assert_eq!(status, "pending");
        assert_eq!(pending_count(&conn).unwrap(), 0);
    }
}
