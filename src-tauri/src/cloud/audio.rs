//! Recording backup. Each session's joined recording is copied to the
//! account's private storage folder next to a small manifest that says where
//! every line sits in it. Another device reads the manifest during sync and
//! downloads the recording itself only when the user plays it.

use super::auth::Session;
use super::sync::SyncError;
use super::{anon_key, supabase_url};
use crate::meeting::store::{LineAudio, ProjectStore, SessionRecording};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

const BUCKET: &str = "meeting-audio";
const LIST_PAGE: usize = 1000;
/// A long backfill is spread over cycles so each one stays short.
const UPLOADS_PER_CYCLE: usize = 5;
const RESTORES_PER_CYCLE: usize = 50;
/// A one-hour recording is about 11 MB; slow links need longer than the
/// client's default timeout.
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Default)]
pub struct AudioReport {
    /// Local recordings still waiting to be uploaded after this run.
    pub waiting: usize,
    /// Recordings from other devices that became playable here.
    pub restored: usize,
    /// The server refused an upload; uploads stay paused until asked again.
    pub refused: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    version: u32,
    lines: Vec<ManifestLine>,
}

#[derive(Serialize, Deserialize)]
struct ManifestLine {
    /// The transcript line's sync id.
    id: String,
    start_ms: i64,
    ms: i64,
}

pub struct Storage<'a> {
    pub http: &'a reqwest::Client,
    pub session: &'a Session,
}

impl Storage<'_> {
    fn object(&self, name: &str) -> String {
        format!("{}/{name}", self.session.user_id)
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}/storage/v1/{path}", supabase_url()))
            .header("apikey", anon_key())
            .bearer_auth(&self.session.access_token)
    }

    /// Names of every object in the account's folder.
    async fn list(&self) -> Result<HashSet<String>, SyncError> {
        let mut names = HashSet::new();
        let mut offset = 0;
        loop {
            let res = self
                .request(reqwest::Method::POST, &format!("object/list/{BUCKET}"))
                .json(&json!({
                    "prefix": self.session.user_id,
                    "limit": LIST_PAGE,
                    "offset": offset,
                    "sortBy": {"column": "name", "order": "asc"},
                }))
                .send()
                .await?;
            let text = check(res).await?;
            let page: Vec<Value> = serde_json::from_str(&text)
                .map_err(|e| SyncError::Other(format!("bad storage list: {e}")))?;
            names.extend(
                page.iter()
                    .filter_map(|o| o["name"].as_str().map(str::to_string)),
            );
            if page.len() < LIST_PAGE {
                return Ok(names);
            }
            offset += LIST_PAGE;
        }
    }

    async fn upload(&self, name: &str, mime: &str, body: Vec<u8>) -> Result<(), SyncError> {
        let res = self
            .request(
                reqwest::Method::POST,
                &format!("object/{BUCKET}/{}", self.object(name)),
            )
            .header("content-type", mime)
            .header("x-upsert", "true")
            .timeout(TRANSFER_TIMEOUT)
            .body(body)
            .send()
            .await?;
        check(res).await.map(|_| ())
    }

    async fn download(&self, name: &str) -> Result<Vec<u8>, SyncError> {
        let res = self
            .request(
                reqwest::Method::GET,
                &format!("object/authenticated/{BUCKET}/{}", self.object(name)),
            )
            .timeout(TRANSFER_TIMEOUT)
            .send()
            .await?;
        let status = res.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(SyncError::Unauthorized);
        }
        if !status.is_success() {
            return Err(SyncError::Other(format!("HTTP {status}")));
        }
        Ok(res.bytes().await?.to_vec())
    }

    async fn remove(&self, names: &[String]) -> Result<(), SyncError> {
        let prefixes: Vec<String> = names.iter().map(|n| self.object(n)).collect();
        let res = self
            .request(reqwest::Method::DELETE, &format!("object/{BUCKET}"))
            .json(&json!({ "prefixes": prefixes }))
            .send()
            .await?;
        check(res).await.map(|_| ())
    }
}

/// Like the table client's check, except that a refused request keeps its
/// status so an upload the server will not take can be told apart.
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
        let message = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| v["message"].as_str().map(str::to_string))
            .unwrap_or(text);
        return Err(SyncError::Other(format!("HTTP {status}: {message}")));
    }
    Ok(text)
}

fn ogg(uid: &str) -> String {
    format!("{uid}.ogg")
}

/// The manifest is written after the recording, so its presence means the
/// upload finished.
fn manifest(uid: &str) -> String {
    format!("{uid}.json")
}

/// One backup pass: remove recordings of deleted meetings, upload new ones
/// (when `upload` is on), and make other devices' recordings playable here.
pub async fn run(
    store: &ProjectStore,
    storage: &Storage<'_>,
    upload: bool,
) -> Result<AudioReport, SyncError> {
    let mut report = AudioReport::default();
    store.prune_orphan_audio()?;

    let deleted = store.audio_deletions()?;
    if !deleted.is_empty() {
        let names: Vec<String> = deleted
            .iter()
            .flat_map(|uid| [manifest(uid), ogg(uid)])
            .collect();
        storage.remove(&names).await?;
        store.clear_audio_deletions(&deleted)?;
    }

    let remote = storage.list().await?;

    let local = store.session_recordings()?;
    let waiting: Vec<&SessionRecording> = local
        .iter()
        .filter(|r| r.on_disk && !remote.contains(&manifest(&r.uid)))
        .collect();
    let mut uploaded = 0;
    if upload {
        for rec in waiting.iter().take(UPLOADS_PER_CYCLE) {
            match upload_one(store, storage, rec).await {
                Ok(()) => uploaded += 1,
                // The server will not take it (too large, storage full).
                Err(SyncError::Other(message)) => {
                    report.refused = Some(message);
                    break;
                }
                Err(e) => return Err(e),
            }
        }
    }
    report.waiting = waiting.len() - uploaded;

    let missing = store.sessions_without_recording()?;
    for (session_id, uid) in missing
        .iter()
        .filter(|(_, uid)| remote.contains(&manifest(uid)))
        .take(RESTORES_PER_CYCLE)
    {
        let bytes = storage.download(&manifest(uid)).await?;
        let Ok(m) = serde_json::from_slice::<Manifest>(&bytes) else {
            log::warn!("recording manifest for session {uid} is unreadable");
            continue;
        };
        let lines: Vec<(String, LineAudio)> = m
            .lines
            .into_iter()
            .map(|l| {
                (
                    l.id,
                    LineAudio {
                        line_id: 0,
                        start_ms: l.start_ms,
                        ms: l.ms,
                    },
                )
            })
            .collect();
        // Applied only once every line has arrived; otherwise next cycle.
        if store.restore_session_audio(*session_id, &lines)? {
            report.restored += 1;
        }
    }
    Ok(report)
}

async fn upload_one(
    store: &ProjectStore,
    storage: &Storage<'_>,
    rec: &SessionRecording,
) -> Result<(), SyncError> {
    let lines = store.session_audio_manifest(rec.session_id)?;
    let body = tokio::fs::read(&rec.path)
        .await
        .map_err(|e| SyncError::Other(format!("read {}: {e}", rec.path.display())))?;
    storage.upload(&ogg(&rec.uid), "audio/ogg", body).await?;
    let m = Manifest {
        version: 1,
        lines: lines
            .into_iter()
            .map(|(id, a)| ManifestLine {
                id,
                start_ms: a.start_ms,
                ms: a.ms,
            })
            .collect(),
    };
    let json = serde_json::to_vec(&m).map_err(|e| SyncError::Other(e.to_string()))?;
    storage
        .upload(&manifest(&rec.uid), "application/json", json)
        .await?;
    log::info!("backed up the recording of session {}", rec.session_id);
    Ok(())
}

/// Download a session's recording to `dest` (for a device that only has the
/// manifest so far).
pub async fn fetch(storage: &Storage<'_>, uid: &str, dest: &Path) -> Result<(), SyncError> {
    let bytes = storage.download(&ogg(uid)).await?;
    let io = |e: std::io::Error| SyncError::Other(format!("save recording: {e}"));
    if let Some(dir) = dest.parent() {
        tokio::fs::create_dir_all(dir).await.map_err(io)?;
    }
    let tmp = dest.with_extension("ogg.part");
    tokio::fs::write(&tmp, bytes).await.map_err(io)?;
    tokio::fs::rename(&tmp, dest).await.map_err(io)?;
    Ok(())
}
