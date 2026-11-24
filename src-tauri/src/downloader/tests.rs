//! Verifies file classification, verified range assembly, and safe output against a local origin.

use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    body::{Body, Bytes},
    extract::{Path as Route, State},
    http::{HeaderMap, Response, StatusCode},
    routing::get,
};
use tokio_util::sync::CancellationToken;

use super::{Error, Mode, Options, transfer};

const SIZE: usize = 17 * 1024 * 1024;

#[derive(Clone, Default)]
struct Counters {
    active: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
    aggregate: Arc<tokio::sync::Mutex<()>>,
}

// Return deterministic bytes and vary range, metadata, and throttle behavior by route.
async fn fixture(
    Route(kind): Route<String>,
    State(counters): State<Counters>,
    headers: HeaderMap,
) -> Response<Body> {
    let build = Response::builder();
    let static_body = |status, mime, bytes| {
        Response::builder()
            .status(status)
            .header("content-type", mime)
            .body(Body::from(bytes))
            .unwrap()
    };
    match kind.as_str() {
        "redirect" => {
            return build
                .status(302)
                .header("location", "/attachment?token=secret")
                .body(Body::empty())
                .unwrap();
        }
        "limited" => return static_body(429, "text/plain", "slow down"),
        "denied" => return static_body(403, "text/plain", "denied"),
        "page" => {
            return static_body(
                200,
                "text/html; charset=utf-8",
                "<html>Download page</html>",
            );
        }
        "attached-html" => {
            return build
                .header("content-type", "text/html")
                .header("content-disposition", "attachment; filename=page.html")
                .body(Body::from("<html>file</html>"))
                .unwrap();
        }
        "attachment" => {
            return build
                .header(
                    "content-disposition",
                    "attachment; filename=archive.zip; filename*=UTF-8''caf%C3%A9.zip",
                )
                .body(Body::from("file bytes"))
                .unwrap();
        }
        "unknown" => {
            return build
                .body(Body::from_stream(futures_util::stream::iter([Ok::<
                    _,
                    std::io::Error,
                >(
                    Bytes::from_static(b"unknown size"),
                )])))
                .unwrap();
        }
        _ => {}
    }
    let range = headers
        .get("range")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("bytes="))
        .and_then(|value| value.split_once('-'))
        .and_then(|(start, end)| {
            Some((
                start.parse::<usize>().ok()?,
                if end.is_empty() {
                    SIZE - 1
                } else {
                    end.parse::<usize>().ok()?
                },
            ))
        });
    if matches!(kind.as_str(), "per-connection" | "aggregate") {
        return throttled(range, kind == "aggregate", counters);
    }
    if let Some((start, end)) = range.filter(|_| kind != "ignore") {
        assert_eq!(headers["if-range"], "\"first\"");
        if start != end {
            let current = counters.active.fetch_add(1, Ordering::SeqCst) + 1;
            counters.peak.fetch_max(current, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(85)).await;
            counters.active.fetch_sub(1, Ordering::SeqCst);
            if kind == "changed" {
                return build
                    .status(200)
                    .header("etag", "\"second\"")
                    .body(Body::from("different"))
                    .unwrap();
            }
        }
        let range = if kind == "malformed" {
            format!("bytes 1-{end}/{SIZE}")
        } else {
            format!("bytes {start}-{end}/{SIZE}")
        };
        return build
            .status(StatusCode::PARTIAL_CONTENT)
            .header("content-range", range)
            .header("etag", "\"first\"")
            .body(Body::from(vec![37; end - start + 1]))
            .unwrap();
    }
    build
        .header("etag", "\"first\"")
        .body(Body::from(vec![37; SIZE]))
        .unwrap()
}

// Serve each test on an ephemeral port without external services.
async fn server() -> (url::Url, Counters) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap())
        .parse()
        .unwrap();
    let counters = Counters::default();
    let app = Router::new()
        .route("/{kind}", get(fixture))
        .with_state(counters.clone());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (url, counters)
}

async fn fetch(
    base: &url::Url,
    route: &str,
    folder: &Path,
    progress: impl Fn(Mode, u64, Option<u64>) + Sync,
) -> super::Result<(std::path::PathBuf, u64)> {
    transfer::run(
        base.join(route).unwrap(),
        None,
        Options {
            workers: 4,
            ..Options::default()
        },
        folder.into(),
        CancellationToken::new(),
        progress,
    )
    .await
}

#[tokio::test]
async fn supports_general_files_and_reports_recovery_errors() {
    let (base, _) = server().await;
    let folder = tempfile::tempdir().unwrap();
    let (path, _) = fetch(&base, "redirect", folder.path(), |_, _, _| {})
        .await
        .unwrap();
    assert_eq!(path.file_name().unwrap(), "café.zip");
    assert_eq!(std::fs::read(path).unwrap(), b"file bytes");
    assert!(matches!(
        fetch(&base, "redirect", folder.path(), |_, _, _| {}).await,
        Err(Error::Conflict)
    ));
    for (route, expected) in [
        ("page", Error::Page),
        ("limited", Error::RateLimited),
        ("denied", Error::AccessDenied),
    ] {
        assert_eq!(
            fetch(&base, route, folder.path(), |_, _, _| {})
                .await
                .unwrap_err(),
            expected
        );
    }
    let (path, _) = fetch(&base, "attached-html", folder.path(), |_, _, _| {})
        .await
        .unwrap();
    assert_eq!(path.file_name().unwrap(), "page.html");
    let (path, _) = fetch(&base, "unknown", folder.path(), |_, _, total| {
        assert!(total.is_none())
    })
    .await
    .unwrap();
    assert_eq!(std::fs::read(path).unwrap(), b"unknown size");
}

#[tokio::test]
async fn validates_parallel_ranges_and_recovers_without_corrupting_output() {
    let (base, counters) = server().await;
    let folder = tempfile::tempdir().unwrap();
    let expected = blake3::hash(&vec![37; SIZE]);
    for (route, expected_mode) in [
        ("parallel", Mode::Parallel),
        ("ignore", Mode::Single),
        ("malformed", Mode::Single),
        ("changed", Mode::Single),
    ] {
        let mode = std::sync::Mutex::new(None);
        let (path, count) = fetch(&base, route, folder.path(), |value, _, _| {
            *mode.lock().unwrap() = Some(value)
        })
        .await
        .unwrap();
        assert!(mode.into_inner().unwrap() == Some(expected_mode));
        assert_eq!(count, SIZE as u64);
        assert_eq!(blake3::hash(&std::fs::read(path).unwrap()), expected);
    }
    assert!(counters.peak.load(Ordering::SeqCst) > 1);
}

#[tokio::test]
async fn cancellation_cleans_partial_output() {
    let (base, _) = server().await;
    let folder = tempfile::tempdir().unwrap();
    let stop = CancellationToken::new();
    let result = transfer::run(
        base.join("parallel").unwrap(),
        None,
        Options {
            workers: 4,
            ..Options::default()
        },
        folder.path().into(),
        stop.clone(),
        |_, _, _| stop.cancel(),
    )
    .await;
    assert!(matches!(result, Err(Error::Cancelled)));
    // Cancellation leaves neither partial files nor reservations in Downloads.
    assert!(std::fs::read_dir(folder.path()).unwrap().next().is_none());
}

// Pace the same deterministic response per request or through a shared origin-wide limiter.
fn throttled(range: Option<(usize, usize)>, shared: bool, counters: Counters) -> Response<Body> {
    let (start, end) = range.unwrap_or((0, SIZE - 1));
    let length = end - start + 1;
    let mut builder = Response::builder()
        .header("etag", "\"first\"")
        .header("accept-ranges", "bytes")
        .header("content-length", length);
    if range.is_some() {
        builder = builder
            .status(206)
            .header("content-range", format!("bytes {start}-{end}/{SIZE}"));
    }
    let body = futures_util::stream::unfold(
        (length, counters),
        move |(remaining, counters)| async move {
            if remaining == 0 {
                return None;
            }
            let size = remaining.min(128 * 1024);
            {
                let _gate = if shared {
                    Some(counters.aggregate.lock().await)
                } else {
                    None
                };
                tokio::time::sleep(Duration::from_millis(15)).await;
            }
            Some((
                Ok::<_, std::io::Error>(Bytes::from(vec![37; size])),
                (remaining - size, counters),
            ))
        },
    );
    builder.body(Body::from_stream(body)).unwrap()
}

// Run manually with ARIA2C_PATH pointing to an existing pinned aria2 executable.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires aria2 and child-process loopback access"]
async fn compares_throughput_with_aria2() {
    let executable = std::env::var_os("ARIA2C_PATH").expect("set ARIA2C_PATH to aria2c");
    let (base, _) = server().await;
    let expected = blake3::hash(&vec![37; SIZE]);
    for route in ["per-connection", "aggregate"] {
        let folder = tempfile::tempdir().unwrap();
        for workers in [1, 4] {
            let span = std::time::Instant::now();
            let options = Options {
                workers,
                filename: Some(format!("wits-{workers}")),
                folder: None,
            };
            let (path, _) = transfer::run(
                base.join(route).unwrap(),
                None,
                options,
                folder.path().into(),
                CancellationToken::new(),
                |_, _, _| {},
            )
            .await
            .unwrap();
            println!("{route} WITS {workers} worker(s) {:?}", span.elapsed());
            assert_eq!(blake3::hash(&std::fs::read(path).unwrap()), expected);
        }
        let span = std::time::Instant::now();
        let result = tokio::process::Command::new(&executable)
            .args([
                "--no-conf",
                "--split=4",
                "--max-connection-per-server=4",
                "--min-split-size=4M",
                "--file-allocation=none",
                "--max-tries=1",
                "--summary-interval=0",
                "--out=aria2",
            ])
            .arg("--dir")
            .arg(folder.path())
            .arg(base.join(route).unwrap().as_str())
            .output()
            .await
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        println!("{route} aria2 4 workers {:?}", span.elapsed());
        assert_eq!(
            blake3::hash(&std::fs::read(folder.path().join("aria2")).unwrap()),
            expected
        );
    }
}

// A captured request may never carry its credentials to a redirected origin.
#[tokio::test]
async fn capture_probe_keeps_credentials_on_the_observed_origin() {
    let destination = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let redirect_to = format!("http://{}/file", destination.local_addr().unwrap());
    let observed = Arc::new(AtomicUsize::new(0));
    let visits = observed.clone();
    let redirected = Router::new().route(
        "/file",
        get(move || {
            visits.fetch_add(1, Ordering::SeqCst);
            async { StatusCode::FORBIDDEN }
        }),
    );
    tokio::spawn(async move {
        axum::serve(destination, redirected).await.unwrap();
    });

    // The first origin expects the browser cookie and tries to redirect the replay.
    let origin = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let source = format!("http://{}/file", origin.local_addr().unwrap());
    let server = Router::new().route(
        "/file",
        get(move |headers: HeaderMap| {
            let redirect_to = redirect_to.clone();
            async move {
                assert_eq!(headers["cookie"], "session=fixture");
                axum::response::Redirect::temporary(&redirect_to)
            }
        }),
    );
    tokio::spawn(async move {
        axum::serve(origin, server).await.unwrap();
    });
    let mut headers = HeaderMap::new();
    headers.insert("cookie", "session=fixture".parse().unwrap());
    let replay = transfer::Replay {
        url: source.parse().unwrap(),
        headers,
        length: SIZE as u64,
        etag: "\"first\"".parse().unwrap(),
    };
    assert!(!transfer::can_handoff(&replay, 4).await);
    assert_eq!(observed.load(Ordering::SeqCst), 0);
}

// A browser may report every byte before it signals completion.
#[test]
fn browser_finishing_has_no_transfer_speed_or_eta() {
    let mut state = super::Inner::default();
    let id = state.add(
        "https://example.com",
        None,
        true,
        super::Status::Downloading,
    );
    let started = std::time::Instant::now() - Duration::from_secs(2);
    state.progress(id, Mode::Browser, 512, Some(1024), started);
    assert!(state.job(id).unwrap().speed.is_some());
    state.progress(id, Mode::Browser, 1024, Some(1024), started);
    let job = state.job(id).unwrap();
    assert_eq!(job.bytes, 1024);
    assert!(job.speed.is_none() && job.eta.is_none());
    assert!(job.status == super::Status::Downloading);
}

// Removing a completed row updates history while preserving the downloaded file and active jobs.
#[test]
fn remove_history_keeps_files_and_active_jobs() {
    let folder = tempfile::tempdir().unwrap();
    let history = folder.path().join("history.json");
    let downloaded = folder.path().join("archive.zip");
    std::fs::write(&downloaded, b"file bytes").unwrap();
    let mut state = super::Inner {
        history_path: Some(history.clone()),
        ..Default::default()
    };
    let active = state.add("https://example.com", None, false, super::Status::Queued);
    let completed = state.add(
        "https://example.com",
        None,
        true,
        super::Status::Downloading,
    );
    state.finish(completed, Ok((downloaded.clone(), 10)));

    assert_eq!(state.remove_history(active), Err(Error::Busy));
    state.remove_history(completed).unwrap();
    let rows: Vec<super::Job> = serde_json::from_slice(&std::fs::read(history).unwrap()).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, active);
    assert_eq!(std::fs::read(downloaded).unwrap(), b"file bytes");
}
