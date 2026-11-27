//! Hosts native WebView2 page tabs on the same UI thread as the bundled toolbar.

use std::{
    cell::RefCell,
    fs::{self, File, OpenOptions},
    os::windows::{ffi::OsStrExt, fs::MetadataExt},
    path::{Path, PathBuf},
    time::Duration,
};

use tauri::{
    AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
use tokio::sync::oneshot;
use url::Url;
use uuid::Uuid;
use webview2_com::{
    BrowserProcessExitedEventHandler, CreateCoreWebView2ControllerCompletedHandler,
    CreateCoreWebView2EnvironmentCompletedHandler, DocumentTitleChangedEventHandler,
    DownloadStartingEventHandler, HistoryChangedEventHandler, Microsoft::Web::WebView2::Win32::*,
    MoveFocusRequestedEventHandler, NavigationCompletedEventHandler,
    NavigationStartingEventHandler, NewWindowRequestedEventHandler, SourceChangedEventHandler,
    take_pwstr,
};
use windows::Win32::Foundation::{HWND, RECT};
use windows_core::{BOOL, Interface, PCWSTR, PWSTR};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, HWND_TOP, SW_HIDE, SW_SHOWNA, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SetWindowPos,
    ShowWindow, WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS,
};

use super::{Action, BrowserState, LABEL, TabState};

mod downloads;

const UNAVAILABLE: &str = "The private browser is unavailable. Try reopening it.";
const BLANK: &str = "about:blank";

// COM objects stay on the desktop thread. Retired environments stay alive until they report exit.
thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
    static RETIRED: RefCell<Vec<(Uuid, ICoreWebView2Environment, Folder)>> = const { RefCell::new(Vec::new()) };
}

struct Session {
    window: WebviewWindow,
    host: HWND,
    folder: Folder,
    environment: Option<ICoreWebView2Environment>,
    tabs: Vec<Tab>,
    next_tab: u32,
    active: Option<u32>,
    top: Option<i32>,
    pending: bool,
    closing: bool,
    notice: Option<&'static str>,
    downloads: downloads::Transfers,
}

struct Tab {
    id: u32,
    controller: ICoreWebView2Controller,
    view: ICoreWebView2,
}

struct Folder {
    id: Uuid,
    path: PathBuf,
    _lease: File,
}

// Keep runtime files below an app-owned directory, with a lease outside the profile itself.
impl Folder {
    fn root(app: &AppHandle) -> Result<PathBuf, String> {
        app.path()
            .app_local_data_dir()
            .map(|path| path.join("private-browser"))
            .map_err(|_| UNAVAILABLE.into())
    }

    fn create(app: &AppHandle) -> Result<Self, String> {
        let id = Uuid::new_v4();
        let root = Self::root(app)?;
        fs::create_dir_all(&root).map_err(|_| UNAVAILABLE)?;
        if !plain_directory(&root) {
            return Err(UNAVAILABLE.into());
        }
        let path = root.join(id.to_string());
        fs::create_dir(&path).map_err(|_| UNAVAILABLE)?;
        let lease = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path.join("lease"))
            .map_err(|_| UNAVAILABLE)?;
        lease.try_lock().map_err(|_| UNAVAILABLE)?;
        fs::create_dir(path.join("profile")).map_err(|_| UNAVAILABLE)?;
        Ok(Self {
            id,
            path,
            _lease: lease,
        })
    }

    fn remove(self) {
        let path = self.path.clone();
        for attempt in 0..6 {
            let staging_removed = downloads::remove_staging(&path);
            let profile_removed = remove_directory(&path.join("profile"));
            if staging_removed && profile_removed {
                drop(self);
                let _ = fs::remove_file(path.join("lease"));
                let _ = fs::remove_dir(&path);
                return;
            }
            std::thread::sleep(Duration::from_millis(50 << attempt));
        }
    }
}

// Reject reparse points so recovery cannot walk into a different tool's files.
fn plain_directory(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|meta| meta.is_dir() && meta.file_attributes() & 0x400 == 0)
}

fn remove_directory(path: &Path) -> bool {
    match fs::symlink_metadata(path) {
        Ok(_) if plain_directory(path) => fs::remove_dir_all(path).is_ok(),
        Err(error) => error.kind() == std::io::ErrorKind::NotFound,
        _ => false,
    }
}

pub(super) fn recover(app: &AppHandle) {
    let Ok(root) = Folder::root(app) else { return };
    std::thread::spawn(move || {
        if !plain_directory(&root) {
            return;
        }
        let Ok(entries) = fs::read_dir(root) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(id) = entry
                .file_name()
                .to_str()
                .and_then(|name| Uuid::parse_str(name).ok())
            else {
                continue;
            };
            if !plain_directory(&path) {
                continue;
            }
            let lease_path = path.join("lease");
            if !fs::symlink_metadata(&lease_path)
                .is_ok_and(|meta| meta.is_file() && meta.file_attributes() & 0x400 == 0)
            {
                continue;
            }
            let Ok(lease) = OpenOptions::new().read(true).write(true).open(lease_path) else {
                continue;
            };
            if lease.try_lock().is_ok() {
                Folder {
                    id,
                    path,
                    _lease: lease,
                }
                .remove();
            }
        }
    });
}

// Bridge asynchronous Tauri commands to short UI-thread operations.
async fn dispatch<T: Send + 'static>(
    app: AppHandle,
    work: impl FnOnce(AppHandle) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (send, receive) = oneshot::channel();
    let desktop = app.clone();
    app.run_on_main_thread(move || {
        let _ = send.send(work(desktop));
    })
    .map_err(|_| UNAVAILABLE)?;
    receive.await.map_err(|_| UNAVAILABLE)?
}

pub(super) async fn open(app: AppHandle) -> Result<(), String> {
    dispatch(app, open_window).await
}

fn open_window(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(LABEL) {
        return window.set_focus().map_err(|_| UNAVAILABLE.into());
    }

    let folder = Folder::create(&app)?;
    let development_origin = app.config().build.dev_url.clone();
    let built = WebviewWindowBuilder::new(
        &app,
        LABEL,
        WebviewUrl::App("index.html?private-browser".into()),
    )
    .title("Private Browser · WITS Tools")
    .inner_size(1120.0, 760.0)
    .min_inner_size(420.0, 360.0)
    .on_navigation(move |url| {
        url.host_str().is_some_and(|host| {
            (matches!(url.scheme(), "http" | "https") && host == "tauri.localhost")
                || (url.scheme() == "tauri" && host == "localhost")
        }) || development_origin
            .as_ref()
            .is_some_and(|origin| url.origin() == origin.origin())
    })
    .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
    .build();
    let window = match built {
        Ok(window) => window,
        Err(_) => {
            std::thread::spawn(move || folder.remove());
            return Err(UNAVAILABLE.into());
        }
    };

    // The site lives in its own child HWND so the full-window Vue WebView cannot cover it.
    let host = window.hwnd().ok().map(|parent| unsafe {
        CreateWindowExW(
            0,
            windows_sys::w!("STATIC"),
            std::ptr::null(),
            WS_CHILD | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
            0,
            0,
            0,
            0,
            parent.0.cast(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
        )
    });
    let Some(host) = host.filter(|host| !host.is_null()) else {
        let _ = window.destroy();
        std::thread::spawn(move || folder.remove());
        return Err(UNAVAILABLE.into());
    };

    let profile = wide(&folder.path.join("profile"));
    let id = folder.id;
    SESSION.with_borrow_mut(|current| {
        *current = Some(Session {
            window: window.clone(),
            host: HWND(host.cast()),
            folder,
            environment: None,
            tabs: Vec::new(),
            next_tab: 1,
            active: None,
            top: None,
            pending: true,
            closing: false,
            notice: None,
            downloads: downloads::Transfers::default(),
        })
    });
    let desktop = app.clone();

    // Route both close buttons through the same authorization and teardown.
    window.on_window_event(move |event| match event {
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            let app = desktop.clone();
            tauri::async_runtime::spawn(async move {
                if prepare_close(app.clone()).await == Ok(true) {
                    let _ = finish_close(app).await;
                }
            });
        }
        WindowEvent::Resized(_)
        | WindowEvent::Moved(_)
        | WindowEvent::ScaleFactorChanged { .. } => {
            SESSION.with_borrow(|current| {
                if let Some(session) = current.as_ref() {
                    session.layout();
                }
            });
        }
        _ => {}
    });

    // Register callbacks before loading pages and verify policy has not redirected the profile.
    let callback = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
        move |result, environment| {
            let outcome = result
                .and_then(|()| {
                    environment.ok_or_else(|| {
                        windows_core::Error::from_hresult(windows_core::HRESULT(
                            0x80004005u32 as i32,
                        ))
                    })
                })
                .and_then(|environment| install_environment(id, environment));
            if outcome.is_err() {
                report_failure(id);
            }
            Ok(())
        },
    ));
    if unsafe {
        CreateCoreWebView2EnvironmentWithOptions(
            PCWSTR::null(),
            PCWSTR(profile.as_ptr()),
            None,
            &callback,
        )
    }
    .is_err()
    {
        report_failure(id);
    }
    Ok(())
}

fn install_environment(
    id: Uuid,
    environment: ICoreWebView2Environment,
) -> windows_core::Result<()> {
    let expected = SESSION.with_borrow(|current| {
        current
            .as_ref()
            .filter(|session| session.folder.id == id)
            .map(|session| session.folder.path.join("profile"))
    });
    let Some(expected) = expected else {
        return Ok(());
    };
    unsafe {
        let mut folder = PWSTR::null();
        environment
            .cast::<ICoreWebView2Environment7>()?
            .UserDataFolder(&mut folder)?;
        let actual = PathBuf::from(take_pwstr(folder));
        if fs::canonicalize(actual)
            .ok()
            .zip(fs::canonicalize(expected).ok())
            .is_none_or(|(a, b)| a != b)
        {
            return Err(windows_core::Error::from_hresult(windows_core::HRESULT(
                0x80070005u32 as i32,
            )));
        }
        let callback = BrowserProcessExitedEventHandler::create(Box::new(move |_, _| {
            let retired = RETIRED.with_borrow_mut(|retired| {
                let position = retired.iter().position(|(session, _, _)| *session == id)?;
                Some(retired.swap_remove(position))
            });
            if let Some((_, _, folder)) = retired {
                std::thread::spawn(move || folder.remove());
            }
            Ok(())
        }));
        environment
            .cast::<ICoreWebView2Environment5>()?
            .add_BrowserProcessExited(&callback, &mut 0)?;
    }

    SESSION.with_borrow_mut(|current| {
        if let Some(session) = current.as_mut().filter(|session| session.folder.id == id) {
            session.environment = Some(environment);
            session.pending = false;
        }
    });
    add_tab(None, None)
        .map_err(|_| windows_core::Error::from_hresult(windows_core::HRESULT(0x80004005u32 as i32)))
}

fn report_failure(id: Uuid) {
    SESSION.with_borrow_mut(|current| {
        if let Some(session) = current.as_mut().filter(|session| session.folder.id == id) {
            session.pending = false;
            session.notice = Some(UNAVAILABLE);
            session.changed();
        }
    });
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn address(input: &str) -> Option<Url> {
    let url = Url::parse(input).ok()?;
    (matches!(url.scheme(), "http" | "https") && url.host().is_some()).then_some(url)
}

// Create a replacement before dropping the old final tab so the private process stays alive.
fn add_tab(url: Option<Url>, replace: Option<u32>) -> Result<(), String> {
    let (environment, host, session_id, tab_id) = SESSION.with_borrow_mut(|current| {
        let session = current.as_mut().ok_or(UNAVAILABLE)?;
        if session.pending || session.closing {
            return Err(UNAVAILABLE);
        }
        let environment = session.environment.clone().ok_or(UNAVAILABLE)?;
        let tab_id = session.next_tab;
        session.next_tab += 1;
        session.pending = true;
        Ok((environment, session.host, session.folder.id, tab_id))
    })?;
    let callback = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
        move |result, controller| {
            if result.is_err() {
                report_failure(session_id);
            } else if let Some(controller) = controller {
                if prepare_tab(session_id, tab_id, controller.clone(), url, replace).is_err() {
                    unsafe {
                        let _ = controller.Close();
                    }
                    report_failure(session_id);
                }
            } else {
                report_failure(session_id);
            }
            Ok(())
        },
    ));
    let created = (|| unsafe {
        let environment = environment.cast::<ICoreWebView2Environment10>()?;
        let options = environment.CreateCoreWebView2ControllerOptions()?;
        options.SetIsInPrivateModeEnabled(true)?;
        environment.CreateCoreWebView2ControllerWithOptions(host, &options, &callback)
    })();
    if created.is_err() {
        report_failure(session_id);
        return Err(UNAVAILABLE.into());
    }
    Ok(())
}

fn changed(id: Uuid) {
    SESSION.with_borrow(|current| {
        if let Some(session) = current.as_ref().filter(|session| session.folder.id == id) {
            session.layout();
            session.changed();
        }
    });
}

// Install page handlers on native controls. They never receive the Tauri initialization script.
fn prepare_tab(
    session_id: Uuid,
    id: u32,
    controller: ICoreWebView2Controller,
    url: Option<Url>,
    replace: Option<u32>,
) -> windows_core::Result<()> {
    let Some(window) = SESSION.with_borrow(|current| {
        current
            .as_ref()
            .filter(|session| session.folder.id == session_id)
            .map(|session| session.window.clone())
    }) else {
        return unsafe { controller.Close() };
    };
    unsafe {
        controller.SetIsVisible(false)?;
        let view = controller.CoreWebView2()?;
        let profile = view.cast::<ICoreWebView2_13>()?.Profile()?;
        let mut private = BOOL(0);
        profile.IsInPrivateModeEnabled(&mut private)?;
        if !private.as_bool() {
            return Err(windows_core::Error::from_hresult(windows_core::HRESULT(
                0x80070005u32 as i32,
            )));
        }
        let settings = view.Settings()?;
        settings.SetIsWebMessageEnabled(false)?;
        settings.SetAreHostObjectsAllowed(false)?;

        view.add_NavigationStarting(
            &NavigationStartingEventHandler::create(Box::new(|_, args| {
                if let Some(args) = args {
                    let mut uri = PWSTR::null();
                    args.Uri(&mut uri)?;
                    let uri = take_pwstr(uri);
                    if uri != BLANK && address(&uri).is_none() {
                        args.SetCancel(true)?;
                    }
                }
                Ok(())
            })),
            &mut 0,
        )?;
        let desktop = window.app_handle().clone();
        view.add_NewWindowRequested(
            &NewWindowRequestedEventHandler::create(Box::new(move |_, args| {
                if let Some(args) = args {
                    args.SetHandled(true)?;
                    let mut gesture = BOOL(0);
                    args.IsUserInitiated(&mut gesture)?;
                    if gesture.as_bool() {
                        let mut uri = PWSTR::null();
                        args.Uri(&mut uri)?;
                        if let Some(url) = address(&take_pwstr(uri)) {
                            let _ = desktop.run_on_main_thread(move || {
                                let allowed = SESSION.with_borrow(|current| {
                                    current.as_ref().is_some_and(|session| {
                                        session.folder.id == session_id
                                            && session.tabs.iter().any(|tab| tab.id == id)
                                    })
                                });
                                if allowed {
                                    let _ = add_tab(Some(url), None);
                                }
                            });
                        }
                    }
                }
                Ok(())
            })),
            &mut 0,
        )?;

        view.add_SourceChanged(
            &SourceChangedEventHandler::create(Box::new(move |_, _| {
                changed(session_id);
                Ok(())
            })),
            &mut 0,
        )?;
        view.add_HistoryChanged(
            &HistoryChangedEventHandler::create(Box::new(move |_, _| {
                changed(session_id);
                Ok(())
            })),
            &mut 0,
        )?;
        view.add_DocumentTitleChanged(
            &DocumentTitleChangedEventHandler::create(Box::new(move |_, _| {
                changed(session_id);
                Ok(())
            })),
            &mut 0,
        )?;
        view.add_NavigationCompleted(
            &NavigationCompletedEventHandler::create(Box::new(move |_, args| {
                if let Some(args) = args {
                    let mut success = BOOL(0);
                    args.IsSuccess(&mut success)?;
                    let mut status = COREWEBVIEW2_WEB_ERROR_STATUS(0);
                    args.WebErrorStatus(&mut status)?;
                    if !success.as_bool()
                        && status != COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED
                    {
                        SESSION.with_borrow_mut(|current| {
                            if let Some(session) = current
                                .as_mut()
                                .filter(|session| session.folder.id == session_id)
                            {
                                session.notice = Some(
                                    "The page could not load. Check the address or try reloading.",
                                );
                                session.changed();
                            }
                        });
                    }
                }
                Ok(())
            })),
            &mut 0,
        )?;
        let toolbar = window.clone();
        controller.add_MoveFocusRequested(
            &MoveFocusRequestedEventHandler::create(Box::new(move |_, args| {
                if let Some(args) = args {
                    args.SetHandled(true)?;
                    let _ = toolbar.set_focus();
                    let _ = toolbar.emit("private-browser-focus", ());
                }
                Ok(())
            })),
            &mut 0,
        )?;
        let desktop = window.app_handle().clone();
        view.cast::<ICoreWebView2_4>()?.add_DownloadStarting(
            &DownloadStartingEventHandler::create(Box::new(move |_, args| {
                if let Some(args) = args {
                    downloads::start(&desktop, session_id, args)?;
                }
                Ok(())
            })),
            &mut 0,
        )?;

        let previous = SESSION.with_borrow_mut(|current| {
            let session = current.as_mut().unwrap();
            let previous = replace.and_then(|replace| session.take_tab(replace));
            session.tabs.push(Tab {
                id,
                controller,
                view: view.clone(),
            });
            session.active = Some(id);
            session.pending = false;
            session.notice = None;
            previous
        });
        if let Some(tab) = previous {
            tab.close();
        }
        SESSION.with_borrow(|current| {
            if let Some(session) = current.as_ref() {
                session.layout();
                session.changed();
            }
        });
        if let Some(url) = url {
            view.Navigate(PCWSTR(wide(Path::new(url.as_str())).as_ptr()))?;
        }
    }
    Ok(())
}

impl Tab {
    fn close(self) {
        unsafe {
            let _ = self.controller.Close();
        }
    }

    fn source(&self) -> String {
        unsafe {
            let mut value = PWSTR::null();
            let _ = self.view.Source(&mut value);
            take_pwstr(value)
        }
    }

    fn state(&self) -> TabState {
        unsafe {
            let mut source = PWSTR::null();
            let mut title = PWSTR::null();
            let mut back = BOOL(0);
            let mut forward = BOOL(0);
            let _ = self.view.Source(&mut source);
            let _ = self.view.DocumentTitle(&mut title);
            let _ = self.view.CanGoBack(&mut back);
            let _ = self.view.CanGoForward(&mut forward);
            let address = take_pwstr(source);
            let title = take_pwstr(title);
            TabState {
                id: self.id,
                address,
                title,
                can_back: back.as_bool(),
                can_forward: forward.as_bool(),
            }
        }
    }
}

impl Session {
    fn changed(&self) {
        let _ = self.window.emit("private-browser-changed", ());
    }

    fn state(&self) -> BrowserState {
        BrowserState {
            tabs: self.tabs.iter().map(Tab::state).collect(),
            active: self.active,
            pending: self.pending,
            downloads: self.downloads.count(),
            notice: self.notice,
        }
    }

    fn layout(&self) {
        let (Some(top), Ok(size)) = (self.top, self.window.inner_size()) else {
            return;
        };
        let bottom = size.height as i32;
        let top = top.clamp(0, bottom);
        let visible = self.tabs.iter().any(|tab| {
            if self.active != Some(tab.id) {
                return false;
            }
            let source = tab.source();
            !source.is_empty() && source != BLANK && top < bottom
        });
        let width = size.width as i32;
        let height = bottom - top;
        let bounds = RECT {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
        };
        unsafe {
            SetWindowPos(
                self.host.0.cast(),
                HWND_TOP,
                0,
                top,
                width,
                height,
                SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            );
            ShowWindow(
                self.host.0.cast(),
                if visible { SW_SHOWNA } else { SW_HIDE },
            );
        }
        for tab in &self.tabs {
            unsafe {
                let _ = tab.controller.SetBounds(bounds);
                let _ = tab.controller.NotifyParentWindowPositionChanged();
                let _ = tab
                    .controller
                    .SetIsVisible(self.active == Some(tab.id) && visible);
            }
        }
    }

    fn take_tab(&mut self, id: u32) -> Option<Tab> {
        let index = self.tabs.iter().position(|tab| tab.id == id)?;
        let tab = self.tabs.remove(index);
        if self.active == Some(id) {
            self.active = self
                .tabs
                .get(index.min(self.tabs.len().saturating_sub(1)))
                .map(|tab| tab.id);
        }
        Some(tab)
    }
}

pub(super) async fn toolbar(app: AppHandle, action: Action) -> Result<BrowserState, String> {
    if let Action::CloseTab { id } = action {
        return close_tab(app, id).await;
    }
    dispatch(app, move |_| {
        if matches!(action, Action::OpenTab) {
            add_tab(None, None)?;
        }
        let operation = SESSION.with_borrow_mut(|current| {
            let session = current.as_mut().ok_or(UNAVAILABLE)?;
            match action {
                Action::Bounds { top } => {
                    session.top = Some(top);
                    session.layout();
                }
                Action::Select { id } => {
                    if session.tabs.iter().any(|tab| tab.id == id) {
                        session.active = Some(id);
                        session.layout();
                    }
                }
                Action::Read | Action::OpenTab => {}
                _ => {
                    let tab = session
                        .tabs
                        .iter()
                        .find(|tab| Some(tab.id) == session.active)
                        .ok_or(UNAVAILABLE)?;
                    session.notice = None;
                    return Ok(Some((tab.view.clone(), tab.controller.clone(), action)));
                }
            }
            Ok::<_, String>(None)
        })?;
        if let Some((view, controller, action)) = operation {
            unsafe {
                match action {
                    Action::Navigate { address: value } => {
                        let input = value.trim();
                        let url = if input.contains("://") {
                            address(input)
                        } else {
                            address(&format!("https://{input}"))
                        }
                        .ok_or("Enter a valid HTTP or HTTPS address.")?;
                        view.Navigate(PCWSTR(wide(Path::new(url.as_str())).as_ptr()))
                    }
                    Action::Back => view.GoBack(),
                    Action::Forward => view.GoForward(),
                    Action::Reload => view.Reload(),
                    Action::FocusPage { backwards } => controller.MoveFocus(if backwards {
                        COREWEBVIEW2_MOVE_FOCUS_REASON_PREVIOUS
                    } else {
                        COREWEBVIEW2_MOVE_FOCUS_REASON_NEXT
                    }),
                    _ => Ok(()),
                }
                .map_err(|_| UNAVAILABLE)?;
            }
        }
        snapshot()
    })
    .await
}

fn snapshot() -> Result<BrowserState, String> {
    SESSION.with_borrow(|current| {
        current
            .as_ref()
            .map(Session::state)
            .ok_or_else(|| UNAVAILABLE.into())
    })
}

async fn close_tab(app: AppHandle, id: u32) -> Result<BrowserState, String> {
    let last = dispatch(app.clone(), move |_| {
        SESSION.with_borrow(|current| {
            let session = current.as_ref().ok_or(UNAVAILABLE)?;
            Ok(session.tabs.len() == 1 && session.tabs[0].id == id)
        })
    })
    .await?;
    if last
        && !confirm(
            app.clone(),
            "Close this tab?",
            "Unsaved changes on this page may be lost.",
            "Close Tab",
        )
        .await?
    {
        return dispatch(app, |_| snapshot()).await;
    }
    dispatch(app, move |_| {
        let replace = SESSION.with_borrow(|current| {
            current
                .as_ref()
                .is_some_and(|session| session.tabs.len() == 1 && session.tabs[0].id == id)
        });
        if replace {
            if last {
                add_tab(None, Some(id))?;
            }
        } else {
            let tab = SESSION.with_borrow_mut(|current| {
                current.as_mut().and_then(|session| session.take_tab(id))
            });
            if let Some(tab) = tab {
                tab.close();
            }
            SESSION.with_borrow(|current| {
                if let Some(session) = current.as_ref() {
                    session.layout();
                }
            });
        }
        snapshot()
    })
    .await
}

async fn confirm(
    app: AppHandle,
    title: &'static str,
    message: &'static str,
    proceed: &'static str,
) -> Result<bool, String> {
    let (send, receive) = oneshot::channel();
    dispatch(app, move |app| {
        let window = app.get_webview_window(LABEL).ok_or(UNAVAILABLE)?;
        app.dialog()
            .message(message)
            .title(title)
            .parent(&window)
            .buttons(MessageDialogButtons::OkCancelCustom(
                proceed.into(),
                "Keep Browser Open".into(),
            ))
            .show(move |choice| {
                let _ = send.send(choice);
            });
        Ok(())
    })
    .await?;
    receive.await.map_err(|_| UNAVAILABLE.into())
}

pub(super) async fn prepare_close(app: AppHandle) -> Result<bool, String> {
    let needed = dispatch(app.clone(), |_| {
        SESSION.with_borrow_mut(|current| {
            let Some(session) = current.as_mut() else {
                return Ok(0);
            };
            if session.closing || session.pending {
                return Ok(1);
            }
            let downloads = session.downloads.count() != 0;
            let website = session.tabs.iter().any(|tab| {
                let address = tab.state().address;
                !address.is_empty() && address != BLANK
            });
            session.closing = true;
            Ok(if downloads {
                3
            } else if website {
                2
            } else {
                0
            })
        })
    })
    .await?;
    if needed < 2 {
        return Ok(needed == 0);
    }
    let result = if needed == 3 {
        confirm(app.clone(), "Close the private browser?", "Unfinished downloads will be canceled and browsing data will be cleared. Unsaved changes on websites may be lost.", "Cancel Downloads and Close").await
    } else {
        confirm(
            app.clone(),
            "Close the private browser?",
            "Browsing data will be cleared. Unsaved changes on websites may be lost.",
            "Close Browser",
        )
        .await
    };
    if result != Ok(true) {
        cancel_close(app).await?;
    }
    result
}

// Prevent interaction while the main window resolves its own save check.
pub(super) async fn prepare_app_close(app: AppHandle) -> Result<bool, String> {
    if !prepare_close(app.clone()).await? {
        return Ok(false);
    }
    dispatch(app, |app| {
        if let Some(browser) = app.get_webview_window(LABEL) {
            let _ = browser.set_enabled(false);
        }
        if let Some(main) = app.get_webview_window("main") {
            let _ = main.set_focus();
        }
        Ok(true)
    })
    .await
}

pub(super) async fn cancel_close(app: AppHandle) -> Result<(), String> {
    dispatch(app, |_| {
        SESSION.with_borrow_mut(|current| {
            if let Some(session) = current.as_mut() {
                session.closing = false;
                let _ = session.window.set_enabled(true);
            }
        });
        Ok(())
    })
    .await
}

pub(super) async fn finish_close(app: AppHandle) -> Result<(), String> {
    let folder = dispatch(app, |_| {
        if SESSION.with_borrow(|current| current.as_ref().is_some_and(|session| !session.closing)) {
            return Err(UNAVAILABLE.into());
        }
        let Some(mut session) = SESSION.with_borrow_mut(Option::take) else {
            return Ok(None);
        };
        session.downloads.cancel();
        let had_tabs = !session.tabs.is_empty();
        let id = session.folder.id;
        let path = session.folder.path.clone();
        if let Some(environment) = session.environment.take().filter(|_| had_tabs) {
            RETIRED.with_borrow_mut(|retired| retired.push((id, environment, session.folder)));
        } else {
            let folder = session.folder;
            std::thread::spawn(move || folder.remove());
        }
        for tab in session.tabs {
            tab.close();
        }
        session.window.destroy().map_err(|_| UNAVAILABLE)?;
        Ok(Some(path))
    })
    .await?;

    // Let normal app exit wait briefly for close-time cleanup; startup recovery handles locked files.
    if let Some(path) = folder {
        for _ in 0..60 {
            if !path.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    Ok(())
}
