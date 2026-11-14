//! Ingest benchmark: serves the ingest router over HTTP on a local port
//! with an on-disk database and posts envelopes with realistic ~5 KB
//! JavaScript error events from concurrent clients. Reports acknowledged
//! events per second, stored events per second (until everything is
//! committed) and acknowledgement latency percentiles, for durability
//! `fast` and `safe`.
//!
//! ```sh
//! cargo run --release -p rupta-ingest --example ingest_bench -- [events] [concurrency]
//! ```

use std::sync::Arc;
use std::time::{Duration, Instant};

use rupta_ingest::{Ingest, IngestConfig};
use rupta_store::{Store, StoreOptions};
use serde_json::{Value, json};

#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

const KEY: &str = "0123456789abcdef0123456789abcdef";

fn event(i: usize) -> Value {
    let kind = i % 50;
    let frames: Vec<Value> = (0..12)
        .map(|f| {
            json!({
                "filename": format!("https://app.example.com/static/js/chunk-{}.js", (kind + f) % 7),
                "abs_path": format!("https://app.example.com/static/js/chunk-{}.js", (kind + f) % 7),
                "function": format!("module{}.handler{}", f % 5, kind),
                "lineno": 1 + (f * 37) % 400,
                "colno": 10 + (f * 13) % 120,
                "in_app": f > 7
            })
        })
        .collect();
    let crumbs: Vec<Value> = (0..10)
        .map(|b| {
            json!({"timestamp": 1.7e9 + b as f64, "category": if b % 3 == 0 { "fetch" } else { "ui.click" },
            "message": format!("button#action-{b} clicked on the checkout page"), "level": "info",
            "data": {"url": format!("/api/v1/items/{b}"), "status_code": 200}})
        })
        .collect();
    json!({
        "event_id": uuid::Uuid::new_v4().simple().to_string(),
        "timestamp": chrono::Utc::now().timestamp_millis() as f64 / 1000.0,
        "platform": "javascript",
        "level": "error",
        "release": format!("web@1.{}.0", i % 3),
        "environment": "production",
        "transaction": format!("/checkout/step-{}", kind % 4),
        "user": {"id": format!("user-{}", i % 1000), "email": format!("user{}@example.com", i % 1000), "ip_address": "{{auto}}"},
        "tags": {"feature": "checkout", "plan": if i.is_multiple_of(2) { "pro" } else { "free" }},
        "contexts": {
            "browser": {"name": "Chrome", "version": "130.0.0"},
            "os": {"name": "Windows", "version": "10"},
            "trace": {"trace_id": "4c79f60c11214eb38604f4ae0781bfb2", "span_id": "fa90fdead5f74052"}
        },
        "request": {"url": "https://app.example.com/checkout", "headers": {
            "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36",
            "Referer": "https://app.example.com/cart"}},
        "sdk": {"name": "sentry.javascript.browser", "version": "11.12.0", "integrations": ["Breadcrumbs", "GlobalHandlers", "LinkedErrors"]},
        "exception": {"values": [{"type": format!("TypeError{kind}"), "value": format!("Cannot read properties of undefined (reading 'item{i}')"),
            "mechanism": {"type": "onerror", "handled": false}, "stacktrace": {"frames": frames}}]},
        "breadcrumbs": {"values": crumbs},
        "extra": {"cart_items": i % 17, "session_length": 1234}
    })
}

fn envelope(i: usize) -> Vec<u8> {
    let ev = event(i);
    rupta_ingest::envelope::serialize(
        &json!({"event_id": ev["event_id"], "sent_at": "2026-11-14T12:00:00Z"}),
        &[(json!({"type": "event"}), serde_json::to_vec(&ev).unwrap())],
    )
}

fn percentile(sorted: &[Duration], p: f64) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

async fn run(safe: bool, events: usize, concurrency: usize) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&StoreOptions::local(dir.path())).unwrap();
    {
        let c = store.connect();
        c.batch_execute("INSERT INTO organizations (slug, name) VALUES ('bench', 'Bench'); INSERT INTO projects (org_id, slug, name) VALUES (1, 'web', 'Web');").unwrap();
        c.execute("INSERT INTO project_keys (project_id, public_key, secret_key) VALUES (1, $1, 's')", &[&KEY])
            .unwrap();
    }
    store.bump_config();
    let ingest = Ingest::start(store.clone(), IngestConfig { safe, ..IngestConfig::default() });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = ingest.router();
    let server = tokio::spawn(async move {
        axum::serve(listener, router.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap();
    });

    let bodies: Arc<Vec<Vec<u8>>> = Arc::new((0..events).map(envelope).collect());
    let avg = bodies.iter().map(Vec::len).sum::<usize>() / events.max(1);
    let client = reqwest::Client::builder().pool_max_idle_per_host(concurrency).build().unwrap();
    let url = format!("http://{addr}/api/1/envelope/");
    // Warm up connections and caches.
    for i in 0..concurrency.min(events) {
        let _ = client
            .post(&url)
            .header("x-sentry-auth", format!("Sentry sentry_key={KEY}"))
            .body(bodies[i].clone())
            .send()
            .await;
    }
    ingest.flush(Duration::from_secs(60)).await;

    let next = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let start = Instant::now();
    let mut tasks = Vec::new();
    for _ in 0..concurrency {
        let (client, url, bodies, next) = (client.clone(), url.clone(), bodies.clone(), next.clone());
        tasks.push(tokio::spawn(async move {
            let mut latencies = Vec::new();
            let mut rejected = 0usize;
            loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if i >= bodies.len() {
                    break;
                }
                let t = Instant::now();
                let res = client
                    .post(&url)
                    .header("x-sentry-auth", format!("Sentry sentry_key={KEY}"))
                    .header("content-type", "application/x-sentry-envelope")
                    .body(bodies[i].clone())
                    .send()
                    .await
                    .unwrap();
                let ok = res.status().is_success();
                let _ = res.bytes().await;
                latencies.push(t.elapsed());
                if !ok {
                    rejected += 1;
                }
            }
            (latencies, rejected)
        }));
    }
    let mut latencies = Vec::new();
    let mut rejected = 0;
    for t in tasks {
        let (l, r) = t.await.unwrap();
        latencies.extend(l);
        rejected += r;
    }
    let acked = start.elapsed();
    ingest.flush(Duration::from_secs(600)).await;
    let stored_time = start.elapsed();
    latencies.sort();
    let stored: i64 = store.connect().query_one("SELECT count(*) FROM events", &[]).unwrap().get(0);
    let issues: i64 = store.connect().query_one("SELECT count(*) FROM issues", &[]).unwrap().get(0);
    println!(
        "durability={:<4} events={events} concurrency={concurrency} avg_envelope_bytes={avg} rejected={rejected} stored={stored} issues={issues}",
        if safe { "safe" } else { "fast" }
    );
    println!(
        "  acked:  {:.0} events/s ({:.2} s)   stored: {:.0} events/s ({:.2} s)   batches={}",
        events as f64 / acked.as_secs_f64(),
        acked.as_secs_f64(),
        stored as f64 / stored_time.as_secs_f64(),
        stored_time.as_secs_f64(),
        ingest.batches()
    );
    println!(
        "  ack latency: p50={:.2} ms p90={:.2} ms p99={:.2} ms max={:.2} ms",
        percentile(&latencies, 0.5).as_secs_f64() * 1e3,
        percentile(&latencies, 0.9).as_secs_f64() * 1e3,
        percentile(&latencies, 0.99).as_secs_f64() * 1e3,
        latencies.last().copied().unwrap_or_default().as_secs_f64() * 1e3,
    );
    let mut metrics = String::new();
    ingest.render_metrics(&mut metrics);
    let batch_secs: f64 = metrics
        .lines()
        .find_map(|l| l.strip_prefix("rupta_ingest_batch_seconds_sum "))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    println!("  writer busy (batch transactions): {batch_secs:.2} s");
    ingest.shutdown().await;
    server.abort();
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--stages") {
        stages();
        return;
    }
    if args.first().map(String::as_str) == Some("--writer") {
        writer_stages();
        return;
    }
    let events: usize = args.first().and_then(|a| a.parse().ok()).unwrap_or(100_000);
    let concurrency: usize = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(64);
    println!(
        "rupta ingest benchmark: {} cores, {} ingest workers",
        std::thread::available_parallelism().map_or(0, |n| n.get()),
        IngestConfig::default().workers
    );
    run(false, events, concurrency).await;
    run(true, events, concurrency).await;
}

/// Per-stage cost of processing one event on one core (`--stages`).
fn stages() {
    use rupta_core::normalize::{Context, normalize_owned};
    use rupta_core::scrub::Scrubber;
    use rupta_core::settings::ProjectSettings;
    let settings = ProjectSettings::default();
    let raws: Vec<Vec<u8>> = (0..5000).map(|i| serde_json::to_vec(&event(i)).unwrap()).collect();
    let n = raws.len() as f64;
    let t = Instant::now();
    let values: Vec<Value> = raws.iter().map(|r| serde_json::from_slice(r).unwrap()).collect();
    println!("parse JSON:  {:.1} us/event", t.elapsed().as_secs_f64() * 1e6 / n);
    let t = Instant::now();
    let mut events: Vec<_> = values
        .into_iter()
        .map(|v| {
            normalize_owned(
                v,
                &Context {
                    project_id: 1,
                    event_id: "0".repeat(32),
                    received: chrono::Utc::now(),
                    client_ip: None,
                    user_agent: None,
                    settings: &settings,
                },
            )
        })
        .collect();
    println!("normalize:   {:.1} us/event", t.elapsed().as_secs_f64() * 1e6 / n);
    let t = Instant::now();
    for e in &mut events {
        Scrubber::new(&settings.scrubbing).scrub(e);
    }
    println!("scrub:       {:.1} us/event", t.elapsed().as_secs_f64() * 1e6 / n);
    let t = Instant::now();
    for e in &events {
        let _ = rupta_core::grouping::group(e);
        let _ = rupta_core::grouping::issue_info(e);
    }
    println!("group:       {:.1} us/event", t.elapsed().as_secs_f64() * 1e6 / n);
    let t = Instant::now();
    let jsons: Vec<Vec<u8>> = events.iter().map(|e| serde_json::to_vec(e).unwrap()).collect();
    println!("serialize:   {:.1} us/event", t.elapsed().as_secs_f64() * 1e6 / n);
    let t = Instant::now();
    let mut bytes = 0;
    let mut z = zstd::bulk::Compressor::new(1).unwrap();
    for j in &jsons {
        bytes += z.compress(j).unwrap().len();
    }
    println!(
        "zstd:        {:.1} us/event ({} -> {} bytes avg)",
        t.elapsed().as_secs_f64() * 1e6 / n,
        jsons.iter().map(Vec::len).sum::<usize>() / jsons.len(),
        bytes / jsons.len()
    );
}

/// Cost of the writer's statements for batches of 1,000 events (`--writer`).
fn writer_stages() {
    use rupta_store::ingest::*;
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&StoreOptions::local(dir.path())).unwrap();
    let c = store.connect();
    c.batch_execute("INSERT INTO organizations (slug, name) VALUES ('b', 'B'); INSERT INTO projects (org_id, slug, name) VALUES (1, 'w', 'W');").unwrap();
    let now = chrono::Utc::now();
    let mut ids = Vec::new();
    for i in 0..50 {
        let (id, _) = create_issue(
            &c,
            &NewIssue {
                project_id: 1,
                fingerprint_hash: format!("h{i}"),
                grouping_version: "v1".into(),
                title: "t".into(),
                culprit: None,
                ty: "error".into(),
                level: "error".into(),
                platform: None,
                first_seen: now,
                last_seen: now,
                release: None,
                metadata: json!({}),
            },
        )
        .unwrap();
        ids.push(id);
    }
    let (mut tp, mut te, mut ti, mut tc) = (Duration::ZERO, Duration::ZERO, Duration::ZERO, Duration::ZERO);
    let rounds = 10;
    for r in 0..rounds {
        let payloads: Vec<PayloadRow> = (0..1000)
            .map(|_| PayloadRow {
                project_id: 1,
                event_id: *uuid::Uuid::new_v4().as_bytes(),
                issue_id: Some(ids[r % 50]),
                day: now.date_naive(),
                data: vec![7u8; 1342],
            })
            .collect();
        let rows: Vec<EventRow> = payloads
            .iter()
            .map(|p| EventRow {
                project_id: 1,
                issue_id: ids[r % 50],
                event_id: p.event_id,
                timestamp: now,
                received: now,
                level: Some("error".into()),
                platform: Some("javascript".into()),
                environment: Some("production".into()),
                release: Some("web@1.0.0".into()),
                message: None,
                exception_type: Some("TypeError".into()),
                exception_value: Some("Cannot read properties of undefined".into()),
                user_id: Some("user-1".into()),
                user_hash: Some(1),
                browser: Some("Chrome 130".into()),
                os: Some("Windows 10".into()),
                sdk: Some("sentry.javascript.browser".into()),
                url: Some("https://app.example.com/checkout".into()),
                tags: json!({"level": "error", "browser": "Chrome 130", "os": "Windows 10", "release": "web@1.0.0"}),
                contexts: json!({"browser": {"name": "Chrome", "version": "130"}, "os": {"name": "Windows"}}),
                ..Default::default()
            })
            .collect();
        let tx = c.transaction().unwrap();
        let t = Instant::now();
        append_payloads(&tx, &payloads).unwrap();
        tp += t.elapsed();
        let t = Instant::now();
        append_events(&tx, &rows).unwrap();
        te += t.elapsed();
        let t = Instant::now();
        for id in &ids {
            add_issue_counts(
                &tx,
                &IssueCounts {
                    id: *id,
                    count: 20,
                    users_added: 1,
                    first_seen: Some(now),
                    last_seen: Some(now),
                    last_release: Some("web@1.0.0".into()),
                },
            )
            .unwrap();
        }
        let _ = issue_states(&tx, &ids).unwrap();
        ti += t.elapsed();
        let t = Instant::now();
        tx.commit().unwrap();
        tc += t.elapsed();
    }
    let per = |d: Duration| d.as_secs_f64() * 1e3 / rounds as f64;
    println!(
        "per batch of 1000: payloads {:.1} ms, events {:.1} ms, 50 issue updates {:.1} ms, commit {:.1} ms",
        per(tp),
        per(te),
        per(ti),
        per(tc)
    );
}
