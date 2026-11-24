//! Keeps WebView2 download operations on the UI thread and hands verified GETs to Rust.

use std::{
    cell::RefCell,
    collections::VecDeque,
    ffi::OsStr,
    fs::File,
    os::windows::ffi::OsStrExt,
    path::PathBuf,
    time::{Duration, Instant},
};

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use tauri::{
    AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent, webview::NewWindowResponse,
};
use url::Url;
use uuid::Uuid;
use webview2_com::{
    DownloadStartingEventHandler, Microsoft::Web::WebView2::Win32::*, StateChangedEventHandler,
    WebResourceResponseReceivedEventHandler, take_pwstr,
};
use windows_core::{Interface, PCWSTR, PWSTR};

use super::{
    Candidate, Error, Mode, Options, Result, StateData, Status, Work, origin,
    transfer::{self, Replay},
};
use crate::http_transfer::output;

pub(super) const LABEL: &str = "downloader-capture-browser";

// The browser owns operation handles and deferred decisions on its COM apartment.
thread_local! {
    static SESSION: RefCell<Session> = RefCell::new(Session::default());
}

#[derive(Default)]
struct Session {
    observed: VecDeque<(Instant, Observation)>,
    pending: Option<Pending>,
    active: Option<Active>,
}

struct Observation {
    url: Url,
    replay: Option<Replay>,
}

struct Pending {
    id: Uuid,
    url: String,
    started: Instant,
    args: ICoreWebView2DownloadStartingEventArgs,
    deferral: ICoreWebView2Deferral,
}

struct Active {
    id: Uuid,
    operation: ICoreWebView2DownloadOperation,
    staged: PathBuf,
    output: PathBuf,
    _reservation: File,
    _directory: tempfile::TempDir,
    cancelled: bool,
    started: Instant,
    state_token: i64,
}

impl Pending {
    fn dismiss(self) {
        // Complete every deferral, including cancellation and expiry.
        unsafe {
            let _ = self.args.SetCancel(true);
            let _ = self.deferral.Complete();
        }
    }
}

// Attach before navigation so the first response and immediate downloads can be observed.
pub(super) async fn open(app: AppHandle, state: StateData, url: Url) -> Result<()> {
    if let Some(window) = app.get_webview_window(LABEL) {
        let idle = {
            let data = state.lock();
            data.candidate.is_none() && data.capture_job.is_none()
        };
        if idle && window.url().ok().as_ref() != Some(&url) {
            window.navigate(url).map_err(|_| Error::Capture)?;
        }
        return window.set_focus().map_err(|_| Error::Capture);
    }
    {
        let mut data = state.lock();
        if data.shutting_down || data.capture_open {
            return Err(Error::Busy);
        }
        data.capture_open = true;
    }
    let window = WebviewWindowBuilder::new(
        &app,
        LABEL,
        WebviewUrl::External("about:blank".parse().unwrap()),
    )
    .title("WITS Tools Download Page")
    .inner_size(1000.0, 720.0)
    .incognito(true)
    .visible(false)
    .on_navigation(|url| matches!(url.scheme(), "http" | "https") || url.as_str() == "about:blank")
    .on_new_window(|_, _| NewWindowResponse::Deny)
    .build()
    .map_err(|_| {
        state.lock().capture_open = false;
        Error::Capture
    })?;
    let closing = state.clone();
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed) {
            close(&closing);
        }
    });
    let (ready, attached) = tokio::sync::oneshot::channel();
    let capture_app = app.clone();
    let setup = window.with_webview(move |webview| {
        // WebView2 delivers both events on this UI thread, in either order.
        let result = unsafe {
            webview.controller().CoreWebView2().and_then(|view| {
                let response =
                    WebResourceResponseReceivedEventHandler::create(Box::new(|_, args| {
                        if let Some(args) = args
                            && let Some(replay) = observe(&args)
                        {
                            SESSION.with_borrow_mut(|session| {
                                if session.observed.len() >= 32 {
                                    session.observed.pop_front();
                                }
                                session.observed.push_back((Instant::now(), replay));
                            });
                        }
                        Ok(())
                    }));
                view.cast::<ICoreWebView2_2>()?
                    .add_WebResourceResponseReceived(&response, &mut 0)?;
                let start = DownloadStartingEventHandler::create(Box::new(move |_, args| {
                    if let Some(args) = args {
                        start_download(&capture_app, &state, args)?;
                    }
                    Ok(())
                }));
                view.cast::<ICoreWebView2_4>()?
                    .add_DownloadStarting(&start, &mut 0)
            })
        };
        let _ = ready.send(result.is_ok());
    });
    if setup.is_err()
        || !attached.await.unwrap_or(false)
        || window.navigate(url).is_err()
        || window.show().is_err()
    {
        let _ = window.destroy();
        return Err(Error::Capture);
    }
    let _ = window.set_focus();
    Ok(())
}

// Record metadata for possible GET replay, with a bounded cache for event correlation.
fn observe(args: &ICoreWebView2WebResourceResponseReceivedEventArgs) -> Option<Observation> {
    unsafe {
        let request = args.Request().ok()?;
        let response = args.Response().ok()?;
        let mut status = 0;
        response.StatusCode(&mut status).ok()?;
        let mut method = PWSTR::null();
        request.Method(&mut method).ok()?;
        let method = take_pwstr(method);
        if status != 200 {
            return None;
        }
        let mut uri = PWSTR::null();
        request.Uri(&mut uri).ok()?;
        let url = crate::crawler::http_url(&take_pwstr(uri)).ok()?;
        // Include non-replayable 200 responses when checking for ambiguous matches.
        let replay = if method == "GET" {
            replay_headers(url.clone(), &request, &response)
        } else {
            None
        };
        Some(Observation { url, replay })
    }
}

// Inspect only the response metadata and explicit request header allowlist.
fn replay_headers(
    url: Url,
    request: &ICoreWebView2WebResourceRequest,
    response: &ICoreWebView2WebResourceResponseView,
) -> Option<Replay> {
    unsafe {
        let response_headers = response.Headers().ok()?;
        let get = |name| {
            let mut value = PWSTR::null();
            let _ = response_headers.GetHeader::<PCWSTR>(name, &mut value);
            take_pwstr(value)
        };
        let encoding = get(windows_core::w!("Content-Encoding"));
        if !encoding.is_empty() && !encoding.eq_ignore_ascii_case("identity") {
            return None;
        }
        let etag = HeaderValue::from_str(&get(windows_core::w!("ETag"))).ok()?;
        if !transfer::strong_etag(&etag) {
            return None;
        }
        let length = get(windows_core::w!("Content-Length")).parse().ok()?;
        let request_headers = request.Headers().ok()?;
        let mut headers = HeaderMap::new();
        for (name, key) in [
            ("cookie", windows_core::w!("Cookie")),
            ("authorization", windows_core::w!("Authorization")),
            ("referer", windows_core::w!("Referer")),
            ("origin", windows_core::w!("Origin")),
            ("user-agent", windows_core::w!("User-Agent")),
            ("accept", windows_core::w!("Accept")),
        ] {
            let mut value = PWSTR::null();
            let _ = request_headers.GetHeader::<PCWSTR>(key, &mut value);
            let value = take_pwstr(value);
            if !value.is_empty()
                && let Ok(mut value) = HeaderValue::from_str(&value)
            {
                value.set_sensitive(true);
                headers.insert(HeaderName::from_static(name), value);
            }
        }
        Some(Replay {
            url,
            headers,
            length,
            etag,
        })
    }
}

// A download prompt is created only for a real WebView2 download operation.
fn start_download(
    app: &AppHandle,
    state: &StateData,
    args: ICoreWebView2DownloadStartingEventArgs,
) -> windows_core::Result<()> {
    unsafe {
        args.SetHandled(true)?;
        if SESSION.with_borrow(|session| session.pending.is_some() || session.active.is_some()) {
            return args.SetCancel(true);
        }
        let operation = args.DownloadOperation()?;
        let mut uri = PWSTR::null();
        let mut path = PWSTR::null();
        let mut size = -1;
        operation.Uri(&mut uri)?;
        args.ResultFilePath(&mut path)?;
        let _ = operation.TotalBytesToReceive(&mut size);
        let url = take_pwstr(uri);
        let name = transfer::safe_name(&take_pwstr(path));
        let id = Uuid::new_v4();
        let deferral = args.GetDeferral()?;
        SESSION.with_borrow_mut(|session| {
            session.pending = Some(Pending {
                id,
                url: url.clone(),
                started: Instant::now(),
                args,
                deferral,
            })
        });
        let source = Url::parse(&url)
            .ok()
            .map(|url| origin(&url))
            .filter(|origin| origin != "null")
            .unwrap_or_else(|| "Capture browser".into());
        state.lock().candidate = Some(Candidate {
            id,
            name,
            origin: source,
            size: u64::try_from(size).ok(),
        });

        // Abandoned prompts never hold native operations indefinitely.
        let app = app.clone();
        let state = state.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(120)).await;
            let _ = app.run_on_main_thread(move || expire(id, &state));
        });
        Ok(())
    }
}

fn expire(id: Uuid, state: &StateData) {
    let pending = SESSION.with_borrow_mut(|session| {
        if session
            .pending
            .as_ref()
            .is_some_and(|pending| pending.id == id)
        {
            session.pending.take()
        } else {
            None
        }
    });
    if let Some(pending) = pending {
        state.lock().candidate = None;
        pending.dismiss();
    }
}

// Keep the original transfer available until the selected filename and replay probe are resolved.
pub(super) async fn confirm(
    app: AppHandle,
    state: StateData,
    id: Uuid,
    options: Options,
    folder: PathBuf,
    use_browser: bool,
) -> Result<Option<super::CaptureFallback>> {
    let candidate = state
        .lock()
        .candidate
        .clone()
        .filter(|candidate| candidate.id == id)
        .ok_or(Error::Capture)?;
    let name = transfer::safe_name(
        options
            .filename
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(&candidate.name),
    );
    let path = folder.join(&name);
    let reservation = output::reserve(&path, false).map_err(transfer::output_error)?;
    let (send, receive) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let replay = SESSION.with_borrow_mut(|session| {
            let pending = session
                .pending
                .as_ref()
                .filter(|pending| pending.id == id)?;
            let url = crate::crawler::http_url(&pending.url).ok()?;
            let mut matches = session.observed.iter().filter(|(seen, observation)| {
                let gap = seen
                    .saturating_duration_since(pending.started)
                    .max(pending.started.saturating_duration_since(*seen));
                gap < Duration::from_secs(2) && observation.url == url
            });
            let (_, observation) = matches.next()?;
            if matches.next().is_some() {
                return None;
            }
            observation
                .replay
                .clone()
                .filter(|replay| candidate.size.is_none_or(|size| replay.length == size))
        });
        let _ = send.send(replay);
    })
    .map_err(|_| Error::Capture)?;
    let replay = receive.await.map_err(|_| Error::Capture)?;
    use super::CaptureFallback;
    let (replay, fallback) = match replay {
        _ if use_browser => (None, None),
        _ if options.workers == 1 => (None, Some(CaptureFallback::SingleWorker)),
        Some(replay) if transfer::can_handoff(&replay, options.workers).await => {
            (Some(replay), None)
        }
        Some(_) => (None, Some(CaptureFallback::UnsupportedRanges)),
        None => (None, Some(CaptureFallback::UnverifiedRequest)),
    };
    let (send, receive) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let result = finish_decision(state, id, path, reservation, options, folder, name, replay);
        let _ = send.send(result);
    })
    .map_err(|_| Error::Capture)?;
    receive.await.map_err(|_| Error::Capture)??;
    Ok(fallback)
}

// Commit exactly one route for the pending operation.
#[allow(clippy::too_many_arguments)]
fn finish_decision(
    state: StateData,
    id: Uuid,
    path: PathBuf,
    reservation: File,
    options: Options,
    folder: PathBuf,
    name: String,
    replay: Option<Replay>,
) -> Result<()> {
    let candidate = state
        .lock()
        .candidate
        .clone()
        .filter(|candidate| candidate.id == id)
        .ok_or(Error::Capture)?;
    let pending = SESSION
        .with_borrow_mut(|session| {
            if session
                .pending
                .as_ref()
                .is_some_and(|pending| pending.id == id)
            {
                session.pending.take()
            } else {
                None
            }
        })
        .ok_or(Error::Capture)?;
    state.lock().candidate = None;
    if let Some(replay) = replay {
        drop(reservation);
        let result = state.enqueue(Work {
            url: replay.url.clone(),
            replay: Some((replay, name)),
            options,
            folder,
        });
        pending.dismiss();
        return result.map(|_| ());
    }
    begin_browser(state, pending, candidate, path, reservation, folder, name)
}

// Route WebView2 to a private sibling staging directory, then publish through the same output guard.
fn begin_browser(
    state: StateData,
    pending: Pending,
    candidate: Candidate,
    path: PathBuf,
    reservation: File,
    folder: PathBuf,
    name: String,
) -> Result<()> {
    let setup = (|| {
        let directory = tempfile::tempdir_in(folder).map_err(|_| Error::File)?;
        let staged = directory.path().join(&name);
        let operation = unsafe { pending.args.DownloadOperation() }.map_err(|_| Error::Capture)?;
        let id = {
            let mut data = state.lock();
            let id = data.add(&candidate.origin, Some(&name), true, Status::Downloading);
            data.capture_job = Some(id);
            data.progress(id, Mode::Browser, 0, candidate.size, Instant::now());
            data.save();
            id
        };
        SESSION.with_borrow_mut(|session| {
            session.active = Some(Active {
                id,
                operation: operation.clone(),
                staged: staged.clone(),
                output: path,
                _directory: directory,
                _reservation: reservation,
                cancelled: false,
                started: Instant::now(),
                state_token: 0,
            })
        });
        let result = unsafe { configure_browser(&state, &pending, &operation, id, &staged) };
        if result.is_err() {
            complete(&state, id, Err(Error::Capture));
        }
        result.map_err(|_| Error::Capture)
    })();
    if setup.is_err() {
        pending.dismiss();
    }
    setup
}

unsafe fn configure_browser(
    state: &StateData,
    pending: &Pending,
    operation: &ICoreWebView2DownloadOperation,
    id: Uuid,
    staged: &std::path::Path,
) -> windows_core::Result<()> {
    let wide: Vec<_> = OsStr::new(staged).encode_wide().chain([0]).collect();
    let changed = state.clone();
    unsafe {
        let state_handler = StateChangedEventHandler::create(Box::new(move |operation, _| {
            if let Some(operation) = operation {
                observe_state(&changed, id, &operation)?;
            }
            Ok(())
        }));
        pending.args.SetResultFilePath(PCWSTR(wide.as_ptr()))?;
        let mut state_token = 0;
        operation.add_StateChanged(&state_handler, &mut state_token)?;
        SESSION
            .with_borrow_mut(|session| session.active.as_mut().unwrap().state_token = state_token);
        pending.deferral.Complete()
    }
}

// Use the same terminal-state handling for events and status polling.
fn observe_state(
    state: &StateData,
    id: Uuid,
    operation: &ICoreWebView2DownloadOperation,
) -> windows_core::Result<bool> {
    let mut status = COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS;
    unsafe { operation.State(&mut status) }?;
    if status == COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED {
        complete(state, id, Ok(()));
    } else if status == COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED {
        let mut reason = COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NONE;
        let _ = unsafe { operation.InterruptReason(&mut reason) };
        #[cfg(debug_assertions)]
        eprintln!(
            "[downloader] WebView2 interrupted download, reason={}",
            reason.0
        );
        let error = match reason {
            COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_FAILED
            | COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_ACCESS_DENIED
            | COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_NO_SPACE
            | COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_NAME_TOO_LONG => Error::File,
            _ => Error::Network,
        };
        complete(state, id, Err(error));
    }
    Ok(status != COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS)
}

// Read the current operation on its COM thread when the workspace polls for jobs.
// WebView2 can learn the size after the download is detected.
pub(super) async fn sample_progress(app: &AppHandle, state: StateData) {
    if state.lock().capture_job.is_none() {
        return;
    }
    let (done, sampled) = tokio::sync::oneshot::channel();
    if app
        .run_on_main_thread(move || {
            let active = SESSION.with_borrow(|session| {
                let active = session.active.as_ref()?;
                Some((active.id, active.operation.clone(), active.started))
            });
            if let Some((id, operation, started)) = active
                && observe_state(&state, id, &operation).is_ok_and(|terminal| !terminal)
            {
                let mut bytes = 0;
                let mut total = -1;
                // Keep the last known size if a later read temporarily has no size.
                if unsafe { operation.BytesReceived(&mut bytes) }.is_ok() {
                    let live_total = unsafe { operation.TotalBytesToReceive(&mut total) }
                        .ok()
                        .and_then(|()| u64::try_from(total).ok())
                        .filter(|total| *total > 0);
                    let mut data = state.lock();
                    let total = live_total.or_else(|| data.job(id).and_then(|job| job.total));
                    data.progress(id, Mode::Browser, bytes.max(0) as u64, total, started);
                }
            }
            let _ = done.send(());
        })
        .is_ok()
    {
        let _ = sampled.await;
    }
}

fn complete(state: &StateData, id: Uuid, result: Result<()>) {
    let active = SESSION.with_borrow_mut(|session| {
        if session
            .active
            .as_ref()
            .is_some_and(|active| active.id == id)
        {
            session.active.take()
        } else {
            None
        }
    });
    let Some(active) = active else {
        return;
    };
    unsafe {
        let _ = active.operation.remove_StateChanged(active.state_token);
    }
    let result = if active.cancelled {
        Err(Error::Cancelled)
    } else {
        result.and_then(|()| {
            let size = std::fs::metadata(&active.staged)
                .map_err(|_| Error::File)?
                .len();
            let staged =
                tempfile::TempPath::try_from_path(&active.staged).map_err(|_| Error::File)?;
            output::publish(staged, &active.output, false).map_err(transfer::output_error)?;
            Ok((active.output.clone(), size))
        })
    };
    let mut data = state.lock();
    data.capture_job = None;
    data.finish(id, result);
}

pub(super) fn cancel(app: &AppHandle, id: Uuid) -> Result<()> {
    let state = app.state::<StateData>().inner().clone();
    app.run_on_main_thread(move || {
        expire(id, &state);
        let operation = SESSION.with_borrow_mut(|session| {
            let active = session.active.as_mut().filter(|active| active.id == id)?;
            active.cancelled = true;
            Some(active.operation.clone())
        });
        if let Some(operation) = operation
            && unsafe { operation.Cancel() }.is_err()
        {
            complete(&state, id, Err(Error::Cancelled));
        }
    })
    .map_err(|_| Error::Capture)
}

fn close(state: &StateData) {
    let pending = SESSION.with_borrow_mut(|session| {
        session.observed.clear();
        session.pending.take()
    });
    if let Some(pending) = pending {
        pending.dismiss();
    }
    let active = SESSION.with_borrow_mut(|session| {
        session.active.as_mut().map(|active| {
            active.cancelled = true;
            (active.id, active.operation.clone())
        })
    });
    if let Some((id, operation)) = active {
        unsafe {
            let _ = operation.Cancel();
        }
        complete(state, id, Err(Error::Cancelled));
    }
    let mut data = state.lock();
    data.capture_open = false;
    data.candidate = None;
}
