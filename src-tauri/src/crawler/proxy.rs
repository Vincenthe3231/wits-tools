//! Provides the opt-in CLI capture proxy for HTTP and byte-transparent HTTPS tunnels.

use std::{sync::Arc, time::Duration};

use axum::body::Body;
use hyper::{Method, Request, Response, StatusCode, body::Incoming, service::service_fn};
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::Semaphore,
};
use tokio_util::sync::CancellationToken;

use super::{Error, Result, http_url, media};

/// Bind only to loopback and return promptly once the user interrupts the command.
pub async fn run(
    port: u16,
    stop: CancellationToken,
    found: impl Fn(String, media::Kind) + Send + Sync + 'static,
) -> Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .await
        .map_err(|_| Error::Network)?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| Error::Network)?;
    let transfers = Arc::new(Semaphore::new(32));
    let found = Arc::new(found);
    loop {
        let (socket, _) = tokio::select! {
            _ = stop.cancelled() => return Ok(()),
            result = listener.accept() => result.map_err(|_| Error::Network)?,
        };
        let Ok(permit) = Arc::clone(&transfers).try_acquire_owned() else {
            continue;
        };
        let found = Arc::clone(&found);
        let client = client.clone();
        let stop = stop.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let service =
                service_fn(|request| forward(request, &client, found.as_ref(), stop.clone()));
            let mut builder = hyper::server::conn::http1::Builder::new();
            builder
                .timer(TokioTimer::new())
                .header_read_timeout(Duration::from_secs(15));
            tokio::select! {
                _ = stop.cancelled() => {},
                _ = builder.serve_connection(TokioIo::new(socket), service).with_upgrades() => {},
            }
        });
    }
}

async fn forward(
    mut request: Request<Incoming>,
    client: &reqwest::Client,
    found: &impl Fn(String, media::Kind),
    stop: CancellationToken,
) -> std::result::Result<Response<Body>, std::convert::Infallible> {
    if request.method() == Method::CONNECT {
        let Some(authority) = request
            .uri()
            .authority()
            .filter(|authority| authority.port().is_some())
        else {
            return Ok(status(StatusCode::BAD_REQUEST));
        };
        let host = authority.as_str().to_owned();
        let Ok(Ok(remote)) =
            tokio::time::timeout(Duration::from_secs(15), TcpStream::connect(&host)).await
        else {
            return Ok(status(StatusCode::BAD_GATEWAY));
        };
        let upgrade = hyper::upgrade::on(&mut request);
        tokio::spawn(async move {
            if let Ok(upgrade) = upgrade.await {
                let mut local = TokioIo::new(upgrade);
                let mut remote = remote;
                tokio::select! {
                    _ = stop.cancelled() => {},
                    _ = tokio::io::copy_bidirectional(&mut local, &mut remote) => {},
                }
            }
        });
        return Ok(status(StatusCode::OK));
    }
    let Ok(url) = http_url(&request.uri().to_string()) else {
        return Ok(status(StatusCode::BAD_REQUEST));
    };
    if url.scheme() != "http" {
        return Ok(status(StatusCode::BAD_REQUEST));
    }
    let (parts, body) = request.into_parts();
    let mut upstream = client.request(parts.method, url.clone());
    for (name, value) in &parts.headers {
        if !hop_header(name.as_str()) {
            upstream = upstream.header(name, value);
        }
    }
    let Ok(response) = upstream
        .body(reqwest::Body::wrap_stream(http_body_stream(body)))
        .send()
        .await
    else {
        return Ok(status(StatusCode::BAD_GATEWAY));
    };
    let mime = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if response.status().is_success()
        && let Some(kind) = media::classify(&url, mime)
    {
        found(url.to_string(), kind);
    }
    let mut builder = Response::builder().status(response.status());
    for (name, value) in response.headers() {
        if !hop_header(name.as_str()) {
            builder = builder.header(name, value);
        }
    }
    Ok(builder
        .body(Body::from_stream(response.bytes_stream()))
        .unwrap_or_else(|_| status(StatusCode::BAD_GATEWAY)))
}

fn http_body_stream(body: Incoming) -> axum::body::BodyDataStream {
    Body::new(body).into_data_stream()
}

fn hop_header(name: &str) -> bool {
    matches!(
        name,
        "connection"
            | "proxy-connection"
            | "keep-alive"
            | "proxy-authorization"
            | "proxy-authenticate"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "host"
    )
}

fn status(code: StatusCode) -> Response<Body> {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = code;
    response
}
