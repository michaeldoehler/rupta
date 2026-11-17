//! Operational endpoints: liveness, readiness, metrics, and the
//! `rupta healthcheck` command used by the container health check.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::get;
use serde_json::json;

use crate::App;
use crate::expo::Expo;

pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/healthz", get(|| async { (StatusCode::OK, "ok") }))
        .route("/readyz", get(readyz))
        .route("/metrics", get(metrics))
        .with_state(app)
}

/// Ready when the database answers and this writer has not been replaced
/// by another one in the object store.
async fn readyz(State(app): State<Arc<App>>) -> Response {
    let alive = app.store.run(|c| Ok(c.query_one("SELECT 1", &[])?.get::<i32>(0))).await;
    let remote = app.store.remote_status();
    let fenced = remote.as_ref().is_some_and(|r| r.fenced);
    let body = json!({
        "database": alive.is_ok(),
        "remote": remote.as_ref().map(|r| json!({
            "fenced": r.fenced,
            "local_lsn": r.local_lsn,
            "shipped_lsn": r.shipped_lsn,
        })),
    });
    let status = if alive.is_ok() && !fenced { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };
    (status, Json(body)).into_response()
}

/// Prometheus text format. Protected by `RUPTA_METRICS_TOKEN` if set.
async fn metrics(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if let Some(token) = &app.config.metrics_token {
        let given =
            headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "));
        let ok = given.is_some_and(|g| rupta_auth::crypto::ct_eq(g.as_bytes(), token.as_bytes()));
        if !ok {
            return StatusCode::UNAUTHORIZED.into_response();
        }
    }
    let out = render(&app).await;
    ([(header::CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")], out).into_response()
}

/// How long the database figures on `/metrics` are reused.
const DB_CACHE: Duration = Duration::from_secs(30);

/// Figures from the database, refreshed at most every [`DB_CACHE`].
#[derive(Clone, Default)]
pub struct DbFigures {
    /// Issues per status.
    pub issues: Vec<(String, i64)>,
    /// Project id -> (organization slug, project slug).
    pub projects: HashMap<i64, (String, String)>,
}

/// The cache of [`DbFigures`].
#[derive(Default)]
pub struct MetricsCache {
    figures: Mutex<Option<(Instant, DbFigures)>>,
}

async fn db_figures(app: &App, project_ids: Vec<i64>) -> DbFigures {
    let cached = app.metrics_cache.figures.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some((at, f)) = &cached
        && at.elapsed() < DB_CACHE
        && project_ids.iter().all(|id| f.projects.contains_key(id))
    {
        return f.clone();
    }
    let ids = project_ids.clone();
    let fresh = app
        .store
        .run(move |c| {
            let issues = c
                .query("SELECT status, count(*) FROM issues GROUP BY status ORDER BY status", &[])?
                .iter()
                .map(|r| (r.get::<String>(0), r.get::<i64>(1)))
                .collect();
            let mut projects = HashMap::new();
            if !ids.is_empty() {
                let list = ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
                for r in c.query(
                    &format!("SELECT p.id, o.slug, p.slug FROM projects p JOIN organizations o ON o.id = p.org_id WHERE p.id IN ({list})"),
                    &[],
                )? {
                    projects.insert(r.get::<i64>(0), (r.get::<String>(1), r.get::<String>(2)));
                }
            }
            Ok(DbFigures { issues, projects })
        })
        .await;
    match fresh {
        Ok(f) => {
            *app.metrics_cache.figures.lock().unwrap_or_else(|e| e.into_inner()) = Some((Instant::now(), f.clone()));
            f
        }
        Err(e) => {
            tracing::warn!(error = %e, "reading figures for /metrics failed");
            cached.map(|(_, f)| f).unwrap_or_default()
        }
    }
}

/// The whole metrics page.
pub async fn render(app: &App) -> String {
    let mut e = Expo::default();
    e.family("rupta_info", "gauge", "Build information (always 1).");
    e.sample("rupta_info", &[("version", env!("CARGO_PKG_VERSION"))], 1.0);
    e.family("rupta_build_info", "gauge", "Build information (always 1).");
    e.sample(
        "rupta_build_info",
        &[("version", env!("CARGO_PKG_VERSION")), ("os", std::env::consts::OS), ("arch", std::env::consts::ARCH)],
        1.0,
    );
    let uptime = app.started.elapsed().as_secs_f64();
    e.single("rupta_uptime_seconds", "gauge", "Seconds since the server started.", uptime);
    process_metrics(&mut e, app);

    // Storage and readiness (the same checks as /readyz).
    let remote = app.store.remote_status();
    let alive = app.store.run(|c| Ok(c.query_one("SELECT 1", &[])?.get::<i32>(0))).await.is_ok();
    let ready = alive && !remote.as_ref().is_some_and(|r| r.fenced);
    e.single(
        "rupta_ready",
        "gauge",
        "1 when the database answers and this instance is the writer (as /readyz).",
        ready as u8 as f64,
    );
    e.single(
        "rupta_storage_remote_configured",
        "gauge",
        "1 if the database is copied to an object store.",
        remote.is_some() as u8 as f64,
    );
    // Without an object store these read 0, so that rules and dashboards
    // always find them.
    let (fenced, local, shipped, wal) =
        remote.as_ref().map(|r| (r.fenced, r.local_lsn, r.shipped_lsn, r.wal_bytes)).unwrap_or_default();
    e.single("rupta_storage_fenced", "gauge", "1 if another writer took over the database.", fenced as u8 as f64);
    e.single(
        "rupta_storage_unshipped_lsn",
        "gauge",
        "Log records not yet in the object store (remote lag).",
        local.saturating_sub(shipped) as f64,
    );
    e.single("rupta_storage_local_lsn", "gauge", "Position of the newest local commit in the log.", local as f64);
    e.single("rupta_storage_shipped_lsn", "gauge", "Highest log position stored in the object store.", shipped as f64);
    e.single("rupta_storage_remote_wal_bytes", "gauge", "Bytes of log objects in the object store.", wal as f64);
    let stats = app.store.database().stats();
    e.single(
        "rupta_storage_database_bytes",
        "gauge",
        "Size of the database pages (8 KiB each).",
        stats.page_count as f64 * 8192.0,
    );
    e.single("rupta_storage_wal_bytes", "gauge", "Size of the local write-ahead log.", stats.wal_bytes as f64);
    let columnar = app.store.database().columnar_memory();
    e.family("rupta_storage_columnar_cache_bytes", "gauge", "Memory held by the caches of columnar tables.");
    for (cache, bytes) in
        [("chunks", columnar.chunk_bytes), ("segments", columnar.segment_bytes), ("blocks", columnar.block_bytes)]
    {
        e.sample("rupta_storage_columnar_cache_bytes", &[("cache", cache)], bytes as f64);
    }
    e.single(
        "rupta_storage_columnar_retired_segments",
        "gauge",
        "Replaced segment objects of columnar tables waiting to be deleted.",
        columnar.retired_segments as f64,
    );
    e.single("rupta_storage_cached_pages", "gauge", "Pages in the page cache.", stats.cached_pages as f64);
    e.single("rupta_storage_fsyncs_total", "counter", "fsync calls of the database.", stats.fsyncs as f64);
    e.single("rupta_storage_checkpoints_total", "counter", "Checkpoints of the database.", stats.checkpoints as f64);
    e.single("rupta_storage_cache_hits_total", "counter", "Page reads served from the cache.", stats.cache_hits as f64);
    e.single(
        "rupta_storage_cache_misses_total",
        "counter",
        "Page reads that missed the cache.",
        stats.cache_misses as f64,
    );
    let lookups = stats.cache_hits + stats.cache_misses;
    e.single(
        "rupta_storage_cache_hit_ratio",
        "gauge",
        "Share of page reads served from the cache since the start (1 before the first read).",
        if lookups == 0 { 1.0 } else { stats.cache_hits as f64 / lookups as f64 },
    );

    // Requests, API, ingest.
    app.http_metrics.render(&mut e);
    e.raw(&app.api.metrics.render());
    let mut ingest = String::new();
    app.ingest.render_metrics(&mut ingest);
    e.raw(&ingest);

    // Per project and per status, with names from the database.
    let counts = app.ingest.project_counts();
    let figures = db_figures(app, counts.iter().filter_map(|c| c.project_id).collect()).await;
    let label = |id: Option<i64>| -> (String, String) {
        match id {
            None => ("other".into(), "other".into()),
            Some(id) => figures.projects.get(&id).cloned().unwrap_or_else(|| ("unknown".into(), format!("id-{id}"))),
        }
    };
    e.family(
        "rupta_project_events_total",
        "counter",
        "Error events stored per project (the first RUPTA_METRICS_PROJECT_LABELS projects seen since the start; the rest as project=\"other\").",
    );
    for c in &counts {
        let (o, p) = label(c.project_id);
        e.sample("rupta_project_events_total", &[("org", &o), ("project", &p)], c.events as f64);
    }
    e.family(
        "rupta_project_new_issues_total",
        "counter",
        "New issues per project (same projects as rupta_project_events_total).",
    );
    for c in &counts {
        let (o, p) = label(c.project_id);
        e.sample("rupta_project_new_issues_total", &[("org", &o), ("project", &p)], c.new_issues as f64);
    }
    e.family("rupta_issues", "gauge", "Issues per status (refreshed every 30 seconds).");
    for status in ["unresolved", "resolved", "ignored"] {
        let n = figures.issues.iter().find(|(s, _)| s == status).map(|(_, n)| *n).unwrap_or(0);
        e.sample("rupta_issues", &[("status", status)], n as f64);
    }
    e.finish()
}

/// Reads a `/proc/self/status` field in kB, as bytes.
fn proc_status_bytes(status: &str, field: &str) -> Option<f64> {
    let line = status.lines().find(|l| l.starts_with(field))?;
    let kb: f64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb * 1024.0)
}

/// Process metrics with the names of the Prometheus client libraries.
/// Linux only (from `/proc/self`); elsewhere only the start time.
fn process_metrics(e: &mut Expo, app: &App) {
    e.single(
        "process_start_time_seconds",
        "gauge",
        "Start time of the process since the Unix epoch, in seconds.",
        app.started_unix,
    );
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        if let Some(v) = proc_status_bytes(&status, "VmRSS:") {
            e.single("process_resident_memory_bytes", "gauge", "Resident memory size in bytes.", v);
        }
        if let Some(v) = proc_status_bytes(&status, "VmSize:") {
            e.single("process_virtual_memory_bytes", "gauge", "Virtual memory size in bytes.", v);
        }
    }
    if let Ok(dir) = std::fs::read_dir("/proc/self/fd") {
        e.single("process_open_fds", "gauge", "Number of open file descriptors.", dir.count() as f64);
    }
    if let Ok(limits) = std::fs::read_to_string("/proc/self/limits")
        && let Some(line) = limits.lines().find(|l| l.starts_with("Max open files"))
        && let Some(soft) = line.split_whitespace().nth(3).and_then(|v| v.parse::<f64>().ok())
    {
        e.single("process_max_fds", "gauge", "Maximum number of open file descriptors.", soft);
    }
    if let Ok(stat) = std::fs::read_to_string("/proc/self/stat")
        && let Some(after) = stat.rsplit_once(')').map(|(_, a)| a)
    {
        // Fields after the command: state is field 3; utime and stime are 14 and 15.
        let f: Vec<&str> = after.split_whitespace().collect();
        if let (Some(u), Some(s)) =
            (f.get(11).and_then(|v| v.parse::<f64>().ok()), f.get(12).and_then(|v| v.parse::<f64>().ok()))
        {
            // Clock ticks are 100 per second on Linux.
            e.single(
                "process_cpu_seconds_total",
                "counter",
                "User and system CPU time spent in seconds.",
                (u + s) / 100.0,
            );
        }
    }
}

/// `rupta healthcheck`: exits 0 if `GET /readyz` on the configured port
/// answers 200. Uses a plain TCP request so the image needs no curl.
pub fn healthcheck_cli() -> anyhow::Result<()> {
    use std::io::{Read, Write};
    let listen = std::env::var("RUPTA_LISTEN").unwrap_or_else(|_| "0.0.0.0:8080".into());
    let port = listen.rsplit(':').next().unwrap_or("8080");
    let addr = format!("127.0.0.1:{port}");
    let mut s = std::net::TcpStream::connect_timeout(&addr.parse()?, std::time::Duration::from_secs(3))?;
    s.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
    s.write_all(b"GET /readyz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;
    let mut buf = String::new();
    s.read_to_string(&mut buf)?;
    if buf.starts_with("HTTP/1.1 200") {
        Ok(())
    } else {
        anyhow::bail!("not ready: {}", buf.lines().next().unwrap_or("no answer"))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    use super::*;
    use crate::config::Config;

    const KEY: &str = "0123456789abcdef0123456789abcdef";

    async fn app() -> Arc<App> {
        let env: HashMap<String, String> = [("RUPTA_SECRET_KEY".to_string(), "x".repeat(40))].into_iter().collect();
        let config = Config::from_env(&env).unwrap();
        let store = rupta_store::Store::open_in_memory().unwrap();
        let c = store.connect();
        let org = rupta_auth::users::create_org(&c, "acme", "Acme").unwrap();
        let project: i64 = c
            .query_one("INSERT INTO projects (org_id, slug, name) VALUES ($1, 'web', 'Web') RETURNING id", &[&org])
            .unwrap()
            .get(0);
        c.execute(
            "INSERT INTO project_keys (project_id, public_key, secret_key) VALUES ($1, $2, 's')",
            &[&project, &KEY],
        )
        .unwrap();
        store.bump_config();
        let ingest = rupta_ingest::Ingest::start(
            store.clone(),
            rupta_ingest::IngestConfig { workers: 1, batch_interval: Duration::from_millis(5), ..Default::default() },
        );
        let api = rupta_api::ApiState::new(store.clone(), rupta_api::ApiConfig::new("http://rupta.test", &[7; 32]));
        Arc::new(App::new(config, store, ingest, api))
    }

    #[tokio::test]
    async fn the_metrics_page_is_valid_exposition_format() {
        let app = app().await;
        let req = Request::post("/api/1/store/")
            .header("x-sentry-auth", format!("Sentry sentry_version=7, sentry_key={KEY}"))
            .header("content-type", "application/json")
            .body(Body::from(r#"{"message": "hello", "level": "error"}"#))
            .unwrap();
        let res = app.ingest.router().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert!(app.ingest.flush(Duration::from_secs(10)).await);
        app.http_metrics.observe(&axum::http::Method::GET, "/api/v1/issues/{id}", 200, 0.02);
        app.api.metrics.count("rupta_alert_notifications_total", &[("channel", "slack"), ("result", "success")]);
        app.api.metrics.count(
            "rupta_integration_requests_total",
            &[("kind", "github"), ("operation", "create"), ("result", "success")],
        );

        let text = render(&app).await;
        let types = crate::expo::check(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
        for (name, kind) in [
            ("rupta_http_requests_total", "counter"),
            ("rupta_http_request_duration_seconds", "histogram"),
            ("rupta_ingest_events_total", "counter"),
            ("rupta_ingest_queue_depth", "gauge"),
            ("rupta_project_events_total", "counter"),
            ("rupta_project_new_issues_total", "counter"),
            ("rupta_issues", "gauge"),
            ("rupta_alert_notifications_total", "counter"),
            ("rupta_notifications_sent_total", "counter"),
            ("rupta_integration_requests_total", "counter"),
            ("rupta_integration_sync_total", "counter"),
            ("rupta_integration_webhooks_total", "counter"),
            ("rupta_storage_database_bytes", "gauge"),
            ("rupta_storage_columnar_cache_bytes", "gauge"),
            ("rupta_storage_columnar_retired_segments", "gauge"),
            ("rupta_storage_cache_hit_ratio", "gauge"),
            ("rupta_storage_remote_configured", "gauge"),
            ("rupta_build_info", "gauge"),
            ("rupta_info", "gauge"),
            ("rupta_uptime_seconds", "gauge"),
            ("rupta_ready", "gauge"),
            ("process_start_time_seconds", "gauge"),
        ] {
            assert_eq!(types.get(name).map(String::as_str), Some(kind), "{name}\n{text}");
        }
        if cfg!(target_os = "linux") {
            assert!(types.contains_key("process_resident_memory_bytes") && types.contains_key("process_open_fds"));
        }
        assert!(text.contains("rupta_project_events_total{org=\"acme\",project=\"web\"} 1\n"), "{text}");
        assert!(text.contains("rupta_project_new_issues_total{org=\"acme\",project=\"web\"} 1\n"), "{text}");
        assert!(text.contains("rupta_issues{status=\"unresolved\"} 1\n"), "{text}");
        assert!(text.contains("rupta_ingest_events_total{category=\"error\",outcome=\"accepted\"} 1\n"), "{text}");
        assert!(text.contains("rupta_alert_notifications_total{channel=\"slack\",result=\"success\"} 1\n"), "{text}");
        // Units: durations in seconds, sizes in bytes.
        for name in types.keys() {
            assert!(!name.contains("millis") && !name.contains("_ms") && !name.ends_with("_kb"), "{name}");
        }

        // The shipped alert rules and dashboard use only metrics that exist.
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../deploy");
        let rules = std::fs::read_to_string(format!("{root}/prometheus/rupta-alerts.yml")).unwrap();
        let dashboard = std::fs::read_to_string(format!("{root}/grafana/rupta-overview.json")).unwrap();
        for text in [&rules, &dashboard] {
            for name in metric_names(text) {
                let family = ["_bucket", "_sum", "_count"]
                    .iter()
                    .find_map(|s| name.strip_suffix(s).filter(|f| types.get(*f).is_some_and(|t| t == "histogram")))
                    .unwrap_or(&name);
                assert!(types.contains_key(family), "{name} is used in deploy/ but not exported");
            }
        }
    }

    /// `rupta_*` and `process_*` names in a text.
    fn metric_names(text: &str) -> Vec<String> {
        let mut out = Vec::new();
        let b = text.as_bytes();
        let mut i = 0;
        while i < b.len() {
            let rest = &text[i..];
            let boundary = i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_');
            if boundary && (rest.starts_with("rupta_") || rest.starts_with("process_")) {
                let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(rest.len());
                out.push(rest[..end].to_string());
                i += end;
            } else {
                i += 1;
            }
        }
        out.sort();
        out.dedup();
        out
    }

    #[test]
    fn the_dashboard_is_importable() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../deploy/grafana/rupta-overview.json");
        let d: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(d["uid"], "rupta-overview");
        let vars = d["templating"]["list"].as_array().unwrap();
        assert!(
            vars.iter()
                .any(|v| v["name"] == "DS_PROMETHEUS" && v["type"] == "datasource" && v["query"] == "prometheus")
        );
        let mut ids = std::collections::HashSet::new();
        for p in d["panels"].as_array().unwrap() {
            assert!(ids.insert(p["id"].as_i64().unwrap()), "panel ids are unique");
            let g = &p["gridPos"];
            assert!(g["x"].as_i64().unwrap() + g["w"].as_i64().unwrap() <= 24);
            if p["type"] == "row" {
                continue;
            }
            assert_eq!(p["datasource"]["uid"], "${DS_PROMETHEUS}", "{}", p["title"]);
            for t in p["targets"].as_array().unwrap() {
                assert_eq!(t["datasource"]["uid"], "${DS_PROMETHEUS}");
                assert!(!t["expr"].as_str().unwrap().is_empty());
            }
        }
    }

    #[tokio::test]
    async fn the_token_protects_the_page() {
        let mut app = Arc::try_unwrap(app().await).ok().unwrap();
        app.config.metrics_token = Some("scrape-me".into());
        let router = router(Arc::new(app));
        let get = |auth: Option<&str>| {
            let mut b = Request::get("/metrics");
            if let Some(a) = auth {
                b = b.header("authorization", a);
            }
            b.body(Body::empty()).unwrap()
        };
        assert_eq!(router.clone().oneshot(get(None)).await.unwrap().status(), StatusCode::UNAUTHORIZED);
        assert_eq!(router.clone().oneshot(get(Some("Bearer nope"))).await.unwrap().status(), StatusCode::UNAUTHORIZED);
        let ok = router.oneshot(get(Some("Bearer scrape-me"))).await.unwrap();
        assert_eq!(ok.status(), StatusCode::OK);
        assert!(ok.headers()["content-type"].to_str().unwrap().starts_with("text/plain; version=0.0.4"));
    }
}
