//! HTTP endpoints for SDKs: `POST /api/{project_id}/envelope/`,
//! `/store/` (legacy JSON events) and `/security/` (CSP reports), with
//! `OPTIONS` preflight and CORS for browser SDKs.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::{ConnectInfo, Path, Request, State};
use axum::response::Response;
use axum::routing::post;
use bytes::Bytes;
use chrono::Utc;
use http::{HeaderMap, HeaderValue, StatusCode, header};
use ipnet::IpNet;
use serde_json::{Map, Value, json};

use crate::auth::{parse_auth_header, parse_dsn, query_param};
use crate::decode::{DecodeError, decode};
use crate::envelope::{self, Envelope, Item};
use crate::metrics::{Category, Outcome};
use crate::ratelimit::Scope;
use crate::{Inner, Job, Msg};

const EXPOSE_HEADERS: &str = "x-sentry-error, x-sentry-rate-limits, retry-after";
const ALLOW_HEADERS: &str =
    "content-type, content-encoding, x-sentry-auth, authorization, sentry-trace, baggage, user-agent";
/// Categories counted against rate limits.
const LIMITED_CATEGORIES: &str = "error;default;security;transaction";

pub(crate) fn router(inner: Arc<Inner>) -> Router {
    Router::new()
        .route("/api/{project_id}/envelope/", post(envelope_handler).options(preflight))
        .route("/api/{project_id}/envelope", post(envelope_handler).options(preflight))
        .route("/api/{project_id}/store/", post(store_handler).options(preflight))
        .route("/api/{project_id}/store", post(store_handler).options(preflight))
        .route("/api/{project_id}/security/", post(security_handler).options(preflight))
        .route("/api/{project_id}/security", post(security_handler).options(preflight))
        .route("/api/{project_id}/csp-report/", post(security_handler).options(preflight))
        .with_state(inner)
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Envelope,
    Store,
    Security,
}

async fn envelope_handler(State(inner): State<Arc<Inner>>, Path(project): Path<String>, req: Request) -> Response {
    handle(inner, project, Kind::Envelope, req).await
}

async fn store_handler(State(inner): State<Arc<Inner>>, Path(project): Path<String>, req: Request) -> Response {
    handle(inner, project, Kind::Store, req).await
}

async fn security_handler(State(inner): State<Arc<Inner>>, Path(project): Path<String>, req: Request) -> Response {
    handle(inner, project, Kind::Security, req).await
}

/// An error answered to the SDK: status, message, extra headers.
#[derive(Debug)]
struct Reject {
    status: StatusCode,
    detail: String,
    headers: Vec<(&'static str, String)>,
}

fn reject(status: StatusCode, detail: impl Into<String>) -> Reject {
    Reject { status, detail: detail.into(), headers: Vec::new() }
}

fn json_response(status: StatusCode, body: Value, origin: Option<&str>, extra: &[(&'static str, String)]) -> Response {
    let mut res = Response::builder().status(status).header(header::CONTENT_TYPE, "application/json");
    if let Some(o) = origin
        && let Ok(v) = HeaderValue::from_str(o)
    {
        res = res
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, v)
            .header(header::VARY, "Origin")
            .header(header::ACCESS_CONTROL_EXPOSE_HEADERS, EXPOSE_HEADERS);
    }
    for (k, v) in extra {
        if let Ok(v) = HeaderValue::from_str(v) {
            res = res.header(*k, v);
        }
    }
    res.body(Body::from(body.to_string())).unwrap_or_else(|_| Response::new(Body::empty()))
}

fn reject_response(r: Reject, origin: Option<&str>) -> Response {
    let mut headers = r.headers;
    headers.push(("x-sentry-error", r.detail.clone()));
    json_response(r.status, json!({"detail": r.detail}), origin, &headers)
}

/// True if `origin` (e.g. `https://app.example.com:8443`) matches one of
/// the allowed patterns: `*`, a full origin, a host name (any scheme and
/// port) or `*.example.com` (the domain and its subdomains).
pub fn origin_allowed(allowed: &[String], origin: &str) -> bool {
    if allowed.iter().any(|a| a.trim() == "*") {
        return true;
    }
    let origin = origin.trim().trim_end_matches('/');
    let host = origin.split_once("://").map_or(origin, |(_, h)| h);
    let host = host.split('/').next().unwrap_or(host);
    let host = if host.starts_with('[') { host } else { host.split(':').next().unwrap_or(host) };
    allowed.iter().any(|a| {
        let a = a.trim().trim_end_matches('/');
        if a.is_empty() {
            return false;
        }
        if a.contains("://") {
            return a.eq_ignore_ascii_case(origin);
        }
        if let Some(domain) = a.strip_prefix("*.") {
            let domain = domain.to_ascii_lowercase();
            let h = host.to_ascii_lowercase();
            return h == domain || h.ends_with(&format!(".{domain}"));
        }
        a.eq_ignore_ascii_case(host)
    })
}

/// The client address: the peer, or, when the peer is a trusted proxy, the
/// last untrusted address in `X-Forwarded-For` (read right to left).
pub fn client_ip(peer: Option<IpAddr>, forwarded_for: Option<&str>, trusted: &[IpNet]) -> Option<IpAddr> {
    let is_trusted = |ip: &IpAddr| trusted.iter().any(|n| n.contains(ip));
    let peer = peer?;
    if !is_trusted(&peer) {
        return Some(peer);
    }
    let mut client = peer;
    if let Some(xff) = forwarded_for {
        for part in xff.split(',').rev() {
            let part = part.trim();
            let Ok(ip) = part.parse::<IpAddr>().or_else(|_| part.parse::<SocketAddr>().map(|s| s.ip())) else { break };
            client = ip;
            if !is_trusted(&ip) {
                break;
            }
        }
    }
    Some(client)
}

fn request_origin(headers: &HeaderMap) -> Option<String> {
    if let Some(o) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        return Some(o.to_string());
    }
    let referer = headers.get(header::REFERER).and_then(|v| v.to_str().ok())?;
    let (scheme, rest) = referer.split_once("://")?;
    let host = rest.split('/').next()?;
    Some(format!("{scheme}://{host}"))
}

async fn preflight(State(inner): State<Arc<Inner>>, Path(project): Path<String>, headers: HeaderMap) -> Response {
    let origin = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()).map(str::to_string);
    let Ok(project_id) = project.parse::<i64>() else {
        return reject_response(reject(StatusCode::BAD_REQUEST, "invalid project id"), None);
    };
    let allowed = match inner.keys.project(project_id).await {
        Ok(Some(p)) => origin.as_deref().is_none_or(|o| origin_allowed(&p.settings.allowed_origins, o)),
        Ok(None) => false,
        Err(_) => return reject_response(reject(StatusCode::SERVICE_UNAVAILABLE, "configuration unavailable"), None),
    };
    if !allowed {
        return reject_response(reject(StatusCode::FORBIDDEN, "origin not allowed"), None);
    }
    let req_headers = headers
        .get(header::ACCESS_CONTROL_REQUEST_HEADERS)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .unwrap_or_else(|| ALLOW_HEADERS.to_string());
    let mut res = Response::builder()
        .status(StatusCode::OK)
        .header(header::ACCESS_CONTROL_ALLOW_METHODS, "POST, OPTIONS")
        .header(header::ACCESS_CONTROL_ALLOW_HEADERS, req_headers)
        .header(header::ACCESS_CONTROL_MAX_AGE, "3600")
        .header(header::ACCESS_CONTROL_EXPOSE_HEADERS, EXPOSE_HEADERS)
        .header(header::VARY, "Origin");
    res = res.header(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin.as_deref().unwrap_or("*"));
    res.body(Body::empty()).unwrap_or_else(|_| Response::new(Body::empty()))
}

async fn handle(inner: Arc<Inner>, project: String, kind: Kind, req: Request) -> Response {
    inner.metrics.http_requests.fetch_add(1, Ordering::Relaxed);
    let origin = request_origin(req.headers());
    match accept(&inner, &project, kind, req, origin.as_deref()).await {
        Ok(body) => json_response(StatusCode::OK, body, origin.as_deref(), &[]),
        Err(r) => reject_response(r, origin.as_deref()),
    }
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

async fn accept(
    inner: &Arc<Inner>,
    project: &str,
    kind: Kind,
    req: Request,
    origin: Option<&str>,
) -> Result<Value, Reject> {
    let received = Utc::now();
    let project_id: i64 = project.parse().map_err(|_| reject(StatusCode::BAD_REQUEST, "invalid project id"))?;
    if inner.closed.load(Ordering::Acquire) {
        return Err(reject(StatusCode::SERVICE_UNAVAILABLE, "shutting down"));
    }
    let (parts, body) = req.into_parts();
    let headers = parts.headers;
    let query = parts.uri.query().map(str::to_string);
    let peer = parts.extensions.get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip());
    let ip = client_ip(peer, header_str(&headers, "x-forwarded-for"), &inner.config.trusted_proxies);
    let user_agent = header_str(&headers, "user-agent").map(str::to_string);

    let limit = inner.config.max_envelope_bytes;
    if let Some(len) = header_str(&headers, "content-length").and_then(|v| v.parse::<usize>().ok())
        && len > limit
    {
        return Err(reject(StatusCode::PAYLOAD_TOO_LARGE, format!("request body exceeds {limit} bytes")));
    }
    let body = axum::body::to_bytes(body, limit)
        .await
        .map_err(|_| reject(StatusCode::PAYLOAD_TOO_LARGE, format!("request body exceeds {limit} bytes")))?;
    let encoding = header_str(&headers, "content-encoding").map(str::to_string);
    let max = inner.config.max_decompressed_bytes;
    let body = if body.len() > 256 << 10 && encoding.is_some() {
        tokio::task::spawn_blocking(move || decode(encoding.as_deref(), body, max))
            .await
            .map_err(|_| reject(StatusCode::INTERNAL_SERVER_ERROR, "decoding failed"))?
    } else {
        decode(encoding.as_deref(), body, max)
    }
    .map_err(|e| match e {
        DecodeError::TooLarge(_) => reject(StatusCode::PAYLOAD_TOO_LARGE, e.to_string()),
        DecodeError::Unsupported(_) => reject(StatusCode::UNSUPPORTED_MEDIA_TYPE, e.to_string()),
        DecodeError::Invalid(..) => reject(StatusCode::BAD_REQUEST, e.to_string()),
    })?;

    let mut envelope = match kind {
        Kind::Envelope => envelope::parse(body).map_err(|e| reject(StatusCode::BAD_REQUEST, e.to_string()))?,
        Kind::Store => store_envelope(body, max)?,
        Kind::Security => security_envelope(body, query.as_deref(), user_agent.as_deref())?,
    };

    // Authentication: header, query parameter or the envelope's DSN.
    let auth = header_str(&headers, "x-sentry-auth")
        .or_else(|| {
            header_str(&headers, "authorization").filter(|v| v.trim_start().to_ascii_lowercase().starts_with("sentry "))
        })
        .map(parse_auth_header);
    let public_key = auth
        .and_then(|a| a.get("sentry_key").cloned())
        .or_else(|| query_param(query.as_deref(), "sentry_key"))
        .or_else(|| envelope.dsn().and_then(parse_dsn).map(|(k, _)| k))
        .filter(|k| !k.is_empty())
        .ok_or_else(|| reject(StatusCode::UNAUTHORIZED, "missing authorization: no sentry_key"))?;
    let (key, snapshot) = inner
        .keys
        .key(&public_key)
        .await
        .map_err(|_| reject(StatusCode::SERVICE_UNAVAILABLE, "configuration unavailable"))?;
    let key = key.ok_or_else(|| reject(StatusCode::UNAUTHORIZED, "unknown public key"))?;
    if key.project_id != project_id {
        return Err(reject(StatusCode::FORBIDDEN, "the key does not belong to this project"));
    }
    if !key.active {
        return Err(reject(StatusCode::FORBIDDEN, "the key is disabled"));
    }
    let project =
        snapshot.projects.get(&project_id).cloned().ok_or_else(|| reject(StatusCode::FORBIDDEN, "unknown project"))?;
    if let Some(o) = origin
        && headers.contains_key(header::ORIGIN)
        && !origin_allowed(&project.settings.allowed_origins, o)
    {
        return Err(reject(StatusCode::FORBIDDEN, format!("origin {o} is not allowed for this project")));
    }

    // Rate limits apply to events and transactions.
    let limited: Vec<&Item> =
        envelope.items.iter().filter(|i| matches!(i.ty.as_str(), "event" | "transaction" | "security")).collect();
    if !limited.is_empty() {
        let n = limited.len() as u64;
        let now = received.timestamp();
        let mut verdict = Ok(());
        if let Some(l) = key.rate_limit_per_min {
            verdict = inner.limiter.check(Scope::Key, key.id, l, n, now).map_err(|s| (s, Scope::Key));
        }
        if verdict.is_ok()
            && let Some(l) = project.settings.rate_limit_per_min
        {
            verdict = inner.limiter.check(Scope::Project, project.id, l, n, now).map_err(|s| (s, Scope::Project));
        }
        if let Err((secs, scope)) = verdict {
            for i in &limited {
                let category = if i.ty == "security" { Category::Security } else { Category::of_item(&i.ty) };
                inner.outcome(Some(project.id), category, Outcome::RateLimited, scope.as_str(), 1, received);
            }
            let mut r = reject(StatusCode::TOO_MANY_REQUESTS, format!("{} rate limit exceeded", scope.as_str()));
            r.headers.push(("retry-after", secs.to_string()));
            r.headers.push(("x-sentry-rate-limits", format!("{secs}:{LIMITED_CATEGORIES}:{}", scope.as_str())));
            return Err(r);
        }
    }

    // The response carries the event id; assign one if the SDK sent none.
    let has_event = envelope.items.iter().any(|i| matches!(i.ty.as_str(), "event" | "transaction" | "feedback"));
    let mut id = envelope.event_id().and_then(|s| uuid::Uuid::try_parse(s).ok()).map(|u| u.simple().to_string());
    if id.is_none() && has_event && kind == Kind::Envelope {
        let new = uuid::Uuid::new_v4().simple().to_string();
        envelope.headers.insert("event_id".into(), Value::String(new.clone()));
        id = Some(new);
    }

    let item_categories: Vec<Category> = envelope.items.iter().map(|i| Category::of_item(&i.ty)).collect();
    let (ack_tx, ack_rx) = if inner.config.safe { Some(tokio::sync::oneshot::channel()).unzip() } else { (None, None) };
    let job = Job { project: project.clone(), envelope, received, client_ip: ip, user_agent, ack: ack_tx };
    inner.in_flight.fetch_add(1, Ordering::AcqRel);
    match inner.tx.try_send(Msg::Job(Box::new(job))) {
        Ok(()) => {}
        Err(e) => {
            inner.in_flight.fetch_sub(1, Ordering::AcqRel);
            if matches!(e, crossbeam_channel::TrySendError::Disconnected(_)) {
                return Err(reject(StatusCode::SERVICE_UNAVAILABLE, "ingestion stopped"));
            }
            for c in item_categories {
                inner.outcome(Some(project.id), c, Outcome::QueueFull, "queue_full", 1, received);
            }
            let mut r = reject(StatusCode::TOO_MANY_REQUESTS, "ingest queue full");
            r.headers.push(("retry-after", "1".into()));
            r.headers.push(("x-sentry-rate-limits", "1::organization".into()));
            return Err(r);
        }
    }
    if let Some(rx) = ack_rx {
        match tokio::time::timeout(Duration::from_secs(60), rx).await {
            Ok(Ok(true)) => {}
            _ => return Err(reject(StatusCode::SERVICE_UNAVAILABLE, "the event could not be stored")),
        }
    }
    Ok(match id {
        Some(id) => json!({ "id": id }),
        None => json!({}),
    })
}

/// The legacy store endpoint: one event as JSON, or (very old SDKs)
/// base64 of zlib-compressed JSON.
fn store_envelope(body: Bytes, max: usize) -> Result<Envelope, Reject> {
    let first = body.iter().find(|b| !b.is_ascii_whitespace()).copied();
    let json: Bytes = if first == Some(b'{') {
        body
    } else {
        use base64::Engine;
        let raw = base64::engine::general_purpose::STANDARD
            .decode(body.iter().copied().filter(|b| !b.is_ascii_whitespace()).collect::<Vec<u8>>())
            .map_err(|_| reject(StatusCode::BAD_REQUEST, "invalid event body"))?;
        decode(Some("deflate"), raw.into(), max).map_err(|_| reject(StatusCode::BAD_REQUEST, "invalid event body"))?
    };
    let v: Value =
        serde_json::from_slice(&json).map_err(|e| reject(StatusCode::BAD_REQUEST, format!("invalid JSON: {e}")))?;
    let ty = if v.get("type").and_then(Value::as_str) == Some("transaction") { "transaction" } else { "event" };
    let id = v
        .get("event_id")
        .and_then(Value::as_str)
        .and_then(|s| uuid::Uuid::try_parse(s).ok())
        .unwrap_or_else(uuid::Uuid::new_v4);
    let mut headers = Map::new();
    headers.insert("event_id".into(), Value::String(id.simple().to_string()));
    let mut item_headers = Map::new();
    item_headers.insert("type".into(), Value::String(ty.into()));
    Ok(Envelope { headers, items: vec![Item { headers: item_headers, ty: ty.into(), payload: json }] })
}

fn csp_key(k: &str) -> String {
    // documentURL -> document_url, blocked-uri -> blocked_uri
    let mut out = String::new();
    for (i, c) in k.char_indices() {
        if c == '-' {
            out.push('_');
        } else if c.is_ascii_uppercase() {
            if i > 0 && !out.ends_with('_') && !k[..i].ends_with(|p: char| p.is_ascii_uppercase()) {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    match out.as_str() {
        "document_url" => "document_uri".into(),
        "blocked_url" => "blocked_uri".into(),
        "sample" => "script_sample".into(),
        _ => out,
    }
}

/// Turns one CSP report body into an event payload.
pub fn csp_event(report: &Map<String, Value>, query: Option<&str>, user_agent: Option<&str>) -> Value {
    let csp: Map<String, Value> = report.iter().map(|(k, v)| (csp_key(k), v.clone())).collect();
    let get = |k: &str| csp.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let directive = {
        let e = get("effective_directive");
        if e.is_empty() { get("violated_directive").split_whitespace().next().unwrap_or("").to_string() } else { e }
    };
    let blocked = get("blocked_uri");
    let document = get("document_uri");
    let mut ev = json!({
        "event_id": uuid::Uuid::new_v4().simple().to_string(),
        "platform": "javascript",
        "logger": "csp",
        "level": "error",
        "csp": Value::Object(csp.clone()),
        "message": format!("Blocked '{directive}' from '{blocked}'"),
        "tags": {"effective-directive": directive, "blocked-uri": blocked},
        "request": {"url": document},
    });
    if let Some(ua) = user_agent {
        ev["request"]["headers"] = json!({"User-Agent": ua});
    }
    if let Some(env) = query_param(query, "sentry_environment") {
        ev["environment"] = Value::String(env);
    }
    if let Some(rel) = query_param(query, "sentry_release") {
        ev["release"] = Value::String(rel);
    }
    ev
}

/// CSP reports: `{"csp-report": {...}}` or the Reporting API's list of
/// `{"type": "csp-violation", "body": {...}}`. Other report types are
/// acknowledged and dropped.
fn security_envelope(body: Bytes, query: Option<&str>, user_agent: Option<&str>) -> Result<Envelope, Reject> {
    let v: Value =
        serde_json::from_slice(&body).map_err(|e| reject(StatusCode::BAD_REQUEST, format!("invalid JSON: {e}")))?;
    let mut reports: Vec<(Map<String, Value>, Option<String>)> = Vec::new();
    let mut other = 0;
    match &v {
        Value::Object(o) => match o.get("csp-report").and_then(Value::as_object) {
            Some(r) => reports.push((r.clone(), None)),
            None => other += 1,
        },
        Value::Array(list) => {
            for r in list {
                match (r.get("type").and_then(Value::as_str), r.get("body").and_then(Value::as_object)) {
                    (Some("csp-violation"), Some(b)) => {
                        reports.push((b.clone(), r.get("user_agent").and_then(Value::as_str).map(str::to_string)))
                    }
                    _ => other += 1,
                }
            }
        }
        _ => return Err(reject(StatusCode::BAD_REQUEST, "invalid security report")),
    }
    let mut items: Vec<Item> = reports
        .iter()
        .map(|(r, ua)| {
            let ev = csp_event(r, query, ua.as_deref().or(user_agent));
            let mut h = Map::new();
            h.insert("type".into(), Value::String("security".into()));
            Item { headers: h, ty: "security".into(), payload: Bytes::from(ev.to_string()) }
        })
        .collect();
    for _ in 0..other {
        let mut h = Map::new();
        h.insert("type".into(), Value::String("report".into()));
        items.push(Item { headers: h, ty: "report".into(), payload: Bytes::new() });
    }
    Ok(Envelope { headers: Map::new(), items })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(origin_allowed(&a(&["*"]), "https://x.y"));
        assert!(origin_allowed(&a(&["https://app.example.com"]), "https://app.example.com"));
        assert!(!origin_allowed(&a(&["https://app.example.com"]), "http://app.example.com"));
        assert!(origin_allowed(&a(&["app.example.com"]), "http://app.example.com:3000"));
        assert!(origin_allowed(&a(&["*.example.com"]), "https://a.b.example.com"));
        assert!(origin_allowed(&a(&["*.example.com"]), "https://example.com"));
        assert!(!origin_allowed(&a(&["*.example.com"]), "https://badexample.com"));
        assert!(!origin_allowed(&a(&[]), "https://x.y"));
        assert!(!origin_allowed(&a(&["example.com"]), "null"));
    }

    #[test]
    fn client_ips_honor_trusted_proxies() {
        let trusted = crate::parse_trusted_proxies(&["10.0.0.0/8"]).unwrap();
        let ip = |s: &str| s.parse::<IpAddr>().unwrap();
        assert_eq!(client_ip(Some(ip("203.0.113.5")), Some("1.1.1.1"), &trusted), Some(ip("203.0.113.5")));
        assert_eq!(
            client_ip(Some(ip("10.0.0.2")), Some("1.1.1.1, 198.51.100.7, 10.0.0.9"), &trusted),
            Some(ip("198.51.100.7"))
        );
        assert_eq!(client_ip(Some(ip("10.0.0.2")), None, &trusted), Some(ip("10.0.0.2")));
        assert_eq!(client_ip(Some(ip("10.0.0.2")), Some("garbage"), &trusted), Some(ip("10.0.0.2")));
        assert_eq!(client_ip(None, Some("1.1.1.1"), &trusted), None);
        assert_eq!(client_ip(Some(ip("10.0.0.2")), Some("1.1.1.1"), &[]), Some(ip("10.0.0.2")));
    }

    #[test]
    fn csp_reports_both_formats() {
        let classic = json!({"csp-report": {"document-uri": "https://app.example.com/page", "violated-directive": "script-src 'self'",
            "effective-directive": "script-src", "blocked-uri": "https://evil.example/x.js", "original-policy": "script-src 'self'"}});
        let e =
            security_envelope(Bytes::from(classic.to_string()), Some("sentry_environment=prod"), Some("UA")).unwrap();
        assert_eq!(e.items.len(), 1);
        let ev: Value = serde_json::from_slice(&e.items[0].payload).unwrap();
        assert_eq!(ev["csp"]["blocked_uri"], "https://evil.example/x.js");
        assert_eq!(ev["csp"]["effective_directive"], "script-src");
        assert_eq!(ev["environment"], "prod");
        let modern = json!([{"type": "csp-violation", "url": "https://a", "user_agent": "UA2", "body": {"documentURL": "https://a/p",
            "blockedURL": "inline", "effectiveDirective": "style-src-elem", "originalPolicy": "x"}}, {"type": "deprecation", "body": {}}]);
        let e = security_envelope(Bytes::from(modern.to_string()), None, None).unwrap();
        assert_eq!(e.items.len(), 2);
        let ev: Value = serde_json::from_slice(&e.items[0].payload).unwrap();
        assert_eq!(ev["csp"]["document_uri"], "https://a/p");
        assert_eq!(ev["csp"]["blocked_uri"], "inline");
        assert_eq!(e.items[1].ty, "report");
    }

    #[test]
    fn legacy_store_bodies() {
        use std::io::Write;
        let json = br#"{"event_id":"0123456789abcdef0123456789abcdef","message":"m"}"#;
        let e = store_envelope(Bytes::from_static(json), 1 << 20).unwrap();
        assert_eq!(e.event_id(), Some("0123456789abcdef0123456789abcdef"));
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(json).unwrap();
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(z.finish().unwrap());
        let e = store_envelope(Bytes::from(b64), 1 << 20).unwrap();
        assert_eq!(&e.items[0].payload[..], &json[..]);
    }
}
