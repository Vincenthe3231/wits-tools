pub(crate) mod commands;
mod error;
mod files;
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
pub struct OpenDocument {
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
pub struct EditorState(Mutex<Option<FileVersion>>);
