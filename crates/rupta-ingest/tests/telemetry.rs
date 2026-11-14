//! Telemetry ingestion: sampling, per-kind switches, OpenTelemetry logs,
//! replays across envelopes, and retention of the telemetry tables.

mod common;

use std::io::Write;

use chrono::{Duration, TimeZone, Utc};
use common::*;
use serde_json::{Value, json};

fn transaction(trace_id: &str) -> (Value, Vec<u8>) {
    let tx = json!({"event_id": new_id(), "type": "transaction", "transaction": "GET /x", "start_timestamp": 1_794_650_000.0,
        "timestamp": 1_794_650_000.5, "contexts": {"trace": {"trace_id": trace_id, "span_id": "a1a1a1a1a1a1a1a1", "op": "http.server"}},
        "measurements": {"lcp": {"value": 1200.0, "unit": "millisecond"}},
        "spans": [{"span_id": "b2b2b2b2b2b2b2b2", "parent_span_id": "a1a1a1a1a1a1a1a1", "trace_id": trace_id, "op": "db", "description": "SELECT 1",
                   "start_timestamp": 1_794_650_000.1, "timestamp": 1_794_650_000.2, "data": {"db.system": "postgresql"}}]});
    (json!({"type": "transaction"}), serde_json::to_vec(&tx).unwrap())
}

fn logs(n: usize, trace: Option<&str>) -> (Value, Vec<u8>) {
    let items: Vec<Value> = (0..n)
        .map(|i| json!({"timestamp": 1_794_650_000.0 + i as f64, "level": "info", "body": format!("line {i}"), "trace_id": trace,
            "attributes": {"i": {"value": i, "type": "integer"}}}))
        .collect();
    (json!({"type": "log", "item_count": n}), serde_json::to_vec(&json!({"items": items})).unwrap())
}

#[tokio::test]
async fn transactions_store_their_spans_and_web_vitals() {
    let h = harness();
    let trace = "c".repeat(32);
    assert_eq!(h.envelope(envelope(json!({}), &[transaction(&trace)])).await.0, 200);
    h.flush().await;
    assert_eq!(h.count("SELECT count(*) FROM spans"), 2);
    let r = h
        .store
        .connect()
        .query_one("SELECT segment_id, parent_span_id, duration_ms, data ->> 'db.system', transaction FROM spans WHERE NOT is_segment", &[])
        .unwrap();
    assert_eq!(r.get::<Option<String>>(0).as_deref(), Some("a1a1a1a1a1a1a1a1"));
    assert_eq!(r.get::<Option<String>>(1).as_deref(), Some("a1a1a1a1a1a1a1a1"));
    assert!((r.get::<Option<f64>>(2).unwrap() - 100.0).abs() < 0.01);
    assert_eq!(r.get::<Option<String>>(3).as_deref(), Some("postgresql"));
    assert_eq!(r.get::<Option<String>>(4).as_deref(), Some("GET /x"));
    assert_eq!(h.count("SELECT lcp::bigint FROM transactions"), 1200);
    assert_eq!(h.count("SELECT lcp::bigint FROM spans WHERE is_segment"), 1200);
}

#[tokio::test]
async fn sampling_and_switches() {
    let h = harness_with(
        json!({"sampling": {"transactions": 0.0, "logs": 0.0}, "data_types": {"metrics": false, "replays": false, "profiles": false}}),
        config(),
    );
    let body = envelope(
        json!({}),
        &[
            transaction(&"d".repeat(32)),
            logs(3, None),
            (json!({"type": "statsd"}), b"a:1|c".to_vec()),
            (
                json!({"type": "replay_event"}),
                serde_json::to_vec(&json!({"replay_id": new_id(), "segment_id": 0})).unwrap(),
            ),
            (json!({"type": "profile"}), b"{}".to_vec()),
        ],
    );
    assert_eq!(h.envelope(body).await.0, 200);
    h.flush().await;
    for t in ["transactions", "spans", "logs", "metrics", "replays", "profiles"] {
        assert_eq!(h.count(&format!("SELECT count(*) FROM {t}")), 0, "{t}");
    }
    let q = |c: &str, r: &str| {
        h.count(&format!(
            "SELECT coalesce(sum(quantity), 0)::bigint FROM outcomes WHERE outcome = 'filtered' AND category = '{c}' AND reason = '{r}'"
        ))
    };
    assert_eq!(q("transaction", "sample_rate"), 1);
    assert_eq!(q("log_item", "sample_rate"), 3);
    assert_eq!(q("metric_bucket", "disabled"), 1);
    assert_eq!(q("replay", "disabled"), 1);
    assert_eq!(q("profile", "disabled"), 1);
}

#[tokio::test]
async fn half_sampled_logs_follow_their_trace() {
    let h = harness_with(json!({"sampling": {"logs": 0.5}}), config());
    for i in 0..40 {
        let trace = format!("{:032x}", i * 7919 + 1);
        h.envelope(envelope(json!({}), &[logs(2, Some(&trace))])).await;
    }
    h.flush().await;
    // Both lines of a trace are kept or dropped together.
    let per_trace: Vec<i64> = h
        .store
        .connect()
        .query("SELECT count(*) FROM logs GROUP BY trace_id", &[])
        .unwrap()
        .iter()
        .map(|r| r.get(0))
        .collect();
    assert!(per_trace.iter().all(|n| *n == 2), "{per_trace:?}");
    assert!((5..35).contains(&per_trace.len()), "{} of 40 traces kept", per_trace.len());
}

#[tokio::test]
async fn otel_logs() {
    let h = harness();
    let record = json!({"timeUnixNano": "1794650000123000000", "severityNumber": 17, "severityText": "ERROR",
        "body": {"stringValue": "db down"}, "traceId": "5B8EFFF798038103D269B633813FC60C", "spanId": "EEE19B7EC3C1B174",
        "attributes": [{"key": "service.name", "value": {"stringValue": "api"}}, {"key": "retry", "value": {"intValue": "3"}}]});
    h.envelope(envelope(json!({}), &[(json!({"type": "otel_log"}), serde_json::to_vec(&record).unwrap())])).await;
    h.flush().await;
    let r = h
        .store
        .connect()
        .query_one("SELECT level, body, trace_id, span_id, attributes ->> 'service.name', attributes ->> 'retry', timestamp FROM logs", &[])
        .unwrap();
    assert_eq!(r.get::<String>(0), "error");
    assert_eq!(r.get::<String>(1), "db down");
    assert_eq!(r.get::<Option<String>>(2).as_deref(), Some("5b8efff798038103d269b633813fc60c"));
    assert_eq!(r.get::<Option<String>>(3).as_deref(), Some("eee19b7ec3c1b174"));
    assert_eq!(r.get::<Option<String>>(4).as_deref(), Some("api"));
    assert_eq!(r.get::<Option<String>>(5).as_deref(), Some("3"));
    assert_eq!(r.get::<chrono::DateTime<Utc>>(6).timestamp_millis(), 1_794_650_000_123);
}

fn recording(segment: i64, events: &Value) -> Vec<u8> {
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(events.to_string().as_bytes()).unwrap();
    let mut out = format!("{{\"segment_id\":{segment}}}\n").into_bytes();
    out.extend(z.finish().unwrap());
    out
}

#[tokio::test]
async fn replays_grow_over_segments_and_link_errors() {
    let h = harness();
    let replay_id = new_id();
    let segment = |n: i64, t: f64| {
        let ev = json!({"type": "replay_event", "replay_id": replay_id, "segment_id": n, "replay_type": "session",
            "replay_start_timestamp": 1_794_650_000.0, "timestamp": t, "urls": [format!("https://app.example.com/{n}")],
            "error_ids": [], "trace_ids": [], "platform": "javascript", "user": {"email": "ann@example.com"},
            "contexts": {"browser": {"name": "Firefox", "version": "140"}}});
        let events = json!([{"type": 3, "data": {"source": 2, "type": 2}, "timestamp": (t * 1000.0) as i64}]);
        envelope(
            json!({"event_id": replay_id}),
            &[
                (json!({"type": "replay_event"}), serde_json::to_vec(&ev).unwrap()),
                (json!({"type": "replay_recording"}), recording(n, &events)),
            ],
        )
    };
    h.envelope(segment(0, 1_794_650_005.0)).await;
    h.flush().await;
    // An error recorded during the replay, in its own envelope.
    let err = json!({"event_id": new_id(), "level": "error", "exception": {"values": [{"type": "Error", "value": "boom"}]},
        "contexts": {"replay": {"replay_id": replay_id}}});
    h.envelope(event_envelope(&err)).await;
    h.envelope(segment(1, 1_794_650_030.0)).await;
    h.flush().await;
    let r = h
        .store
        .connect()
        .query_one("SELECT segment_count, count_clicks, count_errors, urls, browser, user_email, finished - started FROM replays", &[])
        .unwrap();
    assert_eq!((r.get::<i32>(0), r.get::<i32>(1), r.get::<i32>(2)), (2, 2, 1));
    assert_eq!(r.get::<Value>(3), json!(["https://app.example.com/0", "https://app.example.com/1"]));
    assert_eq!(r.get::<Option<String>>(4).as_deref(), Some("Firefox 140"));
    assert_eq!(r.get::<Option<String>>(5).as_deref(), Some("ann@example.com"));
    assert_eq!(h.count("SELECT count(*) FROM issue_replays"), 1);
    let keys: Vec<String> = h
        .store
        .connect()
        .query("SELECT object_key FROM replay_segments ORDER BY segment_id", &[])
        .unwrap()
        .iter()
        .map(|r| r.get(0))
        .collect();
    assert_eq!(keys.len(), 2);
    assert!(
        keys[0].starts_with(&format!("replays/{}/2026-11-", h.project_id)) && keys[0].ends_with("/0.zst"),
        "{}",
        keys[0]
    );
}

#[tokio::test]
async fn telemetry_retention() {
    let h = harness_with(
        json!({"retention": {"logs": 7, "spans": 7, "metrics": 7, "profiles": 7, "replays": 7}}),
        config(),
    );
    let p = h.project_id;
    let now = Utc.with_ymd_and_hms(2026, 11, 14, 12, 0, 0).unwrap();
    let conn = h.store.connect();
    for (ts, n) in [(now - Duration::days(10), 0u8), (now - Duration::days(1), 1)] {
        let row = rupta_store::telemetry::LogRow {
            project_id: p,
            id: [n; 16],
            timestamp: ts,
            received: ts,
            level: "info".into(),
            body: "x".into(),
            ..Default::default()
        };
        conn.append("logs", &[row.into_values()]).unwrap();
        let span = rupta_store::telemetry::SpanRow {
            project_id: p,
            trace_id: "t".into(),
            span_id: format!("s{n}"),
            timestamp: ts,
            received: ts,
            ..Default::default()
        };
        conn.append("spans", &[span.into_values()]).unwrap();
        let day = ts.date_naive();
        let pkey = format!("profiles/{p}/{}/{n}.json.zst", day.format("%Y-%m-%d"));
        h.store.objects().put(&pkey, b"x").unwrap();
        rupta_store::telemetry::insert_profile(
            &conn,
            &rupta_store::telemetry::ProfileRow {
                project_id: p,
                profile_id: [n; 16],
                kind: "chunk".into(),
                transaction_name: None,
                transaction_id: None,
                trace_id: None,
                profiler_id: None,
                platform: None,
                release: None,
                environment: None,
                started: ts,
                duration_ms: None,
                samples: 1,
                size: 1,
                object_key: pkey,
                day,
                received: ts,
            },
        )
        .unwrap();
        let rkey = format!("replays/{p}/{}/{}/0.zst", day.format("%Y-%m-%d"), n);
        h.store.objects().put(&rkey, b"x").unwrap();
        rupta_store::telemetry::apply_replay_update(
            &conn,
            &rupta_store::telemetry::ReplayUpdate {
                project_id: p,
                replay_id: [n; 16],
                started: Some(ts),
                finished: Some(ts),
                received: ts,
                day,
                meta: None,
                urls: vec![],
                error_ids: vec![],
                trace_ids: vec![],
                segment: Some(rupta_store::telemetry::ReplaySegmentRow {
                    segment_id: 0,
                    object_key: rkey,
                    size: 1,
                    clicks: 0,
                }),
            },
        )
        .unwrap();
    }
    let store = h.store.clone();
    let report =
        tokio::task::spawn_blocking(move || rupta_ingest::retention::run_once(&store, 90, now)).await.unwrap().unwrap();
    let t = &report.telemetry;
    assert_eq!((t.logs, t.spans, t.profiles, t.replays, t.replay_segments), (1, 1, 1, 1, 1));
    assert_eq!(report.objects_deleted, 2);
    for table in ["logs", "spans", "profiles", "replays", "replay_segments"] {
        assert_eq!(h.count(&format!("SELECT count(*) FROM {table}")), 1, "{table}");
    }
    let old = (now - Duration::days(10)).date_naive().format("%Y-%m-%d").to_string();
    assert!(h.store.objects().list(&format!("profiles/{p}/{old}/")).unwrap().is_empty());
    assert!(h.store.objects().list(&format!("replays/{p}/{old}/")).unwrap().is_empty());
    let recent = (now - Duration::days(1)).date_naive().format("%Y-%m-%d").to_string();
    assert_eq!(h.store.objects().list(&format!("replays/{p}/{recent}/")).unwrap().len(), 1);
}
