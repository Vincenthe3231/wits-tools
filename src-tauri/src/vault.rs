//! Connects the Vault workspace, native file pickers, and installer handoff to the storage engine.
mod store;

use std::{
    fs,
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use uuid::Uuid;

use store::Vault;

const IDLE_LIMIT: Duration = Duration::from_secs(600);

// Installer invocations carry only the removal target. The password stays in the Vault flow.
#[derive(Clone, Default)]
pub struct Launch {
    pub nsis: bool,
    pub msi: Option<String>,
}

impl Launch {
    pub fn detect() -> Self {
        let mut args = std::env::args().skip(1);
        match args.next().as_deref() {
            Some("--vault-uninstall-nsis") => Self {
                nsis: true,
                msi: None,
            },
            Some("--vault-uninstall-msi") => Self {
                nsis: false,
                msi: args
                    .next()
                    .and_then(|value| Uuid::parse_str(&value).ok())
                    .map(|id| format!("{{{id}}}")),
            },
            _ => Self::default(),
        }
    }

    fn removing(&self) -> bool {
        self.nsis || self.msi.is_some()
    }
}

#[derive(Default)]
pub struct VaultState {
    session: Mutex<Option<(Vault, Instant)>>,
    cancel: AtomicBool,
    pub launch: Launch,
}

impl VaultState {
    pub fn new(launch: Launch) -> Self {
        Self {
            launch,
            ..Self::default()
        }
    }
}

// Keep secrets and native filesystem actions behind one typed command boundary.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Action {
    Status,
    Locate,
    Open { password: String },
    Create { password: String },
    Lock,
    Import { folder: bool, remove: bool },
    Export { id: Option<Uuid> },
    Delete { confirmed: String },
    FinishUninstall { proceed: bool },
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reply {
    path: Option<String>,
    entries: Option<Vec<store::Entry>>,
    output: Option<String>,
    remaining: Vec<String>,
    uninstall: bool,
}

fn preference(app: &AppHandle) -> Result<PathBuf> {
    Ok(app.path().app_local_data_dir()?.join("vault-location.json"))
}

fn remembered(app: &AppHandle) -> Result<Option<PathBuf>> {
    let file = preference(app)?;
    match fs::read(file) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn remember(app: &AppHandle, root: &PathBuf) -> Result<()> {
    let file = preference(app)?;
    let parent = file.parent().context("Invalid settings path.")?;
    fs::create_dir_all(parent)?;
    let data = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer(data.as_file(), root)?;
    data.persist(file)?;
    Ok(())
}

// Run pickers and disk work off the WebView event loop.
#[tauri::command]
pub async fn vault_action(app: AppHandle, action: Action) -> Result<Reply, String> {
    let owned = app.clone();
    tauri::async_runtime::spawn_blocking(move || run(&owned, action))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn vault_cancel(state: State<'_, VaultState>) {
    state.cancel.store(true, Ordering::Relaxed);
}

fn choose(app: &AppHandle, folder: bool) -> Result<Option<PathBuf>> {
    let dialog = app.dialog().file();
    let selected = if folder {
        dialog.blocking_pick_folder()
    } else {
        dialog.blocking_pick_file()
    };
    selected
        .map(|path| path.into_path().map_err(Into::into))
        .transpose()
}

fn run(app: &AppHandle, action: Action) -> Result<Reply> {
    let state = app.state::<VaultState>();
    let mut session = state
        .session
        .lock()
        .map_err(|_| anyhow::anyhow!("Vault session is unavailable."))?;
    if session
        .as_ref()
        .is_some_and(|(_, last)| last.elapsed() >= IDLE_LIMIT)
    {
        *session = None;
    }
    state.cancel.store(false, Ordering::Relaxed);
    let mut root = session
        .as_ref()
        .map(|(vault, _)| vault.root.clone())
        .or_else(|| remembered(app).ok().flatten());
    let mut reply = Reply {
        uninstall: state.launch.removing(),
        ..Reply::default()
    };

    // Installer actions and standard Vault actions use the same selected location.
    let is_status = matches!(action, Action::Status);
    match action {
        Action::Status => {}
        Action::Locate => {
            if let Some(selected) = choose(app, true)? {
                let selected = store::inspect(&selected)?;
                remember(app, &selected)?;
                *session = None;
                root = Some(selected);
            }
        }
        Action::Create { password } => {
            if let Some(parent) = choose(app, true)? {
                let install_dir = std::env::current_exe()?
                    .parent()
                    .context("Missing app folder.")?
                    .canonicalize()?;
                let parent = parent.canonicalize()?;
                let app_data = [app.path().app_local_data_dir()?, app.path().app_data_dir()?];
                ensure!(
                    !parent.starts_with(&install_dir)
                        && !app_data
                            .iter()
                            .any(|dir| dir.canonicalize().is_ok_and(|dir| parent.starts_with(dir))),
                    "Choose a location outside the app’s install and data folders."
                );
                let vault = Vault::create(&parent, password)?;
                remember(app, &vault.root)?;
                root = Some(vault.root.clone());
                *session = Some((vault, Instant::now()));
            }
        }
        Action::Open { password } => {
            let path = root.as_ref().context("Choose an existing Vault first.")?;
            *session = Some((Vault::open(path, password)?, Instant::now()));
        }
        Action::Lock => *session = None,
        Action::Import { folder, remove } => {
            let (vault, _) = session.as_mut().context("Unlock Vault first.")?;
            if let Some(source) = choose(app, folder)? {
                reply.remaining = vault.import(&source, remove, &state.cancel)?;
            }
        }
        Action::Export { id } => {
            let (vault, _) = session.as_ref().context("Unlock Vault first.")?;
            if let Some(parent) = choose(app, true)? {
                reply.output = Some(display(&vault.export(id, &parent, &state.cancel)?));
            }
        }
        Action::Delete { confirmed } => {
            let path = store::inspect(root.as_ref().context("Choose a Vault first.")?)?;
            ensure!(confirmed == display(&path), "Confirm the exact Vault path.");
            *session = None;
            store::delete(&path)?;
            let _ = fs::remove_file(preference(app)?);
            root = None;
        }
        Action::FinishUninstall { proceed } => {
            ensure!(state.launch.removing(), "No installer is waiting.");
            *session = None;
            finish_uninstall(app, &state.launch, proceed)?;
        }
    }

    if let Some((vault, last)) = session.as_mut() {
        if !is_status {
            *last = Instant::now();
        }
        reply.entries = Some(vault.entries().to_vec());
    }
    reply.path = root.as_deref().map(display);
    Ok(reply)
}

fn display(path: &std::path::Path) -> String {
    let value = path.to_string_lossy();
    if let Some(network) = value.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{network}")
    } else {
        value.strip_prefix(r"\\?\").unwrap_or(&value).to_owned()
    }
}

// MSI Settings entries call this app first, then launch native MSI removal on consent.
fn finish_uninstall(app: &AppHandle, launch: &Launch, proceed: bool) -> Result<()> {
    if proceed {
        if let Some(product) = &launch.msi {
            let system = std::env::var_os("SystemRoot")
                .context("Windows system location is unavailable.")?;
            std::process::Command::new(PathBuf::from(system).join("System32").join("msiexec.exe"))
                .args(["/x", product])
                .spawn()?;
        }
    }
    app.exit(if proceed && launch.nsis {
        42
    } else if proceed {
        0
    } else {
        2
    });
    Ok(())
}
