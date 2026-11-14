//! Test harness: an in-memory database, the API router, and helpers to
//! make requests as users with sessions or tokens.

#![allow(dead_code)]

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use rupta_api::{ApiConfig, ApiState};
use rupta_auth::{password, session, users};
use rupta_core::role::Role;
use rupta_store::Store;
use serde_json::{Value, json};
use tower::ServiceExt;

pub const SECRET: &[u8] = b"test secret key that is long enough!!";

pub struct TestApp {
    pub state: ApiState,
    pub router: Router,
}

/// Credentials of a request.
#[derive(Clone, Debug, Default)]
pub struct As {
    pub cookie: Option<String>,
    pub csrf: Option<String>,
    pub token: Option<String>,
}

impl As {
    pub fn anonymous() -> As {
        As::default()
    }
    pub fn token(t: &str) -> As {
        As { token: Some(t.to_string()), ..As::default() }
    }
    pub fn without_csrf(&self) -> As {
        As { csrf: None, ..self.clone() }
    }
}

pub struct Resp {
    pub status: StatusCode,
    pub headers: axum::http::HeaderMap,
    pub body: Value,
    pub bytes: Vec<u8>,
}

impl Resp {
    pub fn code(&self) -> &str {
        self.body["error"]["code"].as_str().unwrap_or("")
    }
}

impl TestApp {
    pub fn new() -> TestApp {
        TestApp::with_config(|_| {})
    }

    pub fn with_config(f: impl FnOnce(&mut ApiConfig)) -> TestApp {
        let _ = tracing_subscriber::fmt().with_env_filter("rupta=debug,warn").with_test_writer().try_init();
        let mut config = ApiConfig::new("http://rupta.test", SECRET);
        f(&mut config);
        let state = ApiState::new(Store::open_in_memory().unwrap(), config);
        let router = rupta_api::router(state.clone());
        TestApp { state, router }
    }

    pub fn conn(&self) -> rupta_store::squad::Connection {
        self.state.store.connect()
    }

    pub async fn request(&self, method: Method, path: &str, who: &As, body: Option<Value>) -> Resp {
        let mut b = Request::builder().method(method).uri(path);
        if let Some(c) = &who.cookie {
            b = b.header(header::COOKIE, format!("{}={c}", session::SESSION_COOKIE));
        }
        if let Some(c) = &who.csrf {
            b = b.header("x-csrf-token", c);
        }
        if let Some(t) = &who.token {
            b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        let req = match body {
            Some(v) => b.header(header::CONTENT_TYPE, "application/json").body(Body::from(v.to_string())).unwrap(),
            None => b.body(Body::empty()).unwrap(),
        };
        self.send(req).await
    }

    pub async fn send(&self, req: Request<Body>) -> Resp {
        let resp = self.router.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let headers = resp.headers().clone();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes().to_vec();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        Resp { status, headers, body, bytes }
    }

    pub async fn get(&self, path: &str, who: &As) -> Resp {
        self.request(Method::GET, path, who, None).await
    }
    pub async fn post(&self, path: &str, who: &As, body: Value) -> Resp {
        self.request(Method::POST, path, who, Some(body)).await
    }
    pub async fn put(&self, path: &str, who: &As, body: Value) -> Resp {
        self.request(Method::PUT, path, who, Some(body)).await
    }
    pub async fn patch(&self, path: &str, who: &As, body: Value) -> Resp {
        self.request(Method::PATCH, path, who, Some(body)).await
    }
    pub async fn delete(&self, path: &str, who: &As) -> Resp {
        self.request(Method::DELETE, path, who, None).await
    }

    /// A user with a session (password hashing skipped: no password).
    pub fn user(&self, email: &str) -> (i64, As) {
        let conn = self.conn();
        let uid = match users::find_by_email(&conn, email).unwrap() {
            Some((u, _)) => u.id,
            None => users::create(&conn, email, "Test User", None, false).unwrap(),
        };
        let s = session::create(&conn, uid, None, None).unwrap();
        (uid, As { cookie: Some(s.token), csrf: Some(s.csrf_token), token: None })
    }

    pub fn user_with_password(&self, email: &str, pw: &str) -> i64 {
        let conn = self.conn();
        users::create(&conn, email, "Pw User", Some(&password::hash(pw)), false).unwrap()
    }

    pub fn org(&self, slug: &str) -> i64 {
        users::create_org(&self.conn(), slug, &slug.to_uppercase()).unwrap()
    }

    pub fn member(&self, org: i64, email: &str, role: Role) -> (i64, As) {
        let (uid, who) = self.user(email);
        users::set_member(&self.conn(), org, uid, role).unwrap();
        (uid, who)
    }

    pub fn project(&self, org: i64, slug: &str) -> i64 {
        self.conn()
            .query_one(
                "INSERT INTO projects (org_id, slug, name, platform) VALUES ($1, $2, $3, 'javascript') RETURNING id",
                &[&org, &slug, &slug.to_uppercase()],
            )
            .unwrap()
            .get(0)
    }

    /// An issue with `n` events, the newest `minutes_ago` minutes ago.
    pub fn issue(&self, project: i64, title: &str, level: &str) -> i64 {
        let conn = self.conn();
        let short: i64 = conn
            .query_one(
                "UPDATE projects SET next_short_id = next_short_id + 1 WHERE id = $1 RETURNING next_short_id - 1",
                &[&project],
            )
            .unwrap()
            .get(0);
        let now = chrono::Utc::now();
        conn.query_one(
            "INSERT INTO issues (project_id, short_id, fingerprint_hash, grouping_version, title, culprit, level, first_seen, last_seen, times_seen, users_seen, metadata)
             VALUES ($1, $2, $3, 'v1', $4, 'app.js in main', $5, $6, $6, 0, 0, $7) RETURNING id",
            &[&project, &short, &format!("fp-{title}"), &title, &level, &now, &json!({"type": "TypeError", "value": title})],
        )
        .unwrap()
        .get(0)
    }

    /// Stores an event row and its payload for an issue.
    pub fn event(&self, project: i64, issue: i64, ts: chrono::DateTime<chrono::Utc>, attrs: Value) -> uuid::Uuid {
        let conn = self.conn();
        let id = uuid::Uuid::new_v4();
        let s = |k: &str| attrs.get(k).and_then(Value::as_str).map(str::to_string);
        let tags = attrs.get("tags").cloned().unwrap_or(json!({}));
        conn.execute(
            "INSERT INTO events (project_id, issue_id, event_id, timestamp, received, level, platform, environment, release,
                                 message, user_id, user_email, user_hash, os, browser, tags, contexts)
             VALUES ($1, $2, $3, $4, $4, $5, 'javascript', $6, $7, $8, $9, $10, $11, $12, $13, $14, '{}')",
            &[
                &project, &issue, id.as_bytes(), &ts, &s("level").unwrap_or("error".into()), &s("environment"), &s("release"),
                &s("message"), &s("user_id"), &s("user_email"), &attrs.get("user_hash").and_then(Value::as_i64), &s("os"),
                &s("browser"), &tags,
            ],
        )
        .unwrap();
        let payload = json!({"event_id": id.simple().to_string(), "message": s("message"), "level": s("level").unwrap_or("error".into()),
                             "tags": [["environment", s("environment")]], "exception": {"values": [{"type": "TypeError", "value": "boom"}]}});
        let data = zstd::encode_all(payload.to_string().as_bytes(), 3).unwrap();
        conn.execute(
            "INSERT INTO event_payloads (project_id, event_id, issue_id, day, data) VALUES ($1, $2, $3, $4, $5)",
            &[&project, id.as_bytes(), &issue, &ts.date_naive(), &data],
        )
        .unwrap();
        conn.execute(
            "UPDATE issues SET last_seen = CASE WHEN times_seen = 0 THEN $2 ELSE greatest(last_seen, $2) END, first_seen = CASE WHEN times_seen = 0 THEN $2 ELSE least(first_seen, $2) END, times_seen = times_seen + 1 WHERE id = $1",
            &[&issue, &ts],
        )
        .unwrap();
        id
    }

    /// An org token with scopes.
    pub fn org_token(&self, org: i64, scopes: &[&str]) -> String {
        let scopes: Vec<String> = scopes.iter().map(|s| s.to_string()).collect();
        rupta_auth::apitoken::create(&self.conn(), "t", None, Some(org), &scopes, None).unwrap().token
    }

    pub fn user_token(&self, user: i64, scopes: &[&str]) -> String {
        let scopes: Vec<String> = scopes.iter().map(|s| s.to_string()).collect();
        rupta_auth::apitoken::create(&self.conn(), "t", Some(user), None, &scopes, None).unwrap().token
    }

    pub fn audit_actions(&self) -> Vec<String> {
        self.conn().query("SELECT action FROM audit_log ORDER BY id", &[]).unwrap().iter().map(|r| r.get(0)).collect()
    }
}
