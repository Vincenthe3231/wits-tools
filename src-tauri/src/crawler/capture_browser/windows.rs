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
    network,
};

/// Create a private browsing window and install the observer before loading the page.
pub(in crate::crawler) async fn open(
    app: AppHandle,
    browser: CaptureBrowser,
    url: &str,
    proxy: Option<String>,
) -> Result<()> {
    let url = http_url(url)?;
    let endpoint = network::proxy(proxy.as_deref())?;
    if let Some(window) = app.get_webview_window(LABEL) {
        // WebView2 cannot change the proxy of an existing browser environment.
        if browser.0.lock().map_err(|_| Error::Busy)?.route != proxy {
            return Err(Error::CaptureRouteChanged);
        }
        // Reuse the private session for another page, including cookies set during sign-in.
        if window.url().ok().as_ref() != Some(&url)
            || browser.0.lock().map_err(|_| Error::Busy)?.status == "failed"
        {
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
    let relay = if let Some(endpoint) = &endpoint {
        crate::crawler::proxy::DnsRelay::start(endpoint).await?
    } else {
        None
    };
    let window_route = relay.as_ref().map(|relay| &relay.url).or(endpoint.as_ref());
    // WebView2 fixes its routing at environment creation. Give configured routes a private profile.
    let profile = window_route
        .map(|_| tempfile::tempdir().map_err(|_| Error::File))
        .transpose()?;
    let mut builder = WebviewWindowBuilder::new(
        &app,
        LABEL,
        WebviewUrl::External("about:blank".parse().unwrap()),
    )
    .title("WITS Tools Capture Browser")
    .inner_size(1000.0, 720.0)
    .incognito(true)
    .visible(false)
    .on_navigation(|url| matches!(url.scheme(), "http" | "https") || url.as_str() == "about:blank")
    .on_new_window(|_, _| NewWindowResponse::Deny);
    if let (Some(route), Some(profile)) = (window_route, &profile) {
        if !matches!(route.scheme(), "http" | "socks5")
            || !route.username().is_empty()
            || route.password().is_some()
        {
            return Err(Error::InvalidProxy);
        }
        builder = builder
            .proxy_url(route.clone())
            .data_directory(profile.path().into());
    }
    let window = builder.build().map_err(|_| {
        trace.record(format_args!("window creation failed"));
        Error::CaptureUnavailable
    })?;

    if let Ok(mut session) = browser.0.lock() {
        session.status = "loading";
        session.route = proxy.clone();
    }

    // Destroying the window releases the native observer and any unselected credentials.
    let capture = browser.clone();
    window.on_window_event(move |event| {
        // The observer window owns the relay and profile until its native handle is destroyed.
        let _routing = (&relay, &profile);
        if matches!(event, WindowEvent::Destroyed)
            && let Ok(mut sources) = capture.0.lock()
        {
            trace.record(format_args!(
                "window closed captured_sources={}",
                sources.sources.len()
            ));
            *sources = Default::default();
        }
    });
    let (ready, attached) = tokio::sync::oneshot::channel();
    let setup = window.with_webview(move |webview| {
        // Tauri schedules this closure on the thread owning the WebView2 controller.
        let result = unsafe {
            webview.controller().CoreWebView2().and_then(|view| {
                let starting_browser = browser.clone();
                let starting = NavigationStartingEventHandler::create(Box::new(move |_, args| {
                    if let Some(args) = args {
                        let mut uri = PWSTR::null();
                        let mut id = 0;
                        args.Uri(&mut uri)?;
                        let uri = take_pwstr(uri);
                        args.NavigationId(&mut id)?;
                        if let Ok(mut session) = starting_browser.0.lock() {
                            session.navigation = Some(id);
                            session.status = "loading";
                        }
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
                let completed_browser = browser.clone();
                let completed =
                    NavigationCompletedEventHandler::create(Box::new(move |_, args| {
                        if let Some(args) = args {
                            log_navigation(trace, &completed_browser, &args)?;
                        }
                        Ok(())
                    }));
                view.add_NavigationCompleted(&completed, &mut 0)?;
                let view: ICoreWebView2_2 = view.cast()?;
                let observer =
                    WebResourceResponseReceivedEventHandler::create(Box::new(move |view, args| {
                        if let Some(args) = args {
                            match candidate(&args, trace, proxy.as_deref()) {
                                Ok(Some((url, mut candidate))) => {
                                    candidate.page_name = view.as_ref().and_then(page_name);
                                    browser.observe(url, candidate);
                                }
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
    browser: &CaptureBrowser,
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
        // Ignore completion from a navigation superseded by a newer request.
        if let Ok(mut session) = browser.0.lock()
            && session.navigation == Some(id)
        {
            session.status = if success.as_bool() && http_status.is_none_or(|status| status < 400) {
                "ready"
            } else {
                "failed"
            };
        }
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

/// Prefer the page title, with the last nonempty URL path component as fallback.
fn page_name(view: &ICoreWebView2) -> Option<String> {
    unsafe {
        let mut title = PWSTR::null();
        let _ = view.DocumentTitle(&mut title);
        let title = take_pwstr(title);
        if !title.trim().is_empty() && http_url(&title).is_err() {
            return Some(title.trim().chars().take(120).collect());
        }
        let mut uri = PWSTR::null();
        let _ = view.Source(&mut uri);
        let url = http_url(&take_pwstr(uri)).ok()?;
        url.path_segments()?
            .rfind(|part| !part.is_empty())
            .or_else(|| url.host_str())
            .map(|name| name.chars().take(120).collect())
    }
}

/// Filter by status and media type before copying the small replay-header allowlist.
fn candidate(
    args: &ICoreWebView2WebResourceResponseReceivedEventArgs,
    trace: Trace,
    proxy: Option<&str>,
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
        Ok(
            Candidate::new(&uri, &mime, headers, proxy.map(str::to_owned))
                .ok()
                .map(|candidate| (uri, candidate)),
        )
    }
}
