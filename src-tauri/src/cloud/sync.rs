//! One sync cycle over projects, meetings, and their children: push the local
//! outbox, then pull rows other devices wrote. Knows nothing about Tauri, so
//! it can run against two temp databases in a test.

use super::auth::Session;
use super::local::{self, Entity, Outgoing, ENTITIES};
use super::{ANON_KEY, SUPABASE_URL};
use crate::meeting::store::ProjectStore;
use serde_json::Value;

const PUSH_BATCH: usize = 500;
const PUSH_BATCHES_PER_CYCLE: usize = 20;
const PULL_PAGE: usize = 1000;
/// Re-read this much before the cursor: a row can commit with an earlier
/// `synced_at` than one already seen. Re-applying it is a no-op.
const PULL_OVERLAP_SECS: i64 = 10;
/// A child whose parent has not arrived holds the cursor back this long, then
/// is dropped so an orphan cannot pin the cursor forever.
const ORPHAN_GRACE_SECS: i64 = 600;

#[derive(Debug)]
pub enum SyncError {
    Network(String),
    Unauthorized,
    Other(String),
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncError::Network(m) | SyncError::Other(m) => f.write_str(m),
            SyncError::Unauthorized => f.write_str("session expired"),
        }
    }
}

impl From<anyhow::Error> for SyncError {
    fn from(e: anyhow::Error) -> Self {
        SyncError::Other(format!("{e:#}"))
    }
}

impl From<reqwest::Error> for SyncError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_connect() || e.is_timeout() || e.is_request() {
            SyncError::Network(e.to_string())
        } else {
            SyncError::Other(e.to_string())
        }
    }
}

#[derive(Debug, Default)]
pub struct CycleReport {
    pub pushed: usize,
    pub pulled: usize,
    pub sources_pulled: bool,
}

pub struct Remote<'a> {
    pub http: &'a reqwest::Client,
    pub session: &'a Session,
}

impl Remote<'_> {
    fn request(&self, method: reqwest::Method, table: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{SUPABASE_URL}/rest/v1/{table}"))
            .header("apikey", ANON_KEY)
            .bearer_auth(&self.session.access_token)
    }

    pub async fn upsert(
        &self,
        table: &str,
        conflict: &str,
        rows: &[Value],
    ) -> Result<(), SyncError> {
        let res = self
            .request(reqwest::Method::POST, table)
            .query(&[("on_conflict", conflict)])
            .header("Prefer", "resolution=merge-duplicates,return=minimal")
            .json(rows)
            .send()
            .await?;
        check(res).await.map(|_| ())
    }

    pub async fn select(
        &self,
        table: &str,
        query: &[(&str, String)],
    ) -> Result<Vec<Value>, SyncError> {
        let res = self
            .request(reqwest::Method::GET, table)
            .query(query)
            .send()
            .await?;
        let text = check(res).await?;
        serde_json::from_str(&text)
            .map_err(|e| SyncError::Other(format!("bad {table} response: {e}")))
    }
}

async fn check(res: reqwest::Response) -> Result<String, SyncError> {
    let status = res.status();
    let text = res.text().await?;
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(SyncError::Unauthorized);
    }
    if status.is_server_error() {
        return Err(SyncError::Network(format!("HTTP {status}")));
    }
    if !status.is_success() {
        return Err(SyncError::Other(format!("HTTP {status}: {text}")));
    }
    Ok(text)
}

pub async fn run_cycle(
    store: &ProjectStore,
    remote: &Remote<'_>,
) -> Result<CycleReport, SyncError> {
    let mut report = CycleReport::default();
    for e in ENTITIES {
        report.pushed += push_entity(store, remote, e).await?;
    }
    for e in ENTITIES {
        let n = pull_entity(store, remote, e).await?;
        report.pulled += n;
        if e.table == "sources" && n > 0 {
            report.sources_pulled = true;
        }
    }
    Ok(report)
}

async fn push_entity(
    store: &ProjectStore,
    remote: &Remote<'_>,
    e: &Entity,
) -> Result<usize, SyncError> {
    let mut pushed = 0;
    for _ in 0..PUSH_BATCHES_PER_CYCLE {
        let batch = local::outbox_batch(&store.conn()?, e, PUSH_BATCH)?;
        if batch.is_empty() {
            break;
        }
        let (tombstones, live): (Vec<&Outgoing>, Vec<&Outgoing>) = batch
            .iter()
            .partition(|o| o.row["deleted"].as_bool().unwrap_or(false));
        if !live.is_empty() {
            let rows: Vec<Value> = live.iter().map(|o| o.row.clone()).collect();
            remote.upsert(e.table, "id", &rows).await?;
        }
        if !tombstones.is_empty() {
            // Only these keys, so a tombstone never blanks fields on a row the
            // server already has, and bulk rows share one key set.
            let rows: Vec<Value> = tombstones
                .iter()
                .map(|o| serde_json::json!({"id": o.row["id"], "updated_at": o.row["updated_at"], "deleted": true}))
                .collect();
            remote.upsert(e.table, "id", &rows).await?;
        }
        local::ack(&mut store.conn()?, e, &batch)?;
        pushed += batch.len();
        if batch.len() < PUSH_BATCH {
            break;
        }
    }
    Ok(pushed)
}

async fn pull_entity(
    store: &ProjectStore,
    remote: &Remote<'_>,
    e: &Entity,
) -> Result<usize, SyncError> {
    let since = local::cursor(&store.conn()?, e.table)?.and_then(|c| shift(&c, -PULL_OVERLAP_SECS));
    let orphan_cutoff = chrono::Utc::now() - chrono::Duration::seconds(ORPHAN_GRACE_SECS);
    let mut after: Option<(String, String)> = None;
    let mut newest: Option<String> = None;
    let mut held_back: Option<String> = None;
    let mut applied = 0;
    loop {
        let mut query = vec![
            ("select", "*".to_string()),
            ("order", "synced_at.asc,id.asc".to_string()),
            ("limit", PULL_PAGE.to_string()),
        ];
        match (&after, &since) {
            (Some((ts, id)), _) => query.push((
                "or",
                format!("(synced_at.gt.\"{ts}\",and(synced_at.eq.\"{ts}\",id.gt.{id}))"),
            )),
            (None, Some(s)) => query.push(("synced_at", format!("gte.{s}"))),
            (None, None) => {}
        }
        let rows = remote.select(e.table, &query).await?;
        let outcome = local::apply_remote(&mut store.conn()?, e, &rows)?;
        applied += outcome.applied;
        if held_back.is_none() {
            if let Some(i) = outcome.first_blocked {
                let ts = rows[i]["synced_at"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                let recent = chrono::DateTime::parse_from_rfc3339(&ts)
                    .map(|t| t > orphan_cutoff)
                    .unwrap_or(false);
                if recent {
                    held_back = Some(ts);
                }
            }
        }
        if let Some(last) = rows.last() {
            let ts = last["synced_at"].as_str().unwrap_or_default().to_string();
            let id = last["id"].as_str().unwrap_or_default().to_string();
            newest = Some(ts.clone());
            after = Some((ts, id));
        }
        if rows.len() < PULL_PAGE {
            break;
        }
    }
    if let Some(next) = held_back.or(newest) {
        local::set_cursor(&store.conn()?, e.table, &next)?;
    }
    Ok(applied)
}

fn shift(ts: &str, secs: i64) -> Option<String> {
    let t = chrono::DateTime::parse_from_rfc3339(ts).ok()?;
    Some((t + chrono::Duration::seconds(secs)).to_rfc3339())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meeting::store::MeetingStatus;

    /// Live round trip against the CASE Supabase project with a throwaway
    /// account. Run with `cargo test sync_round_trip -- --ignored`.
    #[test]
    #[ignore]
    fn sync_round_trip_against_live_project() {
        tauri::async_runtime::block_on(async {
            let http = reqwest::Client::new();
            let email = format!("sync-e2e-{}@case.test", uuid_ish());
            let password = uuid_ish();
            let session = super::super::auth::sign_up(&http, &email, &password)
                .await
                .expect("sign up");
            let remote = Remote {
                http: &http,
                session: &session,
            };

            let dir = tempfile::tempdir().unwrap();
            let a = ProjectStore::open(dir.path().join("a.db")).unwrap();
            let b = ProjectStore::open(dir.path().join("b.db")).unwrap();

            let p = a.create_project("Roadmap").unwrap();
            a.update_project_brief(p.id, "Q4 plan").unwrap();
            let m = a.create_meeting(Some(p.id), "Planning").unwrap();
            let s = a.create_session(m.id).unwrap();
            a.append_line(s.id, "me", "ship sync", 1).unwrap();
            a.append_line(s.id, "them", "agreed", 2).unwrap();
            a.set_session_summary(s.id, "{\"decisions\":[\"ship\"]}")
                .unwrap();
            a.set_meeting_status(m.id, MeetingStatus::Ended).unwrap();
            let gone = a.create_meeting(Some(p.id), "Scratch").unwrap();
            a.delete_meeting(gone.id).unwrap();

            let ra = run_cycle(&a, &remote).await.expect("cycle A");
            println!("A first cycle: {ra:?}");
            assert_eq!(
                local::pending_count(&a.conn().unwrap()).unwrap(),
                0,
                "A outbox drained"
            );

            let rb = run_cycle(&b, &remote).await.expect("cycle B");
            println!("B first cycle: {rb:?}");
            let bp = b.list_projects().unwrap();
            assert_eq!(bp.len(), 1);
            assert_eq!(bp[0].title, "Roadmap");
            assert_eq!(bp[0].brief, "Q4 plan");
            let bm = b.list_meetings(None).unwrap();
            assert_eq!(bm.len(), 1, "deleted meeting did not arrive");
            assert_eq!(bm[0].title, "Planning");
            assert_eq!(bm[0].project_id, Some(bp[0].id));
            let detail = b.get_meeting_detail(bm[0].id).unwrap().unwrap();
            assert_eq!(detail.sessions.len(), 1);
            let lines = b.session_lines(detail.sessions[0].id).unwrap();
            assert_eq!(
                lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(),
                ["ship sync", "agreed"]
            );
            assert_eq!(
                detail.sessions[0].summary.as_deref(),
                Some("{\"decisions\":[\"ship\"]}")
            );
            assert_eq!(
                local::pending_count(&b.conn().unwrap()).unwrap(),
                0,
                "pull did not echo"
            );

            b.update_meeting_title(bm[0].id, "Planning (edited on B)")
                .unwrap();
            run_cycle(&b, &remote).await.expect("cycle B push edit");
            run_cycle(&a, &remote).await.expect("cycle A pull edit");
            let am = a.list_meetings(None).unwrap();
            println!(
                "A after B edit: {:?}",
                am.iter().map(|m| &m.title).collect::<Vec<_>>()
            );
            assert_eq!(am.len(), 1);
            assert_eq!(am[0].title, "Planning (edited on B)");
            assert_eq!(
                local::pending_count(&a.conn().unwrap()).unwrap(),
                0,
                "pulled edit did not echo"
            );

            b.delete_project(bp[0].id).unwrap();
            run_cycle(&b, &remote).await.expect("cycle B push delete");
            run_cycle(&a, &remote).await.expect("cycle A pull delete");
            assert!(
                a.list_projects().unwrap().is_empty(),
                "project delete reached A"
            );
            assert!(
                a.list_meetings(None).unwrap().is_empty(),
                "cascaded meeting delete reached A"
            );
            println!("A after B delete: projects and meetings empty");
        });
    }

    fn uuid_ish() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("{n:x}{:x}", std::process::id())
    }
}
