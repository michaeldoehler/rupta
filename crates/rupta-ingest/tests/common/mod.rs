//! Helpers for the ingest integration tests: an in-memory store with a
//! project and key, the router, and request builders.
#![allow(dead_code)]

use std::net::SocketAddr;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use http::{Request, Response};
use rupta_ingest::{Ingest, IngestConfig};
use rupta_store::Store;
use serde_json::Value;
use tower::ServiceExt;

pub const KEY: &str = "0123456789abcdef0123456789abcdef";

pub struct Harness {
    pub store: Store,
    pub ingest: Ingest,
    pub router: Router,
    pub project_id: i64,
}

pub fn config() -> IngestConfig {
    IngestConfig { workers: 2, batch_interval: Duration::from_millis(5), ..IngestConfig::default() }
}

/// Creates an organization, a project with these settings and an active key.
pub fn seed(store: &Store, settings: &Value) -> i64 {
    let conn = store.connect();
    conn.execute("INSERT INTO organizations (slug, name) VALUES ('acme', 'Acme') ON CONFLICT DO NOTHING", &[]).unwrap();
    let id: i64 = conn
        .query_one(
            "INSERT INTO projects (org_id, slug, name, platform, settings) VALUES (1, 'web' || (SELECT count(*) FROM projects), 'Web', 'javascript', $1) RETURNING id",
            &[settings],
        )
        .unwrap()
        .get(0);
    if id == 1 {
        conn.execute(
            "INSERT INTO project_keys (project_id, public_key, secret_key) VALUES ($1, $2, 'secret')",
            &[&id, &KEY],
        )
        .unwrap();
    }
    store.bump_config();
    id
}

pub fn harness_with(settings: Value, config: IngestConfig) -> Harness {
    let store = Store::open_in_memory().unwrap();
    let project_id = seed(&store, &settings);
    let ingest = Ingest::start(store.clone(), config);
    let router = ingest.router();
    Harness { store, ingest, router, project_id }
}

pub fn harness() -> Harness {
    harness_with(serde_json::json!({}), config())
}

pub fn auth() -> String {
    format!("Sentry sentry_version=7, sentry_client=test/1.0, sentry_key={KEY}")
}

pub fn request(path: &str, headers: &[(&str, &str)], body: Vec<u8>) -> Request<Body> {
    let mut b = Request::builder().method("POST").uri(path);
    for (k, v) in headers {
        b = b.header(*k, *v);
    }
    let mut req = b.body(Body::from(body)).unwrap();
    req.extensions_mut().insert(ConnectInfo("203.0.113.7:5555".parse::<SocketAddr>().unwrap()));
    req
}

impl Harness {
    pub async fn send(&self, req: Request<Body>) -> (u16, http::HeaderMap, Value) {
        let res: Response<Body> = self.router.clone().oneshot(req).await.unwrap();
        let status = res.status().as_u16();
        let headers = res.headers().clone();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let v = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, headers, v)
    }

    pub async fn envelope(&self, body: Vec<u8>) -> (u16, http::HeaderMap, Value) {
        let path = format!("/api/{}/envelope/", self.project_id);
        self.send(request(
            &path,
            &[("x-sentry-auth", &auth()), ("content-type", "application/x-sentry-envelope")],
            body,
        ))
        .await
    }

    pub async fn flush(&self) {
        assert!(self.ingest.flush(Duration::from_secs(20)).await, "ingest did not drain");
    }

    pub fn count(&self, sql: &str) -> i64 {
        self.store.connect().query_one(sql, &[]).unwrap().get(0)
    }

    /// The stored normalized event.
    pub fn payload(&self, event_id: &str) -> Value {
        let id = *uuid::Uuid::parse_str(event_id).unwrap().as_bytes();
        let row =
            self.store.connect().query_one("SELECT data FROM event_payloads WHERE event_id = $1", &[&id]).unwrap();
        let data: Vec<u8> = row.get(0);
        serde_json::from_slice(&zstd::decode_all(&data[..]).unwrap()).unwrap()
    }
}

/// An envelope from a header and `(item header, payload)` pairs.
pub fn envelope(headers: Value, items: &[(Value, Vec<u8>)]) -> Vec<u8> {
    rupta_ingest::envelope::serialize(&headers, items)
}

pub fn event_envelope(event: &Value) -> Vec<u8> {
    let id = event["event_id"].clone();
    envelope(
        serde_json::json!({"event_id": id}),
        &[(serde_json::json!({"type": "event"}), serde_json::to_vec(event).unwrap())],
    )
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}
