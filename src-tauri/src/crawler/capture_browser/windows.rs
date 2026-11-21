//! Observes successful GET responses in the dedicated WebView2 browser.

use tauri::{
    AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent, webview::NewWindowResponse,
};
use webview2_com::{
    Microsoft::Web::WebView2::Win32::*, NavigationCompletedEventHandler,
    NavigationStartingEventHandler, WebResourceResponseReceivedEventHandler, take_pwstr,
};
use windows_core::{Interface, PCWSTR, PWSTR};

use super::{CaptureBrowser, Error, LABEL, Result};
use crate::crawler::{
    diagnostics::{Trace, origin},
    http_url,
    media::{Candidate, classify},
};

/// Create a private browsing window and install the observer before loading the page.
pub(in crate::crawler) async fn open(
    app: AppHandle,
    browser: CaptureBrowser,
    url: &str,
) -> Result<()> {
    let url = http_url(url)?;
    if let Some(window) = app.get_webview_window(LABEL) {
        // Reuse the private session for another page, including cookies set during sign-in.
        if window.url().ok().as_ref() != Some(&url) {
            window
                .navigate(url)
                .map_err(|_| Error::CaptureUnavailable)?;
        }
        return window.set_focus().map_err(|_| Error::CaptureUnavailable);
    }
    // Use a single trace across window setup, navigations, media responses, and closure.
    let trace = Trace::for_component("capture-browser", true);
    trace.record(format_args!(
        "opening private window origin={}",
        origin(url.as_str())
    ));
    let window = WebviewWindowBuilder::new(
        &app,
        LABEL,
        WebviewUrl::External("about:blank".parse().unwrap()),
    )
    .title("WITS Tools Capture Browser")
    .inner_size(1000.0, 720.0)
    .incognito(true)
    .visible(false)
    .on_navigation(|url| matches!(url.scheme(), "http" | "https") || url.as_str() == "about:blank")
    .on_new_window(|_, _| NewWindowResponse::Deny)
    .build()
    .map_err(|_| {
        trace.record(format_args!("window creation failed"));
        Error::CaptureUnavailable
    })?;

    // Destroying the window releases the native observer and any unselected credentials.
    let capture = browser.clone();
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed)
            && let Ok(mut sources) = capture.0.lock()
        {
            trace.record(format_args!(
                "window closed captured_sources={}",
                sources.len()
            ));
            sources.clear();
        }
    });
    let (ready, attached) = tokio::sync::oneshot::channel();
    let setup = window.with_webview(move |webview| {
        // Tauri schedules this closure on the thread owning the WebView2 controller.
        let result = unsafe {
            webview.controller().CoreWebView2().and_then(|view| {
                let starting = NavigationStartingEventHandler::create(Box::new(move |_, args| {
                    if let Some(args) = args {
                        let mut uri = PWSTR::null();
                        let mut id = 0;
                        args.Uri(&mut uri)?;
                        let uri = take_pwstr(uri);
                        args.NavigationId(&mut id)?;
                        let mut redirected = windows_core::BOOL(0);
                        args.IsRedirected(&mut redirected)?;
                        trace.record(format_args!(
                            "navigation started id={id} origin={} redirected={}",
                            origin(&uri),
                            redirected.as_bool()
                        ));
                    }
                    Ok(())
                }));
                view.add_NavigationStarting(&starting, &mut 0)?;
                let completed =
                    NavigationCompletedEventHandler::create(Box::new(move |_, args| {
                        if let Some(args) = args {
                            log_navigation(trace, &args)?;
                        }
                        Ok(())
                    }));
                view.add_NavigationCompleted(&completed, &mut 0)?;
                let view: ICoreWebView2_2 = view.cast()?;
                let observer =
                    WebResourceResponseReceivedEventHandler::create(Box::new(move |_, args| {
                        if let Some(args) = args {
                            match candidate(&args, trace) {
                                Ok(Some((url, candidate))) => browser.observe(url, candidate),
                                Ok(None) => {}
                                Err(error) => trace.record(format_args!(
                                    "response inspection failed hresult={:#010x}",
                                    error.code().0
                                )),
                            }
                        }
                        Ok(())
                    }));
                view.add_WebResourceResponseReceived(&observer, &mut 0)
            })
        };
        if let Err(error) = &result {
            trace.record(format_args!(
                "WebView2 observer setup failed hresult={:#010x}",
                error.code().0
            ));
        }
        let _ = ready.send(result.is_ok());
    });
    if setup.is_err()
        || !attached.await.unwrap_or(false)
        || window.navigate(url).is_err()
        || window.show().is_err()
    {
        trace.record(format_args!("capture window initialization failed"));
        let _ = window.destroy();
        return Err(Error::CaptureUnavailable);
    }
    let _ = window.set_focus();
    Ok(())
}

/// Report navigation outcomes separately from media responses, with no URL or header values.
fn log_navigation(
    trace: Trace,
    args: &ICoreWebView2NavigationCompletedEventArgs,
) -> windows_core::Result<()> {
    // The event object and its newer interface are used only on the WebView2 thread.
    unsafe {
        let mut id = 0;
        let mut success = windows_core::BOOL(0);
        let mut error = COREWEBVIEW2_WEB_ERROR_STATUS_UNKNOWN;
        args.NavigationId(&mut id)?;
        args.IsSuccess(&mut success)?;
        args.WebErrorStatus(&mut error)?;
        let http_status = args
            .cast::<ICoreWebView2NavigationCompletedEventArgs2>()
            .ok()
            .and_then(|args| {
                let mut status = 0;
                args.HttpStatusCode(&mut status)
                    .ok()
                    .filter(|_| status != 0)
                    .map(|_| status)
            });
        let reason = match error {
            COREWEBVIEW2_WEB_ERROR_STATUS_TIMEOUT => "connection timeout",
            COREWEBVIEW2_WEB_ERROR_STATUS_HOST_NAME_NOT_RESOLVED => "DNS lookup failed",
            COREWEBVIEW2_WEB_ERROR_STATUS_CANNOT_CONNECT => "cannot connect",
            COREWEBVIEW2_WEB_ERROR_STATUS_CONNECTION_RESET => "connection reset",
            COREWEBVIEW2_WEB_ERROR_STATUS_DISCONNECTED => "disconnected",
            COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED => "navigation canceled",
            COREWEBVIEW2_WEB_ERROR_STATUS_VALID_AUTHENTICATION_CREDENTIALS_REQUIRED => {
                "authentication required"
            }
            COREWEBVIEW2_WEB_ERROR_STATUS_VALID_PROXY_AUTHENTICATION_REQUIRED => {
                "proxy authentication required"
            }
            _ if success.as_bool() => "completed",
            _ => "WebView2 navigation error",
        };
        trace.record(format_args!("navigation completed id={id} success={} http_status={http_status:?} web_error={} reason={reason:?}", success.as_bool(), error.0));
    }
    Ok(())
}

/// Filter by status and media type before copying the small replay-header allowlist.
fn candidate(
    args: &ICoreWebView2WebResourceResponseReceivedEventArgs,
    trace: Trace,
) -> windows_core::Result<Option<(String, Candidate)>> {
    // All COM methods run inside the WebView callback, and take_pwstr releases returned strings.
    unsafe {
        let request = args.Request()?;
        let response = args.Response()?;
        let mut status = 0;
        response.StatusCode(&mut status)?;
        let mut method = PWSTR::null();
        request.Method(&mut method)?;
        if take_pwstr(method) != "GET" {
            return Ok(None);
        }
        let mut uri = PWSTR::null();
        request.Uri(&mut uri)?;
        let uri = take_pwstr(uri);
        let response_headers = response.Headers()?;
        let mut mime = PWSTR::null();
        let _ = response_headers.GetHeader(windows_core::w!("Content-Type"), &mut mime);
        let mime = take_pwstr(mime);
        let kind = http_url(&uri).ok().and_then(|url| classify(&url, &mime));
        if status >= 400 || kind.is_some() {
            trace.record(format_args!(
                "response origin={} http_status={status} media_kind={kind:?}",
                origin(&uri)
            ));
        }
        if !matches!(status, 200 | 206) || kind.is_none() {
            return Ok(None);
        }
        let request_headers = request.Headers()?;
        let mut headers = Vec::new();
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
            if !value.is_empty() {
                headers.push((name.into(), value));
            }
        }
        // Flag captured credentials without disclosing their values or the media URL.
        trace.record(format_args!(
            "media captured origin={} kind={kind:?} cookie={} authorization={} referer={}",
            origin(&uri),
            headers.iter().any(|(name, _)| name == "cookie"),
            headers.iter().any(|(name, _)| name == "authorization"),
            headers.iter().any(|(name, _)| name == "referer")
        ));
        Ok(Candidate::new(&uri, &mime, headers, None)
            .ok()
            .map(|candidate| (uri, candidate)))
    }
}
