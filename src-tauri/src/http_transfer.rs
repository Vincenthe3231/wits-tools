//! Streams responses and assembles bounded, disk-backed HTTP ranges for native tools.

use std::{
    future::Future,
    path::{Path, PathBuf},
};

use futures_util::{StreamExt, stream};
use reqwest::{Response, header};
use tokio::io::AsyncWriteExt;

// Report stream failures independently of the calling tool.
pub(crate) enum BodyError {
    Network,
    File,
}

pub(crate) mod output;

// Keep at most one disk-backed part per worker and append parts in input order.
pub(crate) async fn assemble<T, E: Copy, F: Future<Output = std::result::Result<u64, E>>>(
    parts: impl IntoIterator<Item = T>,
    output: &Path,
    workers: usize,
    file_error: E,
    fetch: impl Fn(T, PathBuf) -> F,
    progress: impl Fn(u64, u32),
) -> std::result::Result<(), E> {
    let parts = parts.into_iter();
    let workspace =
        tempfile::tempdir_in(output.parent().ok_or(file_error)?).map_err(|_| file_error)?;
    let downloads = stream::iter(parts.enumerate())
        .map(|(index, segment)| {
            let path = workspace.path().join(index.to_string());
            let download = fetch(segment, path.clone());
            async move {
                let bytes = download.await?;
                Ok::<_, E>((path, bytes))
            }
        })
        .buffered(workers);
    tokio::pin!(downloads);
    let mut file = tokio::fs::File::create(output)
        .await
        .map_err(|_| file_error)?;
    let mut completed = 0;
    let mut saved_bytes = 0;
    while let Some(part) = downloads.next().await {
        let (path, bytes) = part?;
        let mut part = tokio::fs::File::open(&path).await.map_err(|_| file_error)?;
        tokio::io::copy(&mut part, &mut file)
            .await
            .map_err(|_| file_error)?;
        drop(part);
        tokio::fs::remove_file(path).await.map_err(|_| file_error)?;
        completed += 1;
        saved_bytes += bytes;
        progress(saved_bytes, completed);
    }
    file.flush().await.map_err(|_| file_error)?;
    file.sync_all().await.map_err(|_| file_error)
}

/// Copy one complete response into a reserved temporary file and verify any advertised length.
pub(crate) async fn save(
    response: Response,
    path: &Path,
    progress: impl Fn(u64),
) -> Result<u64, BodyError> {
    save_bounded(response, path, None, progress).await
}

// Cap streamed ranges even when a server omits their Content-Length.
pub(crate) async fn save_bounded(
    mut response: Response,
    path: &Path,
    limit: Option<u64>,
    progress: impl Fn(u64),
) -> Result<u64, BodyError> {
    if !response.status().is_success() {
        return Err(BodyError::Network);
    }
    let expected = response.content_length();
    let mut target = tokio::fs::File::create(path)
        .await
        .map_err(|_| BodyError::File)?;
    let mut count = 0;
    while let Some(chunk) = response.chunk().await.map_err(|_| BodyError::Network)? {
        if limit.is_some_and(|limit| chunk.len() as u64 > limit.saturating_sub(count)) {
            return Err(BodyError::Network);
        }
        target
            .write_all(&chunk)
            .await
            .map_err(|_| BodyError::File)?;
        count += chunk.len() as u64;
        progress(count);
    }
    if expected.is_some_and(|expected| expected != count) {
        return Err(BodyError::Network);
    }
    target.sync_all().await.map_err(|_| BodyError::File)?;
    Ok(count)
}

pub(crate) fn valid_range(response: &Response, start: u64, end: u64, total: Option<u64>) -> bool {
    response.status() == reqwest::StatusCode::PARTIAL_CONTENT
        && response
            .headers()
            .get(header::CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("bytes "))
            .and_then(|value| value.split_once('/'))
            .is_some_and(|(range, length)| {
                range == format!("{start}-{end}")
                    && length.parse::<u64>().ok().is_some_and(|length| {
                        length > end && total.is_none_or(|total| total == length)
                    })
            })
}
