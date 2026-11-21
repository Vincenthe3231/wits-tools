//! Exercises crawler and download parity against deterministic local HTTP fixtures.

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use aes::Aes128;
use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::{Response, StatusCode},
};
use cbc::cipher::{BlockModeEncrypt, KeyIvInit, block_padding::Pkcs7};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use wits_tools_lib::crawler::{
    Error,
    capture::Capture,
    crawl::{self, Config, Mode},
    media::{self, Download},
    storage::Store,
};

const DIRECT_BYTES: usize = 9 * 1024 * 1024;
const LARGE_ASSET: usize = 3 * 1024 * 1024;

/// Serve a small site, a script-driven player request, and media range and segment fixtures.
async fn fixture() -> (String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let ranges = Arc::new(AtomicUsize::new(0));
    let app = Router::new().fallback(handler).with_state(ranges.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, ranges, task)
}

async fn handler(State(ranges): State<Arc<AtomicUsize>>, request: Request) -> Response<Body> {
    let path = request.uri().path();
    match path {
        "/" => response(
            "text/html",
            format!(
                "<html><head><title>Index</title></head><body>{}<a href='/next'>Next</a><a href='/blocked'>Blocked</a><img src='/image.png'></body></html>",
                "Static content. ".repeat(20),
            ),
        ),
        "/next" => response(
            "text/html",
            format!(
                "<title>Next</title><body>{}</body>",
                "Another page. ".repeat(20)
            ),
        ),
        "/blocked" => response(
            "text/html",
            "<title>Blocked</title><body>Robots excludes this.</body>",
        ),
        "/robots.txt" => response("text/plain", "User-agent: *\nDisallow: /blocked\n"),
        "/js" => response(
            "text/html",
            r#"<html><head><title>JS</title></head><body>
            <script>
              const player = document.createElement("video");
              document.body.append(player);
              player.addEventListener("play", () => fetch("/master.m3u8"));
              player.dispatchEvent(new Event("play"));
              document.title = "Rendered by JavaScript";
            </script><img src="/image.png"></body></html>"#,
        ),
        "/image.png" => response("image/png", vec![4_u8; LARGE_ASSET]),
        "/master.m3u8" => response(
            "application/vnd.apple.mpegurl",
            "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=200\n/video.m3u8\n",
        ),
        "/video.m3u8" => response(
            "application/vnd.apple.mpegurl",
            "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:7\n#EXT-X-KEY:METHOD=AES-128,URI=\"key\"\n#EXTINF:1,\nfirst.ts\n#EXT-X-KEY:METHOD=NONE\n#EXTINF:1,\nsecond.ts\n#EXT-X-ENDLIST\n",
        ),
        "/key" => response("application/octet-stream", vec![5_u8; 16]),
        "/first.ts" => {
            let mut buffer = [0; 32];
            buffer[..5].copy_from_slice(b"first");
            let ciphertext =
                cbc::Encryptor::<Aes128>::new(&[5_u8; 16].into(), &7_u128.to_be_bytes().into())
                    .encrypt_padded::<Pkcs7>(&mut buffer, 5)
                    .unwrap();
            response("video/mp2t", ciphertext.to_vec())
        }
        "/second.ts" => response("video/mp2t", "second"),
        "/manifest.mpd" => response(
            "application/dash+xml",
            r#"<MPD mediaPresentationDuration="PT3S"><Period>
            <AdaptationSet mimeType="video/mp4"><Representation id="720" bandwidth="500">
            <SegmentTemplate media="part-$Time$.m4s" initialization="init.mp4">
              <SegmentTimeline><S t="0" d="1" r="-1"/></SegmentTimeline>
            </SegmentTemplate></Representation></AdaptationSet>
            <AdaptationSet mimeType="audio/mp4"><Representation id="a" bandwidth="900"><BaseURL>audio.mp4</BaseURL></Representation></AdaptationSet>
            </Period></MPD>"#,
        ),
        "/range.mpd" => response(
            "application/dash+xml",
            r#"<MPD><Period>
            <AdaptationSet mimeType="video/mp4"><Representation id="video"><SegmentList>
            <Initialization sourceURL="direct.mp4" range="0-1"/><SegmentURL media="direct.mp4" mediaRange="2-3"/>
            </SegmentList></Representation></AdaptationSet></Period></MPD>"#,
        ),
        "/init.mp4" => response("video/mp4", "init"),
        "/part-0.m4s" => response("video/mp4", "0"),
        "/part-1.m4s" => response("video/mp4", "1"),
        "/part-2.m4s" => response("video/mp4", "2"),
        "/direct.mp4" => {
            if let Some((start, end)) = request
                .headers()
                .get("range")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.strip_prefix("bytes="))
                .and_then(|range| range.split_once('-'))
                .and_then(|(start, end)| {
                    Some((start.parse::<usize>().ok()?, end.parse::<usize>().ok()?))
                })
            {
                ranges.fetch_add(1, Ordering::Relaxed);
                return Response::builder()
                    .status(206)
                    .header("content-type", "video/mp4")
                    .header(
                        "content-range",
                        format!("bytes {start}-{end}/{DIRECT_BYTES}"),
                    )
                    .body(Body::from(vec![7; end - start + 1]))
                    .unwrap();
            }
            response("video/mp4", vec![7; DIRECT_BYTES])
        }
        "/slow.mp4" => {
            tokio::time::sleep(Duration::from_secs(10)).await;
            response("video/mp4", "slow")
        }
        _ => Response::builder().status(404).body(Body::empty()).unwrap(),
    }
}

fn response(mime: &str, body: impl Into<Body>) -> Response<Body> {
    Response::builder()
        .header("content-type", mime)
        .body(body.into())
        .unwrap()
}

fn config(seed: String, mode: Mode) -> Config {
    Config {
        start_urls: vec![seed],
        mode,
        request_delay_millis: 0,
        ..Config::default()
    }
}

async fn start(store: &Store, config: Config, browser: PathBuf) -> Uuid {
    let id = Uuid::new_v4();
    let lease = store.create(id, &config).unwrap();
    crawl::run(
        store.clone(),
        id,
        config,
        browser,
        lease,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    id
}

#[tokio::test(flavor = "multi_thread")]
async fn static_limits_robots_assets_export_and_delete() {
    let (base, _, _server) = fixture().await;
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().into()).unwrap();
    let mut settings = config(base, Mode::Static);
    settings.max_depth = 1;
    settings.max_pages = 2;
    let id = start(&store, settings, dir.path().join("missing-browser.exe")).await;
    let list = store.list().unwrap();
    assert_eq!(list[0].status, "completed");
    assert_eq!(list[0].pages_crawled, 2);
    let results = store.results(id, 1, 0, 1).unwrap();
    assert_eq!(results.pages.len(), 1);
    assert!(results.pages[0].url.ends_with("/next"));
    assert_eq!(results.assets[0].size_bytes, LARGE_ASSET as i64);
    let output = dir.path().join("out.json");
    store.export(id, &output).unwrap();
    let export: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
    assert_eq!(export["jobs"][0]["status"], "completed");
    assert!(
        export["pages"][0]["html"]
            .as_str()
            .unwrap()
            .contains("<title>")
    );
    drop(store);
    let store = Store::open(dir.path().into()).unwrap();
    store.delete(id).unwrap();
    assert!(!store.asset_dir(id).exists());
    assert!(store.list().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn transfers_ranges_encryption_dash_conflict_and_cancel() {
    let (base, ranges, _server) = fixture().await;
    let dir = tempfile::tempdir().unwrap();
    let fetch = |name: &str| {
        let request = Download {
            url: format!("{base}/{name}"),
            ..Download::default()
        };
        media::download(
            request,
            None,
            dir.path().into(),
            CancellationToken::new(),
            |_| {},
        )
    };
    let direct = fetch("direct.mp4").await.unwrap();
    assert_eq!(
        std::fs::metadata(direct).unwrap().len(),
        DIRECT_BYTES as u64
    );
    assert_eq!(ranges.load(Ordering::Relaxed), 3);
    assert!(matches!(
        fetch("direct.mp4").await,
        Err(Error::OutputConflict)
    ));
    assert_eq!(
        std::fs::read(fetch("master.m3u8").await.unwrap()).unwrap(),
        b"firstsecond"
    );
    assert_eq!(
        std::fs::read(fetch("manifest.mpd").await.unwrap()).unwrap(),
        b"init012"
    );
    assert_eq!(
        std::fs::read(fetch("range.mpd").await.unwrap()).unwrap(),
        vec![7; 4]
    );
    let stop = CancellationToken::new();
    let cancel = stop.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        cancel.cancel();
    });
    let request = Download {
        url: format!("{base}/slow.mp4"),
        ..Download::default()
    };
    assert!(matches!(
        media::download(request, None, dir.path().into(), stop, |_| {}).await,
        Err(Error::Cancelled)
    ));
    assert!(!dir.path().join("slow.mp4").exists());
}

#[tokio::test]
async fn manual_pairing_requires_token_and_redacts_signed_urls() {
    let capture = Capture::default();
    let pair = capture.pair().await.unwrap();
    let (endpoint, token) = pair.code.split_once('#').unwrap();
    let url = format!("{endpoint}/v1/firefox/streams");
    let request = reqwest::Client::new().post(url).header("content-type", "application/json")
        .body(r#"{"url":"https://example.com/test.m3u8?secret=token","type":"hls","headers":{"Cookie":"secret=token"}}"#);
    assert_eq!(
        request.try_clone().unwrap().send().await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request
            .header("WitsCaptureSession", token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CREATED
    );
    let sources = capture.sources().unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].url, "https://example.com/test.m3u8");
    capture.stop();
    assert!(capture.sources().unwrap().is_empty());
}

#[cfg(windows)]
#[tokio::test]
async fn native_bridge_requires_the_selected_tab() {
    let capture = Capture::default();
    capture.start().unwrap();
    let mut bridge = tokio::process::Command::new(env!("CARGO_BIN_EXE_wits-tools-capture-host"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut input = bridge.stdin.take().unwrap();
    let mut output = bridge.stdout.take().unwrap();
    async fn request(
        input: &mut tokio::process::ChildStdin,
        output: &mut tokio::process::ChildStdout,
        request: serde_json::Value,
    ) -> serde_json::Value {
        let bytes = request.to_string();
        input.write_u32_le(bytes.len() as u32).await.unwrap();
        input.write_all(bytes.as_bytes()).await.unwrap();
        input.flush().await.unwrap();
        let size = tokio::time::timeout(Duration::from_secs(5), output.read_u32_le())
            .await
            .unwrap()
            .unwrap();
        let mut reply = vec![0; size as usize];
        output.read_exact(&mut reply).await.unwrap();
        serde_json::from_slice(&reply).unwrap()
    }
    let start = request(
        &mut input,
        &mut output,
        serde_json::json!({"protocolVersion": 1, "type": "start", "tabId": 7}),
    )
    .await;
    assert_eq!(start["type"], "started");
    let mut stream = serde_json::json!({"protocolVersion": 1, "type": "stream", "tabId": 42,
        "stream": {"url": "https://example.com/video.m3u8", "type": "hls"}});
    assert_eq!(
        request(&mut input, &mut output, stream.clone()).await["type"],
        "error"
    );
    stream["tabId"] = 7.into();
    assert_eq!(
        request(&mut input, &mut output, stream).await["type"],
        "accepted"
    );
    assert_eq!(capture.sources().unwrap().len(), 1);
    assert_eq!(
        request(&mut input, &mut output, serde_json::json!({"type":"stop"})).await["type"],
        "stopped"
    );
    assert!(capture.sources().unwrap().is_empty());
    drop(input);
    assert!(bridge.wait().await.unwrap().success());
    capture.stop();
}

/// Run browser tests on demand against the actual checksum-verified release executable.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires the staged Obscura Windows runtime"]
async fn obscura_cdp_discovers_hls_and_crawls_both_browser_modes() {
    let browser =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/crawler-browser/obscura.exe");
    let (base, _, _server) = fixture().await;
    let url = format!("{base}/js");
    let sources = media::discover(&browser, &url, None).await.unwrap();
    assert!(
        sources
            .iter()
            .any(|entry| entry.summary(Uuid::nil()).kind == media::Kind::Hls)
    );
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().into()).unwrap();
    for mode in [Mode::Auto, Mode::Browser] {
        let id = start(&store, config(url.clone(), mode), browser.clone()).await;
        let results = store.results(id, 0, 0, 50).unwrap();
        assert_eq!(results.pages[0].title, "Rendered by JavaScript");
        assert!(
            results
                .assets
                .iter()
                .any(|asset| asset.size_bytes == LARGE_ASSET as i64)
        );
    }
}

/// Confirm a configured upstream proxy sees Obscura traffic using two local proxy fixtures.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires the staged Obscura Windows runtime"]
async fn obscura_cdp_uses_http_and_socks5_proxies() {
    let browser =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/crawler-browser/obscura.exe");
    let (base, _, _server) = fixture().await;
    let url = format!("{base}/js");
    let port = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = port.local_addr().unwrap();
    drop(port);
    let stop = CancellationToken::new();
    let proxied = Arc::new(AtomicUsize::new(0));
    let http_count = proxied.clone();
    let cancel = stop.clone();
    let http = tokio::spawn(async move {
        wits_tools_lib::crawler::proxy::run(address.port(), cancel, move |_, _| {
            http_count.fetch_add(1, Ordering::Relaxed);
        })
        .await
        .unwrap()
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    let sources = media::discover(&browser, &url, Some(format!("http://{address}")))
        .await
        .unwrap();
    assert!(!sources.is_empty());
    assert!(proxied.load(Ordering::Relaxed) > 0);
    stop.cancel();
    http.await.unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let target = base.strip_prefix("http://").unwrap().to_owned();
    let socks_count = Arc::new(AtomicUsize::new(0));
    let observed = socks_count.clone();
    let socks = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let target = target.clone();
            let observed = observed.clone();
            tokio::spawn(async move {
                let mut greeting = [0; 2];
                socket.read_exact(&mut greeting).await.unwrap();
                let mut methods = vec![0; usize::from(greeting[1])];
                socket.read_exact(&mut methods).await.unwrap();
                socket.write_all(&[5, 0]).await.unwrap();
                let mut command = [0; 4];
                socket.read_exact(&mut command).await.unwrap();
                let octets = match command[3] {
                    1 => 4,
                    3 => usize::from(socket.read_u8().await.unwrap()),
                    4 => 16,
                    _ => panic!("unexpected SOCKS address"),
                };
                let mut destination = vec![0; octets + 2];
                socket.read_exact(&mut destination).await.unwrap();
                let mut upstream = tokio::net::TcpStream::connect(target).await.unwrap();
                socket
                    .write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 0])
                    .await
                    .unwrap();
                observed.fetch_add(1, Ordering::Relaxed);
                let _ = tokio::io::copy_bidirectional(&mut socket, &mut upstream).await;
            });
        }
    });
    let sources = media::discover(&browser, &url, Some(format!("socks5://{address}")))
        .await
        .unwrap();
    assert!(!sources.is_empty());
    assert!(socks_count.load(Ordering::Relaxed) > 0);
    socks.abort();
}
