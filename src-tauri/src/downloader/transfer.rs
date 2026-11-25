//! Classifies general file responses and coordinates verified HTTP ranges.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use reqwest::{
    Client, Response, StatusCode,
    header::{self, HeaderMap, HeaderValue},
};
use tokio_util::sync::CancellationToken;
use url::Url;

use super::{Error, Mode, Options, Result};
use crate::http_transfer::{self, output};

const CHUNK: u64 = 8 * 1024 * 1024;

// Replay data travels only between native jobs and the capture session.
#[derive(Clone)]
pub(super) struct Replay {
    pub url: Url,
    pub headers: HeaderMap,
    pub length: u64,
    pub etag: HeaderValue,
}

// Use a no-redirect client for replay so no captured header reaches another origin.
fn client(redirects: bool) -> Result<Client> {
    Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(30))
        .user_agent("WITS-Tools/0.1")
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .redirect(if redirects {
            reqwest::redirect::Policy::limited(10)
        } else {
            reqwest::redirect::Policy::none()
        })
        .build()
        .map_err(|_| Error::Network)
}

// Distinguish failures that require action from unsupported range behavior.
fn status(response: &Response) -> Result<()> {
    match response.status() {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(Error::AccessDenied),
        StatusCode::TOO_MANY_REQUESTS => Err(Error::RateLimited),
        _ => Ok(()),
    }
}

pub(super) fn strong_etag(tag: &HeaderValue) -> bool {
    let bytes = tag.as_bytes();
    bytes.len() >= 2 && bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"'
}

fn identity(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_ENCODING)
        .is_none_or(|value| value.as_bytes().eq_ignore_ascii_case(b"identity"))
}

// Pin every range to the representation seen in the original response.
async fn range(client: &Client, replay: &Replay, start: u64, end: u64) -> Result<Response> {
    let response = client
        .get(replay.url.clone())
        .headers(replay.headers.clone())
        .header(header::ACCEPT_ENCODING, "identity")
        .header(header::IF_RANGE, &replay.etag)
        .header(header::RANGE, format!("bytes={start}-{end}"))
        .send()
        .await
        .map_err(|_| Error::Network)?;
    status(&response)?;
    if response.url() != &replay.url
        || !identity(response.headers())
        || response.headers().get(header::ETAG) != Some(&replay.etag)
        || !http_transfer::valid_range(&response, start, end, Some(replay.length))
    {
        return Err(Error::RangeChanged);
    }
    Ok(response)
}

// Consume only the expected byte to validate support before giving up the original transfer.
async fn probe(client: &Client, replay: &Replay) -> Result<()> {
    let mut response = range(client, replay, 0, 0).await?;
    let first = response.chunk().await.map_err(|_| Error::Network)?;
    if first.is_some_and(|chunk| chunk.len() == 1)
        && response
            .chunk()
            .await
            .map_err(|_| Error::Network)?
            .is_none()
    {
        Ok(())
    } else {
        Err(Error::RangeChanged)
    }
}

pub(super) async fn can_handoff(replay: &Replay, workers: usize) -> bool {
    if replay.length <= CHUNK || workers < 2 || !strong_etag(&replay.etag) {
        return false;
    }
    let Ok(client) = client(false) else {
        return false;
    };
    tokio::time::timeout(Duration::from_secs(5), probe(&client, replay))
        .await
        .is_ok_and(|result| result.is_ok())
}

// Read the original response once. Keeping it open through the probe permits a single-use stream.
async fn full(client: &Client, url: Url, headers: HeaderMap) -> Result<Response> {
    let response = client
        .get(url)
        .headers(headers)
        .header(header::ACCEPT_ENCODING, "identity")
        .send()
        .await
        .map_err(|_| Error::Network)?;
    status(&response)?;
    if response.status() != StatusCode::OK {
        return Err(Error::Network);
    }
    if !identity(response.headers()) {
        return Err(Error::Encoding);
    }
    let mime = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let disposition = disposition(response.headers());
    if ["text/html", "application/xhtml+xml"].iter().any(|kind| {
        mime.split(';')
            .next()
            .is_some_and(|mime| mime.trim().eq_ignore_ascii_case(kind))
    }) && !disposition.is_some_and(|value| {
        value
            .split(';')
            .next()
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("attachment"))
    }) {
        return Err(Error::Page);
    }
    Ok(response)
}

fn disposition(headers: &HeaderMap) -> Option<&str> {
    headers.get(header::CONTENT_DISPOSITION)?.to_str().ok()
}

// Decode only a single filename component. Path separators never reach the destination join.
fn decode(value: &str) -> Option<String> {
    let mut decoded = Vec::new();
    let mut bytes = value.bytes();
    while let Some(byte) = bytes.next() {
        decoded.push(if byte == b'%' {
            let first = char::from(bytes.next()?).to_digit(16)?;
            let second = char::from(bytes.next()?).to_digit(16)?;
            (first * 16 + second) as u8
        } else {
            byte
        });
    }
    String::from_utf8(decoded).ok()
}

// Parse parameters independently of semicolons inside quoted filenames.
fn disposition_name(value: &str) -> Option<String> {
    let mut filename = None;
    let mut extended = None;
    let mut fields = Vec::new();
    let (mut start, mut quoted, mut escaped) = (0, false, false);
    for (index, character) in value.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match character {
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            ';' if !quoted => {
                fields.push(&value[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    fields.push(&value[start..]);
    for field in fields.into_iter().skip(1) {
        let Some((name, value)) = field.split_once('=') else {
            continue;
        };
        let value = value.trim();
        if name.trim().eq_ignore_ascii_case("filename*") {
            extended = value
                .split_once('\'')
                .and_then(|(charset, rest)| charset.eq_ignore_ascii_case("utf-8").then_some(rest))
                .and_then(|rest| rest.split_once('\''))
                .and_then(|(_, data)| decode(data));
        } else if name.trim().eq_ignore_ascii_case("filename") {
            filename = Some(value.trim_matches('"').replace("\\\"", "\""));
        }
    }
    extended.or(filename)
}

pub(super) fn safe_name(value: &str) -> String {
    let leaf = value
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .trim_matches(['.', ' ']);
    if leaf.is_empty() {
        return "download".into();
    }
    // Bound the Windows UTF-16 component length and avoid special console devices.
    let leaf: String = leaf.chars().take(120).collect();
    let stem = leaf
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_ascii_uppercase();
    if matches!(
        stem.as_str(),
        "CONIN$" | "CONOUT$" | "COM¹" | "COM²" | "COM³" | "LPT¹" | "LPT²" | "LPT³"
    ) {
        return format!("file-{}", crate::crawler::media::filename(&leaf));
    }
    crate::crawler::media::filename(&leaf)
}

fn name(response: &Response, options: &Options) -> String {
    let suggested = disposition(response.headers())
        .and_then(disposition_name)
        .or_else(|| response.url().path_segments()?.next_back().and_then(decode));
    safe_name(
        options
            .filename
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .or(suggested.as_deref())
            .unwrap_or("download"),
    )
}

// Each validated part is capped at its reported range size before it reaches disk.
async fn parallel(
    client: &Client,
    replay: &Replay,
    output: &Path,
    workers: usize,
    progress: &impl Fn(Mode, u64, Option<u64>),
) -> Result<()> {
    let total = replay.length;
    let parts = (0..total)
        .step_by(CHUNK as usize)
        .map(|start| (start, start.saturating_add(CHUNK - 1).min(total - 1)));
    progress(Mode::Parallel, 0, Some(total));
    http_transfer::assemble(
        parts,
        output,
        workers,
        Error::File,
        |(start, end), path| async move {
            let response = range(client, replay, start, end).await?;
            let expected = end - start + 1;
            if response
                .content_length()
                .is_some_and(|size| size != expected)
            {
                return Err(Error::RangeChanged);
            }
            let bytes = http_transfer::save_bounded(response, &path, Some(expected), |_| {})
                .await
                .map_err(shared_error)?;
            if bytes != expected {
                return Err(Error::RangeChanged);
            }
            Ok(bytes)
        },
        |bytes, _| progress(Mode::Parallel, bytes, Some(total)),
    )
    .await
}

fn shared_error(error: http_transfer::BodyError) -> Error {
    match error {
        http_transfer::BodyError::File => Error::File,
        http_transfer::BodyError::Network => Error::Network,
    }
}

pub(super) fn output_error(error: output::Error) -> Error {
    match error {
        output::Error::Conflict => Error::Conflict,
        output::Error::File => Error::File,
    }
}

// Prepare and verify everything before taking the final cancellation and publish decision.
pub(super) async fn run(
    url: Url,
    captured: Option<(Replay, String)>,
    options: Options,
    folder: PathBuf,
    stopped: CancellationToken,
    progress: impl Fn(Mode, u64, Option<u64>) + Sync,
) -> Result<(PathBuf, u64)> {
    let complete = async {
        let direct = client(true)?;
        let pinned = client(false)?;
        let (mut response, replay, name, probed) = if let Some((replay, name)) = captured {
            let name = safe_name(
                options
                    .filename
                    .as_deref()
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or(&name),
            );
            (None, Some(replay), name, true)
        } else {
            let response = full(&direct, url.clone(), HeaderMap::new()).await?;
            let replay = response
                .content_length()
                .filter(|len| *len > CHUNK && options.workers > 1)
                .zip(
                    response
                        .headers()
                        .get(header::ETAG)
                        .filter(|tag| strong_etag(tag)),
                )
                .map(|(length, etag)| Replay {
                    url: response.url().clone(),
                    headers: HeaderMap::new(),
                    length,
                    etag: etag.clone(),
                });
            let name = name(&response, &options);
            (Some(response), replay, name, false)
        };
        let path = folder.join(name);
        let reservation = output::reserve(&path, false).map_err(output_error)?;
        let staged = output::stage(&folder).map_err(output_error)?;
        let accelerated = if let Some(replay) = &replay
            && (probed || can_handoff(replay, options.workers).await)
        {
            // A successful probe lets the unused full response release its connection.
            drop(response.take());
            match parallel(&pinned, replay, &staged, options.workers, &progress).await {
                Ok(()) => true,
                Err(Error::RangeChanged | Error::Network) => false,
                Err(error) => return Err(error),
            }
        } else {
            false
        };
        if !accelerated {
            let response = match (response, replay) {
                (Some(response), _) => response,
                (None, Some(replay)) => full(&pinned, replay.url, replay.headers).await?,
                _ => return Err(Error::Network),
            };
            let total = response.content_length();
            progress(Mode::Single, 0, total);
            http_transfer::save(response, &staged, |bytes| {
                progress(Mode::Single, bytes, total)
            })
            .await
            .map_err(shared_error)?;
        }
        let size = tokio::fs::metadata(&staged)
            .await
            .map_err(|_| Error::File)?
            .len();
        Ok((staged, path, size, reservation))
    };
    let (staged, path, size, _reservation) = tokio::select! {
        biased;
        _ = stopped.cancelled() => return Err(Error::Cancelled),
        result = complete => result?,
    };
    if stopped.is_cancelled() {
        return Err(Error::Cancelled);
    }
    output::publish(staged, &path, false).map_err(output_error)?;
    Ok((path, size))
}
