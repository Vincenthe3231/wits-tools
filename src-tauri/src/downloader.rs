//! Owns Downloader's native queue, redacted history, and desktop commands.

use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
    time::Instant,
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;
use tokio_util::sync::CancellationToken;
use url::Url;
use uuid::Uuid;

#[cfg(windows)]
mod browser;
#[cfg(test)]
mod tests;
mod transfer;

// Send stable codes to the frontend without exposing request or filesystem diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Error {
    InvalidUrl,
    InvalidWorkers,
    AccessDenied,
    RateLimited,
    Page,
    Network,
    Encoding,
    RangeChanged,
    Conflict,
    File,
    Cancelled,
    Capture,
    Busy,
    MissingSource,
    Storage,
}
type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Options {
    folder: Option<PathBuf>,
    filename: Option<String>,
    workers: usize,
}

impl Options {
    // Resolve a user-selected directory or the native Downloads directory.
    fn folder(&self, app: &AppHandle) -> Result<PathBuf> {
        if !(1..=16).contains(&self.workers) {
            return Err(Error::InvalidWorkers);
        }
        let folder = self
            .folder
            .clone()
            .map(Ok)
            .unwrap_or_else(|| app.path().download_dir())
            .map_err(|_| Error::File)?;
        let folder = std::path::absolute(folder).map_err(|_| Error::File)?;
        if !folder.is_dir() {
            return Err(Error::File);
        }
        Ok(folder)
    }
}

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Status {
    Queued,
    Downloading,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
}

impl Status {
    fn active(self) -> bool {
        matches!(self, Self::Queued | Self::Downloading)
    }
}

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Mode {
    Single,
    Parallel,
    Browser,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Job {
    id: Uuid,
    origin: String,
    name: String,
    captured: bool,
    status: Status,
    mode: Option<Mode>,
    bytes: u64,
    total: Option<u64>,
    speed: Option<f64>,
    eta: Option<f64>,
    path: Option<PathBuf>,
    error: Option<Error>,
    retry_available: bool,
}

#[derive(Clone)]
struct Work {
    url: Url,
    replay: Option<(transfer::Replay, String)>,
    options: Options,
    folder: PathBuf,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Candidate {
    id: Uuid,
    name: String,
    origin: String,
    size: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Snapshot {
    jobs: Vec<Job>,
    candidate: Option<Candidate>,
    capture_open: bool,
    history_error: bool,
}

// COM-owned capture handles stay on the UI thread. Shared state contains data only.
#[derive(Default)]
struct Inner {
    history_path: Option<PathBuf>,
    jobs: Vec<Job>,
    pending: VecDeque<(Uuid, Work)>,
    retries: HashMap<Uuid, Work>,
    active: usize,
    tokens: HashMap<Uuid, CancellationToken>,
    candidate: Option<Candidate>,
    capture_open: bool,
    capture_job: Option<Uuid>,
    history_error: bool,
    shutting_down: bool,
}

#[derive(Clone, Default)]
pub(crate) struct StateData(Arc<Mutex<Inner>>);

impl StateData {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(|error| error.into_inner())
    }

    // Load once and convert persisted in-progress records into recoverable interruptions.
    fn init(&self, app: &AppHandle) -> Result<()> {
        let mut data = self.lock();
        if data.history_path.is_some() {
            return Ok(());
        }
        let folder = app
            .path()
            .app_local_data_dir()
            .map_err(|_| Error::Storage)?
            .join("downloader");
        std::fs::create_dir_all(&folder).map_err(|_| Error::Storage)?;
        let path = folder.join("history.json");
        match std::fs::read(&path) {
            Ok(bytes) => data.jobs = serde_json::from_slice(&bytes).map_err(|_| Error::Storage)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(Error::Storage),
        }
        for job in &mut data.jobs {
            if job.status.active() {
                job.status = Status::Interrupted;
            }
            job.retry_available = false;
            job.speed = None;
            job.eta = None;
        }
        data.history_path = Some(path);
        data.save();
        Ok(())
    }

    // Queue insertion and slot allocation use one lock, keeping submission order stable.
    fn enqueue(&self, work: Work) -> Result<Uuid> {
        let mut data = self.lock();
        if data.shutting_down {
            return Err(Error::Busy);
        }
        let captured = work.replay.is_some();
        let id = data.add(
            &origin(&work.url),
            work.replay.as_ref().map(|(_, name)| name.as_str()),
            captured,
            Status::Queued,
        );
        if !captured {
            data.retries.insert(id, work.clone());
        }
        data.pending.push_back((id, work));
        data.save();
        self.dispatch(&mut data);
        Ok(id)
    }

    fn dispatch(&self, data: &mut Inner) {
        while !data.shutting_down && data.active < 2 {
            let Some((id, work)) = data.pending.pop_front() else {
                break;
            };
            let token = CancellationToken::new();
            data.active += 1;
            data.tokens.insert(id, token.clone());
            data.job(id).expect("queued job").status = Status::Downloading;
            let state = self.clone();
            tauri::async_runtime::spawn(async move {
                let started = Instant::now();
                let result = transfer::run(
                    work.url,
                    work.replay,
                    work.options,
                    work.folder,
                    token,
                    |mode, bytes, total| state.lock().progress(id, mode, bytes, total, started),
                )
                .await;
                let mut data = state.lock();
                data.finish(id, result);
                data.tokens.remove(&id);
                data.active -= 1;
                state.dispatch(&mut data);
            });
        }
    }
}

impl Inner {
    fn job(&mut self, id: Uuid) -> Option<&mut Job> {
        self.jobs.iter_mut().find(|job| job.id == id)
    }

    fn add(&mut self, origin: &str, name: Option<&str>, captured: bool, status: Status) -> Uuid {
        let id = Uuid::new_v4();
        self.jobs.insert(
            0,
            Job {
                id,
                origin: origin.into(),
                name: name.unwrap_or("Download").into(),
                captured,
                status,
                mode: None,
                bytes: 0,
                total: None,
                speed: None,
                eta: None,
                path: None,
                error: None,
                retry_available: !captured,
            },
        );
        id
    }

    // Persist the public job representation only. Keep retry URLs and request headers in RAM.
    fn save(&mut self) {
        let Some(path) = &self.history_path else {
            return;
        };
        self.history_error = write_history(path, &self.jobs).is_err();
    }

    // Remove only terminal records. Restore the row if replacing the history fails.
    fn remove_history(&mut self, id: Uuid) -> Result<()> {
        let Some(index) = self.jobs.iter().position(|job| job.id == id) else {
            return Ok(());
        };
        if self.jobs[index].status.active() {
            return Err(Error::Busy);
        }
        let job = self.jobs.remove(index);
        self.save();
        if self.history_error {
            self.jobs.insert(index, job);
            return Err(Error::Storage);
        }
        self.retries.remove(&id);
        Ok(())
    }

    fn progress(&mut self, id: Uuid, mode: Mode, bytes: u64, total: Option<u64>, started: Instant) {
        let Some(job) = self.job(id).filter(|job| job.status.active()) else {
            return;
        };
        let elapsed = started.elapsed().as_secs_f64();
        // Once all bytes arrive, the browser may still be finalizing the file.
        let transferring = total.is_none_or(|total| bytes < total);
        let speed = (transferring && bytes > 0 && elapsed >= 0.5).then(|| bytes as f64 / elapsed);
        job.mode = Some(mode);
        job.total = total;
        job.bytes = bytes;
        job.speed = speed;
        job.eta = total
            .zip(speed)
            .map(|(total, speed)| total.saturating_sub(bytes) as f64 / speed);
    }

    fn finish(&mut self, id: Uuid, result: Result<(PathBuf, u64)>) {
        let Some(job) = self.job(id) else {
            return;
        };
        job.speed = None;
        job.eta = None;
        match result {
            Ok((path, size)) => {
                job.name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                job.path = Some(path);
                job.bytes = size;
                job.total = Some(size);
                job.status = Status::Completed;
                job.retry_available = false;
                self.retries.remove(&id);
            }
            Err(error) => {
                job.status = if matches!(error, Error::Cancelled) {
                    Status::Cancelled
                } else {
                    Status::Failed
                };
                job.error = Some(error);
            }
        }
        self.save();
    }
}

// Replace a history snapshot only after its complete JSON representation reaches disk.
fn write_history(path: &Path, jobs: &[Job]) -> Result<()> {
    let folder = path.parent().ok_or(Error::Storage)?;
    let file = tempfile::NamedTempFile::new_in(folder).map_err(|_| Error::Storage)?;
    serde_json::to_writer(file.as_file(), jobs).map_err(|_| Error::Storage)?;
    file.as_file().sync_all().map_err(|_| Error::Storage)?;
    file.persist(path).map_err(|_| Error::Storage)?;
    Ok(())
}

fn origin(url: &Url) -> String {
    url.origin().ascii_serialization()
}

#[tauri::command]
pub(crate) async fn downloader_status(
    app: AppHandle,
    state: State<'_, StateData>,
) -> Result<Snapshot> {
    state.init(&app)?;
    #[cfg(windows)]
    browser::sample_progress(&app, state.inner().clone()).await;
    let data = state.lock();
    Ok(Snapshot {
        jobs: data.jobs.clone(),
        candidate: data.candidate.clone(),
        capture_open: data.capture_open,
        history_error: data.history_error,
    })
}

#[tauri::command]
pub(crate) fn downloader_enqueue(
    app: AppHandle,
    state: State<'_, StateData>,
    url: String,
    options: Options,
) -> Result<Uuid> {
    state.init(&app)?;
    let url = crate::crawler::http_url(&url).map_err(|_| Error::InvalidUrl)?;
    let folder = options.folder(&app)?;
    state.enqueue(Work {
        url,
        replay: None,
        options,
        folder,
    })
}

#[tauri::command]
pub(crate) fn downloader_retry(
    app: AppHandle,
    state: State<'_, StateData>,
    id: Uuid,
    options: Options,
) -> Result<Uuid> {
    let mut work = {
        let mut data = state.lock();
        if data
            .job(id)
            .is_none_or(|job| job.status.active() || job.status == Status::Completed)
        {
            return Err(Error::Busy);
        }
        data.retries.get(&id).cloned().ok_or(Error::MissingSource)?
    };
    work.folder = options.folder(&app)?;
    work.options = options;
    state.enqueue(work)
}

#[tauri::command]
pub(crate) async fn downloader_cancel(
    app: AppHandle,
    state: State<'_, StateData>,
    id: Uuid,
) -> Result<()> {
    let browser = {
        let mut data = state.lock();
        if let Some(token) = data.tokens.get(&id) {
            token.cancel();
        }
        if let Some(index) = data.pending.iter().position(|(queued, _)| *queued == id) {
            data.pending.remove(index);
            data.finish(id, Err(Error::Cancelled));
        }
        data.capture_job == Some(id)
            || data
                .candidate
                .as_ref()
                .is_some_and(|candidate| candidate.id == id)
    };
    #[cfg(windows)]
    if browser {
        self::browser::cancel(&app, id)?;
    }
    #[cfg(not(windows))]
    let _ = (app, browser);
    Ok(())
}

#[tauri::command]
pub(crate) fn downloader_remove_history(
    app: AppHandle,
    state: State<'_, StateData>,
    id: Uuid,
) -> Result<()> {
    state.init(&app)?;
    state.lock().remove_history(id)
}

#[tauri::command]
pub(crate) fn downloader_reveal(
    app: AppHandle,
    state: State<'_, StateData>,
    id: Uuid,
) -> Result<()> {
    let path = state
        .lock()
        .job(id)
        .and_then(|job| job.path.clone())
        .ok_or(Error::File)?;
    app.opener()
        .reveal_item_in_dir(path)
        .map_err(|_| Error::File)
}

#[tauri::command]
pub(crate) async fn downloader_open_capture(
    app: AppHandle,
    state: State<'_, StateData>,
    url: String,
) -> Result<()> {
    state.init(&app)?;
    let url = crate::crawler::http_url(&url).map_err(|_| Error::InvalidUrl)?;
    #[cfg(windows)]
    return browser::open(app, state.inner().clone(), url).await;
    #[cfg(not(windows))]
    {
        let _ = url;
        Err(Error::Capture)
    }
}

#[tauri::command]
pub(crate) async fn downloader_confirm_capture(
    app: AppHandle,
    state: State<'_, StateData>,
    id: Uuid,
    options: Options,
    use_browser: bool,
) -> Result<Option<CaptureFallback>> {
    let folder = options.folder(&app)?;
    #[cfg(windows)]
    return browser::confirm(app, state.inner().clone(), id, options, folder, use_browser).await;
    #[cfg(not(windows))]
    {
        let _ = (state, id, folder, use_browser);
        Err(Error::Capture)
    }
}

// Explain why an acceleration request used the original browser transfer.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CaptureFallback {
    SingleWorker,
    UnverifiedRequest,
    UnsupportedRanges,
}

#[tauri::command]
pub(crate) async fn downloader_shutdown(app: AppHandle, state: State<'_, StateData>) -> Result<()> {
    {
        let mut data = state.lock();
        data.shutting_down = true;
        for token in data.tokens.values() {
            token.cancel();
        }
        while let Some((id, _)) = data.pending.pop_front() {
            data.finish(id, Err(Error::Cancelled));
        }
    }
    #[cfg(windows)]
    if let Some(window) = app.get_webview_window(browser::LABEL) {
        let _ = window.destroy();
    }
    for _ in 0..40 {
        if {
            let data = state.lock();
            data.active == 0 && data.capture_job.is_none()
        } {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    Ok(())
}
