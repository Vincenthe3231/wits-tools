//! Saves completed browser downloads and tracks only app-owned partial files for recovery.

use std::{
    collections::HashMap,
    fs,
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
};

use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;
use tempfile::TempPath;
use uuid::Uuid;
use webview2_com::{Microsoft::Web::WebView2::Win32::*, StateChangedEventHandler, take_pwstr};
use windows_core::{PCWSTR, PWSTR};

use super::{LABEL, SESSION, wide};

#[derive(Default)]
pub(super) struct Transfers {
    pending: HashMap<Uuid, Pending>,
    active: HashMap<Uuid, Active>,
}

struct Pending {
    args: ICoreWebView2DownloadStartingEventArgs,
    deferral: ICoreWebView2Deferral,
}

struct Active {
    operation: ICoreWebView2DownloadOperation,
    token: i64,
    stage: TempPath,
    destination: PathBuf,
    journal: PathBuf,
}

// Complete every picker deferral, even if the browser closes before its callback.
impl Drop for Pending {
    fn drop(&mut self) {
        unsafe {
            let _ = self.deferral.Complete();
        }
    }
}

impl Transfers {
    pub(super) fn count(&self) -> usize {
        self.pending.len() + self.active.len()
    }

    pub(super) fn cancel(&mut self) {
        self.pending.clear();
        for (_, active) in self.active.drain() {
            unsafe {
                let _ = active.operation.remove_StateChanged(active.token);
                let _ = active.operation.Cancel();
            }
            // The journal outlives locked partials and the process that created them.
        }
    }
}

// Show Save As from the trusted window. The original destination remains untouched until publication.
pub(super) fn start(
    app: &AppHandle,
    session_id: Uuid,
    args: ICoreWebView2DownloadStartingEventArgs,
) -> windows_core::Result<()> {
    unsafe {
        args.SetHandled(true)?;
        args.SetCancel(true)?;
        let mut suggested = PWSTR::null();
        args.ResultFilePath(&mut suggested)?;
        let suggested = take_pwstr(suggested);
        let file_name = Path::new(&suggested)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("download")
            .to_owned();
        let deferral = args.GetDeferral()?;
        let id = Uuid::new_v4();
        let stored = SESSION.with_borrow_mut(|current| {
            let pending = Pending { args, deferral };
            let Some(session) = current
                .as_mut()
                .filter(|session| session.folder.id == session_id && !session.closing)
            else {
                return false;
            };
            session.downloads.pending.insert(id, pending);
            session.changed();
            true
        });
        if stored {
            let desktop = app.clone();
            let _ = app.run_on_main_thread(move || {
                let Some(window) = desktop.get_webview_window(LABEL) else {
                    return;
                };
                let callback_app = desktop.clone();
                desktop
                    .dialog()
                    .file()
                    .set_parent(&window)
                    .set_file_name(file_name)
                    .save_file(move |choice| {
                        let destination = choice.and_then(|path| path.into_path().ok());
                        let completion_app = callback_app.clone();
                        let _ = callback_app.run_on_main_thread(move || {
                            choose(&completion_app, session_id, id, destination)
                        });
                    });
            });
        }
    }
    Ok(())
}

fn choose(app: &AppHandle, session_id: Uuid, id: Uuid, destination: Option<PathBuf>) {
    let pending = SESSION.with_borrow_mut(|current| {
        let session = current
            .as_mut()
            .filter(|session| session.folder.id == session_id)?;
        Some((
            session.downloads.pending.remove(&id)?,
            session.folder.path.clone(),
        ))
    });
    let Some((pending, folder)) = pending else {
        return;
    };
    if let Some(destination) = destination {
        match begin(app, session_id, id, &pending, &folder, destination) {
            Ok(active) => SESSION.with_borrow_mut(|current| {
                if let Some(session) = current
                    .as_mut()
                    .filter(|session| session.folder.id == session_id)
                {
                    session.downloads.active.insert(id, active);
                }
            }),
            Err(()) => notice(
                session_id,
                "The download could not start. Choose a different destination.",
            ),
        }
    }
    drop(pending);
    super::changed(session_id);
}

fn begin(
    app: &AppHandle,
    session_id: Uuid,
    id: Uuid,
    pending: &Pending,
    folder: &Path,
    destination: PathBuf,
) -> Result<Active, ()> {
    let parent = destination
        .parent()
        .ok_or(())?
        .canonicalize()
        .map_err(|_| ())?;
    let destination = parent.join(destination.file_name().ok_or(())?);
    if destination.exists()
        || parent.starts_with(folder.parent().ok_or(())?.canonicalize().map_err(|_| ())?)
    {
        return Err(());
    }
    // Stage on the destination drive so final publication is atomic, including cross-drive Save As.
    let journal = folder.join(format!("{id}.partial"));
    let staged = parent.join(format!(".wits-browser-{id}.part"));
    let record = serde_json::to_vec(&staged).map_err(|_| ())?;
    if staged.exists() || chromium_partial(&staged).exists() {
        return Err(());
    }
    // Commit the recovery record before giving WebView2 the staging path.
    let mut journal_file =
        tempfile::NamedTempFile::new_in(folder.join("profile")).map_err(|_| ())?;
    journal_file.write_all(&record).map_err(|_| ())?;
    journal_file.as_file().sync_all().map_err(|_| ())?;
    journal_file.persist_noclobber(&journal).map_err(|_| ())?;
    let stage = TempPath::try_from_path(&staged).map_err(|_| ())?;
    let desktop = app.clone();
    let (operation, token) = (|| unsafe {
        let operation = pending.args.DownloadOperation()?;
        let handler = StateChangedEventHandler::create(Box::new(move |_, _| {
            let _ = desktop.run_on_main_thread(move || complete(session_id, id));
            Ok(())
        }));
        let mut token = 0;
        pending
            .args
            .SetResultFilePath(PCWSTR(wide(&staged).as_ptr()))?;
        operation.add_StateChanged(&handler, &mut token)?;
        if let Err(error) = pending.args.SetCancel(false) {
            let _ = operation.remove_StateChanged(token);
            return Err(error);
        }
        Ok::<_, windows_core::Error>((operation, token))
    })()
    .map_err(|_| ())?;
    Ok(Active {
        operation,
        token,
        stage,
        destination,
        journal,
    })
}

fn complete(session_id: Uuid, id: Uuid) {
    let finished = SESSION.with_borrow_mut(|current| {
        let session = current
            .as_mut()
            .filter(|session| session.folder.id == session_id)?;
        let active = session.downloads.active.get(&id)?;
        let mut state = COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS;
        unsafe {
            active.operation.State(&mut state).ok()?;
        }
        if state == COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS {
            return None;
        }
        Some((
            session.downloads.active.remove(&id)?,
            state == COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED,
        ))
    });
    let Some((active, success)) = finished else {
        return;
    };
    unsafe {
        let _ = active.operation.remove_StateChanged(active.token);
    }
    let result = if success {
        crate::http_transfer::output::publish(active.stage, &active.destination, false).is_ok()
    } else {
        drop(active.stage);
        false
    };
    let _ = remove_record(&active.journal);
    notice(
        session_id,
        if result {
            "Download saved."
        } else {
            "The download did not finish. Try saving it again."
        },
    );
}

fn notice(id: Uuid, text: &'static str) {
    SESSION.with_borrow_mut(|current| {
        if let Some(session) = current.as_mut().filter(|session| session.folder.id == id) {
            session.notice = Some(text);
            session.changed();
        }
    });
}

// Recover only the exact randomized staging name recorded for this session.
pub(super) fn remove_staging(folder: &Path) -> bool {
    fs::read_dir(folder).is_ok_and(|entries| {
        let mut complete = true;
        for entry in entries
            .flatten()
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "partial"))
        {
            complete &= remove_record(&entry.path());
        }
        complete
    })
}

fn chromium_partial(staged: &Path) -> PathBuf {
    let mut path = staged.as_os_str().to_owned();
    path.push(".crdownload");
    path.into()
}

fn remove_record(record: &Path) -> bool {
    let Some(id) = record
        .file_stem()
        .and_then(|name| name.to_str())
        .and_then(|name| Uuid::parse_str(name).ok())
    else {
        return false;
    };
    let Ok(data) = fs::read(record) else {
        return false;
    };
    let Ok(staged) = serde_json::from_slice::<PathBuf>(&data) else {
        return false;
    };
    if !staged.is_absolute()
        || staged.file_name().and_then(|name| name.to_str())
            != Some(format!(".wits-browser-{id}.part").as_str())
        || !staged.parent().is_some_and(|parent| {
            parent
                .canonicalize()
                .is_ok_and(|canonical| canonical == parent)
        })
    {
        return false;
    }
    fn remove_file(path: &Path) -> bool {
        fs::remove_file(path)
            .or_else(|error| {
                if error.kind() == ErrorKind::NotFound {
                    Ok(())
                } else {
                    Err(error)
                }
            })
            .is_ok()
    }
    remove_file(&staged) && remove_file(&chromium_partial(&staged)) && remove_file(record)
}

#[cfg(test)]
mod tests {
    use super::*;

    // A crash can leave both the staged name and the Chromium temporary name.
    #[test]
    fn recovery_removes_only_recorded_browser_partials() {
        let directory = tempfile::tempdir().unwrap();
        let parent = directory.path().canonicalize().unwrap();
        let id = Uuid::new_v4();
        let stage = parent.join(format!(".wits-browser-{id}.part"));
        let record = parent.join(format!("{id}.partial"));
        let saved = parent.join("saved.txt");
        fs::write(&stage, "temporary").unwrap();
        fs::write(chromium_partial(&stage), "unfinished").unwrap();
        fs::write(&saved, "keep").unwrap();
        fs::write(&record, serde_json::to_vec(&stage).unwrap()).unwrap();

        assert!(remove_record(&record));
        assert!(!stage.exists());
        assert!(!chromium_partial(&stage).exists());
        assert_eq!(fs::read(&saved).unwrap(), b"keep");

        fs::write(&record, serde_json::to_vec(&saved).unwrap()).unwrap();
        assert!(!remove_record(&record));
        assert_eq!(fs::read(saved).unwrap(), b"keep");
    }
}
