//! Replays the telemetry envelopes captured from the official open-source
//! SDKs (`tests/fixtures/telemetry/`, recorded with `scripts/sdk-smoke.sh`
//! and `SMOKE_CAPTURE_DIR`, converted with `scripts/sdk-smoke/make-fixtures.py`)
//! and checks what was stored: spans, logs, metrics, profiles, replays.
//!
//! SDK versions captured: @sentry/node, @sentry/profiling-node and
//! @sentry/browser 11.12.0, sentry/sentry (PHP) 4.37.0, sentry-sdk
//! (Python) 2.75.0; Chromium 1243 driven by Playwright 1.63.

mod common;

use std::path::PathBuf;

use common::*;
use serde_json::Value;

pub fn telemetry_fixtures() -> Vec<(String, Value, Vec<u8>)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/telemetry");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.ends_with(".envelope"))
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|n| {
            let base = n.trim_end_matches(".envelope").to_string();
            let meta: Value =
                serde_json::from_slice(&std::fs::read(dir.join(format!("{base}.json"))).unwrap()).unwrap();
            (base, meta, std::fs::read(dir.join(&n)).unwrap())
        })
        .collect()
}

async fn replay(h: &Harness) {
    let all = telemetry_fixtures();
    assert!(all.len() >= 40, "fixtures missing");
    for (name, meta, body) in all {
        let path = match meta["query"].as_str() {
            Some(q) if !q.is_empty() => format!("{}?{q}", meta["path"].as_str().unwrap()),
            _ => meta["path"].as_str().unwrap().to_string(),
        };
        let headers: Vec<(String, String)> = meta["headers"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
            .collect();
        let refs: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        let (status, _, res) = h.send(request(&path, &refs, body)).await;
        assert_eq!(status, 200, "{name}: {res}");
    }
    h.flush().await;
}

fn strings(h: &Harness, sql: &str) -> Vec<String> {
    h.store.connect().query(sql, &[]).unwrap().iter().map(|r| r.get::<Option<String>>(0).unwrap_or_default()).collect()
}

#[tokio::test]
async fn telemetry_from_the_sdks_is_stored() {
    let h = harness();
    replay(&h).await;

    // Nothing rejected, nothing dropped as unsupported.
    let bad = strings(
        &h,
        "SELECT category || ':' || outcome || ':' || reason FROM outcomes WHERE outcome IN ('invalid', 'dropped_unsupported')",
    );
    assert!(bad.is_empty(), "{bad:?}");

    // Spans: every SDK, with segments as transactions.
    let span_sdks = strings(&h, "SELECT DISTINCT sdk FROM spans ORDER BY 1");
    for sdk in ["sentry.javascript.browser", "sentry.javascript.node", "sentry.php", "sentry.python"] {
        assert!(span_sdks.contains(&sdk.to_string()), "no spans from {sdk}: {span_sdks:?}");
    }
    for name in
        ["smoke-node-transaction", "smoke-php-transaction", "smoke-python-transaction", "smoke-python-continuous"]
    {
        assert_eq!(h.count(&format!("SELECT count(*) FROM transactions WHERE name = '{name}'")), 1, "{name}");
        assert_eq!(
            h.count(&format!("SELECT count(*) FROM spans WHERE is_segment AND transaction = '{name}'")),
            1,
            "{name}"
        );
    }
    // Child spans point at their segment and parent.
    assert_eq!(
        h.count(
            "SELECT count(*) FROM spans WHERE transaction = 'smoke-node-transaction' AND description = 'compute checksum'
             AND op = 'function' AND parent_span_id = segment_id AND duration_ms >= 250"
        ),
        1
    );
    assert_eq!(
        h.count("SELECT count(*) FROM spans WHERE transaction = 'smoke-php-transaction' AND op = 'db.query' AND description = 'SELECT 1'"),
        1
    );
    // Browser web vitals from streamed spans.
    assert!(h.count("SELECT count(*) FROM spans WHERE sdk = 'sentry.javascript.browser' AND (fcp IS NOT NULL OR ttfb IS NOT NULL)") >= 1);
    assert!(h.count("SELECT count(*) FROM spans WHERE op = 'navigation' AND sdk = 'sentry.javascript.browser'") >= 1);

    // Logs: every SDK, attributes flattened, scrubbed, linked to traces.
    let log_sdks = strings(&h, "SELECT DISTINCT sdk FROM logs ORDER BY 1");
    assert_eq!(log_sdks, vec!["sentry.javascript.browser", "sentry.javascript.node", "sentry.php", "sentry.python"]);
    let r = h
        .store
        .connect()
        .query_one(
            "SELECT level, severity_number, attributes ->> 'smoke.kind', attributes ->> 'order.id', environment, release, trace_id
             FROM logs WHERE body = 'smoke node log'",
            &[],
        )
        .unwrap();
    assert_eq!(r.get::<String>(0), "info");
    assert_eq!(r.get::<Option<i32>>(1), Some(9));
    assert_eq!(r.get::<Option<String>>(2).as_deref(), Some("node"));
    assert_eq!(r.get::<Option<String>>(3).as_deref(), Some("42"));
    assert_eq!(r.get::<Option<String>>(4).as_deref(), Some("smoke"));
    assert_eq!(r.get::<Option<String>>(5).as_deref(), Some("smoke-node@1.0.0"));
    let node_trace: String = r.get::<Option<String>>(6).unwrap();
    assert_eq!(
        h.count(&format!("SELECT count(*) FROM transactions WHERE trace_id = '{node_trace}'")),
        1,
        "log and transaction share the trace"
    );
    assert_eq!(
        strings(&h, "SELECT attributes ->> 'password' FROM logs WHERE body = 'smoke node log error'"),
        vec!["[Filtered]"]
    );
    assert_eq!(
        h.count("SELECT count(*) FROM logs WHERE body = 'smoke node low stock for widget' AND level = 'warn'"),
        1
    );
    assert_eq!(
        h.count("SELECT count(*) FROM logs WHERE body = 'smoke python log widget' AND sdk = 'sentry.python'"),
        1
    );
    assert_eq!(
        h.count(
            "SELECT count(*) FROM logs WHERE body = 'smoke php log widget' AND attributes ->> 'smoke.kind' = 'php'"
        ),
        1
    );
    assert_eq!(h.count("SELECT count(*) FROM logs WHERE body = 'smoke browser log' AND level = 'info'"), 1);

    // Metrics: current SDK format and the deprecated statsd/metric_buckets.
    let m = |sql: &str| h.store.connect().query_one(sql, &[]).unwrap();
    let r = m("SELECT type, sum(value), count(*) FROM metrics WHERE name = 'smoke.node.requests' GROUP BY type");
    assert_eq!((r.get::<String>(0), r.get::<f64>(1)), ("counter".to_string(), 2.0));
    let r = m(
        "SELECT type, unit, count(*), min(value), max(value) FROM metrics WHERE name = 'smoke.node.latency' GROUP BY type, unit",
    );
    assert_eq!(
        (r.get::<String>(0), r.get::<Option<String>>(1)),
        ("distribution".to_string(), Some("millisecond".to_string()))
    );
    assert_eq!((r.get::<i64>(2), r.get::<f64>(3), r.get::<f64>(4)), (2, 80.0, 125.5));
    assert_eq!(
        h.count(
            "SELECT count(*) FROM metrics WHERE name = 'smoke.node.requests' AND attributes ->> 'route' = '/checkout'"
        ),
        1
    );
    for name in ["smoke.python.requests", "smoke.php.requests", "smoke.browser.clicks"] {
        assert!(
            h.count(&format!("SELECT count(*) FROM metrics WHERE name = '{name}' AND type = 'counter'")) >= 1,
            "{name}"
        );
    }
    assert_eq!(h.count("SELECT sum(value)::bigint FROM metrics WHERE name = 'smoke.statsd.requests'"), 3);
    assert_eq!(
        h.count("SELECT count(*) FROM metrics WHERE name = 'smoke.statsd.latency' AND type = 'distribution'"),
        3
    );
    assert_eq!(
        h.count("SELECT count(DISTINCT value) FROM metrics WHERE name = 'smoke.statsd.users' AND type = 'set'"),
        2
    );
    assert_eq!(h.count("SELECT value::bigint FROM metrics WHERE name = 'smoke.statsd.queue' AND type = 'gauge'"), 3);

    // Profiles: a Python transaction profile and continuous chunks from
    // Node, Python and the browser, each document in the object store.
    let kinds = strings(&h, "SELECT platform || ':' || kind FROM profiles GROUP BY 1 ORDER BY 1");
    assert_eq!(kinds, vec!["javascript:chunk", "node:chunk", "python:chunk", "python:transaction"]);
    let r = m(
        "SELECT transaction_name, trace_id, samples, object_key, duration_ms FROM profiles WHERE kind = 'transaction'",
    );
    assert_eq!(r.get::<Option<String>>(0).as_deref(), Some("smoke-python-transaction"));
    assert!(r.get::<i32>(2) > 10, "samples: {}", r.get::<i32>(2));
    assert!(r.get::<Option<f64>>(4).unwrap() > 250.0);
    let key: String = r.get(3);
    assert!(key.starts_with(&format!("profiles/{}/", h.project_id)) && key.ends_with(".json.zst"), "{key}");
    let doc: Value =
        serde_json::from_slice(&zstd::decode_all(&h.store.objects().get(&key).unwrap().unwrap()[..]).unwrap()).unwrap();
    assert_eq!(doc["platform"], "python");
    // Node transactions name their continuous profiler session.
    assert_eq!(
        h.count(
            "SELECT count(*) FROM transactions t WHERE t.name = 'smoke-node-transaction' AND t.profiler_id IN
             (SELECT profiler_id FROM profiles WHERE platform = 'node')"
        ),
        1
    );

    // Replay: one replay with its segment, clicks, URLs and linked errors.
    let r = m(
        "SELECT replay_id, segment_count, count_clicks, count_errors, urls, browser, user_email, environment, release FROM replays",
    );
    assert_eq!(r.get::<i32>(1), 1);
    assert!(r.get::<i32>(2) >= 2, "clicks: {}", r.get::<i32>(2));
    assert!(r.get::<i32>(3) >= 2, "errors: {}", r.get::<i32>(3));
    assert!(r.get::<Value>(4).to_string().contains("localhost:3000"));
    assert!(r.get::<Option<String>>(5).unwrap().starts_with("Chrome"));
    assert_eq!(r.get::<Option<String>>(6).as_deref(), Some("browser@example.com"));
    assert_eq!(r.get::<Option<String>>(8).as_deref(), Some("smoke-browser@1.0.0"));
    let replay_id: [u8; 16] = r.get(0);
    let key: String = h
        .store
        .connect()
        .query_one("SELECT object_key FROM replay_segments WHERE replay_id = $1 AND segment_id = 0", &[&replay_id])
        .unwrap()
        .get(0);
    let events: Vec<Value> =
        serde_json::from_slice(&zstd::decode_all(&h.store.objects().get(&key).unwrap().unwrap()[..]).unwrap()).unwrap();
    assert!(events.iter().any(|e| e["type"] == 2), "a full snapshot");
    assert!(events.iter().any(|e| e["type"] == 4), "a meta event");
    // The browser errors were recorded during the replay.
    let linked = strings(&h, "SELECT i.title FROM issue_replays l JOIN issues i ON i.id = l.issue_id ORDER BY 1");
    assert!(linked.contains(&"TypeError: smoke browser error".to_string()), "{linked:?}");
    assert!(linked.contains(&"Error: smoke browser unhandled".to_string()), "{linked:?}");
    assert_eq!(
        h.count("SELECT count(*) FROM events WHERE trace_id IS NOT NULL AND sdk = 'sentry.javascript.browser'"),
        3
    );
}
