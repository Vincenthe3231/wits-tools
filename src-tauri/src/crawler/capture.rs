//! Owns optional selected-tab capture, credential lifetime, and native host framing.

#[cfg(windows)]
mod windows;

use std::{
    collections::HashMap,
    io::{Read, Write},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{
    Error, Result,
    media::{Candidate, Source},
};

const PROTOCOL: u8 = 1;
const MAX_FRAME: usize = 1 << 20;
const MAX_STREAMS: usize = 300;
const PAIRING_LIFETIME: Duration = Duration::from_secs(300);
pub const HOST_NAME: &str = "com.wits_tools.capture";

#[derive(Clone, Default)]
pub struct Capture {
    inner: Arc<Mutex<Session>>,
}

#[derive(Default)]
struct Session {
    native: Option<CancellationToken>,
    manual: Option<Manual>,
    selected: Option<(Uuid, i64)>,
    seen: Vec<(Uuid, Captured)>,
}

struct Manual {
    token: String,
    started: Instant,
    paired: bool,
    stop: CancellationToken,
}

#[derive(Clone, Deserialize, Serialize)]
struct Captured {
    url: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    headers: HashMap<String, String>,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Message {
    #[serde(default = "protocol")]
    protocol_version: u8,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    tab_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<Captured>,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure: Option<Failure>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Failure {
    code: String,
    user_message: String,
    retryable: bool,
}

#[derive(Serialize)]
pub struct Pairing {
    pub code: String,
    pub expires_in_seconds: u64,
}

impl Capture {
    /// The optional pipe exists only while the user has enabled browser capture.
    pub fn start(&self) -> Result<()> {
        #[cfg(windows)]
        {
            let mut session = self.inner.lock().map_err(|_| Error::CaptureUnavailable)?;
            if session.native.is_none() {
                let stop = CancellationToken::new();
                windows::serve(self.clone(), stop.clone())?;
                session.native = Some(stop);
            }
            Ok(())
        }
        #[cfg(not(windows))]
        Err(Error::CaptureUnavailable)
    }

    /// Start an unguessable, loopback-only pairing session with the extension's existing routes.
    pub async fn pair(&self) -> Result<Pairing> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|_| Error::CaptureUnavailable)?;
        let address = listener
            .local_addr()
            .map_err(|_| Error::CaptureUnavailable)?;
        let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let stop = CancellationToken::new();
        {
            let mut session = self.inner.lock().map_err(|_| Error::CaptureUnavailable)?;
            if let Some(previous) = session.manual.take() {
                previous.stop.cancel();
            }
            session.seen.clear();
            session.selected = None;
            session.manual = Some(Manual {
                token: token.clone(),
                started: Instant::now(),
                paired: false,
                stop: stop.clone(),
            });
        }
        let router = Router::new()
            .route("/v1/firefox/streams", post(manual_stream))
            .route("/v1/firefox/capture-options", get(manual_options))
            .layer(DefaultBodyLimit::max(MAX_FRAME))
            .with_state(self.clone());
        tokio::spawn(async move {
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(stop.cancelled_owned())
                .await;
        });
        Ok(Pairing {
            code: format!("http://{address}#{token}"),
            expires_in_seconds: PAIRING_LIFETIME.as_secs(),
        })
    }

    /// Stop both transports and drop all selected-tab request context.
    pub fn stop(&self) {
        if let Ok(mut session) = self.inner.lock() {
            if let Some(native) = session.native.take() {
                native.cancel();
            }
            if let Some(manual) = session.manual.take() {
                manual.stop.cancel();
            }
            *session = Session::default();
        }
    }

    pub fn sources(&self) -> Result<Vec<Source>> {
        let session = self.inner.lock().map_err(|_| Error::CaptureUnavailable)?;
        Ok(session
            .seen
            .iter()
            .filter_map(|(id, stream)| {
                stream
                    .candidate(None)
                    .ok()
                    .map(|stream| stream.summary(*id))
            })
            .collect())
    }

    pub fn source(&self, id: Uuid, proxy: Option<String>) -> Result<Candidate> {
        let session = self.inner.lock().map_err(|_| Error::CaptureUnavailable)?;
        session
            .seen
            .iter()
            .find(|(stream_id, _)| *stream_id == id)
            .ok_or(Error::NotFound)?
            .1
            .candidate(proxy)
    }

    fn connect(&self, id: Uuid, tab_id: i64) -> Result<()> {
        if tab_id < 0 {
            return Err(Error::CaptureUnavailable);
        }
        let mut session = self.inner.lock().map_err(|_| Error::CaptureUnavailable)?;
        if session.native.is_none() {
            return Err(Error::CaptureUnavailable);
        }
        if let Some(manual) = session.manual.take() {
            manual.stop.cancel();
        }
        session.selected = Some((id, tab_id));
        session.seen.clear();
        Ok(())
    }

    fn disconnect(&self, id: Uuid) {
        if let Ok(mut session) = self.inner.lock()
            && session.selected.is_some_and(|(selected, _)| selected == id)
        {
            session.selected = None;
            session.seen.clear();
        }
    }

    fn native_stream(&self, id: Uuid, tab_id: i64, stream: Captured) -> Result<()> {
        let mut session = self.inner.lock().map_err(|_| Error::CaptureUnavailable)?;
        if session.selected != Some((id, tab_id)) {
            return Err(Error::CaptureUnavailable);
        }
        session.add(stream)
    }
}

impl Session {
    fn add(&mut self, stream: Captured) -> Result<()> {
        if matches!(stream.kind.as_str(), "ts" | "segment") {
            return Ok(());
        }
        stream.candidate(None)?;
        if let Some((_, previous)) = self
            .seen
            .iter_mut()
            .find(|(_, previous)| previous.url == stream.url)
        {
            *previous = stream;
        } else {
            if self.seen.len() >= MAX_STREAMS {
                self.seen.remove(0);
            }
            self.seen.push((Uuid::new_v4(), stream));
        }
        Ok(())
    }
}

impl Captured {
    fn candidate(&self, proxy: Option<String>) -> Result<Candidate> {
        let mime = match self.kind.to_ascii_lowercase().as_str() {
            "hls" | "m3u8" => "application/vnd.apple.mpegurl",
            "dash" | "mpd" => "application/dash+xml",
            "direct" | "mp4" | "webm" | "video" => "video/mp4",
            "mp3" | "audio" => "audio/mpeg",
            _ => "",
        };
        Candidate::new(
            &self.url,
            mime,
            self.headers.clone().into_iter().collect(),
            proxy,
        )
    }
}

/// Authenticate the manual pairing on first use and retain it until explicitly stopped.
fn authenticate(session: &mut Session, headers: &HeaderMap) -> bool {
    let Some(manual) = &mut session.manual else {
        return false;
    };
    let Some(token) = headers
        .get("witscapturesession")
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    if (!manual.paired && manual.started.elapsed() >= PAIRING_LIFETIME)
        || !constant_time_equal(token.as_bytes(), manual.token.as_bytes())
    {
        return false;
    }
    manual.paired = true;
    true
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0, |difference, (a, b)| difference | (a ^ b))
            == 0
}

async fn manual_options(
    State(capture): State<Capture>,
    headers: HeaderMap,
) -> std::result::Result<Json<serde_json::Value>, StatusCode> {
    let mut session = capture
        .inner
        .lock()
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    if !authenticate(&mut session, &headers) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(Json(
        serde_json::json!({"protocolVersion": PROTOCOL, "diagnosticMode": false}),
    ))
}

async fn manual_stream(
    State(capture): State<Capture>,
    headers: HeaderMap,
    Json(stream): Json<Captured>,
) -> StatusCode {
    let Ok(mut session) = capture.inner.lock() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    if !authenticate(&mut session, &headers) {
        return StatusCode::UNAUTHORIZED;
    }
    match session.add(stream) {
        Ok(()) => StatusCode::CREATED,
        Err(Error::UnsupportedMedia) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::BAD_REQUEST,
    }
}

fn protocol() -> u8 {
    PROTOCOL
}

/// Read a bounded browser-native message using the documented little-endian framing.
fn read_frame(reader: &mut impl Read) -> std::io::Result<Vec<u8>> {
    let mut length = [0; 4];
    reader.read_exact(&mut length)?;
    let size = u32::from_le_bytes(length) as usize;
    if size == 0 || size > MAX_FRAME {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    let mut payload = vec![0; size];
    reader.read_exact(&mut payload)?;
    Ok(payload)
}

fn write_frame(writer: &mut impl Write, bytes: &[u8]) -> std::io::Result<()> {
    writer.write_all(&(bytes.len() as u32).to_le_bytes())?;
    writer.write_all(bytes)?;
    writer.flush()
}

fn reply(kind: &str) -> Message {
    Message {
        protocol_version: PROTOCOL,
        kind: kind.into(),
        ..Message::default()
    }
}

fn failure() -> Message {
    Message {
        failure: Some(Failure {
            code: "captureUnavailable".into(),
            user_message: "Enable browser capture in WITS Tools and try again.".into(),
            retryable: true,
        }),
        ..reply("error")
    }
}

/// Run the lightweight bridge launched by the browser's native messaging runtime.
#[cfg(windows)]
pub fn native_host() -> Result<()> {
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    let mut pipe = windows::connect().ok();
    while let Ok(payload) = read_frame(&mut input) {
        let reply = pipe
            .as_mut()
            .and_then(|pipe| {
                write_frame(pipe, &payload).ok()?;
                read_frame(pipe).ok()
            })
            .unwrap_or_else(|| serde_json::to_vec(&failure()).unwrap_or_default());
        write_frame(&mut output, &reply).map_err(|_| Error::CaptureUnavailable)?;
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn native_host() -> Result<()> {
    Err(Error::CaptureUnavailable)
}

/// Register the optional capture bridge under the current user's browser configuration.
pub fn register() -> Result<()> {
    #[cfg(windows)]
    {
        windows::register()
    }
    #[cfg(not(windows))]
    Err(Error::CaptureUnavailable)
}
