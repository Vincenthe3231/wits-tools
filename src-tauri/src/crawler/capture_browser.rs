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
    sources: Vec<(String, Uuid, Candidate)>,
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
    sources: Vec<Source>,
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
                .map(|(_, id, candidate)| candidate.summary(*id))
                .collect(),
        })
    }

    pub fn source(&self, id: Uuid) -> Result<Option<Candidate>> {
        let sources = self.0.lock().map_err(|_| Error::Busy)?;
        Ok(sources
            .sources
            .iter()
            .find(|(_, key, _)| *key == id)
            .map(|(_, _, candidate)| candidate.clone()))
    }

    /// Refresh repeat observations in place so a source keeps its ID and latest headers.
    #[cfg(windows)]
    fn observe(&self, url: String, candidate: Candidate) {
        if let Ok(mut session) = self.0.lock() {
            let sources = &mut session.sources;
            if let Some((_, _, current)) = sources.iter_mut().find(|(key, _, _)| *key == url) {
                *current = candidate;
            } else {
                if sources.len() == 128 {
                    sources.remove(0);
                }
                sources.push((url, Uuid::new_v4(), candidate));
            }
        }
    }
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(super) use windows::open;
