//! Connects the editor's IPC commands to native dialogs, file state, and draft recovery.

use std::{path::PathBuf, sync::MutexGuard};

use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

use super::{
    EditorError, EditorErrorCode, EditorResult, EditorState, FileVersion, OpenDocument,
    RecoveryDraft, files, images, package, recovery,
};

/// Replace the open file only after the picker succeeds and the selected file is readable.
#[tauri::command]
pub async fn editor_open(
    app: AppHandle,
    shortcut: Option<PathBuf>,
) -> EditorResult<Option<OpenDocument>> {
    // Recent files and the picker share the same validation and session replacement.
    let path = if let Some(path) = shortcut {
        path
    } else {
        let Some(path) = app
            .dialog()
            .file()
            .add_filter("Markdown", &["md", "markdown"])
            .blocking_pick_file()
        else {
            return Ok(None);
        };
        path.into_path()
            .map_err(|error| EditorError::new(EditorErrorCode::InvalidPath, error.to_string()))?
    };
    let state = app.state::<EditorState>();
    let mut current = current_file(&state)?;
    let (document, version) = files::open(&path)?;
    recovery::clear(&recovery_path(&app)?)?;
    *current = Some(version);
    Ok(Some(document))
}

/// Save to the existing path or ask the user for a new destination.
#[tauri::command]
pub async fn editor_save(
    app: AppHandle,
    markdown: String,
    save_as: bool,
    image_sources: Vec<String>,
) -> EditorResult<Option<PathBuf>> {
    let state = app.state::<EditorState>();
    // Release the session lock before yielding control to a native file picker.
    let original = current_file(&state)?.clone();
    let selected =
        if save_as || original.is_none() {
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
            Some(path.into_path().map_err(|error| {
                EditorError::new(EditorErrorCode::InvalidPath, error.to_string())
            })?)
        } else {
            None
        };

    let mut current = current_file(&state)?;
    // Update the tracked file version only after the write succeeds.
    let image_root = image_cache(&app)?;
    let parent = current.as_ref().and_then(|file| file.path.parent());
    let saved = files::save(
        selected.as_deref(),
        current.as_ref(),
        &markdown,
        |destination| images::publish(&image_root, parent, destination, &image_sources),
    )?;
    let path = saved.path.clone();
    *current = Some(saved);
    Ok(Some(path))
}

/// Build a shareable snapshot without changing the active document or its recovery state.
#[tauri::command]
pub async fn editor_export_package(
    app: AppHandle,
    markdown: String,
    assets: Vec<package::PackageAsset>,
) -> EditorResult<bool> {
    let state = app.state::<EditorState>();
    let original = current_file(&state)?.clone();
    let Some(destination) = app.dialog().file().blocking_pick_folder() else {
        return Ok(false);
    };
    let destination = destination.into_path().map_err(|error| {
        EditorError::new(EditorErrorCode::InvalidDestination, error.to_string())
    })?;
    package::write(
        &destination,
        original.as_ref().map(|file| file.path.as_path()),
        &image_cache(&app)?,
        &markdown,
        &assets,
    )?;
    Ok(true)
}

/// Clear persisted recovery before releasing the current file association.
#[tauri::command]
pub fn editor_reset(app: AppHandle, state: State<EditorState>) -> EditorResult<()> {
    let mut current = current_file(&state)?;
    recovery::clear(&recovery_path(&app)?)?;
    *current = None;
    Ok(())
}

/// Report the draft name so Vue can offer a recovery choice before loading its content.
#[tauri::command]
pub fn editor_recovery_info(app: AppHandle) -> EditorResult<Option<String>> {
    Ok(recovery::read(&recovery_path(&app)?)?.map(|draft| draft.name))
}

/// Restore both the draft contents and their original file association, if present.
#[tauri::command]
pub fn editor_restore(app: AppHandle, state: State<EditorState>) -> EditorResult<OpenDocument> {
    let mut current = current_file(&state)?;
    let draft = recovery::read(&recovery_path(&app)?)?.ok_or_else(|| {
        EditorError::new(
            EditorErrorCode::RecoveryFailed,
            "The recovery draft is unavailable.",
        )
    })?;
    let path = draft.original.as_ref().map(|file| file.path.clone());
    *current = draft.original;
    Ok(OpenDocument {
        path,
        file_backed: current.is_some(),
        name: draft.name,
        markdown: draft.markdown,
    })
}

/// Persist unsaved Markdown with its file association, or clear the snapshot when requested.
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

/// Store each installation's draft in the application-local data directory.
fn recovery_path(app: &AppHandle) -> EditorResult<PathBuf> {
    app.path()
        .app_local_data_dir()
        .map(|path| path.join("markdown-draft.json"))
        .map_err(|error| EditorError::new(EditorErrorCode::RecoveryFailed, error.to_string()))
}

/// Guard access to the file version shared by save and recovery commands.
fn current_file(state: &EditorState) -> EditorResult<MutexGuard<'_, Option<FileVersion>>> {
    state.0.lock().map_err(|_| {
        EditorError::new(
            EditorErrorCode::SessionUnavailable,
            "The editor session is unavailable.",
        )
    })
}

/// Cache imported bytes before returning a relative path that can survive recovery.
#[tauri::command]
pub async fn editor_stage_image(
    app: AppHandle,
    request: tauri::ipc::Request<'_>,
) -> EditorResult<String> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err(EditorError::new(
            EditorErrorCode::UnsupportedImageType,
            "The image could not be read.",
        ));
    };
    images::stage(&image_cache(&app)?, bytes)
}

/// Preview document-local raster attachments without broadening WebView filesystem access.
#[tauri::command]
pub async fn editor_read_image(app: AppHandle, src: String) -> EditorResult<tauri::ipc::Response> {
    let state = app.state::<EditorState>();
    let original = current_file(&state)?.clone();
    let parent = original.as_ref().and_then(|file| file.path.parent());
    images::preview(&image_cache(&app)?, parent, &src).map(tauri::ipc::Response::new)
}

/// Keep bytes available across application restarts while a draft references them.
fn image_cache(app: &AppHandle) -> EditorResult<PathBuf> {
    Ok(recovery_path(app)?.with_file_name("markdown-images"))
}
