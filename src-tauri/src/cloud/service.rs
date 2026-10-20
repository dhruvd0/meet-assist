//! The app-facing cloud service: account state, the background loop, and
//! settings sync. Signed out, the loop never touches the network.

use super::auth::{self, AuthError, Session};
use super::local;
use super::sync::{run_cycle, Remote, SyncError};
use crate::meeting::manager::ProjectManager;
use crate::meeting::store::ProjectStore;
use crate::settings::{get_settings, write_settings, AppSettings};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use specta::Type;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tauri_specta::Event;
use tokio::sync::Notify;

const CYCLE_EVERY: Duration = Duration::from_secs(30);
const OWNER_KEY: &str = "owner_user_id";
const SETTINGS_SYNCED_KEY: &str = "settings_synced";
const SETTINGS_SEEN_KEY: &str = "settings_remote_updated_at";

/// Preferences that mean the same thing on every machine. Keys, devices,
/// shortcuts, models, and accelerators stay local.
const PORTABLE_SETTINGS: &[&str] = &[
    "audio_feedback",
    "audio_feedback_volume",
    "sound_theme",
    "translate_to_english",
    "selected_language",
    "custom_words",
    "word_correction_threshold",
    "llm_provider_id",
    "llm_providers",
    "llm_models",
    "meeting_assistant_enabled",
    "meeting_system_audio_enabled",
    "meeting_vision_enabled",
    "meeting_jev_enabled",
    "meeting_sensitivity",
    "meeting_observer_interval_secs",
    "meeting_max_line_secs",
    "meeting_beep_enabled",
    "app_language",
    "theme",
    "filler_word_removal_enabled",
    "custom_filler_words",
    "chinese_script",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CloudState {
    SignedOut,
    Idle,
    Syncing,
    Offline,
    Error,
    /// Signed in with an account other than the one this device's data belongs to.
    AccountMismatch,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct CloudStatus {
    pub state: CloudState,
    pub email: Option<String>,
    pub last_synced_at: Option<i64>,
    pub pending: i64,
    pub message: Option<String>,
    /// Bumped whenever a pull changed local data, so the UI knows to reload.
    pub revision: u32,
}

pub struct CloudService {
    app: AppHandle,
    store: Arc<ProjectStore>,
    http: reqwest::Client,
    session: Mutex<Option<Session>>,
    status: Mutex<CloudStatus>,
    kick: Notify,
    cycle: tokio::sync::Mutex<()>,
}

impl CloudService {
    pub fn new(app: AppHandle, store: Arc<ProjectStore>) -> Arc<Self> {
        let session = auth::load();
        let status = CloudStatus {
            state: if session.is_some() {
                CloudState::Idle
            } else {
                CloudState::SignedOut
            },
            email: session.as_ref().map(|s| s.email.clone()),
            last_synced_at: None,
            pending: 0,
            message: None,
            revision: 0,
        };
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default();
        Arc::new(Self {
            app,
            store,
            http,
            session: Mutex::new(session),
            status: Mutex::new(status),
            kick: Notify::new(),
            cycle: tokio::sync::Mutex::new(()),
        })
    }

    pub fn spawn(self: &Arc<Self>) {
        let svc = self.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                svc.run_once().await;
                tokio::select! {
                    _ = tokio::time::sleep(CYCLE_EVERY) => {}
                    _ = svc.kick.notified() => {}
                }
            }
        });
    }

    pub fn sync_now(&self) {
        self.kick.notify_one();
    }

    pub fn status(&self) -> CloudStatus {
        let pending = self
            .store
            .conn()
            .ok()
            .and_then(|c| local::pending_count(&c).ok())
            .unwrap_or(0);
        let mut s = self.status.lock().unwrap();
        s.pending = pending;
        s.clone()
    }

    fn update(&self, f: impl FnOnce(&mut CloudStatus)) {
        f(&mut self.status.lock().unwrap());
        if let Err(e) = self.status().emit(&self.app) {
            log::error!("emit cloud status: {e}");
        }
    }

    fn session(&self) -> Option<Session> {
        self.session.lock().unwrap().clone()
    }

    fn set_session(&self, session: Option<Session>) {
        match &session {
            Some(s) => {
                if let Err(e) = auth::save(s) {
                    log::error!("could not store cloud session in keychain: {e:#}");
                }
            }
            None => auth::clear(),
        }
        *self.session.lock().unwrap() = session;
    }

    pub async fn sign_in(
        &self,
        email: &str,
        password: &str,
        create: bool,
    ) -> Result<CloudStatus, String> {
        let email = email.trim();
        let result = if create {
            auth::sign_up(&self.http, email, password).await
        } else {
            auth::sign_in(&self.http, email, password).await
        };
        let session = result.map_err(|e| e.to_string())?;
        let user = session.email.clone();
        self.set_session(Some(session));
        self.update(|s| {
            s.state = CloudState::Idle;
            s.email = Some(user);
            s.message = None;
        });
        self.sync_now();
        Ok(self.status())
    }

    pub async fn sign_out(&self) -> CloudStatus {
        let _guard = self.cycle.lock().await;
        if let Some(s) = self.session() {
            auth::sign_out(&self.http, &s).await;
        }
        self.set_session(None);
        self.update(|s| {
            s.state = CloudState::SignedOut;
            s.email = None;
            s.message = None;
        });
        self.status()
    }

    async fn run_once(&self) {
        let _guard = self.cycle.lock().await;
        let Some(mut session) = self.session() else {
            return;
        };
        if session.expires_soon() {
            match self.refresh(&session).await {
                Some(s) => session = s,
                None => return,
            }
        }
        match self.bound_to(&session) {
            Ok(true) => {}
            Ok(false) => {
                self.update(|s| {
                    s.state = CloudState::AccountMismatch;
                    s.message = None;
                });
                return;
            }
            Err(e) => {
                self.fail(SyncError::Other(format!("{e:#}")));
                return;
            }
        }
        self.update(|s| s.state = CloudState::Syncing);
        let mut result = self.cycle_with(&session).await;
        if matches!(result, Err(SyncError::Unauthorized)) {
            if let Some(s) = self.refresh(&session).await {
                result = self.cycle_with(&s).await;
            } else {
                return;
            }
        }
        match result {
            Ok(changed) => self.update(|s| {
                s.state = CloudState::Idle;
                s.last_synced_at = Some(chrono::Utc::now().timestamp_millis());
                s.message = None;
                if changed {
                    s.revision = s.revision.wrapping_add(1);
                }
            }),
            Err(e) => self.fail(e),
        }
    }

    /// Returns whether the pull or settings sync changed local data.
    async fn cycle_with(&self, session: &Session) -> Result<bool, SyncError> {
        let remote = Remote {
            http: &self.http,
            session,
        };
        let report = run_cycle(&self.store, &remote).await?;
        if report.sources_pulled {
            self.requeue_pending_sources();
        }
        let settings_changed = self.sync_settings(&remote).await?;
        Ok(report.pulled > 0 || settings_changed)
    }

    async fn refresh(&self, session: &Session) -> Option<Session> {
        match auth::refresh(&self.http, session).await {
            Ok(s) => {
                self.set_session(Some(s.clone()));
                Some(s)
            }
            Err(AuthError::Rejected(m)) => {
                self.set_session(None);
                self.update(|s| {
                    s.state = CloudState::SignedOut;
                    s.email = None;
                    s.message = Some(m);
                });
                None
            }
            Err(AuthError::Network(m)) => {
                self.fail(SyncError::Network(m));
                None
            }
        }
    }

    fn fail(&self, e: SyncError) {
        log::warn!("cloud sync: {e}");
        self.update(|s| {
            s.state = match e {
                SyncError::Network(_) => CloudState::Offline,
                _ => CloudState::Error,
            };
            s.message = Some(e.to_string());
        });
    }

    /// The local database belongs to the first account that syncs it.
    fn bound_to(&self, session: &Session) -> anyhow::Result<bool> {
        let conn = self.store.conn()?;
        match local::meta(&conn, OWNER_KEY)? {
            Some(owner) => Ok(owner == session.user_id),
            None => {
                local::set_meta(&conn, OWNER_KEY, &session.user_id)?;
                Ok(true)
            }
        }
    }

    fn requeue_pending_sources(&self) {
        let pm = self.app.state::<Arc<ProjectManager>>();
        match self.store.list_pending_source_ids() {
            Ok(ids) => ids.into_iter().for_each(|id| pm.enqueue_index(id)),
            Err(e) => log::warn!("list pending sources after pull: {e:#}"),
        }
    }

    fn portable_settings(&self) -> Value {
        let all = serde_json::to_value(get_settings(&self.app)).unwrap_or(Value::Null);
        let picked: Map<String, Value> = PORTABLE_SETTINGS
            .iter()
            .filter_map(|k| all.get(*k).map(|v| (k.to_string(), v.clone())))
            .collect();
        Value::Object(picked)
    }

    /// Last writer wins, except that a device which has never synced settings
    /// takes the account's copy instead of overwriting it with its defaults.
    async fn sync_settings(&self, remote: &Remote<'_>) -> Result<bool, SyncError> {
        let local_json = self.portable_settings().to_string();
        let (last, seen) = {
            let conn = self.store.conn()?;
            let last = local::meta(&conn, SETTINGS_SYNCED_KEY)?;
            let seen = local::meta(&conn, SETTINGS_SEEN_KEY)?
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(0);
            (last, seen)
        };
        let rows = remote
            .select(
                "user_settings",
                &[("select", "data,updated_at".to_string())],
            )
            .await?;
        let remote_row = rows.first();
        let remote_updated = remote_row
            .and_then(|r| r["updated_at"].as_i64())
            .unwrap_or(0);
        let local_unchanged = last.as_deref().is_none_or(|l| l == local_json);

        if remote_updated > seen && local_unchanged {
            if let Some(data) = remote_row
                .and_then(|r| r.get("data"))
                .and_then(Value::as_object)
            {
                self.apply_settings(data);
            }
            let conn = self.store.conn()?;
            local::set_meta(
                &conn,
                SETTINGS_SYNCED_KEY,
                &self.portable_settings().to_string(),
            )?;
            local::set_meta(&conn, SETTINGS_SEEN_KEY, &remote_updated.to_string())?;
            return Ok(true);
        }
        if last.as_deref() != Some(local_json.as_str()) {
            let now = chrono::Utc::now().timestamp_millis();
            let data: Value = serde_json::from_str(&local_json).unwrap_or(Value::Null);
            remote
                .upsert(
                    "user_settings",
                    "user_id",
                    &[serde_json::json!({"user_id": remote.session.user_id, "updated_at": now, "data": data})],
                )
                .await?;
            let conn = self.store.conn()?;
            local::set_meta(&conn, SETTINGS_SYNCED_KEY, &local_json)?;
            local::set_meta(&conn, SETTINGS_SEEN_KEY, &now.to_string())?;
        }
        Ok(false)
    }

    fn apply_settings(&self, data: &Map<String, Value>) {
        let mut current = match serde_json::to_value(get_settings(&self.app)) {
            Ok(Value::Object(m)) => m,
            _ => return,
        };
        for key in PORTABLE_SETTINGS {
            if let Some(v) = data.get(*key) {
                current.insert(key.to_string(), v.clone());
            }
        }
        match serde_json::from_value::<AppSettings>(Value::Object(current)) {
            Ok(s) => write_settings(&self.app, s),
            Err(e) => log::warn!("ignoring synced settings this version cannot read: {e}"),
        }
    }
}
