//! Normalization: turns the event JSON an SDK sent into an [`Event`].
//!
//! SDKs differ in how they spell the same thing (tags as an object or a
//! list of pairs, timestamps as numbers or strings, `exception` as a list
//! or `{values: [...]}`, `message` as a string or an object, ...).
//! Normalization accepts all common forms, fills in defaults, derives
//! tags and contexts, caps sizes, and never fails: whatever cannot be
//! understood is dropped and recorded in `errors`.

use std::net::IpAddr;

use chrono::{DateTime, Duration, NaiveDateTime, TimeZone, Utc};
use serde_json::{Map, Value};

use crate::event::*;
use crate::settings::ProjectSettings;

/// Breadcrumbs kept per event (the most recent ones).
pub const MAX_BREADCRUMBS: usize = 100;
/// Frames kept per stack trace (the oldest and newest halves).
pub const MAX_FRAMES: usize = 250;
/// Longest string kept inside free-form data (extra, contexts, vars, ...).
pub const MAX_STRING: usize = 8192;
/// Longest message and exception value.
pub const MAX_MESSAGE: usize = 8192;
/// Longest tag key and value.
pub const MAX_TAG_KEY: usize = 200;
pub const MAX_TAG_VALUE: usize = 200;
/// Deepest nesting kept in free-form data.
pub const MAX_DEPTH: usize = 8;
/// Most items kept in an array or object in free-form data.
pub const MAX_ITEMS: usize = 200;

const PLATFORMS: &[&str] = &[
    "as3",
    "c",
    "cfml",
    "cocoa",
    "csharp",
    "elixir",
    "go",
    "groovy",
    "haskell",
    "java",
    "javascript",
    "native",
    "node",
    "objc",
    "other",
    "perl",
    "php",
    "python",
    "ruby",
    "rust",
    "dart",
    "swift",
    "kotlin",
];

/// What normalization knows besides the event itself.
#[derive(Debug, Clone)]
pub struct Context<'a> {
    pub project_id: i64,
    pub event_id: String,
    pub received: DateTime<Utc>,
    /// The address the event was sent from (after trusted proxies).
    pub client_ip: Option<IpAddr>,
    /// `User-Agent` of the ingest request (the browser for browser SDKs).
    pub user_agent: Option<&'a str>,
    pub settings: &'a ProjectSettings,
}

/// Normalizes an event payload. `raw` must be a JSON object; anything else
/// yields an empty event with an error.
pub fn normalize(raw: &Value, ctx: &Context<'_>) -> Event {
    normalize_owned(raw.clone(), ctx)
}

/// Like [`normalize`], taking the parsed payload by value so that large
/// free-form parts (extra, contexts, frame variables, ...) are moved
/// instead of copied.
pub fn normalize_owned(mut raw: Value, ctx: &Context<'_>) -> Event {
    let mut ev = Event::empty(ctx.event_id.clone(), ctx.project_id, ctx.received);
    let Some(obj) = raw.as_object_mut() else {
        ev.errors.push(ProcessingError::new("invalid_data", Some("event".to_string()), None));
        ev.title = "<invalid event>".into();
        return ev;
    };

    // Timestamp: SDK time, unless it is far in the future (clock skew).
    if let Some(v) = obj.get("timestamp") {
        match parse_timestamp(v) {
            Some(ts) if ts <= ctx.received + Duration::minutes(1) => ev.timestamp = ts,
            Some(_) => {
                ev.errors.push(ProcessingError::new("future_timestamp", Some("timestamp".into()), Some(v.clone())))
            }
            None => ev.errors.push(ProcessingError::new("invalid_data", Some("timestamp".into()), Some(v.clone()))),
        }
    }

    ev.platform = obj
        .get("platform")
        .and_then(Value::as_str)
        .map(|p| p.to_ascii_lowercase())
        .filter(|p| PLATFORMS.contains(&p.as_str()))
        .unwrap_or_else(|| "other".into());
    ev.level = obj.get("level").and_then(Value::as_str).map(normalize_level).unwrap_or("error").to_string();
    ev.logger = short_str(obj.get("logger"), 128);
    ev.environment = short_str(obj.get("environment"), 64);
    ev.release = short_str(obj.get("release"), 250);
    ev.dist = short_str(obj.get("dist"), 64);
    ev.server_name = short_str(obj.get("server_name"), 250);
    ev.transaction = short_str(obj.get("transaction"), 200);
    ev.fingerprint = match obj.get("fingerprint") {
        Some(Value::Array(a)) => a.iter().filter_map(scalar_string).map(|s| truncate(&s, 1000)).collect(),
        _ => Vec::new(),
    };
    ev.message = message(obj);

    ev.sdk = obj.get("sdk").and_then(Value::as_object).map(sdk);
    ev.user = obj.get_mut("user").and_then(Value::as_object_mut).map(user).filter(|u| !u.is_empty());
    ev.request = obj.get_mut("request").and_then(Value::as_object_mut).map(request);
    ev.contexts = contexts(obj.remove("contexts"));
    ev.exception = exceptions(obj.get_mut("exception"), ctx.settings, &ev.platform);
    ev.threads = threads(obj.get_mut("threads"), ctx.settings, &ev.platform);
    // A top-level stack trace (message with attached stack trace) is shown
    // as the current thread.
    if let Some(st) = obj.get_mut("stacktrace").and_then(|v| stacktrace(v, ctx.settings, &ev.platform)) {
        let thread = Thread { current: Some(true), crashed: Some(false), stacktrace: Some(st), ..Thread::default() };
        match &mut ev.threads {
            Some(t) if t.values.iter().all(|t| t.stacktrace.is_none()) => t.values.insert(0, thread),
            Some(_) => {}
            None => ev.threads = Some(Values { values: vec![thread] }),
        }
    }
    ev.breadcrumbs = breadcrumbs(obj.get_mut("breadcrumbs"));
    ev.extra = object_trimmed(obj.remove("extra"));
    ev.modules = obj
        .get("modules")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter().take(1000).map(|(k, v)| (k.clone(), Value::String(scalar_string(v).unwrap_or_default()))).collect()
        })
        .unwrap_or_default();
    ev.debug_meta = obj.remove("debug_meta").filter(|v| v.is_object());
    ev.csp = obj.remove("csp").filter(|v| v.is_object()).map(|v| trim_owned(v, 0));

    // User: IP address from the connection where the SDK asks for it.
    fill_ip(&mut ev, obj, ctx);

    // Browser and OS from the User-Agent when the SDK sent no context.
    let ua = ev
        .request
        .as_ref()
        .and_then(|r| r.header("user-agent"))
        .map(str::to_string)
        .or_else(|| if ev.platform == "javascript" { ctx.user_agent.map(str::to_string) } else { None });
    if let Some(ua) = ua {
        let parsed = parse_user_agent(&ua);
        if !ev.contexts.contains_key("browser")
            && let Some((name, version)) = parsed.browser
        {
            ev.contexts
                .insert("browser".into(), serde_json::json!({"type": "browser", "name": name, "version": version}));
        }
        if !ev.contexts.contains_key("os")
            && let Some((name, version)) = parsed.os
        {
            let mut os = serde_json::json!({"type": "os", "name": name});
            if let Some(v) = version {
                os["version"] = Value::String(v);
            }
            ev.contexts.insert("os".into(), os);
        }
    }

    ev.ty = if !ev.exceptions().is_empty() {
        "error".into()
    } else if ev.csp.is_some() {
        "csp".into()
    } else {
        "default".into()
    };

    ev.tags = tags(obj.get("tags"));
    derive_tags(&mut ev);
    ev
}

/// `{{auto}}` (or `sdk.settings.infer_ip = "auto"`, or a browser SDK that
/// sent no address) means: use the address the event came from.
fn fill_ip(ev: &mut Event, obj: &Map<String, Value>, ctx: &Context<'_>) {
    let infer = obj.get("sdk").and_then(|s| s.get("settings")).and_then(|s| s.get("infer_ip")).and_then(Value::as_str);
    let current = ev.user.as_ref().and_then(|u| u.ip_address.clone());
    let wants_auto = match current.as_deref() {
        Some("{{auto}}") => true,
        Some(_) => false,
        None => match infer {
            Some("auto") => true,
            Some(_) => false,
            None => {
                ev.platform == "javascript"
                    && ev.sdk.as_ref().and_then(|s| s.name.as_deref()).is_some_and(|n| !n.contains("node"))
            }
        },
    };
    // A request env REMOTE_ADDR of `{{auto}}` is treated the same way.
    if let Some(env) = ev.request.as_mut().and_then(|r| r.env.as_mut()).and_then(Value::as_object_mut)
        && env.get("REMOTE_ADDR").and_then(Value::as_str) == Some("{{auto}}")
    {
        match ctx.client_ip {
            Some(ip) => env.insert("REMOTE_ADDR".into(), Value::String(ip.to_string())),
            None => env.remove("REMOTE_ADDR"),
        };
    }
    if wants_auto {
        let user = ev.user.get_or_insert_with(User::default);
        user.ip_address = ctx.client_ip.map(|ip| ip.to_string());
        if user.is_empty() {
            ev.user = None;
        }
    } else if let Some(user) = &mut ev.user
        && let Some(ip) = &user.ip_address
        && ip.parse::<IpAddr>().is_err()
    {
        user.ip_address = None;
    }
}

fn derive_tags(ev: &mut Event) {
    let mut derived: Vec<(String, String)> = Vec::new();
    let mut add = |k: &str, v: Option<String>| {
        if let Some(v) = v.filter(|v| !v.is_empty()) {
            derived.push((k.to_string(), truncate(&v, MAX_TAG_VALUE)));
        }
    };
    add("level", Some(ev.level.clone()));
    add("environment", ev.environment.clone());
    add("release", ev.release.clone());
    add("dist", ev.dist.clone());
    add("transaction", ev.transaction.clone());
    add("server_name", ev.server_name.clone());
    add("logger", ev.logger.clone());
    for key in ["browser", "os", "runtime", "device"] {
        add(key, ev.context_summary(key));
        let name = ev.contexts.get(key).and_then(|c| c.get("name")).and_then(Value::as_str).map(str::to_string);
        if key != "device" {
            add(&format!("{key}.name"), name);
        }
    }
    add("url", ev.request.as_ref().and_then(|r| r.url.clone()));
    add("user", ev.user.as_ref().and_then(User::identity));
    add("handled", ev.handled().map(|h| if h { "yes" } else { "no" }.to_string()));
    add(
        "mechanism",
        ev.main_exception()
            .and_then(|e| e.mechanism.as_ref())
            .and_then(|m| m.get("type"))
            .and_then(Value::as_str)
            .map(str::to_string),
    );
    for (k, v) in derived {
        if !ev.tags.iter().any(|(tk, _)| *tk == k) {
            ev.tags.push((k, v));
        }
    }
}

pub fn normalize_level(level: &str) -> &'static str {
    match level.to_ascii_lowercase().as_str() {
        "fatal" | "critical" => "fatal",
        "error" | "err" => "error",
        "warning" | "warn" => "warning",
        "info" | "log" | "notice" => "info",
        "debug" | "trace" => "debug",
        _ => "error",
    }
}

/// Parses a timestamp: seconds since the epoch (number or numeric string)
/// or an RFC 3339 / ISO 8601 string (a missing offset means UTC).
pub fn parse_timestamp(v: &Value) -> Option<DateTime<Utc>> {
    match v {
        Value::Number(n) => from_epoch(n.as_f64()?),
        Value::String(s) => {
            let s = s.trim();
            if let Ok(f) = s.parse::<f64>() {
                return from_epoch(f);
            }
            if let Ok(t) = DateTime::parse_from_rfc3339(s) {
                return Some(t.with_timezone(&Utc));
            }
            for fmt in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M:%S"] {
                if let Ok(t) = NaiveDateTime::parse_from_str(s, fmt) {
                    return Some(Utc.from_utc_datetime(&t));
                }
            }
            None
        }
        _ => None,
    }
}

fn from_epoch(f: f64) -> Option<DateTime<Utc>> {
    if !f.is_finite() || !(0.0..1e11).contains(&f) {
        return None;
    }
    let secs = f.trunc() as i64;
    let micros = ((f - f.trunc()) * 1e6).round() as u32;
    Utc.timestamp_opt(secs, micros.min(999_999) * 1000).single()
}

/// Cuts a string to at most `max` characters, marking the cut with `...`.
pub fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max.saturating_sub(3);
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &s[..end])
}

/// Strings, numbers and booleans as a string; nothing for other values.
pub fn scalar_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn short_str(v: Option<&Value>, max: usize) -> Option<String> {
    match v? {
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() { None } else { Some(truncate(t, max)) }
        }
        other => scalar_string(other).filter(|s| !s.is_empty()).map(|s| truncate(&s, max)),
    }
}

/// Caps strings, arrays, objects and depth of free-form data.
pub fn trim(v: &Value, depth: usize) -> Value {
    match v {
        Value::String(s) => Value::String(truncate(s, MAX_STRING)),
        Value::Array(a) if depth >= MAX_DEPTH => Value::String(format!("[{} items]", a.len())),
        Value::Object(o) if depth >= MAX_DEPTH => Value::String(format!("{{{} keys}}", o.len())),
        Value::Array(a) => Value::Array(a.iter().take(MAX_ITEMS).map(|x| trim(x, depth + 1)).collect()),
        Value::Object(o) => {
            Value::Object(o.iter().take(MAX_ITEMS).map(|(k, x)| (truncate(k, 256), trim(x, depth + 1))).collect())
        }
        other => other.clone(),
    }
}

/// Caps strings, arrays, objects and depth of free-form data in place.
pub fn trim_owned(mut v: Value, depth: usize) -> Value {
    trim_in_place(&mut v, depth);
    v
}

fn trim_in_place(v: &mut Value, depth: usize) {
    match v {
        Value::String(s) => {
            if s.len() > MAX_STRING {
                *s = truncate(s, MAX_STRING);
            }
        }
        Value::Array(a) if depth >= MAX_DEPTH => *v = Value::String(format!("[{} items]", a.len())),
        Value::Object(o) if depth >= MAX_DEPTH => *v = Value::String(format!("{{{} keys}}", o.len())),
        Value::Array(a) => {
            a.truncate(MAX_ITEMS);
            a.iter_mut().for_each(|x| trim_in_place(x, depth + 1));
        }
        Value::Object(o) => {
            if o.len() > MAX_ITEMS || o.keys().any(|k| k.len() > 256) {
                *v = trim(v, depth);
            } else {
                o.values_mut().for_each(|x| trim_in_place(x, depth + 1));
            }
        }
        _ => {}
    }
}

fn object_trimmed(v: Option<Value>) -> Object {
    match v.map(|v| trim_owned(v, 0)) {
        Some(Value::Object(o)) => o,
        _ => Object::new(),
    }
}

/// `message` (string or `{message, params, formatted}`) or `logentry`.
fn message(obj: &Map<String, Value>) -> Option<String> {
    let from = |v: &Value| -> Option<String> {
        match v {
            Value::String(s) => Some(s.clone()),
            Value::Object(o) => {
                if let Some(f) = o.get("formatted").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                    return Some(f.to_string());
                }
                let msg = o.get("message").and_then(Value::as_str)?;
                Some(match o.get("params") {
                    Some(p) => format_message(msg, p),
                    None => msg.to_string(),
                })
            }
            _ => None,
        }
    };
    obj.get("logentry")
        .and_then(from)
        .or_else(|| obj.get("message").and_then(from))
        .map(|s| truncate(s.trim_end(), MAX_MESSAGE))
        .filter(|s| !s.is_empty())
}

/// Formats a log message with its parameters: `%s`-style placeholders
/// (positional, or `%(name)s` with an object) and `{}` / `{0}` braces.
pub fn format_message(msg: &str, params: &Value) -> String {
    let as_text = |v: &Value| scalar_string(v).unwrap_or_else(|| v.to_string());
    match params {
        Value::Array(list) => {
            let mut out = String::with_capacity(msg.len());
            let mut next = 0usize;
            let mut chars = msg.chars().peekable();
            while let Some(c) = chars.next() {
                match c {
                    '%' => match chars.peek().copied() {
                        Some('%') => {
                            chars.next();
                            out.push('%');
                        }
                        Some(spec) if "sdifrx".contains(spec) => {
                            chars.next();
                            match list.get(next) {
                                Some(v) => out.push_str(&as_text(v)),
                                None => {
                                    out.push('%');
                                    out.push(spec);
                                }
                            }
                            next += 1;
                        }
                        _ => out.push('%'),
                    },
                    '{' => {
                        let mut inner = String::new();
                        let mut closed = false;
                        let mut look = chars.clone();
                        for d in look.by_ref() {
                            if d == '}' {
                                closed = true;
                                break;
                            }
                            inner.push(d);
                            if inner.len() > 4 {
                                break;
                            }
                        }
                        let idx = if inner.is_empty() { Some(next) } else { inner.parse::<usize>().ok() };
                        match (closed, idx.and_then(|i| list.get(i))) {
                            (true, Some(v)) => {
                                out.push_str(&as_text(v));
                                if inner.is_empty() {
                                    next += 1;
                                }
                                chars = look;
                            }
                            _ => out.push('{'),
                        }
                    }
                    c => out.push(c),
                }
            }
            out
        }
        Value::Object(map) => {
            let mut out = msg.to_string();
            for (k, v) in map {
                out = out.replace(&format!("%({k})s"), &as_text(v)).replace(&format!("{{{k}}}"), &as_text(v));
            }
            out
        }
        _ => msg.to_string(),
    }
}

fn sdk(o: &Map<String, Value>) -> Sdk {
    Sdk {
        name: short_str(o.get("name"), 128),
        version: short_str(o.get("version"), 64),
        integrations: o
            .get("integrations")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(scalar_string).take(100).collect())
            .unwrap_or_default(),
        packages: o
            .get("packages")
            .and_then(Value::as_array)
            .map(|a| a.iter().take(50).cloned().collect())
            .unwrap_or_default(),
    }
}

fn user(o: &mut Map<String, Value>) -> User {
    let mut data = match o.remove("data").map(|v| trim_owned(v, 1)) {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    };
    for (k, v) in o.iter() {
        if !matches!(k.as_str(), "id" | "email" | "username" | "ip_address" | "name" | "geo" | "data") && !v.is_null() {
            data.insert(k.clone(), trim(v, 1));
        }
    }
    User {
        id: short_str(o.get("id"), 128),
        email: short_str(o.get("email"), 256),
        username: short_str(o.get("username"), 128),
        ip_address: short_str(o.get("ip_address"), 64),
        name: short_str(o.get("name"), 128),
        geo: o.remove("geo").filter(|v| v.is_object()).map(|v| trim_owned(v, 1)),
        data: if data.is_empty() { None } else { Some(Value::Object(data)) },
    }
}

fn pairs(v: Option<&Value>) -> Vec<(String, String)> {
    match v {
        Some(Value::Object(o)) => o
            .iter()
            .filter_map(|(k, v)| match v {
                Value::Array(a) => Some((k.clone(), a.iter().filter_map(scalar_string).collect::<Vec<_>>().join(", "))),
                other => scalar_string(other).map(|s| (k.clone(), s)),
            })
            .take(MAX_ITEMS)
            .map(|(k, v)| (truncate(&k, 256), truncate(&v, MAX_STRING)))
            .collect(),
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|p| {
                let p = p.as_array()?;
                Some((scalar_string(p.first()?)?, p.get(1).and_then(scalar_string).unwrap_or_default()))
            })
            .take(MAX_ITEMS)
            .map(|(k, v)| (truncate(&k, 256), truncate(&v, MAX_STRING)))
            .collect(),
        _ => Vec::new(),
    }
}

fn request(o: &mut Map<String, Value>) -> Request {
    let query_string = match o.get("query_string") {
        Some(Value::String(s)) => Some(s.trim_start_matches('?').to_string()),
        v @ Some(Value::Object(_) | Value::Array(_)) => {
            Some(pairs(v).into_iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&"))
        }
        _ => None,
    }
    .filter(|s| !s.is_empty())
    .map(|s| truncate(&s, MAX_STRING));
    let cookies = match o.get("cookies") {
        Some(Value::String(s)) => s
            .split(';')
            .filter_map(|c| {
                let c = c.trim();
                if c.is_empty() {
                    return None;
                }
                let (k, v) = c.split_once('=').unwrap_or((c, ""));
                Some((k.trim().to_string(), v.trim().to_string()))
            })
            .take(MAX_ITEMS)
            .collect(),
        other => pairs(other),
    };
    Request {
        url: short_str(o.get("url"), 2048),
        method: short_str(o.get("method"), 16).map(|m| m.to_ascii_uppercase()),
        headers: pairs(o.get("headers")),
        query_string,
        data: o.remove("data").filter(|v| !v.is_null()).map(|v| trim_owned(v, 1)),
        cookies,
        env: o.remove("env").filter(|v| v.is_object()).map(|v| trim_owned(v, 1)),
    }
}

fn contexts(v: Option<Value>) -> Object {
    let mut out = Object::new();
    if let Some(Value::Object(o)) = v {
        for (k, v) in o.into_iter().take(MAX_ITEMS) {
            if let Value::Object(_) = v {
                let mut c = trim_owned(v, 1);
                if let Value::Object(m) = &mut c
                    && !m.contains_key("type")
                {
                    m.insert("type".into(), Value::String(k.clone()));
                }
                out.insert(truncate(&k, 256), c);
            }
        }
    }
    out
}

fn tags(v: Option<&Value>) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for (k, v) in pairs(v) {
        let k = truncate(k.trim(), MAX_TAG_KEY);
        if k.is_empty() || out.iter().any(|(ok, _)| *ok == k) {
            continue;
        }
        out.push((k, truncate(&v, MAX_TAG_VALUE)));
    }
    out
}

fn list_of_mut(v: Option<&mut Value>) -> Option<&mut Vec<Value>> {
    match v? {
        Value::Array(a) => Some(a),
        Value::Object(o) => o.get_mut("values").and_then(Value::as_array_mut),
        _ => None,
    }
}

fn exceptions(v: Option<&mut Value>, settings: &ProjectSettings, platform: &str) -> Option<Values<Exception>> {
    let mut single;
    let list = match v {
        Some(Value::Object(o)) if !o.contains_key("values") => {
            single = vec![Value::Object(std::mem::take(o))];
            &mut single
        }
        other => list_of_mut(other)?,
    };
    let values: Vec<Exception> = list
        .iter_mut()
        .filter_map(Value::as_object_mut)
        .take(32)
        .map(|e| Exception {
            ty: short_str(e.get("type"), 256),
            value: e.get("value").and_then(scalar_string).map(|s| truncate(&s, MAX_MESSAGE)),
            module: short_str(e.get("module"), 256),
            thread_id: e.get("thread_id").cloned(),
            mechanism: e.remove("mechanism").filter(|m| m.is_object()).map(|m| trim_owned(m, 1)),
            stacktrace: e.get_mut("stacktrace").and_then(|s| stacktrace(s, settings, platform)),
        })
        .filter(|e| e.ty.is_some() || e.value.is_some() || e.stacktrace.is_some())
        .collect();
    if values.is_empty() { None } else { Some(Values { values }) }
}

fn threads(v: Option<&mut Value>, settings: &ProjectSettings, platform: &str) -> Option<Values<Thread>> {
    let values: Vec<Thread> = list_of_mut(v)?
        .iter_mut()
        .filter_map(Value::as_object_mut)
        .take(64)
        .map(|t| Thread {
            id: t.get("id").filter(|v| !v.is_null()).cloned(),
            name: short_str(t.get("name"), 256),
            crashed: t.get("crashed").and_then(Value::as_bool),
            current: t.get("current").and_then(Value::as_bool),
            main: t.get("main").and_then(Value::as_bool),
            stacktrace: t.get_mut("stacktrace").and_then(|s| stacktrace(s, settings, platform)),
        })
        .collect();
    if values.is_empty() { None } else { Some(Values { values }) }
}

/// A stack trace; `None` without frames.
pub fn stacktrace(v: &mut Value, settings: &ProjectSettings, platform: &str) -> Option<Stacktrace> {
    let frames = v.get_mut("frames")?.as_array_mut()?;
    let mut out: Vec<Frame> =
        frames.iter_mut().filter_map(Value::as_object_mut).map(|f| frame(f, settings, platform)).collect();
    if out.is_empty() {
        return None;
    }
    if out.len() > MAX_FRAMES {
        let keep_tail = MAX_FRAMES / 2;
        let tail = out.split_off(out.len() - keep_tail);
        out.truncate(MAX_FRAMES - keep_tail);
        out.extend(tail);
    }
    let registers = v.as_object_mut().and_then(|o| o.remove("registers")).filter(|r| r.is_object());
    Some(Stacktrace { frames: out, registers })
}

fn number(v: Option<&Value>) -> Option<u32> {
    match v? {
        Value::Number(n) => n.as_u64().map(|n| n.min(u32::MAX as u64) as u32),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn lines(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().take(20).map(|l| truncate(&scalar_string(l).unwrap_or_default(), 1024)).collect())
        .unwrap_or_default()
}

fn frame(f: &mut Map<String, Value>, settings: &ProjectSettings, platform: &str) -> Frame {
    let mut fr = Frame {
        filename: short_str(f.get("filename"), 1024),
        abs_path: short_str(f.get("abs_path"), 2048),
        module: short_str(f.get("module"), 512),
        function: short_str(f.get("function"), 512),
        lineno: number(f.get("lineno")),
        colno: number(f.get("colno")),
        in_app: f.get("in_app").and_then(Value::as_bool),
        context_line: f.get("context_line").and_then(scalar_string).map(|s| truncate(&s, 1024)),
        pre_context: lines(f.get("pre_context")),
        post_context: lines(f.get("post_context")),
        vars: f.remove("vars").filter(|v| v.is_object()).map(|v| trim_owned(v, 2)),
        platform: short_str(f.get("platform"), 32),
        package: short_str(f.get("package"), 512),
        instruction_addr: short_str(f.get("instruction_addr"), 32),
        symbolicated: false,
        raw: None,
    };
    if fr.filename.is_none()
        && let Some(abs) = &fr.abs_path
    {
        fr.filename = Some(abs.clone());
    }
    fr.in_app = Some(in_app(&fr, settings, platform));
    fr
}

/// Decides whether a frame belongs to the application: project settings
/// first (exclude wins over include), then what the SDK said, then
/// path heuristics for dependencies and runtimes.
pub fn in_app(fr: &Frame, settings: &ProjectSettings, platform: &str) -> bool {
    let candidates = [fr.module.as_deref(), fr.abs_path.as_deref(), fr.filename.as_deref()];
    let matches = |prefixes: &[String]| {
        candidates.iter().flatten().any(|c| prefixes.iter().any(|p| !p.is_empty() && c.starts_with(p.as_str())))
    };
    if matches(&settings.in_app_exclude) {
        return false;
    }
    if matches(&settings.in_app_include) {
        return true;
    }
    if let Some(v) = fr.in_app {
        return v;
    }
    let path = fr.abs_path.as_deref().or(fr.filename.as_deref()).unwrap_or("");
    let lib_markers = [
        "node_modules/",
        "/site-packages/",
        "/dist-packages/",
        "/vendor/",
        "/lib/python",
        "/usr/lib/",
        "/usr/local/lib/",
        "chrome-extension://",
        "moz-extension://",
        "safari-extension://",
        "webpack/bootstrap",
    ];
    if path.starts_with("node:") || path.starts_with("internal/") || path == "<anonymous>" || path == "native" {
        return false;
    }
    if lib_markers.iter().any(|m| path.contains(m)) {
        return false;
    }
    if let Some(m) = &fr.module
        && platform == "java"
        && ["java.", "javax.", "sun.", "kotlin.", "jdk."].iter().any(|p| m.starts_with(p))
    {
        return false;
    }
    !path.is_empty() || fr.module.is_some()
}

fn breadcrumbs(v: Option<&mut Value>) -> Option<Values<Breadcrumb>> {
    let list = list_of_mut(v)?;
    let skip = list.len().saturating_sub(MAX_BREADCRUMBS);
    let values: Vec<Breadcrumb> = list
        .iter_mut()
        .skip(skip)
        .filter_map(Value::as_object_mut)
        .map(|b| Breadcrumb {
            timestamp: b.get("timestamp").and_then(parse_timestamp),
            ty: short_str(b.get("type"), 64),
            category: short_str(b.get("category"), 128),
            level: b.get("level").and_then(Value::as_str).map(|l| normalize_level(l).to_string()),
            message: b.get("message").and_then(scalar_string).map(|s| truncate(&s, MAX_MESSAGE)),
            data: b.remove("data").filter(|d| !d.is_null()).map(|d| trim_owned(d, 2)),
        })
        .collect();
    if values.is_empty() { None } else { Some(Values { values }) }
}

/// What can be read from a `User-Agent` header.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct UserAgent {
    /// Browser name and major.minor version.
    pub browser: Option<(String, String)>,
    pub os: Option<(String, Option<String>)>,
    pub is_bot: bool,
}

fn version_after(ua: &str, marker: &str) -> Option<String> {
    let start = ua.find(marker)? + marker.len();
    let v: String = ua[start..].chars().take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '_').collect();
    let v = v.replace('_', ".");
    let mut parts = v.split('.').filter(|p| !p.is_empty());
    let major = parts.next()?;
    Some(match parts.next() {
        Some(minor) => format!("{major}.{minor}"),
        None => major.to_string(),
    })
}

/// A small User-Agent parser covering the common browsers, operating
/// systems and crawlers. Unknown agents yield nothing.
pub fn parse_user_agent(ua: &str) -> UserAgent {
    let lower = ua.to_ascii_lowercase();
    let bots = [
        "bot",
        "crawler",
        "spider",
        "slurp",
        "facebookexternalhit",
        "mediapartners-google",
        "headlesschrome",
        "lighthouse",
        "pingdom",
        "uptimerobot",
        "bingpreview",
        "yandex",
        "baiduspider",
        "duckduckgo",
        "embedly",
        "quora link preview",
        "outbrain",
        "pinterest",
        "developers.google.com/+/web/snippet",
        "vkshare",
        "w3c_validator",
        "python-requests",
        "curl/",
        "wget/",
        "go-http-client",
    ];
    let is_bot = bots.iter().any(|b| lower.contains(b));
    let browser = if let Some(v) = version_after(ua, "Edg/").or_else(|| version_after(ua, "Edge/")) {
        Some(("Edge".to_string(), v))
    } else if let Some(v) = version_after(ua, "OPR/").or_else(|| version_after(ua, "Opera/")) {
        Some(("Opera".to_string(), v))
    } else if let Some(v) = version_after(ua, "SamsungBrowser/") {
        Some(("Samsung Internet".to_string(), v))
    } else if let Some(v) = version_after(ua, "Firefox/").or_else(|| version_after(ua, "FxiOS/")) {
        Some(("Firefox".to_string(), v))
    } else if let Some(v) = version_after(ua, "CriOS/") {
        Some(("Chrome Mobile iOS".to_string(), v))
    } else if let Some(v) = version_after(ua, "Chrome/") {
        Some((if lower.contains("mobile") { "Chrome Mobile" } else { "Chrome" }.to_string(), v))
    } else if let Some(v) = version_after(ua, "MSIE ") {
        Some(("IE".to_string(), v))
    } else if lower.contains("trident/") {
        Some(("IE".to_string(), version_after(ua, "rv:").unwrap_or_else(|| "11.0".into())))
    } else if lower.contains("safari/") && lower.contains("version/") {
        let v = version_after(ua, "Version/").unwrap_or_default();
        Some((if lower.contains("mobile") { "Mobile Safari" } else { "Safari" }.to_string(), v))
    } else {
        None
    };
    let os = if lower.contains("windows nt") {
        let v = version_after(ua, "Windows NT ");
        let name = match v.as_deref() {
            Some("10.0") => Some("10".to_string()),
            Some("6.3") => Some("8.1".to_string()),
            Some("6.2") => Some("8".to_string()),
            Some("6.1") => Some("7".to_string()),
            Some("6.0") => Some("Vista".to_string()),
            Some("5.1") => Some("XP".to_string()),
            other => other.map(str::to_string),
        };
        Some(("Windows".to_string(), name))
    } else if lower.contains("iphone") || lower.contains("ipad") || lower.contains("ipod") {
        Some(("iOS".to_string(), version_after(ua, "OS ")))
    } else if lower.contains("android") {
        Some(("Android".to_string(), version_after(ua, "Android ")))
    } else if lower.contains("mac os x") {
        Some(("Mac OS X".to_string(), version_after(ua, "Mac OS X ")))
    } else if lower.contains("cros") {
        Some(("Chrome OS".to_string(), None))
    } else if lower.contains("linux") {
        Some(("Linux".to_string(), None))
    } else {
        None
    };
    UserAgent { browser, os, is_bot }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx(settings: &ProjectSettings) -> Context<'_> {
        Context {
            project_id: 1,
            event_id: "0".repeat(32),
            received: Utc.with_ymd_and_hms(2026, 11, 14, 12, 0, 0).unwrap(),
            client_ip: Some("203.0.113.9".parse().unwrap()),
            user_agent: None,
            settings,
        }
    }

    #[test]
    fn accepts_both_tag_forms_and_derives_tags() {
        let s = ProjectSettings::default();
        let a = normalize(&json!({"tags": {"a": "1", "n": 2}, "environment": "prod"}), &ctx(&s));
        let b = normalize(&json!({"tags": [["a", "1"], ["n", "2"]], "environment": "prod"}), &ctx(&s));
        assert_eq!(a.tags, b.tags);
        assert_eq!(a.tag("a"), Some("1"));
        assert_eq!(a.tag("n"), Some("2"));
        assert_eq!(a.tag("environment"), Some("prod"));
        assert_eq!(a.tag("level"), Some("error"));
    }

    #[test]
    fn timestamps_in_all_forms() {
        let t = Utc.with_ymd_and_hms(2026, 11, 14, 11, 0, 0).unwrap();
        for v in [
            json!(t.timestamp() as f64),
            json!("2026-11-14T11:00:00Z"),
            json!("2026-11-14T11:00:00"),
            json!("2026-11-14T12:00:00+01:00"),
        ] {
            assert_eq!(parse_timestamp(&v), Some(t), "{v}");
        }
        let s = ProjectSettings::default();
        // Far future: replaced by the receive time and recorded as an error.
        let e = normalize(&json!({"timestamp": "2030-01-01T00:00:00Z"}), &ctx(&s));
        assert_eq!(e.timestamp, e.received);
        assert_eq!(e.errors[0].ty, "future_timestamp");
    }

    #[test]
    fn exceptions_as_list_object_or_values() {
        let s = ProjectSettings::default();
        let frames = json!({"frames": [{"filename": "app.js", "function": "f", "lineno": "3", "colno": 7}]});
        for v in [
            json!({"values": [{"type": "TypeError", "value": "x", "stacktrace": frames}]}),
            json!([{"type": "TypeError", "value": "x", "stacktrace": frames}]),
            json!({"type": "TypeError", "value": "x", "stacktrace": frames}),
        ] {
            let e = normalize(&json!({"exception": v, "platform": "javascript"}), &ctx(&s));
            assert_eq!(e.ty, "error");
            let ex = e.main_exception().unwrap();
            assert_eq!(ex.ty.as_deref(), Some("TypeError"));
            let f = &ex.stacktrace.as_ref().unwrap().frames[0];
            assert_eq!((f.lineno, f.colno, f.in_app), (Some(3), Some(7), Some(true)));
        }
    }

    #[test]
    fn mechanism_drives_handled_tag() {
        let s = ProjectSettings::default();
        let e = normalize(
            &json!({"exception": {"values": [{"type": "E", "mechanism": {"type": "onunhandledrejection", "handled": false}}]}}),
            &ctx(&s),
        );
        assert_eq!(e.tag("handled"), Some("no"));
        assert_eq!(e.tag("mechanism"), Some("onunhandledrejection"));
    }

    #[test]
    fn breadcrumbs_are_capped_to_the_latest() {
        let s = ProjectSettings::default();
        let crumbs: Vec<Value> = (0..150).map(|i| json!({"message": format!("c{i}"), "timestamp": 1.7e9})).collect();
        let e = normalize(&json!({"breadcrumbs": {"values": crumbs}}), &ctx(&s));
        let b = e.breadcrumbs.unwrap().values;
        assert_eq!(b.len(), MAX_BREADCRUMBS);
        assert_eq!(b[0].message.as_deref(), Some("c50"));
        assert_eq!(b[99].message.as_deref(), Some("c149"));
    }

    #[test]
    fn auto_ip_uses_the_client_address() {
        let s = ProjectSettings::default();
        let e = normalize(&json!({"user": {"id": 42, "ip_address": "{{auto}}"}}), &ctx(&s));
        let u = e.user.unwrap();
        assert_eq!(u.id.as_deref(), Some("42"));
        assert_eq!(u.ip_address.as_deref(), Some("203.0.113.9"));
        let e = normalize(&json!({"sdk": {"name": "x", "settings": {"infer_ip": "never"}}}), &ctx(&s));
        assert!(e.user.is_none());
        let e = normalize(&json!({"platform": "javascript", "sdk": {"name": "sentry.javascript.browser"}}), &ctx(&s));
        assert_eq!(e.user.unwrap().ip_address.as_deref(), Some("203.0.113.9"));
        let e = normalize(&json!({"platform": "node", "sdk": {"name": "sentry.javascript.node"}}), &ctx(&s));
        assert!(e.user.is_none());
    }

    #[test]
    fn message_formats() {
        let s = ProjectSettings::default();
        let e = normalize(&json!({"logentry": {"message": "user %s has %d items", "params": ["ann", 3]}}), &ctx(&s));
        assert_eq!(e.message.as_deref(), Some("user ann has 3 items"));
        let e = normalize(&json!({"message": {"message": "x {} y {0}", "params": ["a"]}}), &ctx(&s));
        assert_eq!(e.message.as_deref(), Some("x a y a"));
        let e = normalize(&json!({"message": {"formatted": "done", "message": "d%s"}}), &ctx(&s));
        assert_eq!(e.message.as_deref(), Some("done"));
        assert_eq!(format_message("%(name)s is 100%%", &json!({"name": "it"})), "it is 100%%");
        assert_eq!(format_message("100%% of %s", &json!(["x"])), "100% of x");
        assert_eq!(e.ty, "default");
    }

    #[test]
    fn request_shapes() {
        let s = ProjectSettings::default();
        let e = normalize(
            &json!({"request": {"url": "https://x/y", "method": "post", "headers": {"User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"},
                "query_string": {"a": "1"}, "cookies": "s=1; t=2"}}),
            &ctx(&s),
        );
        let r = e.request.as_ref().unwrap();
        assert_eq!(r.method.as_deref(), Some("POST"));
        assert_eq!(r.query_string.as_deref(), Some("a=1"));
        assert_eq!(r.cookies, vec![("s".into(), "1".into()), ("t".into(), "2".into())]);
        assert_eq!(e.tag("browser"), Some("Chrome 120.0"));
        assert_eq!(e.tag("os"), Some("Windows 10"));
    }

    #[test]
    fn user_agents() {
        let ua = parse_user_agent(
            "Mozilla/5.0 (iPhone; CPU iPhone OS 17_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.1 Mobile/15E148 Safari/604.1",
        );
        assert_eq!(ua.browser, Some(("Mobile Safari".into(), "17.1".into())));
        assert_eq!(ua.os, Some(("iOS".into(), Some("17.1".into()))));
        let ua = parse_user_agent("Mozilla/5.0 (compatible; MSIE 9.0; Windows NT 6.1; Trident/5.0)");
        assert_eq!(ua.browser, Some(("IE".into(), "9.0".into())));
        assert!(parse_user_agent("Mozilla/5.0 (compatible; Googlebot/2.1; +http://www.google.com/bot.html)").is_bot);
    }

    #[test]
    fn in_app_settings_and_heuristics() {
        let mut s = ProjectSettings::default();
        let e = normalize(
            &json!({"stacktrace": {"frames": [{"abs_path": "/app/node_modules/x/index.js"}, {"abs_path": "/app/src/a.js"}, {"module": "myapp.views", "in_app": false}]}}),
            &ctx(&s),
        );
        let frames = &e.threads.unwrap().values[0].stacktrace.clone().unwrap().frames;
        assert_eq!(frames.iter().map(|f| f.in_app.unwrap()).collect::<Vec<_>>(), vec![false, true, false]);
        s.in_app_include = vec!["myapp.".into()];
        let e = normalize(&json!({"stacktrace": {"frames": [{"module": "myapp.views", "in_app": false}]}}), &ctx(&s));
        assert_eq!(e.threads.unwrap().values[0].stacktrace.as_ref().unwrap().frames[0].in_app, Some(true));
    }

    #[test]
    fn garbage_never_panics() {
        let s = ProjectSettings::default();
        for v in [
            json!(null),
            json!([1, 2]),
            json!({"exception": 5, "tags": "x", "user": [], "breadcrumbs": {"values": [1, null]}, "timestamp": {}}),
            json!({"exception": {"values": [{"stacktrace": {"frames": [1, {"lineno": -4}]}}]}, "level": 7}),
            json!({"message": {"message": "{", "params": ["a"]}, "logentry": null}),
        ] {
            let _ = normalize(&v, &ctx(&s));
        }
        let deep = (0..200).fold(json!(1), |acc, _| json!({"a": acc}));
        let e = normalize(&json!({"extra": {"deep": deep}}), &ctx(&s));
        assert!(serde_json::to_string(&e).unwrap().len() < 2000);
        assert_eq!(truncate("héllo wörld", 6), "hé...");
    }
}
