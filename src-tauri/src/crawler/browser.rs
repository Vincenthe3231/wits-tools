//! Supervises the pinned Obscura executable and collects one page's CDP output.

use std::{collections::HashMap, path::Path, process::Stdio, time::Duration};

use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::{
    net::TcpStream,
    process::{Child, Command},
    time,
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};
use url::Url;
use uuid::Uuid;

use super::diagnostics::{Trace, browser_error, origin};
use super::{Error, Result};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Keep request credentials behind the native boundary until a source is selected.
#[derive(Clone, Default)]
pub(super) struct Resource {
    pub url: String,
    pub mime: String,
    pub headers: Vec<(String, String)>,
    pub status: u16,
}

pub(super) struct Page {
    pub html: String,
    pub text: String,
    pub title: String,
    pub url: String,
    pub links: Vec<String>,
    pub resources: Vec<Resource>,
    pub status: u16,
}

pub(super) struct Browser {
    socket: Socket,
    child: Child,
    #[cfg(windows)]
    _job: std::os::windows::io::OwnedHandle,
    _runtime: tempfile::TempDir,
    next_id: u64,
    responses: HashMap<String, Resource>,
    trace: Trace,
    _dns_relay: Option<super::proxy::DnsRelay>,
}

/// Obscura publishes a structured readiness file once its authenticated listener is available.
#[derive(Deserialize)]
struct Ready {
    host: std::net::IpAddr,
    port: u16,
    websocket_path: String,
}

impl Browser {
    /// Bind a private, authenticated loopback endpoint and wait for its actual address.
    pub async fn start(
        executable: &Path,
        proxy: Option<&Url>,
        robots: bool,
        local: bool,
        trace: Trace,
    ) -> Result<Self> {
        // Record startup checkpoints without exposing the authenticated CDP endpoint.
        trace.record(format_args!(
            "starting executable={executable:?} proxy={} allow_local={local}",
            proxy.is_some()
        ));
        if !executable.is_file() {
            trace.record(format_args!("executable missing"));
            return Err(Error::BrowserUnavailable);
        }
        // Obscura accepts a forward proxy, so adapt explicit DNS selection through loopback.
        let dns_relay = if let Some(endpoint) = proxy {
            super::proxy::DnsRelay::start(endpoint).await?
        } else {
            None
        };
        let proxy = dns_relay.as_ref().map(|relay| &relay.url).or(proxy);
        let runtime = tempfile::tempdir().map_err(|_| Error::File)?;
        let ready = runtime.path().join("ready");
        let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let mut command = Command::new(executable);
        command
            .args([
                "serve",
                "--host",
                "127.0.0.1",
                "--port",
                "0",
                "--max-connections",
                "1",
                "--ready-file",
            ])
            .arg(&ready)
            .env("OBSCURA_CDP_TOKEN", &token)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        if robots {
            command.arg("--obey-robots");
        }
        if local {
            command.arg("--allow-private-network");
        }
        if let Some(proxy) = proxy {
            command.env("OBSCURA_PROXY", proxy.as_str());
        } else {
            command.env_remove("OBSCURA_PROXY");
        }
        #[cfg(windows)]
        command.creation_flags(0x08000000);

        let mut child = command.spawn().map_err(|error| {
            trace.record(format_args!(
                "spawn failed kind={:?} os_code={:?}",
                error.kind(),
                error.raw_os_error()
            ));
            Error::BrowserUnavailable
        })?;
        trace.record(format_args!("process spawned pid={:?}", child.id()));
        #[cfg(windows)]
        let job = windows_job(&child).inspect_err(|_| {
            trace.record(format_args!("Windows job setup failed"));
        })?;
        let ready = time::timeout(Duration::from_secs(8), async {
            loop {
                if let Ok(text) = tokio::fs::read_to_string(&ready).await {
                    return serde_json::from_str::<Ready>(&text).map_err(|error| {
                        trace.record(format_args!(
                            "invalid readiness file category={:?}",
                            error.classify()
                        ));
                        Error::BrowserFailed
                    });
                }
                if let Some(status) = child.try_wait().map_err(|_| Error::BrowserFailed)? {
                    trace.record(format_args!(
                        "process exited before readiness status={status}"
                    ));
                    return Err(Error::BrowserFailed);
                }
                time::sleep(Duration::from_millis(30)).await;
            }
        })
        .await
        .map_err(|_| {
            trace.record(format_args!("readiness timed out after 8s"));
            Error::BrowserFailed
        })??;
        if !ready.host.is_loopback()
            || ready.port == 0
            || ready.websocket_path != "/devtools/browser"
        {
            trace.record(format_args!(
                "readiness endpoint did not match the expected loopback route"
            ));
            return Err(Error::BrowserFailed);
        }
        trace.record(format_args!("loopback listener ready"));
        let address = std::net::SocketAddr::new(ready.host, ready.port);
        let mut request = format!("ws://{address}/devtools/browser")
            .into_client_request()
            .map_err(|_| Error::BrowserFailed)?;
        request.headers_mut().insert(
            "authorization",
            format!("Bearer {token}")
                .parse()
                .map_err(|_| Error::BrowserFailed)?,
        );
        let (socket, _) = time::timeout(Duration::from_secs(5), connect_async(request))
            .await
            .map_err(|_| {
                trace.record(format_args!("WebSocket handshake timed out after 5s"));
                Error::BrowserFailed
            })?
            .map_err(|error| {
                trace.record(format_args!(
                    "WebSocket handshake failed {}",
                    socket_error(&error)
                ));
                Error::BrowserFailed
            })?;
        trace.record(format_args!("CDP WebSocket connected"));
        Ok(Self {
            socket,
            child,
            #[cfg(windows)]
            _job: job,
            _runtime: runtime,
            next_id: 0,
            responses: HashMap::new(),
            trace,
            _dns_relay: dns_relay,
        })
    }

    /// Open an isolated target, observe requests before navigation, then read the rendered DOM.
    pub async fn page(&mut self, url: &Url) -> Result<Page> {
        let target = self.open_target().await?;
        let result = self.collect(url, &target).await;
        let close = self
            .call("Target.closeTarget", json!({"targetId":target}), None)
            .await;
        result.and_then(|page| close.map(|_| page))
    }

    /// Keep media discovery independent of DOM snapshots and let its private browser own cleanup.
    pub async fn media(&mut self, url: &Url) -> Result<Vec<Resource>> {
        let target = self.open_target().await?;
        let session = self.navigate(url, &target).await?;
        self.collect_media(&session, url).await
    }

    // Reset observations before creating a target so earlier pages cannot supply media candidates.
    async fn open_target(&mut self) -> Result<String> {
        self.responses.clear();
        let target = self
            .call("Target.createTarget", json!({"url":"about:blank"}), None)
            .await?;
        target["targetId"]
            .as_str()
            .map(str::to_owned)
            .ok_or(Error::BrowserFailed)
    }

    // Enable network observations before navigation so early requests are captured.
    async fn navigate(&mut self, url: &Url, target: &str) -> Result<String> {
        let attached = self
            .call(
                "Target.attachToTarget",
                json!({"targetId":target,"flatten":true}),
                None,
            )
            .await?;
        let session = attached["sessionId"].as_str().ok_or(Error::BrowserFailed)?;
        self.call("Network.enable", json!({}), Some(session))
            .await?;
        self.call("Page.enable", json!({}), Some(session)).await?;
        let navigation = self
            .call("Page.navigate", json!({"url":url.as_str()}), Some(session))
            .await?;
        if let Some(error) = navigation["errorText"]
            .as_str()
            .filter(|error| !error.is_empty())
        {
            self.trace.record(format_args!(
                "navigation failed reason={:?}",
                browser_error(error)
            ));
            return Err(Error::BrowserNavigationFailed);
        }
        Ok(session.to_owned())
    }

    // Crawl only needs a short settle period before reading the rendered document.
    async fn collect(&mut self, url: &Url, target: &str) -> Result<Page> {
        let session = self.navigate(url, target).await?;
        let deadline = time::Instant::now() + Duration::from_millis(400);
        while let Ok(message) = time::timeout_at(deadline, self.next()).await {
            self.observe(&message?);
        }
        // A no-render browser can leave visual assets unfetched. Keep the DOM references too.
        let expression = r#"JSON.stringify({
            html: document.documentElement?.outerHTML || "",
            text: document.body?.innerText || document.body?.textContent || "",
            title: document.title,
            url: location.href,
            links: Array.from(document.querySelectorAll("a[href]"), element => element.href),
            assets: [
                ...Array.from(document.querySelectorAll("img[src],script[src],video[src],audio[src],source[src],track[src],embed[src]"),
                    element => element.currentSrc || element.getAttribute("src")),
                ...Array.from(document.querySelectorAll("link[rel=stylesheet],link[rel=icon],link[rel=preload]"),
                    element => element.getAttribute("href")),
                ...Array.from(document.querySelectorAll("video[poster]"), element => element.getAttribute("poster"))
            ].filter(Boolean).map(value => {
                try { return new URL(value, document.baseURI).href; }
                catch { return ""; }
            })
        })"#;
        let result = self
            .call(
                "Runtime.evaluate",
                json!({"expression":expression, "returnByValue":true}),
                Some(&session),
            )
            .await?;
        let value: Value = serde_json::from_str(
            result["result"]["value"]
                .as_str()
                .ok_or(Error::BrowserFailed)?,
        )
        .map_err(|_| Error::BrowserFailed)?;
        let text = |key: &str| value[key].as_str().unwrap_or_default().to_owned();
        let final_url = text("url");
        let mut resources = self
            .responses
            .drain()
            .map(|(_, resource)| resource)
            .filter(|resource| !resource.url.is_empty())
            .collect::<Vec<_>>();
        let mut seen = resources
            .iter()
            .map(|resource| resource.url.clone())
            .collect::<std::collections::HashSet<_>>();
        resources.extend(
            value["assets"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .filter(|url| !url.is_empty() && seen.insert((*url).to_owned()))
                .map(|url| Resource {
                    url: url.to_owned(),
                    ..Resource::default()
                }),
        );
        let status = resources
            .iter()
            .find(|entry| entry.url == final_url && entry.status != 0)
            .map_or(200, |entry| entry.status);
        let links = value["links"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|link| link.as_str().map(str::to_owned))
            .collect();
        Ok(Page {
            html: text("html"),
            text: text("text"),
            title: text("title"),
            url: final_url,
            links,
            resources,
            status,
        })
    }

    /// Give script-driven players time to request a stream without waiting for native playback.
    async fn collect_media(&mut self, session: &str, page_url: &Url) -> Result<Vec<Resource>> {
        // Visit accessible player documents and try each media element only once. Calls to play()
        // may reject or remain pending in a no-render browser, so never await them.
        let probe = r#"(() => {
            const attempted = globalThis.__witsMediaAttempted ||= new WeakSet();
            const urls = [];
            const visit = (doc, depth) => {
                if (!doc || depth > 4) return;
                for (const player of doc.querySelectorAll("video,audio")) {
                    if (!attempted.has(player)) {
                        attempted.add(player);
                        try {
                            player.muted = true;
                            player.play()?.catch(() => {});
                        } catch {}
                    }
                    for (const src of [player.currentSrc, player.getAttribute("src"),
                        ...Array.from(player.querySelectorAll("source[src]"),
                            source => source.getAttribute("src"))]) {
                        if (src) {
                            try { urls.push(new URL(src, doc.baseURI).href); } catch {}
                        }
                    }
                }
                for (const frame of doc.querySelectorAll("iframe")) {
                    try { visit(frame.contentDocument, depth + 1); } catch {}
                }
            };
            visit(document, 0);
            return urls;
        })()"#;
        let deadline = time::Instant::now() + Duration::from_secs(10);
        let mut urls = std::collections::HashSet::new();
        self.trace
            .record(format_args!("capture window started duration=10s"));
        // Probe during the capture window rather than requiring a successful DOM snapshot.
        while time::Instant::now() < deadline {
            let probe_deadline = (time::Instant::now() + Duration::from_millis(750)).min(deadline);
            match time::timeout_at(
                probe_deadline,
                self.call(
                    "Runtime.evaluate",
                    json!({"expression":probe,"returnByValue":true}),
                    Some(session),
                ),
            )
            .await
            {
                Ok(Ok(result)) => {
                    let hints = result["result"]["value"].as_array();
                    self.trace.record(format_args!(
                        "playback probe hints={} exception={}",
                        hints.map_or(0, Vec::len),
                        result.get("exceptionDetails").is_some()
                    ));
                    urls.extend(
                        hints
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_str)
                            .map(str::to_owned),
                    );
                }
                Ok(Err(_)) => self.trace.record(format_args!("playback probe failed")),
                Err(_) => self
                    .trace
                    .record(format_args!("playback probe exceeded its time budget")),
            }
            while let Ok(message) = time::timeout_at(probe_deadline, self.next()).await {
                self.observe(&message?);
            }
        }
        // Obscura can omit cookies from CDP request headers even when its transport sent them.
        // Read only this isolated session, before it shuts down, and preserve wire headers first.
        let cookies = time::timeout(
            Duration::from_secs(1),
            self.call("Network.getCookies", json!({}), Some(session)),
        )
        .await
        .ok()
        .and_then(std::result::Result::ok);
        let mut resources = self
            .responses
            .drain()
            .map(|(_, resource)| resource)
            .filter(|resource| !resource.url.is_empty())
            .collect::<Vec<_>>();
        if let Some(cookies) = cookies.and_then(|value| value["cookies"].as_array().cloned()) {
            for resource in &mut resources {
                if (200..300).contains(&resource.status)
                    && !resource
                        .headers
                        .iter()
                        .any(|(name, _)| name.eq_ignore_ascii_case("cookie"))
                    && let Ok(url) = Url::parse(&resource.url)
                    && url.origin() == page_url.origin()
                    && super::media::classify(&url, &resource.mime).is_some()
                {
                    let cookie = root_cookies(&cookies, &url);
                    if !cookie.is_empty() {
                        resource.headers.push(("cookie".into(), cookie));
                        self.trace
                            .record(format_args!("restored same-origin media cookie context"));
                    }
                }
            }
        }
        // CDP observations carry the request context needed for downloading; use DOM URLs as hints.
        self.trace.record(format_args!(
            "capture completed observed={} dom_hints={}",
            resources.len(),
            urls.len()
        ));
        urls.retain(|url| !resources.iter().any(|resource| &resource.url == url));
        resources.extend(urls.into_iter().map(|url| Resource {
            url,
            ..Resource::default()
        }));
        Ok(resources)
    }

    /// Collect events during each command and reject responses from other request IDs.
    async fn call(&mut self, method: &str, params: Value, session: Option<&str>) -> Result<Value> {
        self.next_id += 1;
        let id = self.next_id;
        let mut request = json!({"id":id,"method":method,"params":params});
        if let Some(session) = session {
            request["sessionId"] = json!(session);
        }
        self.trace
            .record(format_args!("CDP send id={id} method={method}"));
        time::timeout(Duration::from_secs(45), async {
            self.socket
                .send(Message::Text(request.to_string().into()))
                .await
                .map_err(|error| {
                    self.trace.record(format_args!(
                        "CDP send failed id={id} method={method} {}",
                        socket_error(&error)
                    ));
                    Error::BrowserFailed
                })?;
            loop {
                let message = self.next().await?;
                if message["id"].as_u64() == Some(id) {
                    return if let Some(error) = message.get("error") {
                        let code = error["code"].as_i64();
                        let reason = error["message"].as_str().map(browser_error);
                        self.trace.record(format_args!(
                            "CDP rejected id={id} method={method} code={code:?} reason={reason:?}"
                        ));
                        Err(Error::BrowserFailed)
                    } else {
                        self.trace
                            .record(format_args!("CDP ok id={id} method={method}"));
                        Ok(message["result"].clone())
                    };
                }
                self.observe(&message);
            }
        })
        .await
        .map_err(|_| {
            self.trace
                .record(format_args!("CDP timed out id={id} method={method}"));
            Error::BrowserFailed
        })?
    }

    async fn next(&mut self) -> Result<Value> {
        while let Some(message) = self.socket.next().await {
            match message.map_err(|error| {
                self.trace
                    .record(format_args!("CDP read failed {}", socket_error(&error)));
                Error::BrowserFailed
            })? {
                Message::Text(text) => {
                    return serde_json::from_str(&text).map_err(|error| {
                        self.trace.record(format_args!(
                            "invalid CDP JSON category={:?}",
                            error.classify()
                        ));
                        Error::BrowserFailed
                    });
                }
                Message::Close(frame) => {
                    self.trace.record(format_args!(
                        "CDP close frame code={:?}",
                        frame.map(|frame| frame.code)
                    ));
                    break;
                }
                _ => {}
            }
        }
        self.trace.record(format_args!(
            "CDP connection ended process_status={:?}",
            self.child.try_wait().ok().flatten()
        ));
        Err(Error::BrowserFailed)
    }

    /// Correlate requests and responses so headers stay with the resource that supplied them.
    fn observe(&mut self, event: &Value) {
        let params = &event["params"];
        let Some(id) = params["requestId"].as_str() else {
            return;
        };
        if self.responses.len() >= 8_192 && !self.responses.contains_key(id) {
            return;
        }
        match event["method"].as_str() {
            Some("Network.requestWillBeSent") => {
                let request = &params["request"];
                let resource = self.responses.entry(id.into()).or_default();
                let url = request["url"].as_str().unwrap_or_default();
                // Redirects can reuse request IDs. Discard credentials from the preceding hop.
                if !resource.url.is_empty() && resource.url != url {
                    *resource = Resource::default();
                }
                resource.url = url.into();
                resource.headers.extend(headers(&request["headers"]));
                self.trace.record(format_args!(
                    "request id={id:?} origin={}",
                    origin(&resource.url)
                ));
            }
            Some("Network.requestWillBeSentExtraInfo") => {
                self.responses
                    .entry(id.into())
                    .or_default()
                    .headers
                    .extend(headers(&params["headers"]));
            }
            Some("Network.responseReceived") => {
                let response = &params["response"];
                let resource = self.responses.entry(id.into()).or_default();
                resource.url = response["url"].as_str().unwrap_or_default().into();
                resource.mime = response["mimeType"].as_str().unwrap_or_default().into();
                resource.status = response["status"].as_u64().unwrap_or_default() as u16;
                let mime = resource.mime.split(';').next().unwrap_or_default();
                self.trace.record(format_args!(
                    "response id={id:?} status={} mime={mime:?} origin={}",
                    resource.status,
                    origin(&resource.url)
                ));
            }
            Some("Network.loadingFailed") => {
                let reason = params["errorText"].as_str().map(browser_error);
                self.trace.record(format_args!(
                    "request failed id={id:?} reason={reason:?} cancelled={}",
                    params["canceled"] == true
                ));
            }
            _ => {}
        }
    }

    /// Reap the child promptly when Tauri or a CLI command finishes normally.
    pub async fn shutdown(mut self) {
        let _ = self.child.kill().await;
        self.trace.record(format_args!("process stopped"));
    }
}

/// Replay only cookies safe for same-origin manifest children. Path-restricted cookies
/// require the original request header, and cookies for other hosts stay in Obscura.
fn root_cookies(cookies: &[Value], url: &Url) -> String {
    cookies
        .iter()
        .filter_map(|cookie| {
            let domain = cookie["domain"].as_str()?.trim_start_matches('.');
            if !url.host_str()?.eq_ignore_ascii_case(domain)
                || cookie["path"] != "/"
                || (cookie["secure"] == true && url.scheme() != "https")
            {
                return None;
            }
            let name = cookie["name"].as_str()?;
            let value = cookie["value"].as_str()?;
            if reqwest::header::HeaderName::from_bytes(name.as_bytes()).is_err()
                || value.contains(';')
                || reqwest::header::HeaderValue::from_str(value).is_err()
            {
                return None;
            }
            Some(format!("{name}={value}"))
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Extract transport metadata without printing handshake URLs, headers, or response bodies.
fn socket_error(error: &tokio_tungstenite::tungstenite::Error) -> String {
    use tokio_tungstenite::tungstenite::Error;
    match error {
        Error::Http(response) => format!("http_status={}", response.status()),
        Error::Io(error) => format!("kind={:?} os_code={:?}", error.kind(), error.raw_os_error()),
        Error::Protocol(error) => format!("protocol={:?}", std::mem::discriminant(error)),
        error => format!("transport={:?}", std::mem::discriminant(error)),
    }
}

/// Windows should reap Obscura even when the desktop or CLI process exits abruptly.
#[cfg(windows)]
fn windows_job(child: &Child) -> Result<std::os::windows::io::OwnedHandle> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };

    let child = child.raw_handle().ok_or(Error::BrowserUnavailable)?;
    // The owned job handle remains live until the browser session ends.
    unsafe {
        let raw = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if raw.is_null() {
            return Err(Error::BrowserUnavailable);
        }
        let job = OwnedHandle::from_raw_handle(raw);
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let configured = SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        if configured == 0 || AssignProcessToJobObject(job.as_raw_handle(), child) == 0 {
            return Err(Error::BrowserUnavailable);
        }
        Ok(job)
    }
}

fn headers(value: &Value) -> impl Iterator<Item = (String, String)> + '_ {
    value
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(key, value)| value.as_str().map(|value| (key.clone(), value.to_owned())))
}
