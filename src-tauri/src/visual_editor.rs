//! Owns versioned Visual files, native pickers, and the separate crash recovery snapshot.

use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

use crate::markdown_editor::{
    EditorError, EditorErrorCode, EditorResult, write_attachment, write_recovery,
};

const EXTENSION: &str = "witsdoc";
const MAX_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Deserialize, Serialize)]
pub struct VisualDocument {
    version: u32,
    content: Value,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct VisualFile {
    path: PathBuf,
    fingerprint: String,
}

#[derive(Deserialize, Serialize)]
pub struct VisualSnapshot {
    document: VisualDocument,
    file: Option<VisualFile>,
}

#[derive(Deserialize, Serialize)]
struct VisualDraft {
    #[serde(flatten)]
    snapshot: VisualSnapshot,
    #[serde(default)]
    pending_target: Option<PathBuf>,
}

// Bound regular-file reads before decoding an untrusted document or draft.
fn read_bytes(path: &Path) -> EditorResult<Vec<u8>> {
    let file = File::open(path).map_err(EditorError::io)?;
    if !file.metadata().map_err(EditorError::io)?.is_file() {
        return Err(EditorError::new(
            EditorErrorCode::InvalidPath,
            "Choose a regular file.",
        ));
    }
    let mut bytes = Vec::new();
    file.take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(EditorError::io)?;
    ensure_size(bytes.len())?;
    Ok(bytes)
}

fn ensure_size(size: usize) -> EditorResult<()> {
    if size > MAX_BYTES {
        return Err(EditorError::new(
            EditorErrorCode::FileTooLarge,
            "Visual files are limited to 64 MiB.",
        ));
    }
    Ok(())
}

// The envelope is versioned independently of the editor's interactive state.
fn encode(document: &VisualDocument) -> EditorResult<Vec<u8>> {
    if document.version != 1 || document.content.get("type").and_then(Value::as_str) != Some("doc")
    {
        return Err(EditorError::new(
            EditorErrorCode::InvalidEncoding,
            "Unsupported Visual document.",
        ));
    }
    let bytes = serde_json::to_vec(document).map_err(decode_error)?;
    ensure_size(bytes.len())?;
    Ok(bytes)
}

fn decode_error(error: serde_json::Error) -> EditorError {
    EditorError::new(EditorErrorCode::InvalidEncoding, error.to_string())
}

fn fingerprint(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

fn validate_path(path: &Path) -> EditorResult<()> {
    if !path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case(EXTENSION))
    {
        return Err(EditorError::new(
            EditorErrorCode::UnsupportedFileType,
            "Choose a .witsdoc file.",
        ));
    }
    Ok(())
}

// Read creation dates for saved document shortcuts. Missing or unsupported dates stay empty.
#[tauri::command]
pub async fn editor_created_times(paths: Vec<PathBuf>) -> Vec<Option<u64>> {
    paths
        .iter()
        .map(|path| {
            let extension = path.extension()?.to_str()?.to_ascii_lowercase();
            if !matches!(extension.as_str(), "md" | "markdown" | EXTENSION) {
                return None;
            }
            let metadata = fs::metadata(path).ok()?;
            if !metadata.is_file() {
                return None;
            }
            let created = metadata
                .created()
                .ok()?
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?;
            u64::try_from(created.as_millis()).ok()
        })
        .collect()
}

// A shared picker can select either format while a Locate action requests its own type.
#[tauri::command]
pub async fn visual_pick_file(app: AppHandle, visual_only: bool) -> EditorResult<Option<PathBuf>> {
    let dialog = app.dialog().file();
    let dialog = if visual_only {
        dialog.add_filter("Visual documents", &[EXTENSION])
    } else {
        dialog.add_filter("Documents", &["md", "markdown", EXTENSION])
    };
    dialog
        .blocking_pick_file()
        .map(|path| {
            path.into_path()
                .map_err(|error| EditorError::new(EditorErrorCode::InvalidPath, error.to_string()))
        })
        .transpose()
}

// Validate and fingerprint the same bytes before returning them to Vue.
#[tauri::command]
pub async fn visual_open(path: PathBuf) -> EditorResult<VisualSnapshot> {
    read_document(&path)
}

fn read_document(path: &Path) -> EditorResult<VisualSnapshot> {
    validate_path(&path)?;
    let path = path.canonicalize().map_err(EditorError::io)?;
    let bytes = read_bytes(&path)?;
    let document = serde_json::from_slice(&bytes).map_err(decode_error)?;
    encode(&document)?;
    Ok(VisualSnapshot {
        document,
        file: Some(VisualFile {
            path,
            fingerprint: fingerprint(&bytes),
        }),
    })
}

// Keep Save and Save As on one guarded native write path.
#[tauri::command]
pub async fn visual_save(
    app: AppHandle,
    document: VisualDocument,
    original: Option<VisualFile>,
    save_as: bool,
) -> EditorResult<Option<VisualFile>> {
    let bytes = encode(&document)?;
    let (selected, allow_replace) = if let Some(original) = &original
        && !save_as
    {
        (original.path.clone(), true)
    } else {
        let name = original
            .as_ref()
            .and_then(|file| file.path.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("Untitled.witsdoc");
        let Some(path) = app
            .dialog()
            .file()
            .add_filter("Visual documents", &[EXTENSION])
            .set_file_name(name)
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let path = path.into_path().map_err(|error| {
            EditorError::new(EditorErrorCode::InvalidDestination, error.to_string())
        })?;
        if path.extension().is_none() {
            (path.with_extension(EXTENSION), false)
        } else {
            (path, true)
        }
    };
    let draft = draft_path(&app)?;
    let saved = save_at(
        &selected,
        original.as_ref(),
        save_as,
        allow_replace,
        &bytes,
        |path| {
            // Record the intended target so recovery can recognize a successful first Save or Save As.
            let _ = persist_draft(
                &draft,
                &VisualDraft {
                    snapshot: VisualSnapshot {
                        document,
                        file: original.clone(),
                    },
                    pending_target: Some(path.to_owned()),
                },
            );
        },
    )?;
    Ok(Some(saved))
}

// Resolve the target once, then verify the original bytes just before committing the save.
fn save_at(
    selected: &Path,
    original: Option<&VisualFile>,
    save_as: bool,
    allow_replace: bool,
    bytes: &[u8],
    checkpoint: impl FnOnce(&Path),
) -> EditorResult<VisualFile> {
    validate_path(selected)?;
    let exists = selected.try_exists().map_err(EditorError::io)?;
    let path = if exists {
        selected.canonicalize().map_err(EditorError::io)?
    } else {
        let parent = selected.parent().ok_or_else(|| {
            EditorError::new(
                EditorErrorCode::InvalidDestination,
                "Choose a Visual file location.",
            )
        })?;
        parent
            .canonicalize()
            .map_err(EditorError::io)?
            .join(selected.file_name().unwrap())
    };
    checkpoint(&path);
    if let Some(original) = original.filter(|file| !save_as || file.path == path)
        && (path != original.path
            || !read_bytes(&path)
                .ok()
                .is_some_and(|current| fingerprint(&current) == original.fingerprint))
    {
        return Err(EditorError::new(
            EditorErrorCode::ExternalChange,
            "The original Visual file changed or moved.",
        ));
    }
    if exists && allow_replace {
        write_recovery(&path, bytes)?;
    } else {
        write_attachment(&path, bytes)?;
    }
    Ok(VisualFile {
        path,
        fingerprint: fingerprint(bytes),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Exercise actual file bytes, independent of an operating system file picker.
    fn fixture() -> VisualDocument {
        VisualDocument {
            version: 1,
            content: json!({"type":"doc","content":[
                {"type":"paragraph","content":[{"type":"text","text":"Release plan"}]},
                {"type":"documentImage","attrs":{"src":"data:image/png;base64,aW1hZ2U=","alt":"Local image"}},
                {"type":"board","attrs":{"id":"release","objects":[]}}
            ]}),
        }
    }

    #[test]
    fn visual_file_reopens_after_moving_without_its_cache() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("Plan.witsdoc");
        let bytes = encode(&fixture()).unwrap();
        save_at(&target, None, true, false, &bytes, |_| {}).unwrap();
        let moved = dir.path().join("Moved.witsdoc");
        fs::rename(target, &moved).unwrap();
        assert_eq!(
            encode(&read_document(&moved).unwrap().document).unwrap(),
            bytes
        );
    }

    #[test]
    fn visual_save_preserves_external_edits_and_supports_save_as() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("Plan.witsdoc");
        let bytes = encode(&fixture()).unwrap();
        let initial = save_at(&target, None, true, false, &bytes, |_| {}).unwrap();
        fs::write(&target, b"external change").unwrap();
        assert!(save_at(&target, Some(&initial), false, true, &bytes, |_| {}).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"external change");
        assert!(
            save_at(
                &dir.path().join("Copy.witsdoc"),
                Some(&initial),
                true,
                false,
                &bytes,
                |_| {}
            )
            .is_ok()
        );
    }

    #[test]
    fn recovery_distinguishes_an_interrupted_first_save_from_a_committed_save() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("Plan.witsdoc");
        let draft = || VisualDraft {
            snapshot: VisualSnapshot {
                document: fixture(),
                file: None,
            },
            pending_target: Some(target.clone()),
        };
        assert!(recoverable(draft()).unwrap().is_some());
        save_at(
            &target,
            None,
            true,
            false,
            &encode(&fixture()).unwrap(),
            |_| {},
        )
        .unwrap();
        assert!(recoverable(draft()).unwrap().is_none());
    }
}

fn draft_path(app: &AppHandle) -> EditorResult<PathBuf> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("visual-editor-draft.json"))
        .map_err(|error| EditorError::new(EditorErrorCode::RecoveryFailed, error.to_string()))
}

// A crash after updating an existing file must not offer an already-saved snapshot.
#[tauri::command]
pub async fn visual_read_draft(app: AppHandle) -> EditorResult<Option<VisualSnapshot>> {
    let path = draft_path(&app)?;
    if !path.try_exists().map_err(EditorError::io)? {
        return Ok(None);
    }
    let draft: VisualDraft = serde_json::from_slice(&read_bytes(&path)?).map_err(decode_error)?;
    recoverable(draft)
}

// The original path and a pending Save As target can each confirm an already-committed draft.
fn recoverable(draft: VisualDraft) -> EditorResult<Option<VisualSnapshot>> {
    let bytes = encode(&draft.snapshot.document)?;
    let original = draft.snapshot.file.as_ref().map(|file| &file.path);
    let saved = [original, draft.pending_target.as_ref()]
        .into_iter()
        .flatten()
        .any(|path| read_bytes(path).ok().as_deref() == Some(&bytes));
    Ok((!saved).then_some(draft.snapshot))
}

fn persist_draft(path: &Path, draft: &VisualDraft) -> EditorResult<()> {
    let bytes = serde_json::to_vec(draft).map_err(decode_error)?;
    ensure_size(bytes.len())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(EditorError::io)?;
    }
    write_recovery(path, &bytes)
}

// Persist only the Visual session's draft, independently from Markdown recovery.
#[tauri::command]
pub async fn visual_write_draft(
    app: AppHandle,
    snapshot: Option<VisualSnapshot>,
) -> EditorResult<()> {
    let path = draft_path(&app)?;
    if let Some(snapshot) = snapshot {
        encode(&snapshot.document)?;
        persist_draft(
            &path,
            &VisualDraft {
                snapshot,
                pending_target: None,
            },
        )
    } else {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(EditorError::io(error)),
        }
    }
}
