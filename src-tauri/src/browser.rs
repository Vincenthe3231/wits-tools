//! Keeps browser commands separate from workspace and remote page permissions.

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

#[cfg(windows)]
mod windows_host;

pub const LABEL: &str = "private-browser";

// Only the bundled browser toolbar can send these controls.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Action {
    Read,
    Bounds { top: i32 },
    OpenTab,
    Select { id: u32 },
    CloseTab { id: u32 },
    Navigate { address: String },
    Back,
    Forward,
    Reload,
    FocusPage { backwards: bool },
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TabState {
    id: u32,
    address: String,
    title: String,
    can_back: bool,
    can_forward: bool,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserState {
    tabs: Vec<TabState>,
    active: Option<u32>,
    pending: bool,
    downloads: usize,
    notice: Option<&'static str>,
}

// Running the native work on the desktop thread keeps WebView2 COM references on their apartment.
pub fn recover(app: &AppHandle) {
    #[cfg(windows)]
    windows_host::recover(app);
    #[cfg(not(windows))]
    let _ = app;
}

#[tauri::command]
pub async fn browser_open(app: AppHandle) -> Result<(), String> {
    #[cfg(windows)]
    return windows_host::open(app).await;
    #[cfg(not(windows))]
    {
        let _ = app;
        Err("The private browser requires Windows.".into())
    }
}

#[tauri::command]
pub async fn browser_toolbar(app: AppHandle, action: Action) -> Result<BrowserState, String> {
    #[cfg(windows)]
    return windows_host::toolbar(app, action).await;
    #[cfg(not(windows))]
    {
        let _ = (app, action);
        Err("The private browser requires Windows.".into())
    }
}

// Main-window close collects consent before the editor can discard any work.
#[tauri::command]
pub async fn browser_prepare_close(app: AppHandle) -> Result<bool, String> {
    #[cfg(windows)]
    return windows_host::prepare_app_close(app).await;
    #[cfg(not(windows))]
    {
        let _ = app;
        Ok(true)
    }
}

// If the editor cancels shutdown, let the browser accept close requests again.
#[tauri::command]
pub async fn browser_cancel_close(app: AppHandle) -> Result<(), String> {
    #[cfg(windows)]
    return windows_host::cancel_close(app).await;
    #[cfg(not(windows))]
    {
        let _ = app;
        Ok(())
    }
}

#[tauri::command]
pub async fn browser_finish_close(app: AppHandle) -> Result<(), String> {
    #[cfg(windows)]
    return windows_host::finish_close(app).await;
    #[cfg(not(windows))]
    {
        let _ = app;
        Ok(())
    }
}
