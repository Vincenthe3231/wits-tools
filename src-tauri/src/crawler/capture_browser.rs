//! Owns the isolated capture window and its in-memory media request context.

use std::sync::{Arc, Mutex};

use uuid::Uuid;

use super::{
    Error, Result,
    media::{Candidate, Source},
};

pub(crate) const LABEL: &str = "media-capture-browser";
#[derive(Clone, Default)]
pub(super) struct CaptureBrowser(Arc<Mutex<Vec<(String, Uuid, Candidate)>>>);

impl CaptureBrowser {
    /// Return only redacted source summaries, with credentials resolved by ID inside Rust.
    pub fn sources(&self) -> Result<Vec<Source>> {
        let sources = self.0.lock().map_err(|_| Error::Busy)?;
        Ok(sources
            .iter()
            .map(|(_, id, candidate)| candidate.summary(*id))
            .collect())
    }

    pub fn source(&self, id: Uuid) -> Result<Option<Candidate>> {
        let sources = self.0.lock().map_err(|_| Error::Busy)?;
        Ok(sources
            .iter()
            .find(|(_, key, _)| *key == id)
            .map(|(_, _, candidate)| candidate.clone()))
    }

    /// Refresh repeat observations in place so a source keeps its ID and latest headers.
    #[cfg(windows)]
    fn observe(&self, url: String, candidate: Candidate) {
        if let Ok(mut sources) = self.0.lock() {
            if let Some((_, _, current)) = sources.iter_mut().find(|(key, _, _)| *key == url) {
                *current = candidate;
            } else if sources.len() < 128 {
                sources.push((url, Uuid::new_v4(), candidate));
            }
        }
    }
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(super) use windows::open;
