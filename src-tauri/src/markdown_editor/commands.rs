use std::{path::PathBuf, sync::MutexGuard};

use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

use super::{
    EditorError, EditorResult, EditorState, FileVersion, OpenDocument, RecoveryDraft, files,
    recovery,
};

#[tauri::command]
pub async fn editor_open(app: AppHandle) -> EditorResult<Option<OpenDocument>> {
    let Some(path) = app
        .dialog()
        .file()
        .add_filter("Markdown", &["md", "markdown"])
        .blocking_pick_file()
    else {
        return Ok(None);
    };
    let path = path
        .into_path()
        .map_err(|error| EditorError::new("path", error.to_string()))?;
    let state = app.state::<EditorState>();
    let mut current = current_file(&state)?;
    let (document, version) = files::open(&path)?;
    recovery::clear(&recovery_path(&app)?)?;
    *current = Some(version);
    Ok(Some(document))
}

#[tauri::command]
pub async fn editor_save(
    app: AppHandle,
    markdown: String,
    save_as: bool,
) -> EditorResult<Option<String>> {
    let state = app.state::<EditorState>();
    let original = current_file(&state)?.clone();
    let selected = if save_as || original.is_none() {
        let file_name = original.as_ref().map(|file| files::name(&file.path));
        let mut dialog = app
            .dialog()
            .file()
            .add_filter("Markdown", &["md", "markdown"])
            .set_file_name(file_name.unwrap_or_else(|| "Untitled.md".into()));
        if let Some(parent) = original.as_ref().and_then(|file| file.path.parent()) {
            dialog = dialog.set_directory(parent);
        }
        let Some(path) = dialog.blocking_save_file() else {
            return Ok(None);
        };
        Some(
            path.into_path()
                .map_err(|error| EditorError::new("path", error.to_string()))?,
        )
    } else {
        None
    };

    let mut current = current_file(&state)?;
    let saved = files::save(selected.as_deref(), current.as_ref(), &markdown)?;
    let name = files::name(&saved.path);
    *current = Some(saved);
    Ok(Some(name))
}

#[tauri::command]
pub fn editor_reset(app: AppHandle, state: State<EditorState>) -> EditorResult<()> {
    let mut current = current_file(&state)?;
    recovery::clear(&recovery_path(&app)?)?;
    *current = None;
    Ok(())
}

#[tauri::command]
pub fn editor_recovery_info(app: AppHandle) -> EditorResult<Option<String>> {
    Ok(recovery::read(&recovery_path(&app)?)?.map(|draft| draft.name))
}

#[tauri::command]
pub fn editor_restore(app: AppHandle, state: State<EditorState>) -> EditorResult<OpenDocument> {
    let mut current = current_file(&state)?;
    let draft = recovery::read(&recovery_path(&app)?)?
        .ok_or_else(|| EditorError::new("recovery", "The recovery draft is unavailable."))?;
    *current = draft.original;
    Ok(OpenDocument {
        name: draft.name,
        markdown: draft.markdown,
    })
}

#[tauri::command]
pub async fn editor_update_recovery(app: AppHandle, markdown: Option<String>) -> EditorResult<()> {
    let state = app.state::<EditorState>();
    let current = current_file(&state)?;
    let path = recovery_path(&app)?;
    let Some(markdown) = markdown else {
        return recovery::clear(&path);
    };

    files::validate_size(&markdown)?;
    let name = current
        .as_ref()
        .map_or_else(|| "Untitled".into(), |file| files::name(&file.path));
    recovery::write(
        &path,
        &RecoveryDraft {
            name,
            markdown,
            original: current.clone(),
        },
    )
}

fn recovery_path(app: &AppHandle) -> EditorResult<PathBuf> {
    app.path()
        .app_local_data_dir()
        .map(|path| path.join("markdown-draft.json"))
        .map_err(|error| EditorError::new("recovery", error.to_string()))
}

fn current_file(state: &EditorState) -> EditorResult<MutexGuard<'_, Option<FileVersion>>> {
    state
        .0
        .lock()
        .map_err(|_| EditorError::new("state", "The editor session is unavailable."))
}
