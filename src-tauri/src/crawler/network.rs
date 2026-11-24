//! Builds bounded HTTP clients and transfers response bodies without buffering entire files.

use std::{path::Path, time::Duration};

use reqwest::{Client, Response, header};

use url::Url;

use super::{Error, Result};

/// Parse only the upstream proxy protocols supported by both crawler stages.
pub(super) fn proxy(value: Option<&str>) -> Result<Option<Url>> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            // A DNS resolver is an alternative route configuration, never a forward proxy URL.
            if let Some(resolver) = super::dns::endpoint(value) {
                return resolver;
            }
            let url = Url::parse(value).map_err(|_| Error::InvalidProxy)?;
            if !matches!(url.scheme(), "http" | "https" | "socks5" | "socks5h")
                || url.host().is_none()
                || url.query().is_some()
                || url.fragment().is_some()
                || !matches!(url.path(), "" | "/")
            {
                return Err(Error::InvalidProxy);
            }
            Ok(url)
        })
        .transpose()
}

/// Use system trust and retain the configured proxy for every request and redirect.
pub(super) fn client(proxy: Option<&Url>, follow_redirects: bool) -> Result<Client> {
    let mut builder = Client::builder()
        .tls_backend_native()
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(30))
        .user_agent("WITS-Tools/0.1")
        .redirect(if follow_redirects {
            reqwest::redirect::Policy::limited(10)
        } else {
            reqwest::redirect::Policy::none()
        });
    if let Some(proxy) = proxy {
        builder = if proxy.scheme() == "doh" {
            builder
                .no_proxy()
                .dns_resolver(super::dns::Doh::new(proxy.clone())?)
        } else {
            builder.proxy(reqwest::Proxy::all(proxy.as_str()).map_err(|_| Error::InvalidProxy)?)
        };
    }
    builder.build().map_err(|_| Error::Network)
}

/// Keep a strict upper bound on text responses such as manifests and robots.txt.
pub(super) async fn limited(mut response: Response, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| Error::Network)? {
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            return Err(Error::InvalidManifest);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

// Preserve Crawler's error codes while using the shared streaming implementation.
pub(super) async fn save(response: Response, path: &Path, progress: impl Fn(u64)) -> Result<u64> {
    crate::http_transfer::save(response, path, progress)
        .await
        .map_err(|error| match error {
            crate::http_transfer::BodyError::File => Error::File,
            crate::http_transfer::BodyError::Network => Error::Network,
        })
}

/// Copy only request context required for replay, excluding hop-by-hop and managed headers.
pub(super) fn replay_headers(values: &[(String, String)], credentials: bool) -> header::HeaderMap {
    let mut headers = header::HeaderMap::new();
    for (name, value) in values {
        let Ok(name) = header::HeaderName::from_bytes(name.as_bytes()) else {
            continue;
        };
        // Treat Content-Type as origin-scoped when replaying a captured media request.
        let permitted = matches!(
            name.as_str(),
            "referer" | "origin" | "user-agent" | "accept"
        ) || credentials
            && matches!(name.as_str(), "cookie" | "authorization" | "content-type");
        if permitted && let Ok(mut value) = header::HeaderValue::from_str(value) {
            value.set_sensitive(matches!(name.as_str(), "cookie" | "authorization"));
            headers.insert(name, value);
        }
    }
    headers
}
