//! Sends one API Tester request and returns bounded response data to the desktop UI.
use std::time::{Duration, Instant};

use reqwest::header::{
    CONTENT_ENCODING, CONTENT_LENGTH, HeaderMap, HeaderName, HeaderValue, TRANSFER_ENCODING,
};
use reqwest::{Client, Method, Response, Url, redirect, retry};
use serde::{Deserialize, Serialize};

// Bound the whole exchange and stop reading once a response exceeds the preview budget.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const BODY_PREVIEW_LIMIT: usize = 1024 * 1024;

// Use pairs so repeated headers survive both sides of the IPC boundary.
type Header = (String, String);

#[derive(Deserialize)]
pub(crate) struct ApiRequest {
    method: String,
    url: String,
    headers: Vec<Header>,
    body: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiResponse {
    status: u16,
    headers: Vec<Header>,
    time_to_headers_ms: u64,
    body: BodyPreview,
}

// A truncated preview can also be unsupported, so track the limit separately from the content.
#[derive(Serialize)]
struct BodyPreview {
    truncated: bool,
    #[serde(flatten)]
    content: BodyContent,
}

#[derive(Serialize)]
#[serde(tag = "kind", content = "text", rename_all = "camelCase")]
enum BodyContent {
    Empty,
    Text(String),
    Unsupported,
}

// Return safe identifiers and field locations without leaking URLs or header values in errors.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "code", rename_all = "camelCase")]
pub(crate) enum ApiError {
    InvalidMethod,
    InvalidUrl,
    InvalidHeaderName { index: usize },
    InvalidHeaderValue { index: usize },
    ManagedHeader { index: usize },
    Timeout,
    ConnectionFailed,
    ResponseFailed,
    Unexpected,
}

// Delegate to the transport without adding behavior specific to Tauri's invocation mechanism.
#[tauri::command]
pub(crate) async fn api_tester_send(request: ApiRequest) -> Result<ApiResponse, ApiError> {
    send(request, REQUEST_TIMEOUT).await
}

// Validate the IPC input independently and let the HTTP client manage message framing.
fn request_headers(headers: Vec<Header>) -> Result<HeaderMap, ApiError> {
    let mut map = HeaderMap::new();
    for (index, (name, value)) in headers.into_iter().enumerate() {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| ApiError::InvalidHeaderName { index })?;
        if name == CONTENT_LENGTH || name == TRANSFER_ENCODING {
            return Err(ApiError::ManagedHeader { index });
        }
        let mut value =
            HeaderValue::from_str(&value).map_err(|_| ApiError::InvalidHeaderValue { index })?;
        // Headers may contain app-specific credentials, so make all values opaque to debug output.
        value.set_sensitive(true);
        map.append(name, value);
    }
    Ok(map)
}

// Build a fresh client for each send so it cannot reuse connections or state from another request.
async fn send(request: ApiRequest, timeout: Duration) -> Result<ApiResponse, ApiError> {
    let method =
        Method::from_bytes(request.method.as_bytes()).map_err(|_| ApiError::InvalidMethod)?;
    let url = Url::parse(request.url.trim()).map_err(|_| ApiError::InvalidUrl)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(ApiError::InvalidUrl);
    }
    let headers = request_headers(request.headers)?;
    let client = Client::builder()
        .tls_backend_native()
        .timeout(timeout)
        .redirect(redirect::Policy::none())
        .retry(retry::never())
        .build()
        .map_err(|_| ApiError::Unexpected)?;
    let mut builder = client.request(method, url).headers(headers);
    if let Some(body) = request.body {
        builder = builder.body(body);
    }

    // Capture timing at the headers so the same metric applies to every preview size.
    let start = Instant::now();
    let mut response = builder.send().await.map_err(transport_error)?;
    let time_to_headers_ms = start.elapsed().as_micros().div_ceil(1000) as u64;
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.to_string(),
                String::from_utf8_lossy(value.as_bytes()).into_owned(),
            )
        })
        .collect();
    let body = preview(&mut response).await?;
    Ok(ApiResponse {
        status,
        headers,
        time_to_headers_ms,
        body,
    })
}

// Convert network and response failures to stable codes without retaining diagnostics.
fn transport_error(error: reqwest::Error) -> ApiError {
    if error.is_timeout() {
        ApiError::Timeout
    } else if error.is_connect() {
        ApiError::ConnectionFailed
    } else {
        ApiError::ResponseFailed
    }
}

// Read only enough to distinguish a body that fits exactly from one that exceeds the limit.
async fn preview(response: &mut Response) -> Result<BodyPreview, ApiError> {
    let encoded = response
        .headers()
        .get_all(CONTENT_ENCODING)
        .iter()
        .any(|value| !value.as_bytes().eq_ignore_ascii_case(b"identity"));
    let mut bytes = Vec::new();
    let mut truncated = false;
    while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
        let remaining = BODY_PREVIEW_LIMIT - bytes.len();
        bytes.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
        if chunk.len() > remaining {
            truncated = true;
            break;
        }
    }

    // Encoded and binary payloads have no plain-text preview, but still have response metadata.
    let content = if bytes.is_empty() {
        BodyContent::Empty
    } else if encoded {
        BodyContent::Unsupported
    } else {
        text_content(bytes, truncated)
    };
    Ok(BodyPreview { truncated, content })
}

// A size cutoff can split a UTF-8 character, so retain its complete prefix.
fn text_content(mut bytes: Vec<u8>, truncated: bool) -> BodyContent {
    if bytes.contains(&0) {
        return BodyContent::Unsupported;
    }
    if truncated {
        if let Err(error) = std::str::from_utf8(&bytes) {
            if error.error_len().is_none() {
                bytes.truncate(error.valid_up_to());
            }
        }
    }
    match String::from_utf8(bytes) {
        Ok(text) => BodyContent::Text(text),
        Err(_) => BodyContent::Unsupported,
    }
}

#[cfg(test)]
mod tests;
