//! Grouping: which issue an event belongs to.
//!
//! Version `v1` computes, in this order of preference:
//!
//! 1. the SDK's `fingerprint`, where `{{ default }}` expands to the
//!    components of the default grouping below;
//! 2. the exception chain: for every exception, its type and its in-app
//!    frames (all frames if none is in-app), each frame as module or file
//!    base name plus function, normalized; without frames, the type and
//!    the normalized value;
//! 3. the in-app frames of a stack trace (threads);
//! 4. a CSP report's directive and blocked host;
//! 5. the message with variable parts (numbers, hex, UUIDs, e-mail
//!    addresses, paths, URLs, quoted strings) replaced.
//!
//! The hash is the xxh3-128 of the components, as 32 hex digits. Line
//! numbers never take part, so a deploy that moves code keeps its issues.
//! The rules are documented in `docs/grouping.md`.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Value, json};
use xxhash_rust::xxh3::xxh3_128;

use crate::event::{Event, Frame, GroupingInfo, Stacktrace};
use crate::normalize::truncate;

pub const GROUPING_VERSION: &str = "v1";

/// Title, culprit and metadata of the issue an event creates.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueInfo {
    pub title: String,
    pub culprit: Option<String>,
    /// `{type, value, filename, function}` as the API returns it.
    pub metadata: Value,
}

/// Computes the grouping hash of an event.
pub fn group(ev: &Event) -> GroupingInfo {
    let (method, default) = default_components(ev);
    let (method, components) = if ev.fingerprint.is_empty() {
        (method, default)
    } else {
        let mut out = Vec::new();
        for part in &ev.fingerprint {
            let compact: String = part.chars().filter(|c| !c.is_whitespace()).collect();
            match compact.as_str() {
                "{{default}}" => out.extend(default.iter().cloned()),
                "{{transaction}}" => out.push(ev.transaction.clone().unwrap_or_default()),
                "{{level}}" => out.push(ev.level.clone()),
                "{{type}}" | "{{error.type}}" => {
                    out.push(ev.main_exception().and_then(|e| e.ty.clone()).unwrap_or_default())
                }
                "{{message}}" => out.push(ev.message.clone().unwrap_or_default()),
                _ => out.push(part.clone()),
            }
        }
        ("fingerprint", out)
    };
    GroupingInfo { hash: hash(&components), version: GROUPING_VERSION.into(), method: method.into(), components }
}

/// Hex xxh3-128 of the components (separated by a NUL byte).
pub fn hash(components: &[String]) -> String {
    let mut buf = Vec::with_capacity(components.iter().map(|c| c.len() + 1).sum());
    buf.extend_from_slice(GROUPING_VERSION.as_bytes());
    for c in components {
        buf.push(0);
        buf.extend_from_slice(c.as_bytes());
    }
    format!("{:032x}", xxh3_128(&buf))
}

fn default_components(ev: &Event) -> (&'static str, Vec<String>) {
    let exceptions = ev.exceptions();
    if !exceptions.is_empty() {
        let mut out = Vec::new();
        for e in exceptions {
            let ty = e.ty.clone().unwrap_or_else(|| "Error".into());
            let frames = e.stacktrace.as_ref().map(frame_components).unwrap_or_default();
            out.push(format!("type:{ty}"));
            if frames.is_empty() {
                out.push(format!("value:{}", normalize_message(e.value.as_deref().unwrap_or(""))));
            } else {
                out.extend(frames);
            }
        }
        return ("exception", out);
    }
    if let Some(threads) = &ev.threads {
        let st = threads
            .values
            .iter()
            .find(|t| t.crashed == Some(true))
            .or_else(|| threads.values.iter().find(|t| t.current == Some(true)))
            .or(threads.values.first())
            .and_then(|t| t.stacktrace.as_ref());
        if let Some(st) = st {
            let frames = frame_components(st);
            if !frames.is_empty() {
                return ("stacktrace", frames);
            }
        }
    }
    if let Some(csp) = &ev.csp {
        let directive = csp_directive(csp);
        let blocked = csp.get("blocked_uri").and_then(Value::as_str).map(uri_host).unwrap_or_default();
        return ("csp", vec![format!("directive:{directive}"), format!("blocked:{blocked}")]);
    }
    if let Some(m) = &ev.message {
        return ("message", vec![normalize_message(m)]);
    }
    ("fallback", vec![ev.title.clone()])
}

fn frame_components(st: &Stacktrace) -> Vec<String> {
    let in_app: Vec<&Frame> = st.frames.iter().filter(|f| f.in_app == Some(true)).collect();
    let frames: Vec<&Frame> = if in_app.is_empty() { st.frames.iter().collect() } else { in_app };
    frames.into_iter().filter_map(frame_component).collect()
}

fn frame_component(f: &Frame) -> Option<String> {
    let location = f
        .module
        .as_deref()
        .map(normalize_module)
        .or_else(|| f.filename.as_deref().or(f.abs_path.as_deref()).map(normalize_filename))
        .unwrap_or_default();
    let function = f.function.as_deref().map(normalize_function).unwrap_or_default();
    if location.is_empty() && function.is_empty() {
        return None;
    }
    Some(format!("frame:{location}:{function}"))
}

/// True for a build hash: 6+ lowercase hex digits, or 8+ letters/digits
/// mixing both.
fn is_build_hash(seg: &str) -> bool {
    let hex = seg.len() >= 6 && seg.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    let mixed = seg.len() >= 8
        && seg.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && seg.bytes().any(|b| b.is_ascii_digit())
        && seg.bytes().any(|b| b.is_ascii_alphabetic());
    hex || mixed
}

/// File base name without query, fragment or build hash
/// (`app.3f2a9c1e.js`, `index-BkL2q9Xz.js`, `main.1a2b3c4d.chunk.js`).
pub fn normalize_filename(path: &str) -> String {
    let path = path.split(['?', '#']).next().unwrap_or(path);
    let base = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let (stem, ext) = match base.find('.') {
        Some(i) => (&base[..i], &base[i..]),
        None => (base, ""),
    };
    // Hash as its own dotted part: name.HASH.ext / name.HASH.chunk.ext
    let parts: Vec<&str> = ext.split('.').skip(1).collect();
    if parts.len() >= 2 && is_build_hash(parts[0]) {
        return format!("{stem}.{}", parts[1..].join("."));
    }
    // Hash after a dash or underscore: name-HASH.ext
    if let Some(i) = stem.rfind(['-', '_'])
        && i > 0
        && is_build_hash(&stem[i + 1..])
    {
        return format!("{}{ext}", &stem[..i]);
    }
    base.to_string()
}

static MODULE_NOISE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:\$\$Lambda\$[0-9a-f/]+|\$[0-9]+|@[0-9a-f]{6,}|\b[0-9a-f]{8,}\b)").unwrap());

fn normalize_module(m: &str) -> String {
    MODULE_NOISE.replace_all(m, "").to_string()
}

static FUNCTION_NOISE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:\$\$Lambda\$[0-9a-f/]+|lambda\$[0-9]+|\$[0-9]+$|<lambda_[0-9a-f]+>|\{closure#[0-9]+\}|0x[0-9a-f]+)")
        .unwrap()
});

fn normalize_function(f: &str) -> String {
    let f = f.trim();
    if f == "?" || f == "<anonymous>" || f == "<unknown>" {
        return String::new();
    }
    // `Object.foo` / `new Foo` / `async foo` -> the name itself.
    let f = f.strip_prefix("async ").unwrap_or(f);
    let f = f.strip_prefix("new ").unwrap_or(f);
    let f = f.strip_prefix("Object.").unwrap_or(f);
    FUNCTION_NOISE.replace_all(f, "").to_string()
}

static MESSAGE_PATTERNS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    [
        (r"[a-zA-Z0-9._%+\-]+@[a-zA-Z0-9.\-]+\.[a-zA-Z]{2,}", "<email>"),
        (r"\b[a-z][a-z0-9+.\-]*://[^\s'\x22]+", "<url>"),
        (r"\b[0-9a-fA-F]{8}-?[0-9a-fA-F]{4}-?[0-9a-fA-F]{4}-?[0-9a-fA-F]{4}-?[0-9a-fA-F]{12}\b", "<uuid>"),
        (r"\b(?:[0-9]{1,3}\.){3}[0-9]{1,3}\b", "<ip>"),
        (r"\b0x[0-9a-fA-F]+\b", "<hex>"),
        (r"\b[0-9a-fA-F]*[0-9][0-9a-fA-F]*[a-fA-F][0-9a-fA-F]*\b", "<hex>"),
        (r"\b[0-9a-fA-F]*[a-fA-F][0-9a-fA-F]*[0-9][0-9a-fA-F]*\b", "<hex>"),
        (r"(?:[A-Za-z]:)?(?:[/\\][\w.\-]+){2,}[/\\]?", "<path>"),
        (r"\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}(?::\d{2}(?:\.\d+)?)?(?:Z|[+\-]\d{2}:?\d{2})?", "<date>"),
        (r"\d+(?:\.\d+)?", "<int>"),
        (r"'[^']*'", "'<str>'"),
        (r#""[^"]*""#, "\"<str>\""),
    ]
    .into_iter()
    .map(|(re, rep)| (Regex::new(re).unwrap(), rep))
    .collect()
});

/// Replaces the variable parts of a message so that "user 42 not found"
/// and "user 7 not found" group together.
pub fn normalize_message(msg: &str) -> String {
    let msg = truncate(msg, 1024);
    let mut out = msg.to_string();
    for (re, rep) in MESSAGE_PATTERNS.iter() {
        // The hex patterns only match words that mix digits and letters, so
        // ordinary words survive.
        out = re.replace_all(&out, *rep).to_string();
    }
    out
}

fn csp_directive(csp: &Value) -> String {
    csp.get("effective_directive")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            csp.get("violated_directive")
                .and_then(Value::as_str)
                .and_then(|d| d.split_whitespace().next())
                .map(str::to_string)
        })
        .unwrap_or_default()
}

fn uri_host(uri: &str) -> String {
    match uri.split_once("://") {
        Some((_, rest)) => rest.split(['/', '?', '#']).next().unwrap_or("").to_string(),
        None => uri.to_string(),
    }
}

/// Title, culprit and metadata for the issue list.
pub fn issue_info(ev: &Event) -> IssueInfo {
    let top_frame = ev
        .main_exception()
        .and_then(|e| e.stacktrace.as_ref())
        .or_else(|| ev.threads.as_ref().and_then(|t| t.values.iter().find_map(|t| t.stacktrace.as_ref())))
        .and_then(|st| st.frames.iter().rev().find(|f| f.in_app == Some(true)).or(st.frames.last()));
    let filename = top_frame.and_then(|f| f.filename.clone().or(f.abs_path.clone()));
    let function = top_frame.and_then(|f| f.function.clone());

    let frame_culprit = top_frame.map(|f| {
        let loc = f.module.clone().or(f.filename.clone()).or(f.abs_path.clone()).unwrap_or_else(|| "?".into());
        match &f.function {
            Some(func) => format!("{func}({loc})"),
            None => loc,
        }
    });
    let culprit = ev.transaction.clone().or(frame_culprit).map(|c| truncate(&c, 200));

    if let Some(e) = ev.main_exception() {
        // Some SDKs send messages as an exception without a type (a
        // synthetic mechanism) to attach a stack trace: titled by the text.
        if e.ty.is_none() {
            let text = ev.message.clone().or_else(|| e.value.clone()).unwrap_or_else(|| "<unlabeled event>".into());
            let first_line = text.lines().next().unwrap_or("").to_string();
            return IssueInfo {
                title: truncate(&first_line, 200),
                culprit,
                metadata: json!({"type": null, "value": truncate(&text, 1024), "filename": filename, "function": function, "title": first_line}),
            };
        }
        let ty = e.ty.clone().unwrap_or_else(|| "Error".into());
        let value = e.value.clone().unwrap_or_default();
        let first_line = value.lines().next().unwrap_or("").to_string();
        let title = if first_line.is_empty() { ty.clone() } else { truncate(&format!("{ty}: {first_line}"), 200) };
        return IssueInfo {
            title,
            culprit,
            metadata: json!({"type": ty, "value": truncate(&value, 1024), "filename": filename, "function": function}),
        };
    }
    if let Some(csp) = &ev.csp {
        let directive = csp_directive(csp);
        let blocked = csp.get("blocked_uri").and_then(Value::as_str).unwrap_or("");
        let kind = directive.strip_suffix("-src").or(directive.strip_suffix("-src-elem")).unwrap_or(&directive);
        let title = match blocked {
            "inline" | "'inline'" => format!("Blocked inline '{kind}'"),
            "eval" | "'eval'" => format!("Blocked 'eval' in '{kind}'"),
            b => format!("Blocked '{kind}' from '{}'", uri_host(b)),
        };
        return IssueInfo {
            title: truncate(&title, 200),
            culprit: Some(directive.clone()),
            metadata: json!({"type": "csp", "value": title, "directive": directive, "uri": blocked, "filename": null, "function": null}),
        };
    }
    let msg = ev.message.clone().unwrap_or_else(|| "<unlabeled event>".into());
    let first_line = msg.lines().next().unwrap_or("").to_string();
    IssueInfo {
        title: truncate(&first_line, 200),
        culprit,
        metadata: json!({"type": null, "value": truncate(&msg, 1024), "filename": filename, "function": function, "title": first_line}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::normalize::{Context, normalize};
    use crate::settings::ProjectSettings;
    use chrono::Utc;

    fn ev(v: Value) -> Event {
        let s = ProjectSettings::default();
        normalize(
            &v,
            &Context {
                project_id: 1,
                event_id: "0".repeat(32),
                received: Utc::now(),
                client_ip: None,
                user_agent: None,
                settings: &s,
            },
        )
    }

    fn exc(value: &str, line: u32, file: &str) -> Value {
        json!({"platform": "javascript", "exception": {"values": [{"type": "TypeError", "value": value,
        "stacktrace": {"frames": [
            {"filename": "node_modules/lib/x.js", "function": "lib", "lineno": 1, "in_app": false},
            {"filename": file, "function": "handleClick", "lineno": line, "colno": 4, "in_app": true}
        ]}}]}})
    }

    #[test]
    fn line_numbers_values_and_build_hashes_do_not_split_issues() {
        let a = group(&ev(exc("x is undefined", 10, "https://cdn/app.3f2a9c1e.js")));
        let b = group(&ev(exc("y is undefined", 99, "https://cdn/app.77aa00ff.js?v=2")));
        assert_eq!(a.hash, b.hash);
        assert_eq!(a.method, "exception");
        assert_eq!(a.hash.len(), 32);
        let c = group(&ev(exc("x", 10, "https://cdn/other.js")));
        assert_ne!(a.hash, c.hash);
    }

    #[test]
    fn exception_without_frames_groups_by_normalized_value() {
        let a = group(&ev(json!({"exception": [{"type": "KeyError", "value": "user 42 missing"}]})));
        let b = group(&ev(json!({"exception": [{"type": "KeyError", "value": "user 7 missing"}]})));
        let c = group(&ev(json!({"exception": [{"type": "ValueError", "value": "user 7 missing"}]})));
        assert_eq!(a.hash, b.hash);
        assert_ne!(a.hash, c.hash);
    }

    #[test]
    fn fingerprint_overrides_and_expands_default() {
        let base = exc("boom", 1, "a.js");
        let mut with_fp = base.clone();
        with_fp["fingerprint"] = json!(["my-group"]);
        let g = group(&ev(with_fp));
        assert_eq!(g.method, "fingerprint");
        assert_eq!(g.components, vec!["my-group"]);
        let mut expanded = base.clone();
        expanded["fingerprint"] = json!(["{{ default }}", "tenant-a"]);
        let g2 = group(&ev(expanded));
        let d = group(&ev(base));
        assert_eq!(g2.components[..d.components.len()], d.components[..]);
        assert_ne!(g2.hash, d.hash);
    }

    #[test]
    fn messages_strip_variable_parts() {
        assert_eq!(
            normalize_message(
                "Payment 1234 for bob@example.com failed at /var/app/pay.py id 9f1c2d3e-0000-4000-8000-000000000000"
            ),
            "Payment <int> for <email> failed at <path> id <uuid>"
        );
        assert_eq!(normalize_message("hash deadbeef01 then 0xff"), "hash <hex> then <hex>");
        let a = group(&ev(json!({"message": "Timeout after 30s on 'db1'"})));
        let b = group(&ev(json!({"message": "Timeout after 31s on 'db2'"})));
        assert_eq!(a.method, "message");
        assert_eq!(a.hash, b.hash);
    }

    #[test]
    fn titles_and_culprits() {
        let e = ev(exc("x is undefined\nmore", 10, "src/app.js"));
        let info = issue_info(&e);
        assert_eq!(info.title, "TypeError: x is undefined");
        assert_eq!(info.culprit.as_deref(), Some("handleClick(src/app.js)"));
        assert_eq!(info.metadata["function"], "handleClick");
        let e = ev(
            json!({"message": "hello", "exception": [{"value": "hello", "mechanism": {"type": "generic", "synthetic": true}}]}),
        );
        assert_eq!(issue_info(&e).title, "hello");
        let e = ev(json!({"message": "hello\nworld", "transaction": "/checkout"}));
        let info = issue_info(&e);
        assert_eq!(info.title, "hello");
        assert_eq!(info.culprit.as_deref(), Some("/checkout"));
        let e = ev(json!({"csp": {"effective_directive": "script-src", "blocked_uri": "https://evil.example/x.js"}}));
        assert_eq!(issue_info(&e).title, "Blocked 'script' from 'evil.example'");
        assert_eq!(group(&e).method, "csp");
    }

    #[test]
    fn filenames_lose_hashes() {
        assert_eq!(normalize_filename("https://x/static/js/main.3f2a9c1e.chunk.js"), "main.chunk.js");
        assert_eq!(normalize_filename("/assets/index-BkL2q9Xz.js"), "index.js");
        assert_eq!(normalize_filename("/assets/app.a1b2c3d4.js"), "app.js");
        assert_eq!(normalize_filename("C:\\src\\views.py"), "views.py");
        assert_eq!(normalize_filename("/srv/handlers.js"), "handlers.js");
    }
}
