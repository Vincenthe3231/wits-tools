//! Connects the typed desktop boundary to crawler jobs, media transfers, and native pickers.

use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{
    Error, Result, browser_path,
    capture_browser::{CaptureBrowser, CaptureSnapshot},
    crawl::{self, Config},
    data_dir,
    media::{self, Candidate, Download, Progress},
    storage::{Job, Results, Store},
};

#[derive(Clone, Default)]
pub(crate) struct DesktopState {
    inner: Arc<Mutex<Jobs>>,
    capture_browser: CaptureBrowser,
}

#[derive(Default)]
struct Jobs {
    crawls: HashMap<Uuid, CancellationToken>,
    transfers: Vec<Transfer>,
    closing: bool,
}

struct Transfer {
    summary: TransferSummary,
    samples: VecDeque<(Instant, u64)>,
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
    duration_seconds: Option<f64>,
    file_size: Option<u64>,
    eta_seconds: Option<u64>,
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

impl DesktopState {
    /// Pair a safe source ID with the captured request context only inside the native process.
    fn resolve_source(&self, source: Option<Uuid>) -> Result<Option<Candidate>> {
        source
            .map(|id| self.capture_browser.source(id)?.ok_or(Error::NotFound))
            .transpose()
    }
}

/// Start an isolated WebView with its request observer attached before navigation.
#[tauri::command]
pub(crate) async fn crawler_open_capture_browser(
    app: AppHandle,
    state: State<'_, DesktopState>,
    url: String,
    proxy: Option<String>,
) -> Result<()> {
    // A shutdown in progress must not create a new desktop window.
    if state.inner.lock().map_err(|_| Error::Busy)?.closing {
        return Err(Error::Busy);
    }
    #[cfg(windows)]
    return super::capture_browser::open(app, state.capture_browser.clone(), &url, proxy).await;
    #[cfg(not(windows))]
    {
        let _ = (app, state, url, proxy);
        Err(Error::CaptureUnavailable)
    }
}

/// Poll navigation status and redacted candidates from WebView2.
#[tauri::command]
pub(crate) fn crawler_capture_browser_sources(
    state: State<'_, DesktopState>,
) -> Result<CaptureSnapshot> {
    state.capture_browser.snapshot()
}

#[tauri::command]
pub(crate) async fn crawler_pick_folder(app: AppHandle) -> Result<Option<PathBuf>> {
    app.dialog()
        .file()
        .blocking_pick_folder()
        .map(|path| path.into_path().map_err(|_| Error::File))
        .transpose()
}

/// Start a transfer with settings or an opaque candidate from the active capture session.
#[tauri::command]
pub(crate) async fn crawler_download(
    app: AppHandle,
    state: State<'_, DesktopState>,
    request: Download,
    source: Option<Uuid>,
) -> Result<Uuid> {
    let folder = app.path().download_dir().map_err(|_| Error::File)?;
    let id = Uuid::new_v4();
    let stop = CancellationToken::new();
    let candidate = state.resolve_source(source)?;
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
                duration_seconds: None,
                file_size: None,
                eta_seconds: None,
                error: None,
            },
            samples: VecDeque::new(),
            stop: stop.clone(),
        });
    }
    let jobs = state.inner.clone();
    tauri::async_runtime::spawn(async move {
        update_transfer(&jobs, id, |transfer| {
            transfer.summary.status = "downloading"
        });
        let result = media::download_with_size(request, candidate, folder, stop, |progress| {
            update_transfer(&jobs, id, |transfer| transfer.record(progress));
        })
        .await;
        update_transfer(&jobs, id, |transfer| {
            let summary = &mut transfer.summary;
            summary.eta_seconds = None;
            match result {
                Ok((path, size)) => {
                    summary.status = "completed";
                    summary.output = Some(path);
                    summary.file_size = Some(size);
                }
                Err(Error::Cancelled) => summary.status = "cancelled",
                Err(error) => {
                    summary.status = "failed";
                    summary.error = Some(error);
                }
            }
        });
    });
    Ok(id)
}

impl Transfer {
    // Keep a bounded recent history and use bytes when the server reports a reliable total.
    fn record(&mut self, progress: Progress) {
        self.summary.progress = progress;
        self.summary.duration_seconds = progress.duration_seconds;
        let (completed, total) = if let Some(total) = progress.total_bytes {
            (progress.bytes, total)
        } else if let Some(total) = progress.total_segments {
            (u64::from(progress.segments), u64::from(total))
        } else {
            return;
        };
        let now = Instant::now();
        if self
            .samples
            .back()
            .is_none_or(|(at, _)| now.duration_since(*at) >= Duration::from_millis(500))
        {
            self.samples.push_back((now, completed));
        }
        while self.samples.len() > 1
            && self
                .samples
                .front()
                .is_some_and(|(at, _)| now.duration_since(*at) > Duration::from_secs(15))
        {
            self.samples.pop_front();
        }
        self.summary.eta_seconds = self.samples.front().and_then(|(at, initial)| {
            let elapsed = now.duration_since(*at).as_secs_f64();
            let advanced = completed.saturating_sub(*initial);
            (elapsed >= 2.0 && advanced > 0 && completed < total)
                .then(|| ((total - completed) as f64 * elapsed / advanced as f64).ceil() as u64)
        });
    }

    // Drop stale estimates if a transfer stops reporting progress.
    fn snapshot(&self) -> TransferSummary {
        let mut summary = self.summary.clone();
        if self
            .samples
            .back()
            .is_some_and(|(at, _)| at.elapsed() > Duration::from_secs(15))
        {
            summary.eta_seconds = None;
        }
        summary
    }
}

fn update_transfer(jobs: &Mutex<Jobs>, id: Uuid, update: impl FnOnce(&mut Transfer)) {
    if let Ok(mut jobs) = jobs.lock()
        && let Some(transfer) = jobs
            .transfers
            .iter_mut()
            .find(|transfer| transfer.summary.id == id)
    {
        update(transfer);
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
        .map(Transfer::snapshot)
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

/// Cancel desktop work before the shell destroys the window. Other CLI jobs own their leases.
#[tauri::command]
pub(crate) async fn crawler_shutdown(app: AppHandle, state: State<'_, DesktopState>) -> Result<()> {
    {
        let mut jobs = state.inner.lock().map_err(|_| Error::Busy)?;
        jobs.closing = true;
        for stop in jobs.crawls.values() {
            stop.cancel();
        }
        for transfer in &jobs.transfers {
            transfer.stop.cancel();
        }
    }
    // Stop embedded capture before the main window closes.
    if let Some(window) = app.get_webview_window(super::capture_browser::LABEL) {
        let _ = window.destroy();
    }
    let _ = tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let idle = state.inner.lock().is_ok_and(|jobs| {
                jobs.crawls.is_empty()
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
