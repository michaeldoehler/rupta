//! Measures the main telemetry queries through the HTTP API on a local
//! database with generated data: 100,000 transactions with 1,000,000
//! spans, 1,000,000 logs and 1,000,000 metric values over 7 days in one
//! project.
//!
//! ```sh
//! cargo run --release -p rupta-api --example telemetry_bench [-- <rows>]
//! ```

use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::Request;
use chrono::{Duration as ChronoDuration, Utc};
use http_body_util::BodyExt;
use rupta_api::{ApiConfig, ApiState};
use rupta_core::role::Role;
use rupta_store::telemetry::{LogRow, MetricRow, SpanRow, Vitals};
use rupta_store::{Store, StoreOptions};
use serde_json::json;
use tower::ServiceExt;

fn main() {
    let rows: usize = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(1_000_000);
    let dir = tempfile::tempdir().expect("temp dir");
    let store = Store::open(&StoreOptions::local(dir.path())).expect("open");
    let conn = store.connect();
    let org = rupta_auth::users::create_org(&conn, "bench", "Bench").unwrap();
    let project: i64 = conn
        .query_one("INSERT INTO projects (org_id, slug, name) VALUES ($1, 'p', 'P') RETURNING id", &[&org])
        .unwrap()
        .get(0);
    let uid = rupta_auth::users::create(&conn, "bench@example.com", "Bench", None, false).unwrap();
    rupta_auth::users::set_member(&conn, org, uid, Role::Viewer).unwrap();
    let session = rupta_auth::session::create(&conn, uid, None, None).unwrap();

    let now = Utc::now();
    let span_ms = 7 * 24 * 3600 * 1000 / rows as i64;
    let names = ["GET /", "GET /orders", "POST /checkout", "GET /api/users", "GET /api/items", "worker.process"];
    let ops = [
        "db.query",
        "http.client",
        "cache.get",
        "function",
        "db.query",
        "template.render",
        "db.query",
        "serialize",
        "http.client",
    ];
    let levels = ["trace", "debug", "info", "info", "info", "info", "warn", "error"];
    let mut trace_ids = Vec::new();
    let start = Instant::now();
    let chunk = 50_000;
    let mut i = 0;
    while i < rows {
        let n = chunk.min(rows - i);
        let mut spans = Vec::with_capacity(n);
        let mut txs = Vec::new();
        let mut logs = Vec::with_capacity(n);
        let mut metrics = Vec::with_capacity(n);
        for k in i..i + n {
            let ts = now - ChronoDuration::days(7) + ChronoDuration::milliseconds(k as i64 * span_ms);
            let tx = k / 10;
            let trace = format!("{:032x}", tx as u128 * 0x9e37_79b9_7f4a_7c15);
            let name = names[tx % names.len()];
            let segment = k % 10 == 0;
            let duration = ((k * 7919) % 2000) as f64 / if segment { 1.0 } else { 10.0 };
            if segment {
                if trace_ids.len() < 100 && tx % 997 == 0 {
                    trace_ids.push(trace.clone());
                }
                txs.push(rupta_store::ingest::TransactionRow {
                    project_id: project,
                    event_id: *uuid::Uuid::new_v4().as_bytes(),
                    trace_id: Some(trace.clone()),
                    span_id: Some(format!("{:016x}", k)),
                    name: Some(name.into()),
                    op: Some("http.server".into()),
                    status: Some(if k % 50 == 0 { "internal_error" } else { "ok" }.into()),
                    timestamp: ts + ChronoDuration::milliseconds(duration as i64),
                    start_timestamp: Some(ts),
                    duration_ms: Some(duration),
                    received: ts,
                    environment: Some("production".into()),
                    release: Some("1.0".into()),
                    user_hash: Some((k % 5000) as i64),
                    tags: "{}".into(),
                    measurements: "{}".into(),
                    lcp: (tx % 3 == 0).then_some(duration * 2.0),
                    ..Default::default()
                });
            }
            spans.push(
                SpanRow {
                    project_id: project,
                    trace_id: trace.clone(),
                    span_id: format!("{:016x}", k),
                    parent_span_id: (!segment).then(|| format!("{:016x}", k - k % 10)),
                    segment_id: Some(format!("{:016x}", k - k % 10)),
                    is_segment: segment,
                    transaction: Some(name.into()),
                    op: Some(if segment { "http.server" } else { ops[k % ops.len()] }.into()),
                    description: Some(if segment {
                        name.to_string()
                    } else {
                        format!("SELECT * FROM t{} WHERE id = ?", k % 40)
                    }),
                    status: Some("ok".into()),
                    timestamp: ts,
                    duration_ms: Some(duration),
                    environment: Some("production".into()),
                    release: Some("1.0".into()),
                    received: ts,
                    data: json!({"db.system": "postgresql", "thread.id": k % 8}),
                    vitals: Vitals { lcp: (segment && tx % 3 == 0).then_some(duration * 2.0), ..Default::default() },
                    ..Default::default()
                }
                .into_values(),
            );
            logs.push(
                LogRow {
                    project_id: project,
                    id: *uuid::Uuid::now_v7().as_bytes(),
                    timestamp: ts,
                    received: ts,
                    trace_id: Some(trace.clone()),
                    span_id: Some(format!("{:016x}", k)),
                    level: levels[k % levels.len()].into(),
                    severity_number: Some(9),
                    body: format!("request {k} handled for customer {} in {duration} ms", k % 1000),
                    environment: Some("production".into()),
                    release: Some("1.0".into()),
                    sdk: Some("sentry.javascript.node".into()),
                    attributes: json!({"customer.id": format!("c{}", k % 1000), "http.route": name, "region": (["eu", "us", "ap"][k % 3])}),
                }
                .into_values(),
            );
            metrics.push(
                MetricRow {
                    project_id: project,
                    timestamp: ts,
                    received: ts,
                    name: ["http.requests", "http.latency", "queue.depth"][k % 3].into(),
                    ty: ["counter", "distribution", "gauge"][k % 3].into(),
                    unit: None,
                    value: duration,
                    environment: Some("production".into()),
                    attributes: json!({"route": name}),
                    ..Default::default()
                }
                .into_values(),
            );
        }
        let txn = conn.transaction().unwrap();
        rupta_store::ingest::append_transactions(&txn, &txs).unwrap();
        txn.append("spans", &spans).unwrap();
        txn.append("logs", &logs).unwrap();
        txn.append("metrics", &metrics).unwrap();
        txn.commit().unwrap();
        i += n;
    }
    for t in ["transactions", "spans", "logs", "metrics"] {
        conn.execute(&format!("VACUUM {t}"), &[]).unwrap();
    }
    println!(
        "generated {} transactions, {rows} spans, {rows} logs, {rows} metric values over 7 days in {:.1?}",
        rows / 10,
        start.elapsed()
    );

    if std::env::var_os("BENCH_EXPLAIN").is_some() {
        let since = now - ChronoDuration::days(7);
        for sql in [
            "SELECT date_trunc('hour', timestamp), approx_percentile(value, 0.95) FROM metrics WHERE project_id IN (1) AND timestamp >= $1 AND name = 'http.latency' GROUP BY 1",
            "SELECT date_trunc('hour', timestamp), approx_percentile(value, 0.95) FROM metrics WHERE project_id IN (1) AND timestamp >= $1 AND name = 'http.latency' AND (TRUE) GROUP BY 1",
            "SELECT type, unit FROM metrics WHERE project_id IN (1) AND timestamp >= $1 AND name = 'http.latency' LIMIT 1",
            "SELECT id, timestamp FROM logs WHERE project_id IN (1) AND timestamp >= $1 ORDER BY timestamp DESC, id DESC LIMIT 101",
            "SELECT id, project_id, timestamp, received, level, severity_number, body, trace_id, span_id, environment, release, sdk, attributes FROM logs WHERE project_id IN (1) AND timestamp >= $1 ORDER BY timestamp DESC, id DESC LIMIT 101",
        ] {
            for _ in 0..2 {
                let t = Instant::now();
                let n = conn.query(sql, &[&since]).unwrap().len();
                println!("{:>8.1}ms {n:>6} {sql}", t.elapsed().as_secs_f64() * 1e3);
            }
            for r in conn.query(&format!("EXPLAIN {sql}"), &[&since]).unwrap() {
                println!("    {}", r.get::<String>(0));
            }
        }
    }
    let state = ApiState::new(store.clone(), ApiConfig::new("http://bench", b"bench secret key that is long enough!!"));
    let router = rupta_api::router(state);
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    let get = |path: String| {
        let router = router.clone();
        let cookie = format!("rupta_session={}", session.token);
        rt.block_on(async move {
            let req = Request::builder().uri(&path).header("cookie", cookie).body(Body::empty()).unwrap();
            let t = Instant::now();
            let res = router.oneshot(req).await.unwrap();
            let status = res.status();
            let bytes = res.into_body().collect().await.unwrap().to_bytes();
            assert!(status.is_success(), "{path}: {status} {}", String::from_utf8_lossy(&bytes));
            (t.elapsed(), bytes.len())
        })
    };
    let trace = trace_ids[trace_ids.len() / 2].clone();
    let queries: Vec<(&str, String)> = vec![
        ("transactions, 24h", "/api/v1/orgs/bench/performance/transactions?period=24h".into()),
        ("transactions, 7d", "/api/v1/orgs/bench/performance/transactions?period=7d".into()),
        (
            "transaction summary, 7d",
            "/api/v1/orgs/bench/performance/transactions/summary?period=7d&name=GET%20%2Forders".into(),
        ),
        ("trace by id", format!("/api/v1/orgs/bench/traces/{trace}")),
        ("spans by op, 7d", "/api/v1/orgs/bench/spans?period=7d&group_by=op".into()),
        ("spans by description, 24h", "/api/v1/orgs/bench/spans?period=24h&group_by=description".into()),
        ("logs, newest 100, 24h", "/api/v1/orgs/bench/logs?period=24h&limit=100".into()),
        ("logs, level:error, 7d", "/api/v1/orgs/bench/logs?period=7d&query=level:error".into()),
        ("logs, attribute filter, 7d", "/api/v1/orgs/bench/logs?period=7d&query=customer.id:c42".into()),
        ("logs, body text, 7d", "/api/v1/orgs/bench/logs?period=7d&query=customer%20999".into()),
        ("logs by trace", format!("/api/v1/orgs/bench/logs?period=7d&query=trace:{trace}")),
        ("log histogram, 7d", "/api/v1/orgs/bench/logs/histogram?period=7d".into()),
        ("log histogram, 24h", "/api/v1/orgs/bench/logs/histogram?period=24h".into()),
        ("metric names, 7d", "/api/v1/orgs/bench/metrics/names?period=7d".into()),
        ("metric p95, 7d", "/api/v1/orgs/bench/metrics/query?period=7d&name=http.latency&op=p95".into()),
        (
            "metric sum by route, 7d",
            "/api/v1/orgs/bench/metrics/query?period=7d&name=http.requests&op=sum&group_by=route".into(),
        ),
    ];
    println!("{:<30} {:>10} {:>10} {:>10}", "query", "first", "median", "bytes");
    for (label, path) in queries {
        let (first, size) = get(path.clone());
        let mut times: Vec<Duration> = (0..5).map(|_| get(path.clone()).0).collect();
        times.sort();
        println!("{label:<30} {:>8.1}ms {:>8.1}ms {size:>10}", first.as_secs_f64() * 1e3, times[2].as_secs_f64() * 1e3);
    }
}
