//! Formats correlated development-terminal records for crawler browsers.

use std::{fmt::Arguments, time::Instant};

use url::Url;
use uuid::Uuid;

/// Keep a single identifier and elapsed clock across startup, capture, and shutdown.
#[derive(Clone, Copy)]
pub(super) struct Trace {
    id: Option<Uuid>,
    started: Instant,
    component: &'static str,
}

impl Trace {
    pub(super) fn new(discovery: bool) -> Self {
        Self::for_component("obscura", discovery)
    }

    /// Keep each browser's terminal records distinguishable while sharing the same format.
    pub(super) fn for_component(component: &'static str, enabled: bool) -> Self {
        Self {
            id: (enabled && cfg!(debug_assertions)).then(Uuid::new_v4),
            started: Instant::now(),
            component,
        }
    }

    /// Debug desktop builds inherit stderr from the terminal running Tauri.
    pub(super) fn record(self, message: Arguments<'_>) {
        if let Some(id) = self.id {
            eprintln!(
                "[crawler::{} {id} +{}ms] {message}",
                self.component,
                self.started.elapsed().as_millis()
            );
        }
    }
}

/// Log the public origin only. Paths, queries, and fragments can carry signed tokens.
pub(super) fn origin(value: &str) -> String {
    Url::parse(value)
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https"))
        .map_or_else(
            || "[non-http URL]".into(),
            |url| url.origin().ascii_serialization(),
        )
}

/// Browser error text can include URLs. Extract only the standard network error identifier.
fn network_error(value: &str) -> Option<&str> {
    let (_, tail) = value.split_once("net::ERR_")?;
    let end = tail
        .find(|ch: char| !ch.is_ascii_uppercase() && !ch.is_ascii_digit() && ch != '_')
        .unwrap_or(tail.len());
    (end > 0 && end <= 64).then_some(&tail[..end])
}

/// Classify free-form errors without exposing embedded URLs or credentials.
pub(super) fn browser_error(value: &str) -> String {
    if let Some(code) = network_error(value) {
        return format!("net::ERR_{code}");
    }
    let description = value.to_ascii_lowercase();
    for (label, patterns) in [
        (
            "timeout",
            &["timed out", "timeout", "deadline has elapsed"][..],
        ),
        (
            "dns",
            &["dns", "name resolution", "lookup address", "no such host"],
        ),
        ("tls", &["certificate", "tls", "ssl"]),
        (
            "connection",
            &["connection refused", "connection reset", "tcp connect"],
        ),
        ("redirect", &["redirect loop", "too many redirects"]),
        ("private network policy", &["private network", "ssrf"]),
        (
            "request transport",
            &["error sending request", "request failed", "fetch failed"],
        ),
        ("browser target", &["no page for session", "target closed"]),
    ] {
        if patterns.iter().any(|pattern| description.contains(pattern)) {
            return label.into();
        }
    }
    format!("unclassified browser error ({} bytes)", value.len())
}
