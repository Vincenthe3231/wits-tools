//! Owns the isolated capture window and its in-memory media request context.

use std::sync::{Arc, Mutex};

use serde::Serialize;
use uuid::Uuid;

use super::{
    Error, Result,
    media::{Candidate, Source},
};

pub(crate) const LABEL: &str = "media-capture-browser";
#[derive(Clone, Default)]
pub(super) struct CaptureBrowser(Arc<Mutex<Session>>);

// Keep navigation and the candidate list consistent in the same desktop snapshot.
struct Session {
    status: &'static str,
    navigation: Option<u64>,
    route: Option<String>,
    sources: Vec<CapturedStream>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            status: "closed",
            navigation: None,
            route: None,
            sources: Vec::new(),
        }
    }
}

#[derive(Serialize)]
pub(crate) struct CaptureSnapshot {
    status: &'static str,
    sources: Vec<CapturedSummary>,
}

// Keep the first observation time and asynchronously resolved duration stable on repeats.
struct CapturedStream {
    url: String,
    id: Uuid,
    candidate: Candidate,
    captured_at: u64,
    duration_seconds: Option<f64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CapturedSummary {
    #[serde(flatten)]
    source: Source,
    captured_at: u64,
    duration_seconds: Option<f64>,
}

impl CaptureBrowser {
    /// Return only redacted source summaries, with credentials resolved by ID inside Rust.
    pub fn snapshot(&self) -> Result<CaptureSnapshot> {
        let session = self.0.lock().map_err(|_| Error::Busy)?;
        Ok(CaptureSnapshot {
            status: session.status,
            sources: session
                .sources
                .iter()
                .rev()
                .map(|stream| CapturedSummary {
                    source: stream.candidate.summary(stream.id),
                    captured_at: stream.captured_at,
                    duration_seconds: stream.duration_seconds,
                })
                .collect(),
        })
    }

    pub fn source(&self, id: Uuid) -> Result<Option<Candidate>> {
        let sources = self.0.lock().map_err(|_| Error::Busy)?;
        Ok(sources
            .sources
            .iter()
            .find(|stream| stream.id == id)
            .map(|stream| stream.candidate.clone()))
    }

    /// Refresh repeat observations in place so a source keeps its ID and latest headers.
    #[cfg(windows)]
    fn observe(&self, url: String, candidate: Candidate) {
        use std::time::{Duration, SystemTime, UNIX_EPOCH};

        static PROBES: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(3);
        let Ok(mut session) = self.0.lock() else {
            return;
        };
        let sources = &mut session.sources;
        if let Some(stream) = sources.iter_mut().find(|stream| stream.url == url) {
            let page_name = stream.candidate.page_name.clone();
            stream.candidate = candidate;
            stream.candidate.page_name = page_name.or(stream.candidate.page_name.take());
            return;
        }
        if sources.len() == 128 {
            sources.remove(0);
        }
        let id = Uuid::new_v4();
        sources.push(CapturedStream {
            url,
            id,
            candidate,
            captured_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            duration_seconds: None,
        });
        drop(session);

        // Keep probes off the WebView thread and discard results after the capture closes.
        let browser = self.clone();
        tauri::async_runtime::spawn(async move {
            let Ok(_permit) = PROBES.acquire().await else {
                return;
            };
            let Ok(Some(candidate)) = browser.source(id) else {
                return;
            };
            let Ok(Ok(Some(duration))) = tokio::time::timeout(
                Duration::from_secs(8),
                super::media::captured_duration(candidate),
            )
            .await
            else {
                return;
            };
            if let Ok(mut session) = browser.0.lock()
                && let Some(stream) = session.sources.iter_mut().find(|stream| stream.id == id)
            {
                stream.duration_seconds = Some(duration);
            }
        });
    }
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(super) use windows::open;
