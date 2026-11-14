//! The telemetry API against data from the official SDKs: the envelopes
//! in `rupta-ingest/tests/fixtures/telemetry` go through the ingest
//! pipeline into the same database, then the endpoints are queried.

mod common;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use common::{As, TestApp};
use http_body_util::BodyExt;
use rupta_core::role::Role;
use serde_json::{Value, json};
use tower::ServiceExt;

const KEY: &str = "0123456789abcdef0123456789abcdef";
/// The fixtures were recorded on this day.
const RANGE: &str = "start=2026-11-14T00:00:00Z&end=2026-11-15T00:00:00Z";

struct Setup {
    app: TestApp,
    viewer: As,
    ingest: rupta_ingest::Ingest,
}

async fn setup() -> Setup {
    let app = TestApp::new();
    let org = app.org("acme");
    let project = app.project(org, "web");
    assert_eq!(project, 1, "the fixtures post to project 1");
    app.conn()
        .execute(
            "INSERT INTO project_keys (project_id, public_key, secret_key) VALUES ($1, $2, 's')",
            &[&project, &KEY],
        )
        .unwrap();
    app.state.store.bump_config();
    let (_, viewer) = app.member(org, "viewer@example.com", Role::Viewer);
    let ingest = rupta_ingest::Ingest::start(
        app.state.store.clone(),
        rupta_ingest::IngestConfig { workers: 2, batch_interval: Duration::from_millis(5), ..Default::default() },
    );
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../rupta-ingest/tests/fixtures/telemetry");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.ends_with(".envelope"))
        .collect();
    names.sort();
    let router = ingest.router();
    for n in names {
        let meta: Value =
            serde_json::from_slice(&std::fs::read(dir.join(n.replace(".envelope", ".json"))).unwrap()).unwrap();
        let mut uri = meta["path"].as_str().unwrap().to_string();
        if let Some(q) = meta["query"].as_str().filter(|q| !q.is_empty()) {
            uri = format!("{uri}?{q}");
        }
        let mut b = Request::builder().method("POST").uri(uri);
        for (k, v) in meta["headers"].as_object().unwrap() {
            b = b.header(k.as_str(), v.as_str().unwrap());
        }
        let mut req = b.body(Body::from(std::fs::read(dir.join(&n)).unwrap())).unwrap();
        req.extensions_mut().insert(ConnectInfo("203.0.113.7:5555".parse::<SocketAddr>().unwrap()));
        let res = router.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK, "{n}");
    }
    assert!(ingest.flush(Duration::from_secs(20)).await);
    Setup { app, viewer, ingest }
}

fn find<'a>(list: &'a Value, key: &str, value: &str) -> &'a Value {
    list.as_array().unwrap().iter().find(|x| x[key] == value).unwrap_or_else(|| panic!("no {key}={value} in {list}"))
}

#[tokio::test]
async fn performance_and_traces() {
    let Setup { app, viewer, .. } = setup().await;
    let r = app.get(&format!("/api/v1/orgs/acme/performance/transactions?{RANGE}"), &viewer).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let node = find(&r.body["data"], "name", "smoke-node-transaction");
    assert_eq!(node["count"], 1);
    assert_eq!(node["op"], "task");
    assert!(node["p50"].as_f64().unwrap() >= 250.0, "{node}");
    assert_eq!(node["failure_rate"], 0.0);
    assert_eq!(node["apdex"], 0.5, "between 300 ms and 1.2 s: tolerating");
    let pageload =
        r.body["data"].as_array().unwrap().iter().find(|t| t["op"] == "pageload").expect("a browser pageload").clone();
    // Sorting and paging.
    let r = app.get(&format!("/api/v1/orgs/acme/performance/transactions?{RANGE}&sort=-p95&limit=2"), &viewer).await;
    assert_eq!(r.body["data"].as_array().unwrap().len(), 2);
    let p95: Vec<f64> = r.body["data"].as_array().unwrap().iter().map(|t| t["p95"].as_f64().unwrap_or(0.0)).collect();
    assert!(p95[0] <= p95[1]);
    let next = r.body["next_cursor"].as_str().unwrap();
    let r2 = app
        .get(&format!("/api/v1/orgs/acme/performance/transactions?{RANGE}&sort=-p95&limit=2&cursor={next}"), &viewer)
        .await;
    assert_ne!(r2.body["data"][0]["name"], r.body["data"][0]["name"]);
    let filtered = app.get(&format!("/api/v1/orgs/acme/performance/transactions?{RANGE}&query=python"), &viewer).await;
    assert!(filtered.body["data"].as_array().unwrap().iter().all(|t| t["name"].as_str().unwrap().contains("python")));
    assert_eq!(
        app.get(&format!("/api/v1/orgs/acme/performance/transactions?{RANGE}&sort=bogus"), &viewer).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    let s = app
        .get(
            &format!("/api/v1/orgs/acme/performance/transactions/summary?{RANGE}&name=smoke-node-transaction"),
            &viewer,
        )
        .await;
    assert_eq!(s.status, StatusCode::OK, "{}", s.body);
    assert_eq!(s.body["stats"]["count"], 1);
    assert_eq!(s.body["interval"], 3600);
    assert_eq!(s.body["series"].as_array().unwrap().len(), 24);
    assert_eq!(s.body["series"].as_array().unwrap().iter().map(|p| p["count"].as_i64().unwrap()).sum::<i64>(), 1);
    assert_eq!(s.body["histogram"].as_array().unwrap().len(), 20);
    let sample = &s.body["slowest"][0];
    let trace_id = sample["trace_id"].as_str().unwrap().to_string();
    assert!(sample["profiler_id"].is_string(), "Node transactions name their profiler session");
    // Browser web vitals.
    let name = pageload["name"].as_str().unwrap();
    let v = app
        .get(&format!("/api/v1/orgs/acme/performance/transactions/summary?{RANGE}&name={}", urlencode(name)), &viewer)
        .await;
    assert!(v.body["web_vitals"]["fcp"]["count"].as_i64().unwrap() >= 1, "{}", v.body["web_vitals"]);
    assert!(v.body["web_vitals"]["fcp"]["p75"].as_f64().unwrap() > 0.0);

    // The Node trace: one transaction, the span tree, its logs.
    let t = app.get(&format!("/api/v1/orgs/acme/traces/{trace_id}"), &viewer).await;
    assert_eq!(t.status, StatusCode::OK, "{}", t.body);
    assert_eq!(t.body["transactions"].as_array().unwrap().len(), 1);
    assert_eq!(t.body["transactions"][0]["project_slug"], "web");
    let roots = t.body["spans"].as_array().unwrap();
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0]["is_segment"], true);
    let children: Vec<&str> =
        roots[0]["children"].as_array().unwrap().iter().map(|c| c["description"].as_str().unwrap()).collect();
    assert_eq!(children, vec!["child", "compute checksum"]);
    assert_eq!(t.body["logs_count"], 3);
    assert!(t.body["duration_ms"].as_f64().unwrap() >= 250.0);
    // The browser trace holds the browser errors.
    let browser_trace: String = app
        .conn()
        .query_one("SELECT trace_id FROM events WHERE message IS NULL AND exception_value = 'smoke browser error'", &[])
        .unwrap()
        .get::<Option<String>>(0)
        .unwrap();
    let bt = app.get(&format!("/api/v1/orgs/acme/traces/{browser_trace}"), &viewer).await;
    assert_eq!(bt.status, StatusCode::OK, "{}", bt.body);
    assert!(
        bt.body["errors"].as_array().unwrap().iter().any(|e| e["title"] == "TypeError: smoke browser error"),
        "{}",
        bt.body["errors"]
    );
    assert_eq!(
        app.get(&format!("/api/v1/orgs/acme/traces/{}", "0".repeat(32)), &viewer).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(app.get("/api/v1/orgs/acme/traces/xyz", &viewer).await.status, StatusCode::NOT_FOUND);

    // Span explorer.
    let sp = app.get(&format!("/api/v1/orgs/acme/spans?{RANGE}&group_by=op&sort=count"), &viewer).await;
    assert_eq!(sp.status, StatusCode::OK, "{}", sp.body);
    let db = find(&sp.body["groups"], "group", "db.query");
    assert!(db["count"].as_i64().unwrap() >= 3);
    let rows = app.get(&format!("/api/v1/orgs/acme/spans?{RANGE}&group_by=none&query=op:function"), &viewer).await;
    assert!(rows.body["spans"].as_array().unwrap().iter().all(|s| s["op"] == "function"));
    assert!(!rows.body["spans"].as_array().unwrap().is_empty());

    // Other organizations see nothing.
    let other = app.org("other");
    let (_, outsider) = app.member(other, "out@example.com", Role::Owner);
    assert_eq!(app.get(&format!("/api/v1/orgs/acme/traces/{trace_id}"), &outsider).await.status, StatusCode::NOT_FOUND);
    assert_eq!(
        app.get(&format!("/api/v1/orgs/acme/performance/transactions?{RANGE}"), &outsider).await.status,
        StatusCode::NOT_FOUND
    );
}

fn urlencode(s: &str) -> String {
    form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

#[tokio::test]
async fn logs_histogram_and_live_tail() {
    let Setup { app, viewer, ingest } = setup().await;
    let r = app.get(&format!("/api/v1/orgs/acme/logs?{RANGE}&query=level:error"), &viewer).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let bodies: Vec<&str> = r.body["data"].as_array().unwrap().iter().map(|l| l["body"].as_str().unwrap()).collect();
    for b in ["smoke node log error", "smoke python log error", "smoke php log error"] {
        assert!(bodies.contains(&b), "{bodies:?}");
    }
    let node = find(&r.body["data"], "body", "smoke node log error");
    assert_eq!(node["attributes"]["password"], "[Filtered]");
    assert_eq!(node["sdk"], "sentry.javascript.node");
    let attr = app.get(&format!("/api/v1/orgs/acme/logs?{RANGE}&query=smoke.kind:node"), &viewer).await;
    assert_eq!(attr.body["data"].as_array().unwrap().len(), 1);
    assert_eq!(attr.body["data"][0]["attributes"]["order.id"], 42);
    let text = app.get(&format!("/api/v1/orgs/acme/logs?{RANGE}&query=low%20stock"), &viewer).await;
    assert_eq!(text.body["data"][0]["level"], "warn");
    let trace = node["trace_id"].as_str().unwrap();
    let by_trace = app.get(&format!("/api/v1/orgs/acme/logs?{RANGE}&query=trace:{trace}"), &viewer).await;
    assert_eq!(by_trace.body["data"].as_array().unwrap().len(), 3);
    // Paging newest first, no record twice.
    let mut seen = Vec::new();
    let mut cursor = String::new();
    loop {
        let p = app.get(&format!("/api/v1/orgs/acme/logs?{RANGE}&limit=3&cursor={cursor}"), &viewer).await;
        for l in p.body["data"].as_array().unwrap() {
            seen.push((l["timestamp"].as_str().unwrap().to_string(), l["id"].as_str().unwrap().to_string()));
        }
        match p.body["next_cursor"].as_str() {
            Some(c) => cursor = c.to_string(),
            None => break,
        }
    }
    let total = app.conn().query_one("SELECT count(*) FROM logs", &[]).unwrap().get::<i64>(0);
    assert_eq!(seen.len() as i64, total);
    assert!(seen.windows(2).all(|w| w[0] >= w[1]));
    assert_eq!(
        app.get(&format!("/api/v1/orgs/acme/logs?{RANGE}&query=bad%27key:1"), &viewer).await.status,
        StatusCode::OK
    );
    assert_eq!(
        app.get(&format!("/api/v1/orgs/acme/logs?{RANGE}&query=level:%3E3"), &viewer).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    let h = app.get(&format!("/api/v1/orgs/acme/logs/histogram?{RANGE}"), &viewer).await;
    assert_eq!(h.status, StatusCode::OK, "{}", h.body);
    assert_eq!(h.body["interval"], 3600);
    assert_eq!(h.body["buckets"].as_array().unwrap().len(), 24);
    let sum: i64 = h.body["totals"].as_object().unwrap().values().map(|v| v.as_i64().unwrap()).sum();
    assert_eq!(sum, total);
    assert!(h.body["totals"]["error"].as_i64().unwrap() >= 3);

    // Live tail: a `ready` event, then records received after the start.
    let req = Request::builder()
        .uri("/api/v1/orgs/acme/logs/stream?query=level:fatal")
        .header("cookie", format!("rupta_session={}", viewer.cookie.clone().unwrap()))
        .body(Body::empty())
        .unwrap();
    let res = app.router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["content-type"], "text/event-stream");
    let mut body = res.into_body();
    let mut text = String::new();
    let mut read_until = async |needle: &str, text: &mut String| {
        while !text.contains(needle) {
            let frame = tokio::time::timeout(Duration::from_secs(10), body.frame())
                .await
                .expect("event in time")
                .unwrap()
                .unwrap();
            if let Ok(d) = frame.into_data() {
                text.push_str(&String::from_utf8_lossy(&d));
            }
        }
    };
    read_until("event: ready", &mut text).await;
    let logs = json!({"items": [{"timestamp": 1_794_650_000.0, "level": "fatal", "body": "tail me"},
                                {"timestamp": 1_794_650_000.0, "level": "info", "body": "not me"}]});
    let env =
        rupta_ingest::envelope::serialize(&json!({}), &[(json!({"type": "log"}), serde_json::to_vec(&logs).unwrap())]);
    let mut req = Request::builder()
        .method("POST")
        .uri("/api/1/envelope/")
        .header("x-sentry-auth", format!("Sentry sentry_key={KEY}"))
        .body(Body::from(env))
        .unwrap();
    req.extensions_mut().insert(ConnectInfo("203.0.113.7:5555".parse::<SocketAddr>().unwrap()));
    assert_eq!(ingest.router().oneshot(req).await.unwrap().status(), StatusCode::OK);
    read_until("tail me", &mut text).await;
    assert!(text.contains("event: logs"));
    assert!(!text.contains("not me"));
}

#[tokio::test]
async fn metrics_and_dashboards() {
    let Setup { app, viewer, .. } = setup().await;
    let names = app.get(&format!("/api/v1/orgs/acme/metrics/names?{RANGE}"), &viewer).await;
    assert_eq!(names.status, StatusCode::OK, "{}", names.body);
    let lat = find(&names.body["data"], "name", "smoke.node.latency");
    assert_eq!(
        (lat["type"].as_str(), lat["unit"].as_str(), lat["count"].as_i64()),
        (Some("distribution"), Some("millisecond"), Some(2))
    );
    find(&names.body["data"], "name", "smoke.statsd.users");

    let q = app.get(&format!("/api/v1/orgs/acme/metrics/query?{RANGE}&name=smoke.node.requests"), &viewer).await;
    assert_eq!(q.status, StatusCode::OK, "{}", q.body);
    assert_eq!(q.body["op"], "sum");
    assert_eq!(q.body["type"], "counter");
    assert_eq!(q.body["series"][0]["total"], 2.0);
    assert_eq!(q.body["series"][0]["points"].as_array().unwrap().len(), 24);
    let g = app
        .get(&format!("/api/v1/orgs/acme/metrics/query?{RANGE}&name=smoke.node.latency&op=max&group_by=route"), &viewer)
        .await;
    assert_eq!(g.body["series"][0]["group"], json!({"route": "/checkout"}));
    assert_eq!(g.body["series"][0]["total"], 125.5);
    let u = app.get(&format!("/api/v1/orgs/acme/metrics/query?{RANGE}&name=smoke.statsd.users"), &viewer).await;
    assert_eq!((u.body["op"].as_str(), u.body["series"][0]["total"].as_f64()), (Some("count_unique"), Some(2.0)));
    let none = app.get(&format!("/api/v1/orgs/acme/metrics/query?{RANGE}&name=missing"), &viewer).await;
    assert_eq!(none.body["series"], json!([]));
    assert_eq!(
        app.get(&format!("/api/v1/orgs/acme/metrics/query?{RANGE}&name=x&op=median"), &viewer).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    // Dashboards: viewers read, members write.
    let (_, member) = app.member(1, "member@example.com", Role::Member);
    let widgets = json!([{"id": "w1", "title": "Requests", "type": "metric", "chart": "line",
        "query": {"name": "smoke.node.requests", "op": "sum"}, "layout": {"x": 0, "y": 0, "w": 6, "h": 4}}]);
    assert_eq!(
        app.post("/api/v1/orgs/acme/dashboards", &viewer, json!({"title": "Ops"})).await.status,
        StatusCode::FORBIDDEN
    );
    let d = app.post("/api/v1/orgs/acme/dashboards", &member, json!({"title": "Ops", "widgets": widgets})).await;
    assert_eq!(d.status, StatusCode::CREATED, "{}", d.body);
    let id = d.body["id"].as_str().unwrap().to_string();
    assert_eq!(d.body["widgets"][0]["query"]["name"], "smoke.node.requests");
    assert_eq!(app.get("/api/v1/orgs/acme/dashboards", &viewer).await.body["data"][0]["title"], "Ops");
    let p = app
        .patch(
            &format!("/api/v1/orgs/acme/dashboards/{id}"),
            &member,
            json!({"title": "Operations", "description": "main"}),
        )
        .await;
    assert_eq!((p.body["title"].as_str(), p.body["description"].as_str()), (Some("Operations"), Some("main")));
    assert_eq!(p.body["widgets"], d.body["widgets"], "widgets kept when not sent");
    let bad =
        app.patch(&format!("/api/v1/orgs/acme/dashboards/{id}"), &member, json!({"widgets": [{"type": "pie"}]})).await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(app.get(&format!("/api/v1/orgs/acme/dashboards/{id}"), &viewer).await.body["title"], "Operations");
    assert_eq!(app.delete(&format!("/api/v1/orgs/acme/dashboards/{id}"), &member).await.status, StatusCode::NO_CONTENT);
    assert_eq!(app.get(&format!("/api/v1/orgs/acme/dashboards/{id}"), &viewer).await.status, StatusCode::NOT_FOUND);
    let audit = app
        .conn()
        .query_one("SELECT count(*) FROM audit_log WHERE action LIKE 'dashboard.%'", &[])
        .unwrap()
        .get::<i64>(0);
    assert_eq!(audit, 3);
}

#[tokio::test]
async fn profiles_and_flamegraphs() {
    let Setup { app, viewer, .. } = setup().await;
    let list = app.get(&format!("/api/v1/orgs/acme/profiles?{RANGE}"), &viewer).await;
    assert_eq!(list.status, StatusCode::OK, "{}", list.body);
    assert!(list.body["data"].as_array().unwrap().len() >= 4);
    let tx = app.get(&format!("/api/v1/orgs/acme/profiles?{RANGE}&kind=transaction"), &viewer).await;
    let p = &tx.body["data"][0];
    assert_eq!(p["transaction_name"], "smoke-python-transaction");
    let id = p["profile_id"].as_str().unwrap();
    let page = app.get(&format!("/api/v1/orgs/acme/profiles?{RANGE}&limit=1"), &viewer).await;
    assert!(page.body["next_cursor"].is_string());

    let g = app.get(&format!("/api/v1/orgs/acme/profiles/web/{id}/flamegraph"), &viewer).await;
    assert_eq!(g.status, StatusCode::OK, "{}", g.body);
    assert_eq!(g.body["platform"], "python");
    assert!(g.body["root"]["frame_index"].is_null());
    assert_eq!(g.body["root"]["total_weight"], g.body["samples"]);
    let frames: Vec<&str> = g.body["frames"].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert!(frames.contains(&"compute_checksum") && frames.contains(&"fibonacci"), "{frames:?}");
    assert!(g.body["threads"].as_array().unwrap().iter().any(|t| t["id"] == g.body["thread_id"]));
    // Cached answer is the same.
    assert_eq!(app.get(&format!("/api/v1/orgs/acme/profiles/web/{id}/flamegraph"), &viewer).await.body, g.body);
    let raw = app.get(&format!("/api/v1/orgs/acme/profiles/1/{id}/raw"), &viewer).await;
    assert_eq!(raw.headers["content-type"], "application/json");
    assert_eq!(raw.body["platform"], "python");
    assert_eq!(
        app.get(&format!("/api/v1/orgs/acme/profiles/web/{}/flamegraph", "0".repeat(32)), &viewer).await.status,
        StatusCode::NOT_FOUND
    );

    // Aggregated: the Python transaction profile, and the Node transaction
    // via its continuous profiling chunks.
    let a = app
        .get(
            &format!("/api/v1/orgs/acme/profiles/aggregate-flamegraph?{RANGE}&transaction=smoke-python-transaction"),
            &viewer,
        )
        .await;
    assert_eq!(a.status, StatusCode::OK, "{}", a.body);
    assert_eq!(a.body["profile_ids"], json!([id]));
    let n = app
        .get(
            &format!("/api/v1/orgs/acme/profiles/aggregate-flamegraph?{RANGE}&transaction=smoke-node-transaction"),
            &viewer,
        )
        .await;
    assert!(!n.body["profile_ids"].as_array().unwrap().is_empty(), "{}", n.body);
    let names: Vec<&str> = n.body["frames"].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"computeChecksum"), "{names:?}");
    let app_frame = n.body["frames"].as_array().unwrap().iter().find(|f| f["name"] == "computeChecksum").unwrap();
    assert_eq!(app_frame["is_application"], true);
}

#[tokio::test]
async fn replays_and_their_issues() {
    let Setup { app, viewer, .. } = setup().await;
    let list = app.get(&format!("/api/v1/orgs/acme/replays?{RANGE}"), &viewer).await;
    assert_eq!(list.status, StatusCode::OK, "{}", list.body);
    let r = &list.body["data"][0];
    assert!(r["count_clicks"].as_i64().is_some_and(|n| n >= 2), "{}", list.body);
    assert!(r["count_errors"].as_i64().unwrap() >= 2);
    assert!(r["duration_ms"].as_i64().unwrap() >= 5000);
    assert_eq!(r["user"]["email"], "browser@example.com");
    let replay_id = r["replay_id"].as_str().unwrap().to_string();
    for q in ["count_errors:>0", "url:checkout", "user.email:browser@example.com", "browser:Chrome"] {
        let f = app.get(&format!("/api/v1/orgs/acme/replays?{RANGE}&query={}", urlencode(q)), &viewer).await;
        assert_eq!(f.body["data"].as_array().unwrap().len(), 1, "{q}: {}", f.body);
    }
    let f = app.get(&format!("/api/v1/orgs/acme/replays?{RANGE}&query=count_clicks:%3E100"), &viewer).await;
    assert_eq!(f.body["data"], json!([]));

    let d = app.get(&format!("/api/v1/orgs/acme/replays/web/{replay_id}"), &viewer).await;
    assert_eq!(d.status, StatusCode::OK, "{}", d.body);
    let titles: Vec<&str> = d.body["issues"].as_array().unwrap().iter().map(|i| i["title"].as_str().unwrap()).collect();
    assert!(titles.contains(&"TypeError: smoke browser error"), "{titles:?}");
    let seg = app.get(d.body["recording_url"].as_str().unwrap(), &viewer).await;
    assert_eq!(seg.status, StatusCode::OK);
    assert!(seg.body["next_cursor"].is_null());
    let events = seg.body["data"][0].as_array().unwrap();
    assert!(events.iter().any(|e| e["type"] == 2) && events.iter().any(|e| e["type"] == 4));

    // From the issue: its replays, and the event names the replay.
    let issue_id = d.body["issues"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["title"] == "TypeError: smoke browser error")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let ir = app.get(&format!("/api/v1/issues/{issue_id}/replays"), &viewer).await;
    assert_eq!(ir.status, StatusCode::OK, "{}", ir.body);
    assert_eq!(ir.body["data"][0]["replay_id"], replay_id.as_str());
    let ev = app.get(&format!("/api/v1/issues/{issue_id}/events/latest"), &viewer).await;
    assert_eq!(ev.body["replay_id"], replay_id.as_str());
    assert!(ev.body["trace_id"].is_string());
    assert_eq!(
        app.get(&format!("/api/v1/orgs/acme/replays/web/{}", "0".repeat(32)), &viewer).await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn telemetry_settings() {
    let app = TestApp::new();
    let org = app.org("acme");
    app.project(org, "web");
    let (_, admin) = app.member(org, "admin@example.com", Role::Admin);
    let r = app
        .patch(
            "/api/v1/projects/acme/web",
            &admin,
            json!({"settings": {"retention": {"logs": 14}, "sampling": {"transactions": 0.25}, "data_types": {"replays": false}}}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let s = &r.body["settings"];
    assert_eq!((s["retention"]["logs"].as_i64(), s["retention"]["spans"].as_i64()), (Some(14), Some(30)));
    assert_eq!(s["sampling"]["transactions"], 0.25);
    assert_eq!((s["data_types"]["replays"].as_bool(), s["data_types"]["logs"].as_bool()), (Some(false), Some(true)));
    let bad = app.patch("/api/v1/projects/acme/web", &admin, json!({"settings": {"sampling": {"logs": 2}}})).await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(bad.body["error"]["details"]["field"], "settings.sampling.logs");
}
