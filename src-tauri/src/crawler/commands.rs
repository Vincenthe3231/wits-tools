//! Connects the typed desktop boundary to crawler jobs, media transfers, and native pickers.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{
    Error, Result, browser_path,
    capture::{Capture, Pairing},
    capture_browser::CaptureBrowser,
    crawl::{self, Config},
    data_dir,
    media::{self, Candidate, Download, Progress, Source},
    storage::{Job, Results, Store},
};

#[derive(Clone, Default)]
pub(crate) struct DesktopState {
    inner: Arc<Mutex<Jobs>>,
    capture: Capture,
    capture_browser: CaptureBrowser,
}

#[derive(Default)]
struct Jobs {
    crawls: HashMap<Uuid, CancellationToken>,
    transfers: Vec<Transfer>,
    discovery: Option<CancellationToken>,
    discovered: HashMap<Uuid, Candidate>,
    closing: bool,
}

struct Transfer {
    summary: TransferSummary,
    stop: CancellationToken,
}

/// Publish status and progress without exposing input URLs, credentials, or raw errors.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TransferSummary {
    id: Uuid,
    status: &'static str,
    progress: Progress,
    output: Option<PathBuf>,
    error: Option<Error>,
}

async fn blocking<T: Send + 'static>(
    action: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(action)
        .await
        .map_err(|_| Error::Storage)?
}

fn store() -> Result<Store> {
    Store::open(data_dir()?)
}

/// Persist a job before returning its identifier and keep cancellation with its owner.
#[tauri::command]
pub(crate) async fn crawler_start(state: State<'_, DesktopState>, config: Config) -> Result<Uuid> {
    config.validate()?;
    let id = Uuid::new_v4();
    // Check the selected destination before accepting the job so path errors reach the form.
    let folder = config
        .asset_folder
        .clone()
        .filter(|_| config.download_assets);
    let store = blocking(move || {
        if let Some(folder) = folder {
            std::fs::create_dir_all(&folder).map_err(|_| Error::File)?;
            tempfile::NamedTempFile::new_in(folder).map_err(|_| Error::File)?;
        }
        store()
    })
    .await?;
    let lease = {
        let mut jobs = state.inner.lock().map_err(|_| Error::Busy)?;
        if jobs.closing {
            return Err(Error::Busy);
        }
        let lease = store.create(id, &config)?;
        jobs.crawls.insert(id, CancellationToken::new());
        lease
    };
    let state = state.inner.clone();
    let stop = state.lock().map_err(|_| Error::Busy)?.crawls[&id].clone();
    tauri::async_runtime::spawn(async move {
        let _ = crawl::run(store, id, config, browser_path(), lease, stop).await;
        if let Ok(mut jobs) = state.lock() {
            jobs.crawls.remove(&id);
        }
    });
    Ok(id)
}

#[tauri::command]
pub(crate) async fn crawler_jobs() -> Result<Vec<Job>> {
    blocking(|| store()?.list()).await
}

#[tauri::command]
pub(crate) async fn crawler_results(id: Uuid, pages: u32, assets: u32) -> Result<Results> {
    blocking(move || store()?.results(id, pages, assets, 50)).await
}

#[tauri::command]
pub(crate) async fn crawler_stop(state: State<'_, DesktopState>, id: Uuid) -> Result<()> {
    blocking(move || store()?.stop(id)).await?;
    if let Some(stop) = state.inner.lock().map_err(|_| Error::Busy)?.crawls.get(&id) {
        stop.cancel();
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn crawler_delete(id: Uuid) -> Result<()> {
    blocking(move || store()?.delete(id)).await
}

/// Open a native save dialog and treat closing it as a normal outcome.
#[tauri::command]
pub(crate) async fn crawler_export(app: AppHandle, id: Uuid) -> Result<bool> {
    let Some(path) = app
        .dialog()
        .file()
        .add_filter("JSON", &["json"])
        .set_file_name(format!("crawl-{id}.json"))
        .blocking_save_file()
    else {
        return Ok(false);
    };
    let path = path.into_path().map_err(|_| Error::File)?;
    blocking(move || store()?.export(id, &path)).await?;
    Ok(true)
}

/// Read the variants of a manual URL or an in-memory capture candidate.
#[tauri::command]
pub(crate) async fn crawler_media_info(
    state: State<'_, DesktopState>,
    request: Download,
    source: Option<Uuid>,
    captured: bool,
) -> Result<media::Info> {
    let candidate = state.resolve_source(source, captured, request.proxy.clone())?;
    media::info(&request, candidate).await
}

impl DesktopState {
    /// Pair a safe source ID with the captured request context only inside the native process.
    fn resolve_source(
        &self,
        source: Option<Uuid>,
        captured: bool,
        proxy: Option<String>,
    ) -> Result<Option<Candidate>> {
        source
            .map(|id| {
                if captured {
                    if let Some(candidate) = self.capture_browser.source(id)? {
                        return Ok(candidate);
                    }
                    return self.capture.source(id, proxy);
                }
                self.inner
                    .lock()
                    .map_err(|_| Error::Busy)?
                    .discovered
                    .get(&id)
                    .cloned()
                    .ok_or(Error::NotFound)
            })
            .transpose()
    }
}

/// Start an isolated WebView with its request observer attached before navigation.
#[tauri::command]
pub(crate) async fn crawler_open_capture_browser(
    app: AppHandle,
    state: State<'_, DesktopState>,
    url: String,
) -> Result<()> {
    // A shutdown in progress must not create a new desktop window.
    if state.inner.lock().map_err(|_| Error::Busy)?.closing {
        return Err(Error::Busy);
    }
    #[cfg(windows)]
    return super::capture_browser::open(app, state.capture_browser.clone(), &url).await;
    #[cfg(not(windows))]
    {
        let _ = (app, state, url);
        Err(Error::CaptureUnavailable)
    }
}

/// Poll redacted candidates independently of the optional extension session.
#[tauri::command]
pub(crate) fn crawler_capture_browser_sources(
    state: State<'_, DesktopState>,
) -> Result<Vec<Source>> {
    state.capture_browser.sources()
}

#[tauri::command]
pub(crate) async fn crawler_pick_folder(app: AppHandle) -> Result<Option<PathBuf>> {
    app.dialog()
        .file()
        .blocking_pick_folder()
        .map(|path| path.into_path().map_err(|_| Error::File))
        .transpose()
}

/// Allow one discovery session per window and release its context when replaced.
#[tauri::command]
pub(crate) async fn crawler_discover(
    state: State<'_, DesktopState>,
    url: String,
    proxy: Option<String>,
) -> Result<Vec<Source>> {
    let stop = CancellationToken::new();
    {
        let mut jobs = state.inner.lock().map_err(|_| Error::Busy)?;
        if jobs.closing || jobs.discovery.is_some() {
            return Err(Error::Busy);
        }
        jobs.discovery = Some(stop.clone());
        jobs.discovered.clear();
    }
    let executable = browser_path();
    let results = tokio::select! {
        _ = stop.cancelled() => Err(Error::Cancelled),
        results = media::discover(&executable, &url, proxy) => results,
    };
    let mut jobs = state.inner.lock().map_err(|_| Error::Busy)?;
    jobs.discovery = None;
    let mut sources = Vec::new();
    for candidate in results? {
        let id = Uuid::new_v4();
        sources.push(candidate.summary(id));
        jobs.discovered.insert(id, candidate);
    }
    Ok(sources)
}

#[tauri::command]
pub(crate) fn crawler_cancel_discovery(state: State<'_, DesktopState>) -> Result<()> {
    let mut jobs = state.inner.lock().map_err(|_| Error::Busy)?;
    if let Some(stop) = &jobs.discovery {
        stop.cancel();
    }
    jobs.discovered.clear();
    Ok(())
}

/// Start a transfer with settings or an opaque candidate from the active capture session.
#[tauri::command]
pub(crate) async fn crawler_download(
    app: AppHandle,
    state: State<'_, DesktopState>,
    request: Download,
    source: Option<Uuid>,
    captured: bool,
) -> Result<Uuid> {
    let folder = app.path().download_dir().map_err(|_| Error::File)?;
    let id = Uuid::new_v4();
    let stop = CancellationToken::new();
    let candidate = state.resolve_source(source, captured, request.proxy.clone())?;
    {
        let mut jobs = state.inner.lock().map_err(|_| Error::Busy)?;
        if jobs.closing {
            return Err(Error::Busy);
        }
        jobs.transfers.push(Transfer {
            summary: TransferSummary {
                id,
                status: "queued",
                progress: Progress::default(),
                output: None,
                error: None,
            },
            stop: stop.clone(),
        });
    }
    let jobs = state.inner.clone();
    tauri::async_runtime::spawn(async move {
        update_transfer(&jobs, id, |summary| summary.status = "downloading");
        let result = media::download(request, candidate, folder, stop, |progress| {
            update_transfer(&jobs, id, |summary| summary.progress = progress);
        })
        .await;
        update_transfer(&jobs, id, |summary| match result {
            Ok(path) => {
                summary.status = "completed";
                summary.output = Some(path);
            }
            Err(Error::Cancelled) => summary.status = "cancelled",
            Err(error) => {
                summary.status = "failed";
                summary.error = Some(error);
            }
        });
    });
    Ok(id)
}

fn update_transfer(jobs: &Mutex<Jobs>, id: Uuid, update: impl FnOnce(&mut TransferSummary)) {
    if let Ok(mut jobs) = jobs.lock()
        && let Some(transfer) = jobs
            .transfers
            .iter_mut()
            .find(|transfer| transfer.summary.id == id)
    {
        update(&mut transfer.summary);
    }
}

#[tauri::command]
pub(crate) fn crawler_downloads(state: State<'_, DesktopState>) -> Result<Vec<TransferSummary>> {
    Ok(state
        .inner
        .lock()
        .map_err(|_| Error::Busy)?
        .transfers
        .iter()
        .rev()
        .map(|entry| entry.summary.clone())
        .collect())
}

#[tauri::command]
pub(crate) fn crawler_cancel_download(state: State<'_, DesktopState>, id: Uuid) -> Result<()> {
    let jobs = state.inner.lock().map_err(|_| Error::Busy)?;
    let transfer = jobs
        .transfers
        .iter()
        .find(|transfer| transfer.summary.id == id)
        .ok_or(Error::NotFound)?;
    transfer.stop.cancel();
    Ok(())
}

#[tauri::command]
pub(crate) async fn crawler_capture_start(state: State<'_, DesktopState>) -> Result<()> {
    state.capture.start()
}

#[tauri::command]
pub(crate) async fn crawler_capture_register() -> Result<()> {
    blocking(super::capture::register).await
}

#[tauri::command]
pub(crate) async fn crawler_capture_pair(state: State<'_, DesktopState>) -> Result<Pairing> {
    state.capture.pair().await
}

#[tauri::command]
pub(crate) fn crawler_capture_list(state: State<'_, DesktopState>) -> Result<Vec<Source>> {
    state.capture.sources()
}

#[tauri::command]
pub(crate) fn crawler_capture_stop(state: State<'_, DesktopState>) {
    state.capture.stop();
}

/// Cancel desktop work before the shell destroys the window. Other CLI jobs own their leases.
#[tauri::command]
pub(crate) async fn crawler_shutdown(app: AppHandle, state: State<'_, DesktopState>) -> Result<()> {
    state.capture.stop();
    {
        let mut jobs = state.inner.lock().map_err(|_| Error::Busy)?;
        jobs.closing = true;
        for stop in jobs.crawls.values() {
            stop.cancel();
        }
        for transfer in &jobs.transfers {
            transfer.stop.cancel();
        }
        if let Some(stop) = &jobs.discovery {
            stop.cancel();
        }
        jobs.discovered.clear();
    }
    // Stop embedded capture before the main window closes.
    if let Some(window) = app.get_webview_window(super::capture_browser::LABEL) {
        let _ = window.destroy();
    }
    let _ = tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let idle = state.inner.lock().is_ok_and(|jobs| {
                jobs.crawls.is_empty()
                    && jobs.discovery.is_none()
                    && !jobs
                        .transfers
                        .iter()
                        .any(|job| matches!(job.summary.status, "queued" | "downloading"))
            });
            if idle {
                break;
            }
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
    })
    .await;
    Ok(())
}
