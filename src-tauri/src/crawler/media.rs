//! Discovers media sources and streams selected files or ordered segments to disk.

mod manifest;

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use aes::Aes128;
use cbc::cipher::{BlockModeDecrypt, KeyIvInit, block_padding::Pkcs7};
use futures_util::{StreamExt, stream};
use reqwest::{Client, Response, header};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;
use url::Url;
use uuid::Uuid;

use super::{
    Error, Result,
    browser::{Browser, Resource},
    crawl::local_seed,
    http_url, network,
};
use manifest::{Segment, Variant};

const MANIFEST_LIMIT: usize = 8 * 1024 * 1024;
const ENCRYPTED_SEGMENT_LIMIT: usize = 64 * 1024 * 1024;
const RANGE_CHUNK: u64 = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Direct,
    Hls,
    Dash,
}

/// Return only the display metadata to the frontend. Headers and signed URLs stay in Rust.
#[derive(Clone, Serialize)]
pub struct Source {
    pub id: Uuid,
    pub url: String,
    pub kind: Kind,
    pub segments: usize,
}

#[derive(Clone)]
pub struct Candidate {
    url: Url,
    kind: Kind,
    headers: Vec<(String, String)>,
    proxy: Option<String>,
    segments: usize,
}

impl Candidate {
    /// Classify the main source; a loose transport stream segment is never a manifest.
    pub(super) fn new(
        url: &str,
        mime: &str,
        headers: Vec<(String, String)>,
        proxy: Option<String>,
    ) -> Result<Self> {
        let url = http_url(url)?;
        let kind = classify(&url, mime).ok_or(Error::UnsupportedMedia)?;
        Ok(Self {
            url,
            kind,
            headers,
            proxy,
            segments: 0,
        })
    }

    pub fn summary(&self, id: Uuid) -> Source {
        let mut url = self.url.clone();
        url.set_query(None);
        Source {
            id,
            url: url.to_string(),
            kind: self.kind,
            segments: self.segments,
        }
    }
}

/// Detect manifests and media files using both the response MIME type and URL suffix.
pub fn classify(url: &Url, mime: &str) -> Option<Kind> {
    let path = url.path().to_ascii_lowercase();
    let mime = mime
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if path.ends_with(".m3u8")
        || matches!(
            mime.as_str(),
            "application/vnd.apple.mpegurl"
                | "application/x-mpegurl"
                | "audio/mpegurl"
                | "audio/x-mpegurl"
        )
    {
        Some(Kind::Hls)
    } else if path.ends_with(".mpd") || mime == "application/dash+xml" {
        Some(Kind::Dash)
    } else if path.ends_with(".m4s") || path.ends_with(".ts") || mime == "video/mp2t" {
        None
    } else if mime.starts_with("video/")
        || mime.starts_with("audio/")
        || [
            ".mp4", ".webm", ".mov", ".mkv", ".mp3", ".m4a", ".ogg", ".wav", ".flac", ".aac",
        ]
        .iter()
        .any(|extension| path.ends_with(extension))
    {
        Some(Kind::Direct)
    } else {
        None
    }
}

/// Discover script-driven requests and associate observed segments with nearby manifests.
pub async fn discover(
    executable: &Path,
    page: &str,
    proxy: Option<String>,
) -> Result<Vec<Candidate>> {
    let url = http_url(page)?;
    let endpoint = network::proxy(proxy.as_deref())?;
    let mut browser =
        Browser::start(executable, endpoint.as_ref(), false, local_seed(&url)).await?;
    let result = browser.page(&url, true).await;
    browser.shutdown().await;
    Ok(group(result?.resources, proxy))
}

fn group(resources: Vec<Resource>, proxy: Option<String>) -> Vec<Candidate> {
    let mut urls = HashSet::new();
    let mut sources = Vec::new();
    let mut segments = Vec::new();
    for resource in resources {
        if !urls.insert(resource.url.clone()) || resource.status >= 400 {
            continue;
        }
        if let Ok(candidate) = Candidate::new(
            &resource.url,
            &resource.mime,
            resource.headers,
            proxy.clone(),
        ) {
            sources.push(candidate);
        } else if let Ok(url) = http_url(&resource.url)
            && (url.path().ends_with(".m4s") || url.path().ends_with(".ts"))
        {
            segments.push(url);
        }
    }
    for segment in segments {
        if let Some(source) = sources
            .iter_mut()
            .filter(|source| source.kind != Kind::Direct && source.url.origin() == segment.origin())
            .filter_map(|source| {
                let directory = source.url.path().rsplit_once('/')?.0;
                segment
                    .path()
                    .strip_prefix(directory)?
                    .starts_with('/')
                    .then_some((directory.len(), source))
            })
            .max_by_key(|(length, _)| *length)
        {
            source.1.segments += 1;
        }
    }
    sources.sort_by(|left, right| left.url.as_str().cmp(right.url.as_str()));
    sources
}

/// A desktop or CLI request can supply its own direct URL or use an observed source.
#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Download {
    pub url: String,
    pub folder: Option<PathBuf>,
    pub filename: Option<String>,
    pub format: Option<String>,
    pub proxy: Option<String>,
    pub workers: Option<usize>,
    pub overwrite: bool,
    pub headers: Vec<(String, String)>,
}

#[derive(Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub bytes: u64,
    pub total_bytes: Option<u64>,
    pub segments: u32,
    pub total_segments: Option<u32>,
}

#[derive(Serialize)]
pub struct Info {
    pub kind: Kind,
    pub formats: Vec<Format>,
    pub bytes: Option<u64>,
}

#[derive(Clone, Serialize)]
pub struct Format {
    pub id: String,
    pub bandwidth: u64,
}

struct Transfer {
    client: Client,
    source: Url,
    headers: header::HeaderMap,
    workers: usize,
}

/// Inspect the same manifest formats accepted by the downloader.
pub async fn info(request: &Download, candidate: Option<Candidate>) -> Result<Info> {
    let (transfer, hint) = Transfer::new(request, candidate)?;
    let (response, kind) = transfer.response(hint).await?;
    let url = response.url().clone();
    let bytes = response.content_length();
    let formats = match kind {
        Kind::Direct => Vec::new(),
        Kind::Hls => manifest::hls(&network::limited(response, MANIFEST_LIMIT).await?, &url)?
            .variants
            .into_iter()
            .map(|entry| entry.format)
            .collect(),
        Kind::Dash => manifest::dash(&network::limited(response, MANIFEST_LIMIT).await?, &url)?
            .into_iter()
            .map(|entry| entry.format)
            .collect(),
    };
    Ok(Info {
        kind,
        formats,
        bytes,
    })
}

/// Publish only complete files and leave the existing destination intact on failure or cancel.
pub async fn download(
    request: Download,
    candidate: Option<Candidate>,
    default_folder: PathBuf,
    cancellation: CancellationToken,
    progress: impl Fn(Progress) + Send + Sync,
) -> Result<PathBuf> {
    let (transfer, hint) = Transfer::new(&request, candidate)?;
    let prepared = async {
        let (response, kind) = transfer.response(hint).await?;
        let final_url = response.url().clone();
        let folder = request.folder.unwrap_or(default_folder);
        tokio::fs::create_dir_all(&folder)
            .await
            .map_err(|_| Error::File)?;
        let name = output_name(&final_url, kind, request.filename.as_deref());
        let output = folder.join(name);
        // Lock a separate reservation file to avoid simultaneous writers across app and CLI.
        let digest = blake3::hash(
            output
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase()
                .as_bytes(),
        );
        let reservation = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(folder.join(format!(".wits-download-{digest}.lock")))
            .map_err(|_| Error::File)?;
        reservation.try_lock().map_err(|_| Error::OutputConflict)?;
        if output.exists() && !request.overwrite {
            return Err(Error::OutputConflict);
        }
        let file = tempfile::NamedTempFile::new_in(&folder)
            .map_err(|_| Error::File)?
            .into_temp_path();
        let format = request.format.as_deref().unwrap_or("best");
        match kind {
            Kind::Direct => transfer.direct(response, file.as_ref(), &progress).await?,
            Kind::Hls => {
                let playlist = manifest::hls(
                    &network::limited(response, MANIFEST_LIMIT).await?,
                    &final_url,
                )?;
                let segments = if playlist.variants.is_empty() {
                    playlist.segments
                } else {
                    let selected = select(&playlist.variants, format)?;
                    let url = selected.url.as_ref().ok_or(Error::InvalidManifest)?;
                    let response = transfer.fetch(url, None).await?;
                    if !response.status().is_success() {
                        return Err(Error::Network);
                    }
                    let base = response.url().clone();
                    manifest::hls(&network::limited(response, MANIFEST_LIMIT).await?, &base)?
                        .segments
                };
                transfer
                    .segments(segments, file.as_ref(), &progress)
                    .await?;
            }
            Kind::Dash => {
                let variants = manifest::dash(
                    &network::limited(response, MANIFEST_LIMIT).await?,
                    &final_url,
                )?;
                transfer
                    .segments(
                        select(&variants, format)?.segments.clone(),
                        file.as_ref(),
                        &progress,
                    )
                    .await?;
            }
        }
        Ok((file, output, reservation))
    };
    let (file, output, _reservation) = tokio::select! {
        biased;
        _ = cancellation.cancelled() => return Err(Error::Cancelled),
        result = prepared => result?,
    };
    if cancellation.is_cancelled() {
        return Err(Error::Cancelled);
    }
    if request.overwrite {
        file.persist(&output).map_err(|_| Error::File)?;
    } else {
        file.persist_noclobber(&output).map_err(|error| {
            if error.error.kind() == std::io::ErrorKind::AlreadyExists {
                Error::OutputConflict
            } else {
                Error::File
            }
        })?;
    }
    Ok(output)
}

impl Transfer {
    fn new(request: &Download, candidate: Option<Candidate>) -> Result<(Self, Option<Kind>)> {
        let (source, hint, headers, proxy) = if let Some(candidate) = candidate {
            (
                candidate.url,
                Some(candidate.kind),
                candidate.headers,
                candidate.proxy,
            )
        } else {
            let url = http_url(&request.url)?;
            let kind = classify(&url, "");
            (url, kind, request.headers.clone(), request.proxy.clone())
        };
        let proxy = network::proxy(proxy.as_deref())?;
        let mut headers = network::replay_headers(&headers, true);
        if hint.is_none() || !request.headers.is_empty() {
            for (name, value) in &request.headers {
                let name = header::HeaderName::from_bytes(name.as_bytes())
                    .map_err(|_| Error::InvalidConfig)?;
                if matches!(
                    name.as_str(),
                    "host"
                        | "range"
                        | "content-length"
                        | "transfer-encoding"
                        | "connection"
                        | "proxy-authorization"
                ) {
                    continue;
                }
                let mut value =
                    header::HeaderValue::from_str(value).map_err(|_| Error::InvalidConfig)?;
                value.set_sensitive(true);
                headers.insert(name, value);
            }
        }
        Ok((
            Self {
                client: network::client(proxy.as_ref(), false)?,
                source,
                headers,
                workers: request.workers.unwrap_or(4).clamp(1, 16),
            },
            hint,
        ))
    }

    /// Replay credentials only on requests for the selected origin, including manifest children.
    fn get(&self, url: &Url) -> reqwest::RequestBuilder {
        let mut headers = self.headers.clone();
        if self.source.origin() != url.origin() {
            headers = headers
                .iter()
                .filter(|(name, _)| {
                    matches!(
                        name.as_str(),
                        "referer" | "origin" | "user-agent" | "accept"
                    )
                })
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect();
        }
        self.client.get(url.clone()).headers(headers)
    }

    /// Re-evaluate credential scope on every redirect and keep range requests intact.
    async fn fetch(&self, url: &Url, range: Option<(u64, u64)>) -> Result<Response> {
        let mut url = url.clone();
        for _ in 0..=10 {
            let mut request = self.get(&url);
            if let Some((start, end)) = range {
                request = request.header(header::RANGE, format!("bytes={start}-{end}"));
            }
            let response = request.send().await.map_err(|_| Error::Network)?;
            if !matches!(response.status().as_u16(), 301 | 302 | 303 | 307 | 308) {
                return Ok(response);
            }
            let target = response
                .headers()
                .get(header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| url.join(value).ok())
                .ok_or(Error::Network)?;
            url = http_url(target.as_str())?;
        }
        Err(Error::Network)
    }

    async fn response(&self, hint: Option<Kind>) -> Result<(Response, Kind)> {
        let response = self.fetch(&self.source, None).await?;
        if !response.status().is_success() {
            return Err(Error::Network);
        }
        let mime = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        let kind = classify(response.url(), mime)
            .or(hint)
            .unwrap_or(Kind::Direct);
        if kind == Kind::Direct
            && (mime.contains("text/html") || mime.contains("application/xhtml+xml"))
        {
            return Err(Error::UnsupportedMedia);
        }
        Ok((response, kind))
    }

    /// Probe actual range support and validate each partial response before assembly.
    async fn direct(
        &self,
        response: Response,
        output: &Path,
        progress: &(impl Fn(Progress) + Sync),
    ) -> Result<()> {
        let total = response.content_length();
        if let Some(total) = total.filter(|total| *total > RANGE_CHUNK && self.workers > 1) {
            let url = response.url().clone();
            if let Ok(probe) = self.fetch(&url, Some((0, 0))).await
                && valid_range(&probe, 0, 0, Some(total))
            {
                drop(response);
                drop(probe);
                let ranges = (0..total)
                    .step_by(RANGE_CHUNK as usize)
                    .map(|start| Segment {
                        url: url.clone(),
                        range: Some((start, start.saturating_add(RANGE_CHUNK - 1).min(total - 1))),
                        encryption: None,
                    });
                return self
                    .assemble(ranges, output, Some(total), None, progress)
                    .await;
            }
        }
        network::save(response, output, |bytes| {
            progress(Progress {
                bytes,
                total_bytes: total,
                ..Progress::default()
            })
        })
        .await?;
        Ok(())
    }

    async fn segments(
        &self,
        segments: Vec<Segment>,
        output: &Path,
        progress: &(impl Fn(Progress) + Sync),
    ) -> Result<()> {
        if segments.is_empty() {
            return Err(Error::InvalidManifest);
        }
        let count = segments.len() as u32;
        self.assemble(segments, output, None, Some(count), progress)
            .await
    }

    /// Download a bounded window in parallel, then append disk-backed parts in manifest order.
    async fn assemble(
        &self,
        parts: impl IntoIterator<Item = Segment>,
        output: &Path,
        total: Option<u64>,
        count: Option<u32>,
        progress: &(impl Fn(Progress) + Sync),
    ) -> Result<()> {
        let parts = parts.into_iter();
        let workspace =
            tempfile::tempdir_in(output.parent().ok_or(Error::File)?).map_err(|_| Error::File)?;
        let downloads = stream::iter(parts.enumerate())
            .map(|(index, segment)| {
                let path = workspace.path().join(index.to_string());
                async move {
                    let bytes = self.part(&segment, &path, total).await?;
                    Ok::<_, Error>((path, bytes))
                }
            })
            .buffered(self.workers);
        tokio::pin!(downloads);
        let mut file = tokio::fs::File::create(output)
            .await
            .map_err(|_| Error::File)?;
        let mut completed = 0;
        let mut saved_bytes = 0;
        while let Some(part) = downloads.next().await {
            let (path, bytes) = part?;
            let mut part = tokio::fs::File::open(&path)
                .await
                .map_err(|_| Error::File)?;
            tokio::io::copy(&mut part, &mut file)
                .await
                .map_err(|_| Error::File)?;
            drop(part);
            tokio::fs::remove_file(path)
                .await
                .map_err(|_| Error::File)?;
            completed += 1;
            saved_bytes += bytes;
            progress(Progress {
                bytes: saved_bytes,
                total_bytes: total,
                segments: completed,
                total_segments: count,
            });
        }
        file.flush().await.map_err(|_| Error::File)?;
        file.sync_all().await.map_err(|_| Error::File)
    }

    async fn part(&self, segment: &Segment, path: &Path, total: Option<u64>) -> Result<u64> {
        let response = self.fetch(&segment.url, segment.range).await?;
        if !response.status().is_success()
            || segment
                .range
                .is_some_and(|(start, end)| !valid_range(&response, start, end, total))
        {
            return Err(Error::Network);
        }
        if let Some(encryption) = &segment.encryption {
            let key = self.fetch(&encryption.key, None).await?;
            if !key.status().is_success() {
                return Err(Error::Network);
            }
            let key: [u8; 16] = network::limited(key, 16)
                .await?
                .try_into()
                .map_err(|_| Error::InvalidManifest)?;
            let mut body = network::limited(response, ENCRYPTED_SEGMENT_LIMIT).await?;
            let body = cbc::Decryptor::<Aes128>::new(&key.into(), &encryption.iv.into())
                .decrypt_padded::<Pkcs7>(&mut body)
                .map_err(|_| Error::InvalidManifest)?;
            tokio::fs::write(path, body)
                .await
                .map_err(|_| Error::File)?;
            return Ok(body.len() as u64);
        }
        let bytes = network::save(response, path, |_| {}).await?;
        if segment
            .range
            .is_some_and(|(start, end)| bytes != end - start + 1)
        {
            return Err(Error::Network);
        }
        Ok(bytes)
    }
}

fn valid_range(response: &Response, start: u64, end: u64, total: Option<u64>) -> bool {
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

fn select<'a>(variants: &'a [Variant], format: &str) -> Result<&'a Variant> {
    match format {
        "best" | "" => variants.iter().max_by_key(|entry| entry.format.bandwidth),
        "worst" => variants.iter().min_by_key(|entry| entry.format.bandwidth),
        id => variants.iter().find(|entry| entry.format.id == id),
    }
    .ok_or(Error::InvalidManifest)
}

/// Normalize filesystem names and avoid reserved Windows devices and trailing dots.
pub(super) fn filename(value: &str) -> String {
    let name: String = value
        .chars()
        .take(150)
        .map(|character| {
            if character.is_control() || "<>:\"/\\|?*".contains(character) {
                '_'
            } else {
                character
            }
        })
        .collect();
    let name = name.trim_matches(['.', ' ']);
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    if name.is_empty()
        || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.len() == 4
    {
        format!("file-{name}")
    } else {
        name.to_owned()
    }
}

fn output_name(url: &Url, kind: Kind, custom: Option<&str>) -> String {
    if let Some(name) = custom.filter(|name| !name.trim().is_empty()) {
        return filename(name);
    }
    let raw = url
        .path_segments()
        .and_then(|mut parts| parts.next_back())
        .filter(|name| !name.is_empty())
        .unwrap_or("download");
    let safe = filename(raw);
    match kind {
        Kind::Direct => safe,
        Kind::Hls | Kind::Dash => {
            let stem = Path::new(&safe)
                .file_stem()
                .and_then(|part| part.to_str())
                .unwrap_or(&safe);
            format!("{stem}.{}", if kind == Kind::Hls { "ts" } else { "mp4" })
        }
    }
}
