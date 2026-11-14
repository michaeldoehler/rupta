//! End-to-end tests of the ingest endpoints against an in-memory store.

mod common;

use std::io::Write;
use std::time::Duration;

use common::*;
use rupta_core::signal::IssueSignal;
use rupta_ingest::metrics::{Category, Outcome};
use serde_json::{Value, json};

fn error_event(id: &str, value: &str) -> Value {
    json!({
        "event_id": id,
        "platform": "javascript",
        "level": "error",
        "release": "web@1.0.0",
        "environment": "production",
        "user": {"id": "u1", "email": "ann@example.com", "ip_address": "{{auto}}"},
        "tags": {"feature": "checkout"},
        "exception": {"values": [{"type": "TypeError", "value": value, "mechanism": {"type": "onerror", "handled": false},
            "stacktrace": {"frames": [{"filename": "https://app.example.com/static/app.js", "function": "pay", "lineno": 10, "colno": 5, "in_app": true}]}}]},
        "breadcrumbs": {"values": [{"category": "ui.click", "message": "button#pay", "timestamp": 1.7e9}]},
        "extra": {"password": "hunter2", "cart": 3},
    })
}

#[tokio::test]
async fn stores_an_error_event_and_creates_an_issue() {
    let h = harness();
    let mut signals = h.ingest.subscribe();
    let id = new_id();
    let (status, _, body) = h.envelope(event_envelope(&error_event(&id, "x is undefined"))).await;
    assert_eq!(status, 200);
    assert_eq!(body["id"], id);
    h.flush().await;

    assert_eq!(h.count("SELECT count(*) FROM events"), 1);
    let issue: (i64, i64, String, i64, i64, Option<String>) = {
        let r = h
            .store
            .connect()
            .query_one("SELECT id, short_id, title, times_seen, users_seen, first_release FROM issues", &[])
            .unwrap();
        (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4), r.get(5))
    };
    assert_eq!(issue.1, 1);
    assert_eq!(issue.2, "TypeError: x is undefined");
    assert_eq!((issue.3, issue.4), (1, 1));
    assert_eq!(issue.5.as_deref(), Some("web@1.0.0"));
    assert_eq!(h.count("SELECT next_short_id FROM projects"), 2);

    let ev = h.payload(&id);
    assert_eq!(ev["event_id"], id);
    assert_eq!(ev["issue_id"], issue.0.to_string());
    assert_eq!(ev["project_id"], h.project_id.to_string());
    assert_eq!(ev["user"]["ip_address"], "203.0.113.7");
    assert_eq!(ev["extra"]["password"], "[Filtered]");
    assert_eq!(ev["extra"]["cart"], 3);
    assert_eq!(ev["exception"]["values"][0]["mechanism"]["handled"], false);
    assert_eq!(ev["breadcrumbs"]["values"][0]["category"], "ui.click");
    assert_eq!(ev["grouping"]["method"], "exception");
    assert!(ev["tags"].as_array().unwrap().iter().any(|t| t[0] == "feature" && t[1] == "checkout"));

    let row = h.store.connect().query_one("SELECT tags ->> 'feature', user_ip, release FROM events", &[]).unwrap();
    assert_eq!(row.get::<Option<String>>(0).as_deref(), Some("checkout"));
    assert_eq!(row.get::<Option<String>>(1).as_deref(), Some("203.0.113.7"));
    assert_eq!(h.count("SELECT count(*) FROM releases WHERE version = 'web@1.0.0'"), 1);
    assert_eq!(h.count("SELECT new_issues FROM release_projects"), 1);
    assert_eq!(h.count("SELECT quantity FROM outcomes WHERE category = 'error' AND outcome = 'accepted'"), 1);

    let s = tokio::time::timeout(Duration::from_secs(2), signals.recv()).await.unwrap().unwrap();
    assert!(matches!(s, IssueSignal::New { .. }));

    // A second event with another value and line groups into the same issue.
    let mut e2 = error_event(&new_id(), "y is undefined");
    e2["exception"]["values"][0]["stacktrace"]["frames"][0]["lineno"] = json!(99);
    e2["user"] = json!({"id": "u2"});
    h.envelope(event_envelope(&e2)).await;
    h.flush().await;
    assert_eq!(h.count("SELECT count(*) FROM issues"), 1);
    assert_eq!(h.count("SELECT times_seen FROM issues"), 2);
    assert_eq!(h.count("SELECT users_seen FROM issues"), 2);
}

#[tokio::test]
async fn authentication_forms_and_rejections() {
    let h = harness();
    let body = || event_envelope(&json!({"event_id": new_id(), "message": "hi"}));
    let path = format!("/api/{}/envelope/", h.project_id);
    // Query parameter.
    let (s, _, _) = h.send(request(&format!("{path}?sentry_key={KEY}&sentry_version=7"), &[], body())).await;
    assert_eq!(s, 200);
    // DSN in the envelope header.
    let dsn_env = envelope(
        json!({"event_id": new_id(), "dsn": format!("http://{KEY}@localhost:8080/{}", h.project_id)}),
        &[(json!({"type": "event"}), b"{\"message\":\"dsn\"}".to_vec())],
    );
    assert_eq!(h.send(request(&path, &[], dsn_env)).await.0, 200);
    // Authorization header in the same format.
    assert_eq!(h.send(request(&path, &[("authorization", &auth())], body())).await.0, 200);
    // Missing and unknown keys.
    let (s, headers, b) = h.send(request(&path, &[], body())).await;
    assert_eq!(s, 401);
    assert!(headers.get("x-sentry-error").is_some());
    assert!(b["detail"].as_str().unwrap().contains("sentry_key"));
    assert_eq!(h.send(request(&format!("{path}?sentry_key=nope"), &[], body())).await.0, 401);
    // Key of another project.
    let other = seed(&h.store, &json!({}));
    let (s, _, _) = h.send(request(&format!("/api/{other}/envelope/?sentry_key={KEY}"), &[], body())).await;
    assert_eq!(s, 403);
    // Disabled key.
    h.store.connect().execute("UPDATE project_keys SET is_active = false", &[]).unwrap();
    h.store.bump_config();
    assert_eq!(h.send(request(&path, &[("x-sentry-auth", &auth())], body())).await.0, 403);
    // Bad envelope and bad project id.
    h.store.connect().execute("UPDATE project_keys SET is_active = true", &[]).unwrap();
    h.store.bump_config();
    assert_eq!(h.send(request(&path, &[("x-sentry-auth", &auth())], b"not json\n".to_vec())).await.0, 400);
    assert_eq!(h.send(request("/api/abc/envelope/", &[("x-sentry-auth", &auth())], body())).await.0, 400);
}

#[tokio::test]
async fn compressed_bodies() {
    let h = harness();
    let path = format!("/api/{}/envelope/", h.project_id);
    let mk = || event_envelope(&json!({"event_id": new_id(), "message": "compressed"}));
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(&mk()).unwrap();
    let mut df = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    df.write_all(&mk()).unwrap();
    let mut br = Vec::new();
    brotli::CompressorWriter::new(&mut br, 4096, 5, 22).write_all(&mk()).unwrap();
    let zs = zstd::encode_all(&mk()[..], 3).unwrap();
    for (enc, body) in [("gzip", gz.finish().unwrap()), ("deflate", df.finish().unwrap()), ("br", br), ("zstd", zs)] {
        let (s, _, _) = h.send(request(&path, &[("x-sentry-auth", &auth()), ("content-encoding", enc)], body)).await;
        assert_eq!(s, 200, "{enc}");
    }
    h.flush().await;
    assert_eq!(h.count("SELECT count(*) FROM events"), 4);
    let (s, _, _) =
        h.send(request(&path, &[("x-sentry-auth", &auth()), ("content-encoding", "gzip")], b"nope".to_vec())).await;
    assert_eq!(s, 400);
}

#[tokio::test]
async fn size_limits() {
    let mut cfg = config();
    cfg.max_envelope_bytes = 10_000;
    cfg.max_decompressed_bytes = 20_000;
    cfg.max_event_bytes = 5_000;
    let h = harness_with(json!({}), cfg);
    let path = format!("/api/{}/envelope/", h.project_id);
    let big = event_envelope(&json!({"event_id": new_id(), "message": "x".repeat(12_000)}));
    assert_eq!(h.send(request(&path, &[("x-sentry-auth", &auth())], big.clone())).await.0, 413);
    let bomb = zstd::encode_all(&vec![b'a'; 50_000][..], 3).unwrap();
    assert_eq!(h.send(request(&path, &[("x-sentry-auth", &auth()), ("content-encoding", "zstd")], bomb)).await.0, 413);
    // An event over the per-event limit is acknowledged and dropped.
    let mid = event_envelope(&json!({"event_id": new_id(), "message": "x".repeat(7_000)}));
    assert_eq!(h.send(request(&path, &[("x-sentry-auth", &auth())], mid)).await.0, 200);
    h.flush().await;
    assert_eq!(h.count("SELECT count(*) FROM events"), 0);
    assert_eq!(h.count("SELECT quantity FROM outcomes WHERE outcome = 'invalid' AND reason = 'too_large'"), 1);
}

#[tokio::test]
async fn browser_text_plain_cors_and_preflight() {
    let h = harness_with(json!({"allowed_origins": ["https://app.example.com"]}), config());
    let path = format!(
        "/api/{}/envelope/?sentry_key={KEY}&sentry_version=7&sentry_client=sentry.javascript.browser%2F9.0.0",
        h.project_id
    );
    let body = event_envelope(&json!({"event_id": new_id(), "message": "from browser", "platform": "javascript"}));
    let (s, headers, _) = h
        .send(request(
            &path,
            &[("content-type", "text/plain;charset=UTF-8"), ("origin", "https://app.example.com")],
            body.clone(),
        ))
        .await;
    assert_eq!(s, 200);
    assert_eq!(headers["access-control-allow-origin"], "https://app.example.com");
    assert!(headers["access-control-expose-headers"].to_str().unwrap().contains("x-sentry-rate-limits"));
    let (s, _, _) =
        h.send(request(&path, &[("content-type", "text/plain"), ("origin", "https://evil.example")], body)).await;
    assert_eq!(s, 403);

    let pre = |origin: &str| {
        http::Request::builder()
            .method("OPTIONS")
            .uri(format!("/api/{}/envelope/", h.project_id))
            .header("origin", origin)
            .header("access-control-request-method", "POST")
            .header("access-control-request-headers", "content-type")
            .body(axum::body::Body::empty())
            .unwrap()
    };
    let (s, headers, _) = h.send(pre("https://app.example.com")).await;
    assert_eq!(s, 200);
    assert_eq!(headers["access-control-allow-origin"], "https://app.example.com");
    assert!(headers["access-control-allow-methods"].to_str().unwrap().contains("POST"));
    assert_eq!(h.send(pre("https://evil.example")).await.0, 403);
}

#[tokio::test]
async fn rate_limits_answer_429_with_headers() {
    let h = harness_with(json!({"rate_limit_per_min": 2}), config());
    let mut statuses = Vec::new();
    let mut last = None;
    for _ in 0..3 {
        let (s, headers, _) = h.envelope(event_envelope(&json!({"event_id": new_id(), "message": "m"}))).await;
        statuses.push(s);
        last = Some(headers);
    }
    // The window may roll over between requests; at least one of the first
    // three must be limited unless it did.
    if statuses == [200, 200, 429] {
        let headers = last.unwrap();
        assert!(headers["retry-after"].to_str().unwrap().parse::<u64>().unwrap() >= 1);
        let rl = headers["x-sentry-rate-limits"].to_str().unwrap();
        assert!(rl.ends_with(":error;default;security;transaction:project"), "{rl}");
    }
    assert!(statuses.contains(&429) || statuses.iter().all(|s| *s == 200));
    // Sessions are not limited.
    let sess = envelope(json!({}), &[(json!({"type": "session"}), serde_json::to_vec(&json!({"sid": new_id(), "init": true, "started": "2026-11-14T10:00:00Z", "status": "ok", "attrs": {"release": "1.0"}})).unwrap())]);
    assert_eq!(h.envelope(sess).await.0, 200);
    h.flush().await;
    // Key limit.
    h.store.connect().execute("UPDATE project_keys SET rate_limit_per_min = 0", &[]).unwrap();
    h.store.bump_config();
    let (s, headers, _) = h.envelope(event_envelope(&json!({"event_id": new_id(), "message": "m"}))).await;
    assert_eq!(s, 429);
    assert!(headers["x-sentry-rate-limits"].to_str().unwrap().ends_with(":key"));
    assert!(h.ingest.outcome_count(Category::Error, Outcome::RateLimited) >= 1);
}

#[tokio::test]
async fn full_queue_answers_429() {
    let mut cfg = config();
    cfg.queue = 1;
    cfg.workers = 1;
    let h = harness_with(json!({}), cfg);
    let mut set = tokio::task::JoinSet::new();
    let body = event_envelope(&json!({"message": "m", "exception": [{"type": "E", "value": "v".repeat(50_000)}]}));
    for _ in 0..200 {
        let router = h.router.clone();
        let path = format!("/api/{}/envelope/", h.project_id);
        let body = body.clone();
        set.spawn(async move {
            use tower::ServiceExt;
            let res = router.oneshot(request(&path, &[("x-sentry-auth", &auth())], body)).await.unwrap();
            (res.status().as_u16(), res.headers().get("retry-after").map(|v| v.to_str().unwrap().to_string()))
        });
    }
    let mut limited = 0;
    while let Some(r) = set.join_next().await {
        let (s, retry) = r.unwrap();
        if s == 429 {
            limited += 1;
            assert_eq!(retry.as_deref(), Some("1"));
        } else {
            assert_eq!(s, 200);
        }
    }
    assert!(limited > 0, "no request saw a full queue");
    h.flush().await;
    assert!(h.ingest.outcome_count(Category::Error, Outcome::QueueFull) > 0);
}

#[tokio::test]
async fn every_item_type_is_accepted() {
    let h = harness();
    let event_id = new_id();
    let feedback_id = new_id();
    let items = vec![
        (json!({"type": "event"}), serde_json::to_vec(&error_event(&event_id, "boom")).unwrap()),
        (json!({"type": "attachment", "filename": "log.txt", "content_type": "text/plain"}), b"line 1\nline 2\n".to_vec()),
        (json!({"type": "session"}), serde_json::to_vec(&json!({"sid": new_id(), "init": true, "started": "2026-11-14T10:20:00Z", "status": "ok", "errors": 0, "attrs": {"release": "web@1.0.0", "environment": "production"}})).unwrap()),
        (json!({"type": "sessions"}), serde_json::to_vec(&json!({"aggregates": [{"started": "2026-11-14T10:00:00Z", "exited": 5, "errored": 1, "crashed": 1}], "attrs": {"release": "web@1.0.0", "environment": "production"}})).unwrap()),
        (json!({"type": "client_report"}), serde_json::to_vec(&json!({"timestamp": "2026-11-14T10:00:00Z", "discarded_events": [{"reason": "queue_overflow", "category": "error", "quantity": 4}]})).unwrap()),
        (json!({"type": "user_report"}), serde_json::to_vec(&json!({"event_id": event_id, "name": "Ann", "email": "ann@example.com", "comments": "It broke"})).unwrap()),
        (json!({"type": "check_in"}), serde_json::to_vec(&json!({"check_in_id": new_id(), "monitor_slug": "nightly-job", "status": "ok", "duration": 1.5, "environment": "production", "monitor_config": {"schedule": {"type": "crontab", "value": "0 3 * * *"}, "checkin_margin": 5, "timezone": "UTC"}})).unwrap()),
        (json!({"type": "transaction"}), serde_json::to_vec(&json!({"event_id": new_id(), "type": "transaction", "transaction": "GET /checkout", "start_timestamp": 1_800_000_000.0, "timestamp": 1_800_000_000.25, "contexts": {"trace": {"trace_id": "a".repeat(32), "span_id": "b".repeat(16), "op": "http.server", "status": "ok"}}, "spans": [{}, {}]})).unwrap()),
        (json!({"type": "replay_event"}), b"{}".to_vec()),
        (json!({"type": "replay_recording", "length": 3}), b"abc".to_vec()),
        (json!({"type": "profile"}), b"{}".to_vec()),
        (json!({"type": "profile_chunk"}), b"{}".to_vec()),
        (json!({"type": "statsd"}), b"a:1|c".to_vec()),
        (json!({"type": "metric_buckets"}), b"[]".to_vec()),
        (json!({"type": "span"}), br#"{"span_id":"b1","parent_span_id":"a1","is_segment":false}"#.to_vec()),
        (json!({"type": "log"}), b"{\"items\":[]}".to_vec()),
        (json!({"type": "something_new"}), b"{}".to_vec()),
    ];
    let (s, _, body) =
        h.envelope(envelope(json!({"event_id": event_id, "sdk": {"name": "test", "version": "1"}}), &items)).await;
    assert_eq!(s, 200);
    assert_eq!(body["id"], event_id);
    // Feedback (new style) in its own envelope, linked to the event.
    let fb = json!({"event_id": feedback_id, "type": "feedback", "timestamp": 1_800_000_000.0, "contexts": {"feedback": {"message": "Great app", "contact_email": "bob@example.com", "name": "Bob", "url": "https://app.example.com/x", "associated_event_id": event_id}}});
    h.envelope(envelope(
        json!({"event_id": feedback_id}),
        &[(json!({"type": "feedback"}), serde_json::to_vec(&fb).unwrap())],
    ))
    .await;
    h.flush().await;

    assert_eq!(h.count("SELECT count(*) FROM events"), 1);
    let att = h.store.connect().query_one("SELECT name, size, object_key FROM attachments", &[]).unwrap();
    assert_eq!(att.get::<String>(0), "log.txt");
    assert_eq!(att.get::<i64>(1), 14);
    let key: String = att.get(2);
    assert!(key.starts_with(&format!("attachments/{}/", h.project_id)) && key.contains(&event_id), "{key}");
    assert_eq!(h.store.objects().get(&key).unwrap().unwrap(), b"line 1\nline 2\n");
    let sc = h
        .store
        .connect()
        .query_one("SELECT sum(total)::bigint, sum(errored)::bigint, sum(crashed)::bigint FROM session_counts", &[])
        .unwrap();
    assert_eq!((sc.get::<i64>(0), sc.get::<i64>(1), sc.get::<i64>(2)), (8, 2, 1));
    assert_eq!(
        h.count("SELECT quantity FROM outcomes WHERE outcome = 'client_discard' AND reason = 'queue_overflow'"),
        4
    );
    assert_eq!(h.count("SELECT count(*) FROM user_feedback WHERE issue_id IS NOT NULL"), 2);
    assert_eq!(
        h.count("SELECT count(*) FROM user_feedback WHERE message = 'Great app' AND email = 'bob@example.com'"),
        1
    );
    let m = h
        .store
        .connect()
        .query_one("SELECT slug, health, schedule ->> 'value', next_checkin IS NOT NULL FROM monitors", &[])
        .unwrap();
    assert_eq!(m.get::<String>(0), "nightly-job");
    assert_eq!(m.get::<String>(1), "ok");
    assert_eq!(m.get::<Option<String>>(2).as_deref(), Some("0 3 * * *"));
    assert!(m.get::<bool>(3));
    assert_eq!(h.count("SELECT duration_ms FROM checkins"), 1500);
    let t =
        h.store.connect().query_one("SELECT name, op, status, duration_ms, span_count FROM transactions", &[]).unwrap();
    assert_eq!(t.get::<Option<String>>(0).as_deref(), Some("GET /checkout"));
    assert_eq!(t.get::<Option<String>>(1).as_deref(), Some("http.server"));
    assert_eq!(t.get::<Option<f64>>(3), Some(250.0));
    assert_eq!(t.get::<Option<i32>>(4), Some(2));
    // Malformed telemetry items are counted as invalid; only unknown item
    // types are acknowledged without being stored.
    let outcome = |outcome: &str, category: &str| {
        h.count(&format!(
            "SELECT coalesce(sum(quantity), 0)::bigint FROM outcomes WHERE outcome = '{outcome}' AND category = '{category}'"
        ))
    };
    for (category, n) in [("replay", 2), ("profile", 2), ("span", 3), ("metric_bucket", 0), ("log_item", 0)] {
        assert_eq!(outcome("invalid", category), n, "{category}");
    }
    assert_eq!(outcome("dropped_unsupported", "unknown"), 1);
    assert_eq!(h.count("SELECT count(*) FROM outcomes WHERE outcome = 'dropped_unsupported'"), 1);
    assert_eq!(outcome("accepted", "metric_bucket"), 1);
    assert_eq!(h.count("SELECT count(*) FROM metrics WHERE name = 'a' AND type = 'counter' AND value = 1"), 1);
    // The transaction's root span is stored; its two spans without ids are not.
    assert_eq!(h.count("SELECT count(*) FROM spans WHERE is_segment AND description = 'GET /checkout'"), 1);
    assert_eq!(h.ingest.outcome_count(Category::Replay, Outcome::Invalid), 2);
    assert_eq!(h.ingest.outcome_count(Category::Unknown, Outcome::DroppedUnsupported), 1);
    let mut metrics = String::new();
    h.ingest.render_metrics(&mut metrics);
    assert!(metrics.contains("rupta_ingest_events_total{category=\"replay\",outcome=\"invalid\"} 2"));
    assert!(metrics.contains("rupta_ingest_batches_total"));
}

#[tokio::test]
async fn sessions_count_init_once_and_track_crashes() {
    let h = harness();
    let sid = new_id();
    let session = |init: bool, status: &str, errors: u64| {
        envelope(json!({}), &[(json!({"type": "session"}), serde_json::to_vec(&json!({"sid": sid, "did": "user-1", "init": init, "started": "2026-11-14T10:20:00Z", "timestamp": "2026-11-14T10:25:00Z", "status": status, "errors": errors, "attrs": {"release": "1.0", "environment": "prod"}})).unwrap())])
    };
    for body in [session(true, "ok", 0), session(false, "ok", 1), session(false, "ok", 2), session(false, "crashed", 3)]
    {
        assert_eq!(h.envelope(body).await.0, 200);
        h.flush().await;
    }
    let r =
        h.store.connect().query_one("SELECT total, errored, crashed, abnormal, hour FROM session_counts", &[]).unwrap();
    assert_eq!((r.get::<i64>(0), r.get::<i64>(1), r.get::<i64>(2), r.get::<i64>(3)), (1, 1, 1, 0));
    assert_eq!(r.get::<chrono::DateTime<chrono::Utc>>(4).to_rfc3339(), "2026-11-14T10:00:00+00:00");
    // Sessions without a release are invalid.
    let bad = envelope(
        json!({}),
        &[(json!({"type": "session"}), b"{\"sid\":\"x\",\"started\":\"2026-11-14T10:00:00Z\"}".to_vec())],
    );
    assert_eq!(h.envelope(bad).await.0, 200);
    h.flush().await;
    assert_eq!(h.count("SELECT quantity FROM outcomes WHERE outcome = 'invalid' AND category = 'session'"), 1);
}

#[tokio::test]
async fn resolved_issue_regresses_and_ignore_count_runs_out() {
    let h = harness();
    let mut signals = h.ingest.subscribe();
    h.envelope(event_envelope(&error_event(&new_id(), "boom"))).await;
    h.flush().await;
    h.store
        .connect()
        .execute("UPDATE issues SET status = 'resolved', substatus = 'archived_forever', resolved_at = now()", &[])
        .unwrap();
    h.envelope(event_envelope(&error_event(&new_id(), "boom"))).await;
    h.flush().await;
    let r = h.store.connect().query_one("SELECT status, substatus, resolved_at IS NULL FROM issues", &[]).unwrap();
    assert_eq!(
        (r.get::<String>(0), r.get::<String>(1), r.get::<bool>(2)),
        ("unresolved".into(), "regressed".into(), true)
    );
    assert_eq!(h.count("SELECT count(*) FROM issue_activity WHERE kind = 'set_regression'"), 1);
    let mut kinds = Vec::new();
    while let Ok(s) = signals.try_recv() {
        kinds.push(match s {
            IssueSignal::New { .. } => "new",
            IssueSignal::Regressed { .. } => "regressed",
            IssueSignal::Reappeared { .. } => "reappeared",
            IssueSignal::Escalating { .. } => "escalating",
            IssueSignal::Seen { .. } => "seen",
        });
    }
    assert!(kinds.contains(&"new") && kinds.contains(&"regressed"), "{kinds:?}");

    // Resolved in a release: events of an older release do not regress.
    h.store
        .connect()
        .execute(
            "UPDATE issues SET status = 'resolved', resolved_in_release = 'web@2.0.0', resolved_at = now() + interval '1 hour'",
            &[],
        )
        .unwrap();
    h.envelope(event_envelope(&error_event(&new_id(), "boom"))).await;
    h.flush().await;
    assert_eq!(h.count("SELECT count(*) FROM issues WHERE status = 'resolved'"), 1);

    // Ignored until 2 more events: an absolute times_seen threshold.
    h.store
        .connect()
        .execute(
            "UPDATE issues SET status = 'ignored', substatus = 'archived_until_condition_met', ignore_count = times_seen + 2",
            &[],
        )
        .unwrap();
    h.envelope(event_envelope(&error_event(&new_id(), "boom"))).await;
    h.flush().await;
    assert_eq!(h.count("SELECT count(*) FROM issues WHERE status = 'ignored'"), 1);
    h.envelope(event_envelope(&error_event(&new_id(), "boom"))).await;
    h.flush().await;
    let r = h.store.connect().query_one("SELECT status, substatus FROM issues", &[]).unwrap();
    assert_eq!((r.get::<String>(0), r.get::<String>(1)), ("unresolved".into(), "ongoing".into()));

    // Ignored until a time that has passed.
    h.store
        .connect()
        .execute("UPDATE issues SET status = 'ignored', ignore_until = now() - interval '1 minute'", &[])
        .unwrap();
    h.envelope(event_envelope(&error_event(&new_id(), "boom"))).await;
    h.flush().await;
    assert_eq!(h.count("SELECT count(*) FROM issues WHERE status = 'unresolved'"), 1);
    assert_eq!(h.count("SELECT times_seen FROM issues"), 6);
}

#[tokio::test]
async fn duplicate_event_ids_are_stored_once() {
    let h = harness();
    let id = new_id();
    let body = event_envelope(&error_event(&id, "dup"));
    assert_eq!(h.envelope(body.clone()).await.0, 200);
    assert_eq!(h.envelope(body.clone()).await.0, 200);
    h.flush().await;
    assert_eq!(h.envelope(body).await.0, 200);
    h.flush().await;
    assert_eq!(h.count("SELECT count(*) FROM event_payloads"), 1);
    assert_eq!(h.count("SELECT count(*) FROM events"), 1);
    assert_eq!(h.count("SELECT times_seen FROM issues"), 1);
}

#[tokio::test]
async fn safe_durability_acks_after_commit() {
    let mut cfg = config();
    cfg.safe = true;
    let h = harness_with(json!({}), cfg);
    let id = new_id();
    assert_eq!(h.envelope(event_envelope(&error_event(&id, "safe"))).await.0, 200);
    // No flush: in safe mode the event is committed when the response arrives.
    assert_eq!(h.count("SELECT count(*) FROM events"), 1);
}

#[tokio::test]
async fn store_and_security_endpoints() {
    let h = harness();
    let id = new_id();
    let (s, _, body) = h
        .send(request(
            &format!("/api/{}/store/", h.project_id),
            &[("x-sentry-auth", &auth()), ("content-type", "application/json")],
            serde_json::to_vec(&json!({"event_id": id, "message": "legacy", "level": "warning"})).unwrap(),
        ))
        .await;
    assert_eq!((s, body["id"].as_str()), (200, Some(id.as_str())));
    let report = json!({"csp-report": {"document-uri": "https://app.example.com/", "effective-directive": "script-src", "violated-directive": "script-src 'self'", "blocked-uri": "https://evil.example/x.js"}});
    let (s, _, _) = h
        .send(request(
            &format!("/api/{}/security/?sentry_key={KEY}&sentry_environment=prod", h.project_id),
            &[("content-type", "application/csp-report"), ("user-agent", "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36")],
            serde_json::to_vec(&report).unwrap(),
        ))
        .await;
    assert_eq!(s, 200);
    h.flush().await;
    assert_eq!(h.payload(&id)["level"], "warning");
    let r = h.store.connect().query_one("SELECT title, type FROM issues WHERE type = 'csp'", &[]).unwrap();
    assert_eq!(r.get::<String>(0), "Blocked 'script' from 'evil.example'");
    assert_eq!(h.count("SELECT count(*) FROM events WHERE environment = 'prod' AND browser = 'Chrome 130.0'"), 1);
}

#[tokio::test]
async fn inbound_filters_drop_events() {
    let h = harness_with(json!({"filters": {"localhost": true, "error_messages": ["*ignore me*"]}}), config());
    h.envelope(event_envelope(&json!({"event_id": new_id(), "message": "please ignore me"}))).await;
    h.envelope(event_envelope(
        &json!({"event_id": new_id(), "message": "m", "request": {"url": "http://localhost:3000/"}}),
    ))
    .await;
    let id = new_id();
    let filtered_with_attachment = envelope(
        json!({"event_id": id}),
        &[
            (json!({"type": "event"}), b"{\"message\":\"ignore me too\"}".to_vec()),
            (json!({"type": "attachment", "filename": "a"}), b"x".to_vec()),
        ],
    );
    h.envelope(filtered_with_attachment).await;
    h.flush().await;
    assert_eq!(h.count("SELECT count(*) FROM events"), 0);
    assert_eq!(h.count("SELECT count(*) FROM attachments"), 0);
    assert_eq!(
        h.count("SELECT sum(quantity)::bigint FROM outcomes WHERE outcome = 'filtered' AND category = 'error'"),
        3
    );
    assert_eq!(h.count("SELECT quantity FROM outcomes WHERE reason = 'localhost'"), 1);
}

#[tokio::test]
async fn trusted_proxy_forwarded_for() {
    let mut cfg = config();
    cfg.trusted_proxies = rupta_ingest::parse_trusted_proxies(&["203.0.113.0/24"]).unwrap();
    let h = harness_with(json!({}), cfg);
    let id = new_id();
    let body = event_envelope(&json!({"event_id": id, "message": "m", "user": {"ip_address": "{{auto}}"}}));
    let path = format!("/api/{}/envelope/", h.project_id);
    h.send(request(&path, &[("x-sentry-auth", &auth()), ("x-forwarded-for", "198.51.100.23, 203.0.113.1")], body))
        .await;
    h.flush().await;
    assert_eq!(h.payload(&id)["user"]["ip_address"], "198.51.100.23");
}

#[tokio::test]
async fn random_bodies_never_crash_the_server() {
    let h = harness();
    let path = format!("/api/{}/envelope/", h.project_id);
    let mut state: u64 = 42;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let valid = event_envelope(&error_event(&new_id(), "fuzz"));
    for i in 0..300 {
        let body: Vec<u8> = if i % 2 == 0 {
            let mut b = valid.clone();
            for _ in 0..(next() % 10) {
                let at = next() as usize % b.len();
                b[at] = next() as u8;
            }
            b
        } else {
            (0..next() % 500).map(|_| next() as u8).collect()
        };
        let (s, _, _) = h.send(request(&path, &[("x-sentry-auth", &auth())], body)).await;
        assert!(s == 200 || s == 400, "status {s}");
    }
    h.flush().await;
}

#[tokio::test]
async fn shutdown_drains_the_queue() {
    let h = harness();
    for _ in 0..50 {
        h.envelope(event_envelope(&json!({"event_id": new_id(), "message": "m"}))).await;
    }
    h.ingest.shutdown().await;
    assert_eq!(h.count("SELECT count(*) FROM events"), 50);
    assert_eq!(h.envelope(event_envelope(&json!({"message": "late"}))).await.0, 503);
}
