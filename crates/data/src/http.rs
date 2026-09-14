//! The only place this crate talks to the network.
//!
//! Requests go straight to the provider named in the URL (ADR 0007). There is
//! no proxy, no cache server, no telemetry and no retry-with-backoff service:
//! rate limits are the user's own.

use replay_core::{Error, Result};
use std::io::Read;
use std::time::Duration;

/// Cap on a single response body. Dukascopy days and Binance pages are well
/// under this; anything larger means we asked for the wrong thing.
const MAX_BODY: u64 = 64 * 1024 * 1024;

/// One agent for the whole process. Providers are fetched a file at a time,
/// so without a shared connection pool every single day of history would pay
/// for a fresh TLS handshake.
fn agent() -> &'static ureq::Agent {
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(15))
            .timeout_read(Duration::from_secs(60))
            .user_agent(concat!("bar-replay/", env!("CARGO_PKG_VERSION")))
            .build()
    })
}

/// How long to wait before each retry. Dukascopy answers a sustained fetch
/// with intermittent 503s, so giving up on the first one would strand a user
/// halfway through a download for no reason.
const BACKOFF: [Duration; 5] = [
    Duration::from_millis(500),
    Duration::from_millis(2_000),
    Duration::from_millis(5_000),
    Duration::from_millis(10_000),
    Duration::from_millis(20_000),
];

/// Worth trying again: the provider is busy or rate-limiting us, or the
/// connection broke. A 4xx other than 429 means the request itself is wrong,
/// and repeating it would only add load.
fn transient(e: &ureq::Error) -> bool {
    match e {
        ureq::Error::Status(429, _) => true,
        ureq::Error::Status(code, _) => *code >= 500,
        ureq::Error::Transport(_) => true,
    }
}

/// `Ok(None)` means the provider has no file there (404) — a normal answer for
/// a weekend or a date before listing, not an error.
///
/// Transient failures are retried a few times with a fixed backoff. This is a
/// plain client-side retry against the provider's own API; it is not, and must
/// never become, infrastructure of ours in the data path (ADR 0007).
pub fn get_bytes(url: &str) -> Result<Option<Vec<u8>>> {
    let mut attempt = 0;
    loop {
        match agent().get(url).call() {
            Ok(resp) => {
                let mut buf = Vec::new();
                resp.into_reader()
                    .take(MAX_BODY)
                    .read_to_end(&mut buf)
                    .map_err(|e| Error::Provider(format!("{url}: reading body: {e}")))?;
                return Ok(Some(buf));
            }
            Err(ureq::Error::Status(404, _)) => return Ok(None),
            Err(e) if transient(&e) && attempt < BACKOFF.len() => {
                std::thread::sleep(BACKOFF[attempt]);
                attempt += 1;
            }
            Err(ureq::Error::Status(code, resp)) => {
                let detail = resp.into_string().unwrap_or_default();
                let detail: String = detail.chars().take(300).collect();
                return Err(Error::Provider(format!(
                    "{url}: HTTP {code} after {} attempt(s): {detail}",
                    attempt + 1
                )));
            }
            Err(e) => {
                return Err(Error::Provider(format!(
                    "{url}: {e} (after {} attempt(s))",
                    attempt + 1
                )))
            }
        }
    }
}

pub fn get_string(url: &str) -> Result<Option<String>> {
    match get_bytes(url)? {
        Some(b) => String::from_utf8(b)
            .map(Some)
            .map_err(|e| Error::Provider(format!("{url}: response is not UTF-8: {e}"))),
        None => Ok(None),
    }
}
