//! Defines the native Markdown session and the document data returned to Vue.
pub(crate) mod commands;
mod error;
mod files;
mod images;
mod recovery;

use std::{path::PathBuf, sync::Mutex};

use serde::{Deserialize, Serialize};

use error::{EditorError, EditorResult};

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
