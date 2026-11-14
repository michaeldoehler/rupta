//! The one HTTP client for calls to issue trackers and paging services.
//!
//! - Every host is resolved and checked like single sign-on issuers
//!   (`routes::sso_test::resolve_guarded`): loopback, private, link-local
//!   and other internal addresses are refused unless
//!   `RUPTA_INTEGRATIONS_ALLOW_PRIVATE_HOSTS=true`, and the connection goes
//!   to the checked addresses.
//! - Redirects are not followed; a connection takes at most 5 seconds and
//!   a request at most 15; answers are limited to 5 MiB.
//! - `429` and `503` are retried for every request, other `5xx` answers
//!   and connection failures only for requests marked idempotent; at most
//!   three attempts, with exponential backoff (250 ms, 500 ms, plus
//!   jitter) or the server's `Retry-After` when it is at most 30 seconds.

use std::net::IpAddr;
use std::time::Duration;

use reqwest::Method;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::Value;

use super::TrackerError;
use crate::routes::sso_test::resolve_guarded;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_BYTES: usize = 5 << 20;
const MAX_RETRY_AFTER: Duration = Duration::from_secs(30);
/// The variable that allows internal hosts.
pub const ALLOW_PRIVATE_SETTING: &str = "RUPTA_INTEGRATIONS_ALLOW_PRIVATE_HOSTS";

/// Settings of the outbound client.
#[derive(Clone, Debug)]
pub struct Outbound {
    pub allow_private: bool,
    pub max_attempts: u32,
    pub base_delay: Duration,
}

impl Outbound {
    pub fn new(allow_private: bool) -> Outbound {
        Outbound { allow_private, max_attempts: 3, base_delay: Duration::from_millis(250) }
    }
}

/// A request.
#[derive(Debug, Clone)]
pub struct Req {
    pub method: Method,
    pub url: url::Url,
    pub headers: Vec<(HeaderName, String)>,
    pub body: Option<Vec<u8>>,
    /// Safe to send again after a server error or a broken connection.
    pub idempotent: bool,
}

impl Req {
    pub fn new(method: Method, url: url::Url) -> Req {
        let idempotent = matches!(method, Method::GET | Method::HEAD | Method::PUT | Method::DELETE | Method::PATCH);
        Req { method, url, headers: Vec::new(), body: None, idempotent }
    }

    pub fn header(mut self, name: &'static str, value: impl Into<String>) -> Req {
        self.headers.push((HeaderName::from_static(name), value.into()));
        self
    }

    pub fn json(mut self, body: &Value) -> Req {
        self.body = Some(serde_json::to_vec(body).expect("JSON serializes"));
        self.header("content-type", "application/json")
    }

    pub fn idempotent(mut self, yes: bool) -> Req {
        self.idempotent = yes;
        self
    }
}

/// An answer.
#[derive(Debug)]
pub struct Resp {
    pub status: u16,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

impl Resp {
    pub fn json(&self) -> Result<Value, TrackerError> {
        if self.body.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&self.body)
            .map_err(|e| TrackerError::Invalid(format!("the answer is not JSON ({e}): {}", snippet(&self.body))))
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }
}

/// The first 300 characters of a body, for error messages.
pub fn snippet(body: &[u8]) -> String {
    let s = String::from_utf8_lossy(body);
    let s = s.trim();
    if s.chars().count() > 300 { format!("{}…", s.chars().take(300).collect::<String>()) } else { s.to_string() }
}

/// `Retry-After` as seconds or an HTTP date.
pub fn retry_after(v: Option<&str>) -> Option<Duration> {
    let v = v?.trim();
    if let Ok(secs) = v.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }
    let at = chrono::DateTime::parse_from_rfc2822(v).ok()?;
    let wait = at.with_timezone(&chrono::Utc) - chrono::Utc::now();
    Some(wait.to_std().unwrap_or(Duration::ZERO))
}

fn retryable_status(status: u16, idempotent: bool) -> bool {
    status == 429 || status == 503 || (idempotent && matches!(status, 500 | 502 | 504))
}

impl Outbound {
    /// Checks that the URL's host may be reached (no request is made).
    pub async fn check_host(&self, url: &url::Url) -> Result<(), TrackerError> {
        resolve_guarded(url, self.allow_private, None, ALLOW_PRIVATE_SETTING, CONNECT_TIMEOUT)
            .await
            .map(|_| ())
            .map_err(TrackerError::Blocked)
    }

    fn client(&self, url: &url::Url, addrs: &[std::net::SocketAddr]) -> Result<reqwest::Client, TrackerError> {
        let mut builder = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .user_agent(concat!("rupta/", env!("CARGO_PKG_VERSION")));
        let host = url.host_str().unwrap_or_default();
        if host.trim_start_matches('[').trim_end_matches(']').parse::<IpAddr>().is_err() {
            builder = builder.resolve_to_addrs(host, addrs);
        }
        builder.build().map_err(|e| TrackerError::Network(e.to_string()))
    }

    /// Sends a request with retries; any status is returned as an answer.
    pub async fn send(&self, req: Req) -> Result<Resp, TrackerError> {
        if !matches!(req.url.scheme(), "http" | "https") {
            return Err(TrackerError::Invalid("only http and https URLs are supported".into()));
        }
        let addrs = resolve_guarded(&req.url, self.allow_private, None, ALLOW_PRIVATE_SETTING, CONNECT_TIMEOUT)
            .await
            .map_err(TrackerError::Blocked)?;
        let client = self.client(&req.url, &addrs)?;
        let mut attempt = 0;
        loop {
            attempt += 1;
            let last = attempt >= self.max_attempts;
            let mut r = client.request(req.method.clone(), req.url.clone());
            for (k, v) in &req.headers {
                let v = HeaderValue::from_str(v).map_err(|_| TrackerError::Invalid(format!("invalid {k} header")))?;
                r = r.header(k.clone(), v);
            }
            if let Some(b) = &req.body {
                r = r.body(b.clone());
            }
            match r.send().await {
                Ok(mut resp) => {
                    let status = resp.status().as_u16();
                    let headers = resp.headers().clone();
                    if !last && retryable_status(status, req.idempotent) {
                        let wait = retry_after(headers.get("retry-after").and_then(|v| v.to_str().ok()));
                        match wait {
                            Some(w) if w > MAX_RETRY_AFTER => {}
                            _ => {
                                tokio::time::sleep(wait.unwrap_or_else(|| self.backoff(attempt))).await;
                                continue;
                            }
                        }
                    }
                    let mut body = Vec::new();
                    loop {
                        match resp.chunk().await {
                            Ok(Some(chunk)) => {
                                body.extend_from_slice(&chunk);
                                if body.len() > MAX_BYTES {
                                    return Err(TrackerError::Invalid("the answer is larger than 5 MiB".into()));
                                }
                            }
                            Ok(None) => break,
                            Err(e) => return Err(TrackerError::Network(format!("reading the answer failed: {e}"))),
                        }
                    }
                    return Ok(Resp { status, headers, body });
                }
                Err(e) => {
                    let connect = e.is_connect();
                    if !last && (req.idempotent || connect) {
                        tokio::time::sleep(self.backoff(attempt)).await;
                        continue;
                    }
                    return Err(TrackerError::Network(if e.is_timeout() {
                        format!("{} timed out", req.url.host_str().unwrap_or("the host"))
                    } else {
                        format!("the request to {} failed: {e}", req.url.host_str().unwrap_or("the host"))
                    }));
                }
            }
        }
    }

    fn backoff(&self, attempt: u32) -> Duration {
        let base = self.base_delay.saturating_mul(1 << (attempt - 1).min(6));
        let jitter = rand::random_range(0..=base.as_millis() as u64 / 4 + 1);
        base + Duration::from_millis(jitter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_after_forms() {
        assert_eq!(retry_after(Some("3")), Some(Duration::from_secs(3)));
        assert_eq!(retry_after(Some("Wed, 21 Oct 2015 07:28:00 GMT")), Some(Duration::ZERO));
        assert_eq!(retry_after(Some("soon")), None);
        assert_eq!(retry_after(None), None);
    }

    #[test]
    fn which_statuses_are_retried() {
        assert!(retryable_status(429, false));
        assert!(retryable_status(503, false));
        assert!(!retryable_status(500, false));
        assert!(retryable_status(500, true));
        assert!(retryable_status(502, true));
        assert!(!retryable_status(404, true));
    }
}
