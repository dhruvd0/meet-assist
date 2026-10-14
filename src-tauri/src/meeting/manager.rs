//! Tauri-facing owner of the project store. Runs source indexing on a
//! worker thread and emits status events to the frontend.

use crate::meeting::indexer;
use crate::meeting::store::{ProjectStore, Source, SourceKind, SourceStatus};
use anyhow::{anyhow, Result};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use tauri::AppHandle;
use tauri_specta::Event;

#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct SourceStatusPayload {
    pub source: Source,
}

/// Turns an image into a short text digest using a vision-capable model.
pub trait Describer: Send + Sync {
    fn describe(&self, mime: &str, bytes: &[u8]) -> Result<String, String>;
}

const DESCRIBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Production describer backed by the configured meeting provider.
pub struct LlmDescriber {
    app: AppHandle,
}

impl Describer for LlmDescriber {
    fn describe(&self, mime: &str, bytes: &[u8]) -> Result<String, String> {
        let settings = crate::settings::get_settings(&self.app);
        let provider = settings
            .meeting_provider()
            .ok_or_else(|| "no provider".to_string())?
            .clone();
        let model = settings
            .meeting_model()
            .ok_or_else(|| "no provider".to_string())?;
        let key = settings.meeting_api_key();
        let fut = async {
            tokio::time::timeout(
                DESCRIBE_TIMEOUT,
                crate::llm_client::describe_image(
                    &provider,
                    key,
                    &model,
                    indexer::image_digest_prompt(),
                    mime,
                    bytes,
                ),
            )
            .await
            .unwrap_or_else(|_| Err("image description timed out".to_string()))
        };
        match tokio::runtime::Handle::try_current() {
            Ok(h) => tokio::task::block_in_place(|| h.block_on(fut)),
            Err(_) => tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| e.to_string())?
                .block_on(fut),
        }
    }
}

const MAX_DIGEST_CHARS: usize = 1200;

pub struct ProjectManager {
    store: Arc<ProjectStore>,
    images_dir: PathBuf,
    tx: Mutex<Option<mpsc::Sender<i64>>>,
    app: Option<AppHandle>,
    describer: Option<Arc<dyn Describer>>,
    vision_unsupported_logged: AtomicBool,
}

impl ProjectManager {
    pub fn store_arc(&self) -> Arc<ProjectStore> {
        Arc::clone(&self.store)
    }

    pub fn new(app: &AppHandle) -> Result<Self> {
        let data_dir = crate::portable::app_data_dir(app)?;
        Self::build(
            data_dir.join("meet.db"),
            data_dir.join("sources").join("images"),
            Some(app.clone()),
            Some(Arc::new(LlmDescriber { app: app.clone() })),
        )
    }

    fn build(
        db_path: PathBuf,
        images_dir: PathBuf,
        app: Option<AppHandle>,
        describer: Option<Arc<dyn Describer>>,
    ) -> Result<Self> {
        let store = Arc::new(ProjectStore::open(db_path)?);
        std::fs::create_dir_all(&images_dir)?;
        let (tx, rx) = mpsc::channel::<i64>();
        let worker_store = Arc::clone(&store);
        let worker_images = images_dir.clone();
        let worker_app = app.clone();
        let worker_describer = describer.clone();
        std::thread::Builder::new()
            .name("meet-indexer".into())
            .spawn(move || {
                let worker = ProjectManager {
                    store: worker_store,
                    images_dir: worker_images,
                    tx: Mutex::new(None),
                    app: worker_app,
                    describer: worker_describer,
                    vision_unsupported_logged: AtomicBool::new(false),
                };
                for source_id in rx {
                    match catch_unwind(AssertUnwindSafe(|| worker.index_source_now(source_id))) {
                        Ok(Ok(())) => {}
                        Ok(Err(e)) => warn!("indexing source {} failed: {:#}", source_id, e),
                        Err(_) => {
                            error!("indexer panicked on source {}", source_id);
                            let marked = catch_unwind(AssertUnwindSafe(|| {
                                worker.set_status(
                                    source_id,
                                    SourceStatus::Failed,
                                    Some("indexer panicked"),
                                )
                            }));
                            match marked {
                                Ok(Ok(())) => {}
                                Ok(Err(e)) => {
                                    error!("mark source {} failed: {:#}", source_id, e)
                                }
                                Err(_) => error!("marking source {} failed panicked", source_id),
                            }
                        }
                    }
                }
            })?;
        let manager = Self {
            store,
            images_dir,
            tx: Mutex::new(Some(tx)),
            app,
            describer,
            vision_unsupported_logged: AtomicBool::new(false),
        };
        // Resume sources left pending by a quit or crash mid-index.
        match manager.store.list_pending_source_ids() {
            Ok(ids) => {
                for id in ids {
                    manager.enqueue_index(id);
                }
            }
            Err(e) => warn!("list pending sources: {:#}", e),
        }
        Ok(manager)
    }

    pub fn store(&self) -> &ProjectStore {
        &self.store
    }

    fn emit_status(&self, source: &Source) {
        if let Some(app) = &self.app {
            if let Err(e) = (SourceStatusPayload {
                source: source.clone(),
            })
            .emit(app)
            {
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
        match self.index_inner(&src) {
            Ok(n) => {
                info!("indexed source {} ({} chunks)", source_id, n);
                self.set_status(source_id, SourceStatus::Ready, None)
            }
            Err(e) => {
                // `{:#}` keeps the anyhow cause chain (e.g. the OS error).
                let msg = format!("{e:#}");
                self.set_status(source_id, SourceStatus::Failed, Some(&msg))?;
                Err(e)
            }
        }
    }

    /// Ask the vision model for a digest and persist it. None means fall back
    /// to the (empty) caption path.
    fn describe_image_source(&self, src: &Source) -> Option<String> {
        let describer = self.describer.as_ref()?;
        // Images leave the machine, so both the assistant opt-in and the
        // vision toggle must be on.
        let settings = self.app.as_ref().map(crate::settings::get_settings);
        let vision_on = settings.as_ref().is_none_or(|s| s.meeting_vision_enabled);
        if !vision_on || !crate::meeting::session::cloud_allowed(settings.as_ref()) {
            return None;
        }
        let (mime, bytes) = match indexer::read_image_for_vision(Path::new(&src.locator)) {
            Ok(v) => v,
            Err(e) => {
                warn!("image {} not sent for digest: {:#}", src.id, e);
                return None;
            }
        };
        match describer.describe(mime, &bytes) {
            Ok(text) => {
                let text: String = text.trim().chars().take(MAX_DIGEST_CHARS).collect();
                if text.is_empty() {
                    return None;
                }
                if let Err(e) = self.store.set_source_digest(src.id, &text) {
                    warn!("store digest for image {}: {:#}", src.id, e);
                }
                Some(text)
            }
            Err(e) if e == "vision_unsupported" => {
                if !self.vision_unsupported_logged.swap(true, Ordering::Relaxed) {
                    info!("meeting model does not support images; skipping digests");
                }
                None
            }
            Err(e) => {
                warn!("image digest failed for {}: {}", src.id, e);
                None
            }
        }
    }

    fn index_inner(&self, src: &Source) -> Result<usize> {
        let chunks: Vec<(String, String)> = match src.kind {
            SourceKind::Text => indexer::chunks_for_text(&src.locator),
            SourceKind::File => indexer::chunks_for_file(Path::new(&src.locator))?,
            SourceKind::Folder => indexer::chunks_for_folder(Path::new(&src.locator))?,
            SourceKind::Link => {
                let url = src.locator.clone();
                match tokio::runtime::Handle::try_current() {
                    Ok(h) => {
                        tokio::task::block_in_place(|| h.block_on(indexer::chunks_for_link(&url)))?
                    }
                    Err(_) => tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()?
                        .block_on(indexer::chunks_for_link(&url))?,
                }
            }
            SourceKind::Image => {
                let mut digest = src.digest.clone().unwrap_or_default();
                if digest.trim().is_empty() {
                    if let Some(text) = self.describe_image_source(src) {
                        digest = text;
                    }
                }
                indexer::chunks_for_caption(&digest)
            }
        };
        self.store.replace_chunks(src.id, &chunks)
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
            let copy_result =
                indexer::copy_image_into(Path::new(locator), &self.images_dir, source.id).and_then(
                    |copied| {
                        let copied_str = copied.to_string_lossy().to_string();
                        self.store.update_source_locator(source.id, &copied_str)?;
                        Ok(copied_str)
                    },
                );
            match copy_result {
                Ok(copied_str) => source.locator = copied_str,
                Err(e) => {
                    if let Err(e2) = self.store.remove_source(source.id) {
                        warn!("remove orphan source {}: {}", source.id, e2);
                    }
                    return Err(e);
                }
            }
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
