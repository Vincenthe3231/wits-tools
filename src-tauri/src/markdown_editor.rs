//! Defines the native Markdown session and the document data returned to Vue.
pub(crate) mod commands;
mod error;
mod files;
mod images;
mod package;
mod recovery;

// Let the Visual document tool reuse the editor's native errors and atomic file writers.
pub(crate) use error::{EditorError, EditorErrorCode, EditorResult};
pub(crate) use files::{write_attachment, write_recovery};

use std::{path::PathBuf, sync::Mutex};

use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct FileVersion {
    path: PathBuf,
    fingerprint: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
/// Carries the file association explicitly so Vue can choose Save or Save As.
pub struct OpenDocument {
    file_backed: bool,
    name: String,
    markdown: String,
    path: Option<PathBuf>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RecoveryDraft {
    name: String,
    markdown: String,
    original: Option<FileVersion>,
}

#[derive(Default)]
/// Tracks the file version used to detect an external edit during save.
pub struct EditorState(Mutex<Option<FileVersion>>);
