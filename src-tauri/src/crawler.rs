//! Shares crawler and media operations between the desktop and command-line tools.

pub mod capture;
pub(crate) mod commands;
pub mod crawl;
pub mod media;
pub mod proxy;
pub mod storage;

mod browser;
mod network;

use std::path::PathBuf;

use serde::Serialize;

/// Expose a stable error code while keeping native diagnostics and credentials behind IPC.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(tag = "code", rename_all = "camelCase")]
pub enum Error {
    InvalidConfig,
    InvalidUrl,
    InvalidProxy,
    NotFound,
    Busy,
    Storage,
    BrowserUnavailable,
    BrowserFailed,
    Network,
    UnsupportedMedia,
    InvalidManifest,
    OutputConflict,
    File,
    Cancelled,
    CaptureUnavailable,
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// Resolve the same per-user data directory for the installed CLI and desktop app.
pub fn data_dir() -> Result<PathBuf> {
    let base = std::env::var_os(if cfg!(windows) {
        "LOCALAPPDATA"
    } else {
        "XDG_DATA_HOME"
    })
    .map(PathBuf::from)
    .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
    .ok_or(Error::Storage)?;
    Ok(base.join("com.user.wits-tools").join("crawler"))
}

/// Resolve generated browser resources relative to an installed executable.
pub fn browser_path() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    exe.with_file_name("crawler-browser").join("obscura.exe")
}

/// Keep browser-independent HTTP URL validation consistent across all entry points.
pub fn http_url(value: &str) -> Result<url::Url> {
    let mut url = url::Url::parse(value.trim()).map_err(|_| Error::InvalidUrl)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(Error::InvalidUrl);
    }
    url.set_fragment(None);
    Ok(url)
}
