//! Exercises the native HTTP transport against a single-request loopback fixture.
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::{ApiError, ApiRequest, BODY_PREVIEW_LIMIT, BodyContent, send, text_content};

// Bind before spawning so each test gets its own ready-to-connect fixture.
fn fixture(handler: impl FnOnce(TcpStream) + Send + 'static) -> (SocketAddr, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let worker = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        handler(stream);
    });
    (address, worker)
}

// Consume the complete request so the fixture observes the bytes built by the HTTP client.
fn read_request(stream: &TcpStream) -> (String, Vec<u8>) {
    let mut reader = BufReader::new(stream);
    let mut headers = String::new();
    let mut length = 0;
    loop {
        let mut line = String::new();
        assert_ne!(reader.read_line(&mut line).unwrap(), 0);
        if line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = value.trim().parse().unwrap();
            }
        }
        headers.push_str(&line);
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    (headers, body)
}

// Produce exact HTTP responses including repeated headers, binary bytes, and empty bodies.
fn response_fixture(
    status: &'static str,
    headers: &'static str,
    body: Vec<u8>,
) -> (SocketAddr, JoinHandle<()>) {
    fixture(move |mut stream| {
        read_request(&stream);
        write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\n{headers}\r\n",
            body.len()
        )
        .unwrap();
        stream.write_all(&body).unwrap();
    })
}

fn request(address: SocketAddr) -> ApiRequest {
    ApiRequest {
        method: "GET".into(),
        url: format!("http://{address}/health"),
        headers: Vec::new(),
        body: None,
    }
}

// Capture request details and ensure all instances of a repeated response header survive IPC.
#[test]
fn sends_query_headers_and_body() {
    let (address, worker) = fixture(|mut stream| {
        let (headers, body) = read_request(&stream);
        assert!(headers.starts_with("POST /health?include=a%20b HTTP/1.1\r\n"));
        let headers = headers.to_ascii_lowercase();
        assert!(headers.contains("authorization: bearer fixture-token\r\n"));
        assert!(headers.contains("x-example: first\r\n"));
        assert!(headers.contains("x-example: second\r\n"));
        assert_eq!(body, br#"{"healthy":true}"#);
        stream
            .write_all(
                b"HTTP/1.1 201 Created\r\nx-result: one\r\nx-result: two\r\nContent-Length: 2\r\n\r\nOK",
            )
            .unwrap();
    });
    let mut request = request(address);
    request.method = "POST".into();
    request.url.push_str("?include=a%20b");
    request.headers = vec![
        ("Authorization".into(), "Bearer fixture-token".into()),
        ("x-example".into(), "first".into()),
        ("x-example".into(), "second".into()),
    ];
    request.body = Some(r#"{"healthy":true}"#.into());

    let response = tauri::async_runtime::block_on(send(request, Duration::from_secs(5))).unwrap();
    worker.join().unwrap();
    let response = serde_json::to_value(response).unwrap();
    assert_eq!(response["status"], 201);
    assert!(response["timeToHeadersMs"].is_u64());
    assert_eq!(
        response["body"],
        serde_json::json!({ "kind": "text", "text": "OK", "truncated": false })
    );
    let headers = response["headers"].as_array().unwrap();
    assert!(headers.contains(&serde_json::json!(["x-result", "one"])));
    assert!(headers.contains(&serde_json::json!(["x-result", "two"])));
}

// Redirects and unsuccessful statuses remain inspectable, as do empty and binary bodies.
#[test]
fn displays_received_statuses_and_body_states() {
    let (address, worker) =
        response_fixture("302 Found", "Location: http://127.0.0.1:1/\r\n", vec![]);
    let response =
        tauri::async_runtime::block_on(send(request(address), Duration::from_secs(5))).unwrap();
    worker.join().unwrap();
    assert_eq!(response.status, 302);
    assert!(matches!(response.body.content, BodyContent::Empty));

    let (address, worker) = response_fixture("404 Not Found", "", vec![255, 0]);
    let response =
        tauri::async_runtime::block_on(send(request(address), Duration::from_secs(5))).unwrap();
    worker.join().unwrap();
    assert_eq!(response.status, 404);
    assert!(matches!(response.body.content, BodyContent::Unsupported));
}

// Check exact-cap and oversized responses, including a split UTF-8 code point at the cap.
#[test]
fn caps_body_preview_without_discarding_complete_text() {
    for extra in [0, 1] {
        let (address, worker) =
            response_fixture("200 OK", "", vec![b'a'; BODY_PREVIEW_LIMIT + extra]);
        let response =
            tauri::async_runtime::block_on(send(request(address), Duration::from_secs(5))).unwrap();
        worker.join().unwrap();
        assert_eq!(response.body.truncated, extra > 0);
        assert!(
            matches!(response.body.content, BodyContent::Text(text) if text.len() == BODY_PREVIEW_LIMIT)
        );
    }
    assert!(matches!(
        text_content(vec![b'a', 0xe2, 0x82], true),
        BodyContent::Text(text) if text == "a"
    ));
}

// A stalled body still counts against the request deadline, separately from connection failures.
#[test]
fn classifies_timeout_and_connection_failures() {
    let (address, worker) = fixture(|mut stream| {
        read_request(&stream);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n")
            .unwrap();
        thread::sleep(Duration::from_millis(400));
    });
    let outcome =
        tauri::async_runtime::block_on(send(request(address), Duration::from_millis(150)));
    worker.join().unwrap();
    assert!(matches!(outcome, Err(ApiError::Timeout)));

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let outcome = tauri::async_runtime::block_on(send(request(address), Duration::from_secs(3)));
    assert!(matches!(outcome, Err(ApiError::ConnectionFailed)));
}
