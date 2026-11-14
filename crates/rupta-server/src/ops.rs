//! Operational endpoints: liveness, readiness, metrics, and the
//! `rupta healthcheck` command used by the container health check.

use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::get;
use serde_json::json;

use crate::App;

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
        if given != Some(token.as_str()) {
            return StatusCode::UNAUTHORIZED.into_response();
        }
    }
    let mut out = String::new();
    out.push_str("# HELP rupta_info Build information.\n# TYPE rupta_info gauge\n");
    out.push_str(&format!("rupta_info{{version=\"{}\"}} 1\n", env!("CARGO_PKG_VERSION")));
    if let Some(r) = app.store.remote_status() {
        out.push_str("# HELP rupta_storage_fenced 1 if another writer took over the database.\n# TYPE rupta_storage_fenced gauge\n");
        out.push_str(&format!("rupta_storage_fenced {}\n", r.fenced as u8));
        out.push_str("# HELP rupta_storage_unshipped_lsn Log records not yet in the object store.\n# TYPE rupta_storage_unshipped_lsn gauge\n");
        out.push_str(&format!("rupta_storage_unshipped_lsn {}\n", r.local_lsn.saturating_sub(r.shipped_lsn)));
    }
    let stats = app.store.database().stats();
    out.push_str("# TYPE rupta_storage_fsyncs_total counter\n");
    out.push_str(&format!("rupta_storage_fsyncs_total {}\n", stats.fsyncs));
    out.push_str("# TYPE rupta_storage_cache_hits_total counter\n");
    out.push_str(&format!("rupta_storage_cache_hits_total {}\n", stats.cache_hits));
    out.push_str("# TYPE rupta_storage_cache_misses_total counter\n");
    out.push_str(&format!("rupta_storage_cache_misses_total {}\n", stats.cache_misses));
    ([(header::CONTENT_TYPE, "text/plain; version=0.0.4")], out).into_response()
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
