//! Readable renderings of API documents for agents: issue summaries,
//! events with stack traces, and trace waterfalls.

use std::fmt::Write as _;

use serde_json::{Value, json};

/// Cuts `s` to at most `max` bytes (at a character boundary) and says so.
pub(crate) fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let note = "\n[... cut to keep the result small]";
    let mut end = max.saturating_sub(note.len());
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{note}", &s[..end])
}

/// A value on one line, at most `max` characters.
fn compact(v: &Value, max: usize) -> String {
    let s = match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    let s = s.replace('\n', " ");
    if s.chars().count() <= max { s } else { format!("{}…", s.chars().take(max).collect::<String>()) }
}

fn str_of<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(Value::as_str).filter(|s| !s.is_empty())
}

/// The fields of an issue an agent needs, from the API's issue object.
pub(crate) fn issue_summary(i: &Value) -> Value {
    let events_in_period: i64 = i["stats"].as_array().into_iter().flatten().filter_map(|p| p.get(1)?.as_i64()).sum();
    let assignee = match i.get("assignee") {
        Some(a) if a.is_object() => json!({"type": a["type"], "id": a["id"], "name": a["name"], "email": a["email"]}),
        _ => Value::Null,
    };
    json!({
        "id": i["id"],
        "short_id": i["short_id"],
        "title": i["title"],
        "culprit": i["culprit"],
        "level": i["level"],
        "status": i["status"],
        "substatus": i["substatus"],
        "priority": i["priority"],
        "project": i["project"].get("slug"),
        "count": i["count"],
        "user_count": i["user_count"],
        "events_in_period": events_in_period,
        "first_seen": i["first_seen"],
        "last_seen": i["last_seen"],
        "first_release": i["first_release"],
        "last_release": i["last_release"],
        "assignee": assignee,
        "is_bookmarked": i["is_bookmarked"],
        "permalink": i["permalink"],
    })
}

/// Everything about an issue's state, for the detail view.
pub(crate) fn issue_detail(i: &Value) -> Value {
    let mut out = issue_summary(i);
    for k in [
        "type",
        "platform",
        "metadata",
        "resolved_in_release",
        "resolved_in_mode",
        "resolved_at",
        "ignore_until",
        "ignore_count",
        "ignore_user_count",
        "ignore_user_window",
        "has_seen",
    ] {
        if let Some(v) = i.get(k)
            && !v.is_null()
        {
            out[k] = v.clone();
        }
    }
    out["events_last_24h"] = out["events_in_period"].take();
    if let Some(o) = out.as_object_mut() {
        o.remove("events_in_period");
    }
    out
}

/// Options for rendering an event.
pub(crate) struct EventOptions {
    pub all_frames: bool,
    pub max_breadcrumbs: usize,
}

/// An event as text: what happened, the stack trace with source context
/// of in-app frames, breadcrumbs, request, user, contexts and tags.
pub(crate) fn event_text(e: &Value, opts: &EventOptions) -> String {
    let mut out = String::new();
    let id = str_of(e, "event_id").unwrap_or("?");
    let _ = writeln!(out, "Event {id}");
    if let Some(t) = str_of(e, "title") {
        let _ = writeln!(out, "Title: {t}");
    }
    let mut facts = Vec::new();
    for (label, key) in [
        ("issue", "issue_id"),
        ("time", "timestamp"),
        ("level", "level"),
        ("platform", "platform"),
        ("environment", "environment"),
        ("release", "release"),
        ("dist", "dist"),
        ("server", "server_name"),
        ("transaction", "transaction"),
        ("culprit", "culprit"),
        ("logger", "logger"),
    ] {
        if let Some(v) = e.get(key).filter(|v| !v.is_null()) {
            facts.push(format!("{label}: {}", compact(v, 200)));
        }
    }
    if !facts.is_empty() {
        let _ = writeln!(out, "{}", facts.join(" | "));
    }
    if let Some(m) = str_of(e, "message") {
        let _ = writeln!(out, "Message: {}", truncate(m, 2000));
    }
    let mut links = Vec::new();
    for (label, key) in [
        ("trace", "trace_id"),
        ("replay", "replay_id"),
        ("previous event", "previous_event_id"),
        ("next event", "next_event_id"),
    ] {
        if let Some(v) = str_of(e, key) {
            links.push(format!("{label}: {v}"));
        }
    }
    if !links.is_empty() {
        let _ = writeln!(out, "{}", links.join(" | "));
    }
    // A summary (from the event row) has no `type`: retention removed the full event.
    if e.get("type").is_none() {
        let _ = writeln!(out, "\nThe full event was removed by retention; only this summary is left.");
    }

    // Exceptions: the API keeps the SDK order, the raised one last.
    let exceptions: Vec<&Value> = e["exception"]["values"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
    if !exceptions.is_empty() {
        let _ = writeln!(out, "\n## Exception");
        if exceptions.len() > 1 {
            let _ = writeln!(out, "{} chained exceptions; the last one is the one that was raised.", exceptions.len());
        }
        for ex in &exceptions {
            let ty = str_of(ex, "type").unwrap_or("Error");
            let val = str_of(ex, "value").unwrap_or("");
            let _ = writeln!(out, "\n{ty}: {}", truncate(val, 2000));
            if let Some(m) = ex.get("mechanism").filter(|m| m.is_object()) {
                let handled = match m.get("handled").and_then(Value::as_bool) {
                    Some(true) => "handled",
                    Some(false) => "unhandled",
                    None => "handled: unknown",
                };
                let _ = writeln!(out, "Mechanism: {} ({handled})", str_of(m, "type").unwrap_or("generic"));
            }
            if let Some(frames) = ex["stacktrace"]["frames"].as_array() {
                stack(&mut out, frames, opts.all_frames);
            }
        }
    }
    // Threads with stack traces (when there is no exception stack).
    if let Some(threads) = e["threads"]["values"].as_array() {
        let with_stack: Vec<&Value> =
            threads.iter().filter(|t| t["stacktrace"]["frames"].as_array().is_some_and(|f| !f.is_empty())).collect();
        if !with_stack.is_empty() && exceptions.iter().all(|x| x["stacktrace"].is_null()) {
            let _ = writeln!(out, "\n## Threads");
            for t in with_stack.iter().take(5) {
                let name = str_of(t, "name").map(str::to_string).unwrap_or_else(|| compact(&t["id"], 40));
                let crashed = if t["crashed"].as_bool() == Some(true) { " (crashed)" } else { "" };
                let _ = writeln!(out, "\nThread {name}{crashed}");
                stack(
                    &mut out,
                    t["stacktrace"]["frames"].as_array().map(Vec::as_slice).unwrap_or(&[]),
                    opts.all_frames,
                );
            }
        }
    }

    if let Some(crumbs) = e["breadcrumbs"]["values"].as_array().filter(|b| !b.is_empty()) {
        let n = crumbs.len();
        let shown = &crumbs[n.saturating_sub(opts.max_breadcrumbs)..];
        let _ = writeln!(
            out,
            "\n## Breadcrumbs (oldest first{})",
            if shown.len() < n { format!(", last {} of {n}", shown.len()) } else { String::new() }
        );
        for b in shown {
            let mut line = String::new();
            if let Some(t) = str_of(b, "timestamp") {
                line.push_str(t);
                line.push(' ');
            }
            line.push_str(str_of(b, "category").or(str_of(b, "type")).unwrap_or("default"));
            if let Some(l) = str_of(b, "level") {
                let _ = write!(line, " [{l}]");
            }
            if let Some(m) = str_of(b, "message") {
                let _ = write!(line, " {}", compact(&json!(m), 300));
            }
            if let Some(d) = b.get("data").filter(|d| d.as_object().is_some_and(|o| !o.is_empty())) {
                let _ = write!(line, " {}", compact(d, 300));
            }
            let _ = writeln!(out, "- {line}");
        }
    }

    if let Some(r) = e.get("request").filter(|r| r.is_object()) {
        let _ = writeln!(out, "\n## Request");
        let _ = writeln!(out, "{} {}", str_of(r, "method").unwrap_or("GET"), str_of(r, "url").unwrap_or("?"));
        if let Some(q) = r.get("query_string").filter(|q| !q.is_null()) {
            let _ = writeln!(out, "Query: {}", compact(q, 500));
        }
        if let Some(h) = r["headers"].as_array().filter(|h| !h.is_empty()) {
            let _ = writeln!(out, "Headers:");
            for pair in h.iter().take(30) {
                if let Some(p) = pair.as_array() {
                    let _ =
                        writeln!(out, "  {}: {}", compact(&p[0], 80), compact(p.get(1).unwrap_or(&Value::Null), 300));
                }
            }
        }
        if let Some(d) = r.get("data").filter(|d| !d.is_null()) {
            let _ = writeln!(out, "Body: {}", compact(d, 1500));
        }
    }

    if let Some(u) = e.get("user").filter(|u| u.as_object().is_some_and(|o| !o.is_empty())) {
        let fields: Vec<String> =
            u.as_object().into_iter().flatten().map(|(k, v)| format!("{k}: {}", compact(v, 200))).collect();
        let _ = writeln!(out, "\n## User\n{}", fields.join(" | "));
    }

    if let Some(c) = e["contexts"].as_object().filter(|c| !c.is_empty()) {
        let _ = writeln!(out, "\n## Contexts");
        for (k, v) in c.iter().filter(|(_, v)| v.as_object().is_none_or(|o| o.keys().any(|k| k != "type"))).take(20) {
            let _ = writeln!(out, "- {k}: {}", context_line(v));
        }
    }

    let pairs: Vec<String> = match &e["tags"] {
        Value::Array(tags) => tags
            .iter()
            .filter_map(|p| {
                let p = p.as_array()?;
                Some(format!("{}={}", compact(p.first()?, 80), compact(p.get(1)?, 200)))
            })
            .collect(),
        Value::Object(tags) => tags.iter().map(|(k, v)| format!("{k}={}", compact(v, 200))).collect(),
        _ => Vec::new(),
    };
    if !pairs.is_empty() {
        let _ = writeln!(out, "\n## Tags\n{}", pairs.join(", "));
    }

    if let Some(x) = e["extra"].as_object().filter(|x| !x.is_empty()) {
        let _ = writeln!(out, "\n## Extra");
        for (k, v) in x.iter().take(30) {
            let _ = writeln!(out, "- {k}: {}", compact(v, 500));
        }
    }

    if let Some(s) = e.get("sdk").filter(|s| s.is_object()) {
        let _ = writeln!(out, "\nSDK: {} {}", str_of(s, "name").unwrap_or("?"), str_of(s, "version").unwrap_or(""));
    }
    if let Some(errs) = e["errors"].as_array().filter(|x| !x.is_empty()) {
        let _ = writeln!(out, "\n## Processing problems");
        for x in errs.iter().take(10) {
            let _ = writeln!(out, "- {}", compact(x, 300));
        }
    }
    out
}

/// A context object on one line: `name version (k=v, ...)`.
fn context_line(v: &Value) -> String {
    let Some(o) = v.as_object() else { return compact(v, 300) };
    let parts: Vec<String> =
        o.iter().filter(|(k, _)| k.as_str() != "type").map(|(k, v)| format!("{k}={}", compact(v, 120))).collect();
    truncate(&parts.join(", "), 600)
}

/// Frames, most recent call last as the SDKs send them. Library frames are
/// collapsed unless `all` is set or there are no in-app frames; the frame
/// that raised is always shown. The innermost in-app frame gets all the
/// source context the SDK sent, the other frames two lines around theirs.
fn stack(out: &mut String, frames: &[Value], all: bool) {
    const MAX_FRAMES: usize = 60;
    const CONTEXT: usize = 2;
    let any_in_app = frames.iter().any(|f| f["in_app"].as_bool() == Some(true));
    let focus = if any_in_app {
        frames.iter().rposition(|f| f["in_app"].as_bool() == Some(true))
    } else {
        frames.len().checked_sub(1)
    };
    let show_all = all || !any_in_app;
    let _ = writeln!(out, "Stack trace (most recent call last; `*` marks in-app frames):");
    let last = frames.len().saturating_sub(1);
    let mut hidden = 0;
    let mut shown = 0;
    let flush = |out: &mut String, hidden: &mut usize| {
        if *hidden > 0 {
            let _ = writeln!(out, "    ... {hidden} library frame{} ...", if *hidden == 1 { "" } else { "s" });
            *hidden = 0;
        }
    };
    let start = frames.len().saturating_sub(MAX_FRAMES);
    if start > 0 {
        let _ = writeln!(out, "    ... {start} older frames left out ...");
    }
    for (i, f) in frames.iter().enumerate().skip(start) {
        let in_app = f["in_app"].as_bool() == Some(true);
        if !show_all && !in_app && i != last {
            hidden += 1;
            continue;
        }
        flush(out, &mut hidden);
        shown += 1;
        let file = str_of(f, "filename").or(str_of(f, "abs_path")).or(str_of(f, "module")).unwrap_or("?");
        let mut loc = file.to_string();
        if let Some(l) = f["lineno"].as_u64() {
            let _ = write!(loc, ":{l}");
            if let Some(c) = f["colno"].as_u64() {
                let _ = write!(loc, ":{c}");
            }
        }
        let func = str_of(f, "function").unwrap_or("?");
        let mark = if in_app { "*" } else { " " };
        let _ = writeln!(out, "  {mark} {loc} in {func}");
        if let Some(raw) = f.get("raw").filter(|r| r.is_object())
            && f["symbolicated"].as_bool() == Some(true)
        {
            let rf = str_of(raw, "filename").or(str_of(raw, "abs_path")).unwrap_or("?");
            let _ = writeln!(out, "      (minified: {rf}:{}:{})", raw["lineno"], raw["colno"]);
        }
        if (in_app || !any_in_app) && f.get("context_line").is_some_and(|c| !c.is_null()) {
            let lineno = f["lineno"].as_i64();
            let mut pre: Vec<&str> =
                f["pre_context"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
            let mut post: Vec<&str> =
                f["post_context"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
            if Some(i) != focus {
                pre.drain(..pre.len().saturating_sub(CONTEXT));
                post.truncate(CONTEXT);
            }
            let n = |offset: i64| lineno.map(|l| format!("{:>5}", l + offset)).unwrap_or_else(|| "     ".into());
            for (k, line) in pre.iter().enumerate() {
                let _ = writeln!(out, "      {} | {}", n(k as i64 - pre.len() as i64), cut(line));
            }
            let _ = writeln!(out, "    > {} | {}", n(0), cut(f["context_line"].as_str().unwrap_or("")));
            for (k, line) in post.iter().enumerate() {
                let _ = writeln!(out, "      {} | {}", n(k as i64 + 1), cut(line));
            }
        }
        if in_app && let Some(vars) = f.get("vars").filter(|v| v.as_object().is_some_and(|o| !o.is_empty())) {
            let _ = writeln!(out, "      locals: {}", compact(vars, 400));
        }
    }
    flush(out, &mut hidden);
    if shown == 0 {
        let _ = writeln!(out, "    (no frames)");
    }
}

/// A source line, shortened (minified files have very long lines).
fn cut(line: &str) -> String {
    if line.chars().count() <= 160 {
        line.to_string()
    } else {
        format!("{}…", line.chars().take(160).collect::<String>())
    }
}

/// A trace as an indented span tree.
pub(crate) fn trace_text(t: &Value, max_spans: usize) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "Trace {}", str_of(t, "trace_id").unwrap_or("?"));
    let _ = writeln!(
        out,
        "start: {} | duration: {} ms | spans: {} | errors: {} | logs: {}",
        compact(&t["start"], 40),
        compact(&t["duration_ms"], 20),
        compact(&t["span_count"], 20),
        t["errors"].as_array().map(Vec::len).unwrap_or(0),
        compact(&t["logs_count"], 20),
    );
    if t["truncated"].as_bool() == Some(true) {
        let _ = writeln!(out, "The trace has more than 10,000 spans; only the first were loaded.");
    }
    if let Some(txs) = t["transactions"].as_array().filter(|x| !x.is_empty()) {
        let _ = writeln!(out, "\n## Transactions");
        for x in txs.iter().take(50) {
            let _ = writeln!(
                out,
                "- {} [{}] {} ms, status {}, project {}, event {}",
                str_of(x, "name").unwrap_or("?"),
                str_of(x, "op").unwrap_or("-"),
                compact(&x["duration_ms"], 20),
                str_of(x, "status").unwrap_or("-"),
                str_of(x, "project_slug").unwrap_or("?"),
                str_of(x, "event_id").unwrap_or("?"),
            );
        }
    }
    if let Some(errs) = t["errors"].as_array().filter(|x| !x.is_empty()) {
        let _ = writeln!(out, "\n## Errors in this trace");
        for x in errs.iter().take(50) {
            let _ = writeln!(
                out,
                "- {} (issue {}, event {}, span {}, {})",
                str_of(x, "title").unwrap_or("?"),
                compact(&x["issue_id"], 30),
                str_of(x, "event_id").unwrap_or("?"),
                str_of(x, "span_id").unwrap_or("-"),
                compact(&x["timestamp"], 40),
            );
        }
    }
    let start = t["start"].as_str().and_then(chrono_ms);
    let mut shown = 0usize;
    let mut total = 0usize;
    let _ = writeln!(out, "\n## Spans (offset from trace start, duration, op, description, status)");
    fn walk(
        out: &mut String,
        n: &Value,
        depth: usize,
        start: Option<f64>,
        shown: &mut usize,
        total: &mut usize,
        max: usize,
    ) {
        *total += 1;
        if *shown < max {
            *shown += 1;
            let offset = match (start, n["start_timestamp"].as_str().and_then(chrono_ms)) {
                (Some(s), Some(t)) => format!("+{:.1}ms", t - s),
                _ => "?".into(),
            };
            let status = str_of(n, "status").filter(|s| *s != "ok").map(|s| format!(" [{s}]")).unwrap_or_default();
            let seg = if n["is_segment"].as_bool() == Some(true) { " (transaction)" } else { "" };
            let _ = writeln!(
                out,
                "{}{offset} {}ms {}: {}{status}{seg} span={}",
                "  ".repeat(depth.min(30)),
                n["duration_ms"].as_f64().map(|d| format!("{d:.1}")).unwrap_or_else(|| "?".into()),
                str_of(n, "op").unwrap_or("-"),
                cut(str_of(n, "description").or(str_of(n, "transaction")).unwrap_or("")),
                str_of(n, "span_id").unwrap_or("?"),
            );
        }
        for c in n["children"].as_array().into_iter().flatten() {
            walk(out, c, depth + 1, start, shown, total, max);
        }
    }
    for root in t["spans"].as_array().into_iter().flatten() {
        walk(&mut out, root, 0, start, &mut shown, &mut total, max_spans);
    }
    if total > shown {
        let _ = writeln!(out, "... {} more spans not shown (raise max_spans to see them)", total - shown);
    }
    out
}

/// Milliseconds since the epoch of an RFC 3339 timestamp.
fn chrono_ms(s: &str) -> Option<f64> {
    let t = chrono::DateTime::parse_from_rfc3339(s).ok()?;
    Some(t.timestamp_micros() as f64 / 1000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event() -> Value {
        json!({
            "event_id": "abc", "issue_id": "7", "type": "error", "title": "TypeError: x is undefined", "level": "error",
            "timestamp": "2026-11-14T10:00:00Z", "environment": "production", "release": "web@1.2.0",
            "trace_id": "68dee3f33b104e13918523b1fbfdb832",
            "exception": {"values": [
                {"type": "Error", "value": "cause"},
                {"type": "TypeError", "value": "x is undefined", "mechanism": {"type": "onerror", "handled": false},
                 "stacktrace": {"frames": [
                    {"filename": "node_modules/react.js", "function": "a", "lineno": 1, "in_app": false},
                    {"filename": "node_modules/react.js", "function": "b", "lineno": 2, "in_app": false},
                    {"filename": "src/index.js", "function": "main", "lineno": 10, "in_app": true,
                     "pre_context": ["a", "b", "c", "d"], "context_line": "render()", "post_context": ["e", "f", "g"]},
                    {"filename": "src/app.js", "function": "render", "lineno": 41, "colno": 7, "in_app": true,
                     "pre_context": ["function render() {", "  const x = undefined;"], "context_line": "  x.y()",
                     "post_context": ["}"], "vars": {"x": null}},
                    {"filename": "node_modules/lib.js", "function": "c", "lineno": 3, "in_app": false}
                 ]}}
            ]},
            "breadcrumbs": {"values": (0..30).map(|i| json!({"category": "ui.click", "message": format!("click {i}")})).collect::<Vec<_>>()},
            "request": {"method": "POST", "url": "https://shop.example.com/checkout", "headers": [["User-Agent", "Mozilla"]], "data": {"a": 1}},
            "user": {"id": "42", "email": "ada@example.com"},
            "contexts": {"browser": {"name": "Chrome", "version": "120.0", "type": "browser"}},
            "tags": [["browser", "Chrome 120.0"], ["region", "eu"]],
            "sdk": {"name": "sentry.javascript.browser", "version": "10.0.0"}
        })
    }

    #[test]
    fn events_read_like_a_stack_trace() {
        let t = event_text(&event(), &EventOptions { all_frames: false, max_breadcrumbs: 20 });
        assert!(t.contains("Title: TypeError: x is undefined"), "{t}");
        assert!(t.contains("2 chained exceptions"), "{t}");
        assert!(t.contains("Mechanism: onerror (unhandled)"), "{t}");
        assert!(t.contains("... 2 library frames ..."), "{t}");
        assert!(t.contains("    8 | c\n") && !t.contains("    7 | b\n"), "two lines around other frames: {t}");
        assert!(t.contains("   12 | f\n") && !t.contains("   13 | g\n"), "{t}");
        assert!(t.contains("* src/app.js:41:7 in render"), "{t}");
        assert!(t.contains(">    41 |   x.y()"), "{t}");
        assert!(t.contains("   39 | function render() {"), "{t}");
        assert!(t.contains("   42 | }"), "{t}");
        assert!(t.contains("node_modules/lib.js:3 in c"), "the raising frame is always shown: {t}");
        assert!(t.contains("last 20 of 30"), "{t}");
        assert!(t.contains("click 29") && !t.contains("click 9 "), "{t}");
        assert!(t.contains("POST https://shop.example.com/checkout"), "{t}");
        assert!(t.contains("email: ada@example.com"), "{t}");
        assert!(t.contains("browser: name=Chrome, version=120.0"), "{t}");
        assert!(t.contains("region=eu"), "{t}");
        assert!(t.contains("trace: 68dee3f33b104e13918523b1fbfdb832"), "{t}");
        assert!(!t.contains("removed by retention"), "{t}");
        let all = event_text(&event(), &EventOptions { all_frames: true, max_breadcrumbs: 5 });
        assert!(all.contains("node_modules/react.js:1 in a"), "{all}");
    }

    #[test]
    fn summaries_left_by_retention_are_marked() {
        let s = json!({"event_id": "abc", "title": "TypeError: x", "tags": {"region": "eu"}, "user": {"id": "1"}});
        let t = event_text(&s, &EventOptions { all_frames: false, max_breadcrumbs: 20 });
        assert!(t.contains("removed by retention") && t.contains("region=eu"), "{t}");
    }

    #[test]
    fn traces_are_trees_with_offsets() {
        let t = json!({"trace_id": "t1", "start": "2026-11-14T10:00:00.000Z", "duration_ms": 120.0, "span_count": 3,
            "transactions": [{"name": "GET /", "op": "http.server", "duration_ms": 120.0, "project_slug": "web", "event_id": "e"}],
            "errors": [], "logs_count": 2,
            "spans": [{"span_id": "a", "op": "http.server", "description": "GET /", "start_timestamp": "2026-11-14T10:00:00.000Z",
                       "duration_ms": 120.0, "is_segment": true,
                       "children": [{"span_id": "b", "op": "db", "description": "SELECT 1", "status": "internal_error",
                                     "start_timestamp": "2026-11-14T10:00:00.010Z", "duration_ms": 5.5, "children": []}]}]});
        let s = trace_text(&t, 100);
        assert!(s.contains("+0.0ms 120.0ms http.server: GET / (transaction) span=a"), "{s}");
        assert!(s.contains("  +10.0ms 5.5ms db: SELECT 1 [internal_error] span=b"), "{s}");
        let short = trace_text(&t, 1);
        assert!(short.contains("1 more spans not shown"), "{short}");
    }

    #[test]
    fn truncation_keeps_char_boundaries() {
        let s = "ä".repeat(100);
        let t = truncate(&s, 60);
        assert!(t.len() <= 60 && t.ends_with("small]"));
        assert_eq!(truncate("short", 60), "short");
    }

    #[test]
    fn timestamps_parse() {
        assert_eq!(chrono_ms("1970-01-01T00:00:01.5Z"), Some(1500.0));
        assert_eq!(chrono_ms("2026-11-14T00:00:00Z"), Some(1_794_614_400_000.0));
    }
}
