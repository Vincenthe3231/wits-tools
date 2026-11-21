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
    ) -> Result<Self> {
        if !executable.is_file() {
            return Err(Error::BrowserUnavailable);
        }
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

        let mut child = command.spawn().map_err(|_| Error::BrowserUnavailable)?;
        #[cfg(windows)]
        let job = windows_job(&child)?;
        let ready = time::timeout(Duration::from_secs(8), async {
            loop {
                if let Ok(text) = tokio::fs::read_to_string(&ready).await {
                    return serde_json::from_str::<Ready>(&text).map_err(|_| Error::BrowserFailed);
                }
                if child
                    .try_wait()
                    .map_err(|_| Error::BrowserFailed)?
                    .is_some()
                {
                    return Err(Error::BrowserFailed);
                }
                time::sleep(Duration::from_millis(30)).await;
            }
        })
        .await
        .map_err(|_| Error::BrowserFailed)??;
        if !ready.host.is_loopback()
            || ready.port == 0
            || ready.websocket_path != "/devtools/browser"
        {
            return Err(Error::BrowserFailed);
        }
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
            .map_err(|_| Error::BrowserFailed)?
            .map_err(|_| Error::BrowserFailed)?;
        Ok(Self {
            socket,
            child,
            #[cfg(windows)]
            _job: job,
            _runtime: runtime,
            next_id: 0,
            responses: HashMap::new(),
        })
    }

    /// Open an isolated target, observe requests before navigation, then read the rendered DOM.
    pub async fn page(&mut self, url: &Url, discover: bool) -> Result<Page> {
        self.responses.clear();
        let target = self
            .call("Target.createTarget", json!({"url":"about:blank"}), None)
            .await?;
        let target = target["targetId"]
            .as_str()
            .ok_or(Error::BrowserFailed)?
            .to_owned();
        let result = self.collect(url, &target, discover).await;
        let close = self
            .call("Target.closeTarget", json!({"targetId":target}), None)
            .await;
        result.and_then(|page| close.map(|_| page))
    }

    async fn collect(&mut self, url: &Url, target: &str, discover: bool) -> Result<Page> {
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
        if navigation.get("errorText").is_some() {
            return Err(Error::BrowserFailed);
        }
        let settle = if discover {
            Duration::from_secs(4)
        } else {
            Duration::from_millis(400)
        };
        let deadline = time::Instant::now() + settle;
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
                Some(session),
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

    /// Collect events during each command and reject responses from other request IDs.
    async fn call(&mut self, method: &str, params: Value, session: Option<&str>) -> Result<Value> {
        self.next_id += 1;
        let id = self.next_id;
        let mut request = json!({"id":id,"method":method,"params":params});
        if let Some(session) = session {
            request["sessionId"] = json!(session);
        }
        time::timeout(Duration::from_secs(45), async {
            self.socket
                .send(Message::Text(request.to_string().into()))
                .await
                .map_err(|_| Error::BrowserFailed)?;
            loop {
                let message = self.next().await?;
                if message["id"].as_u64() == Some(id) {
                    return if message.get("error").is_some() {
                        Err(Error::BrowserFailed)
                    } else {
                        Ok(message["result"].clone())
                    };
                }
                self.observe(&message);
            }
        })
        .await
        .map_err(|_| Error::BrowserFailed)?
    }

    async fn next(&mut self) -> Result<Value> {
        while let Some(message) = self.socket.next().await {
            match message.map_err(|_| Error::BrowserFailed)? {
                Message::Text(text) => {
                    return serde_json::from_str(&text).map_err(|_| Error::BrowserFailed);
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
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
                resource.url = request["url"].as_str().unwrap_or_default().into();
                resource.headers.extend(headers(&request["headers"]));
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
            }
            _ => {}
        }
    }

    /// Reap the child promptly when Tauri or a CLI command finishes normally.
    pub async fn shutdown(mut self) {
        let _ = self.child.kill().await;
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
