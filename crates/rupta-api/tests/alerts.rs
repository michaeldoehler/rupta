mod common;

use std::sync::{Arc, Mutex};

use axum::http::{HeaderMap, StatusCode};
use chrono::Utc;
use common::{TestApp, smtp_sink};
use rupta_core::role::Role;
use rupta_core::signal::IssueSignal;
use serde_json::{Value, json};

/// (path, headers, JSON body, raw body)
type Post = (String, HeaderMap, Value, Vec<u8>);

#[derive(Clone, Default)]
struct Received(Arc<Mutex<Vec<Post>>>);

/// A local HTTP server recording every POST.
async fn receiver() -> (String, Received) {
    let got = Received::default();
    let g = got.clone();
    let app = axum::Router::new().route(
        "/{*path}",
        axum::routing::post(move |uri: axum::http::Uri, headers: HeaderMap, body: axum::body::Bytes| {
            let g = g.clone();
            async move {
                let v = serde_json::from_slice(&body).unwrap_or(Value::Null);
                g.0.lock().unwrap().push((uri.path().to_string(), headers, v, body.to_vec()));
                if uri.path() == "/fail" { StatusCode::INTERNAL_SERVER_ERROR } else { StatusCode::OK }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), got)
}

async fn wait_for<T>(f: impl Fn() -> Option<T>) -> T {
    for _ in 0..200 {
        if let Some(v) = f() {
            return v;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("timed out");
}

#[tokio::test]
async fn channels_crud_secrets_and_test_delivery() {
    let (url, got) = receiver().await;
    // The test receivers listen on loopback, refused by default.
    let app = TestApp::with_config(|c| c.integrations_allow_private_hosts = true);
    let org = app.org("acme");
    let (_, admin) = app.member(org, "admin@example.com", Role::Admin);
    let r = app
        .post(
            "/api/v1/orgs/acme/channels",
            &admin,
            json!({"kind": "webhook", "name": "Hook", "config": {"url": format!("{url}/hook"), "secret": "s3cret"}}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["config"]["has_secret"], true);
    assert!(r.body["config"].get("secret").is_none() && r.body["config"].get("secret_enc").is_none());
    let stored: Value = app.conn().query_one("SELECT config FROM notification_channels", &[]).unwrap().get(0);
    assert!(!stored.to_string().contains("s3cret"), "the secret is stored encrypted");
    let id = r.body["id"].as_str().unwrap().to_string();

    let t = app.post(&format!("/api/v1/orgs/acme/channels/{id}/test"), &admin, json!({})).await;
    assert_eq!(t.status, StatusCode::OK, "{:?}", t.body);
    let (path, headers, body, raw) = got.0.lock().unwrap()[0].clone();
    assert_eq!(path, "/hook");
    assert_eq!(body["trigger"], "test");
    let ts: i64 = headers["x-rupta-timestamp"].to_str().unwrap().parse().unwrap();
    assert_eq!(headers["x-rupta-signature"].to_str().unwrap(), rupta_api::notify::sign("s3cret", ts, &raw));

    // Renaming keeps the secret.
    let r = app
        .patch(
            &format!("/api/v1/orgs/acme/channels/{id}"),
            &admin,
            json!({"name": "Renamed", "config": {"url": format!("{url}/fail")}}),
        )
        .await;
    assert_eq!(r.body["config"]["has_secret"], true);
    let t = app.post(&format!("/api/v1/orgs/acme/channels/{id}/test"), &admin, json!({})).await;
    assert_eq!(t.status, StatusCode::BAD_GATEWAY);
    assert_eq!(t.code(), "delivery_failed");

    for bad in [
        json!({"kind": "pager", "name": "x", "config": {}}),
        json!({"kind": "slack", "name": "x", "config": {"webhook_url": "ftp://x"}}),
        json!({"kind": "email", "name": "x", "config": {"to": ["nope"]}}),
    ] {
        assert_eq!(app.post("/api/v1/orgs/acme/channels", &admin, bad).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    }
    let list = app.get("/api/v1/orgs/acme/channels", &admin).await;
    assert_eq!(list.body.as_array().unwrap().len(), 1);
    assert_eq!(app.delete(&format!("/api/v1/orgs/acme/channels/{id}"), &admin).await.status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn rules_fire_on_signals_with_filters_and_rate_limits() {
    let (url, got) = receiver().await;
    let (smtp, mails) = smtp_sink().await;
    let app = TestApp::with_config(|c| {
        c.integrations_allow_private_hosts = true;
        c.smtp_url = Some(smtp.clone());
        c.smtp_from = Some("rupta <rupta@example.com>".into());
    });
    let org = app.org("acme");
    let project = app.project(org, "web");
    let (admin_id, admin) = app.member(org, "admin@example.com", Role::Admin);
    let mk = |kind: &str, cfg: Value| json!({"kind": kind, "name": kind, "config": cfg});
    let slack = app
        .post("/api/v1/orgs/acme/channels", &admin, mk("slack", json!({"webhook_url": format!("{url}/slack")})))
        .await
        .body["id"]
        .clone();
    let teams = app
        .post("/api/v1/orgs/acme/channels", &admin, mk("teams", json!({"webhook_url": format!("{url}/teams")})))
        .await
        .body["id"]
        .clone();
    let hook =
        app.post("/api/v1/orgs/acme/channels", &admin, mk("webhook", json!({"url": format!("{url}/hook")}))).await.body
            ["id"]
            .clone();

    let rule = app
        .post(
            "/api/v1/projects/acme/web/alert-rules",
            &admin,
            json!({
                "name": "New errors",
                "conditions": [{"type": "first_seen"}, {"type": "regression"}],
                "filters": [{"type": "level", "level": "error", "match": "gte"}],
                "actions": [{"type": "channel", "channel_id": slack}, {"type": "channel", "channel_id": teams},
                            {"type": "channel", "channel_id": hook}, {"type": "email", "user_ids": [admin_id.to_string()]}],
                "frequency_minutes": 60
            }),
        )
        .await;
    assert_eq!(rule.status, StatusCode::CREATED, "{:?}", rule.body);
    let warn_issue = app.issue(project, "Just a warning", "warning");
    let issue = app.issue(project, "TypeError: boom", "error");
    app.event(project, issue, Utc::now(), json!({"environment": "production", "level": "error"}));

    // A warning does not pass the level filter.
    rupta_api::alerts::handle(&app.state, &IssueSignal::New { project_id: project, issue_id: warn_issue })
        .await
        .unwrap();
    assert!(got.0.lock().unwrap().is_empty());

    let (tx, rx) = tokio::sync::broadcast::channel(16);
    let engine = rupta_api::alerts::spawn(app.state.clone(), rx);
    tx.send(IssueSignal::New { project_id: project, issue_id: issue }).unwrap();
    let posts = wait_for(|| {
        let g = got.0.lock().unwrap();
        (g.len() >= 3).then(|| g.clone())
    })
    .await;
    let by_path = |p: &str| posts.iter().find(|x| x.0 == p).map(|x| x.2.clone()).unwrap();
    let issue_url = format!("http://rupta.test/organizations/acme/issues/{issue}");
    assert!(by_path("/slack")["blocks"].to_string().contains(&issue_url));
    assert_eq!(by_path("/teams")["attachments"][0]["content"]["actions"][0]["url"], issue_url);
    let h = by_path("/hook");
    assert_eq!(h["trigger"], "first_seen");
    assert_eq!(h["issue"]["url"], issue_url);
    assert_eq!(h["issue"]["short_id"], "WEB-2");
    let mail = wait_for(|| mails.lock().unwrap().first().cloned()).await;
    assert_eq!(mail.0, vec!["admin@example.com"]);
    assert!(mail.1.contains("TypeError: boom"), "{}", mail.1);

    // Rate limit: a regression right after does not notify again.
    tx.send(IssueSignal::Regressed { project_id: project, issue_id: issue }).unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(got.0.lock().unwrap().len(), 3);
    let fires: i64 = app.conn().query_one("SELECT count(*) FROM alert_fires", &[]).unwrap().get(0);
    assert_eq!(fires, 1);
    drop(tx);
    engine.await.unwrap();
}

#[tokio::test]
async fn frequency_conditions_count_events() {
    let (url, got) = receiver().await;
    // The test receivers listen on loopback, refused by default.
    let app = TestApp::with_config(|c| c.integrations_allow_private_hosts = true);
    let org = app.org("acme");
    let project = app.project(org, "web");
    let (_, admin) = app.member(org, "admin@example.com", Role::Admin);
    let hook = app
        .post(
            "/api/v1/orgs/acme/channels",
            &admin,
            json!({"kind": "webhook", "name": "h", "config": {"url": format!("{url}/hook")}}),
        )
        .await
        .body["id"]
        .clone();
    let r = app
        .post(
            "/api/v1/projects/acme/web/alert-rules",
            &admin,
            json!({"name": "Busy", "conditions": [{"type": "event_frequency", "count": 3, "interval": "1h"}],
                   "filters": [{"type": "environment", "value": "production"}, {"type": "tag", "key": "region", "value": "eu"}],
                   "actions": [{"type": "channel", "channel_id": hook}]}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let issue = app.issue(project, "Busy issue", "error");
    let seen = |n| IssueSignal::Seen {
        project_id: project,
        issue_id: issue,
        count: n,
        environment: Some("production".into()),
        level: Some("error".into()),
    };
    for _ in 0..2 {
        app.event(project, issue, Utc::now(), json!({"environment": "production", "tags": {"region": "eu"}}));
    }
    rupta_api::alerts::handle(&app.state, &seen(2)).await.unwrap();
    assert!(got.0.lock().unwrap().is_empty(), "2 < 3");
    app.event(project, issue, Utc::now(), json!({"environment": "production", "tags": {"region": "eu"}}));
    rupta_api::alerts::handle(&app.state, &seen(1)).await.unwrap();
    assert_eq!(got.0.lock().unwrap().len(), 1);
    assert_eq!(got.0.lock().unwrap()[0].2["trigger"], "event_frequency");

    // A disabled rule stays silent.
    let rid = r.body["id"].as_str().unwrap();
    app.patch(&format!("/api/v1/projects/acme/web/alert-rules/{rid}"), &admin, json!({"enabled": false})).await;
    app.conn().execute("DELETE FROM alert_fires", &[]).unwrap();
    rupta_api::alerts::handle(&app.state, &seen(1)).await.unwrap();
    assert_eq!(got.0.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn rule_validation_and_permissions() {
    // The test receivers listen on loopback, refused by default.
    let app = TestApp::with_config(|c| c.integrations_allow_private_hosts = true);
    let org = app.org("acme");
    app.project(org, "web");
    let (_, admin) = app.member(org, "admin@example.com", Role::Admin);
    let (_, member) = app.member(org, "member@example.com", Role::Member);
    let other = app.org("other");
    let foreign: i64 = app.conn().query_one("INSERT INTO notification_channels (org_id, kind, name, config) VALUES ($1, 'slack', 'x', '{}') RETURNING id", &[&other]).unwrap().get(0);
    let base =
        json!({"name": "r", "conditions": [{"type": "first_seen"}], "actions": [{"type": "email", "user_ids": []}]});
    assert_eq!(
        app.post("/api/v1/projects/acme/web/alert-rules", &admin, base).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let foreign_action = json!({"name": "r", "conditions": [{"type": "first_seen"}], "actions": [{"type": "channel", "channel_id": foreign.to_string()}]});
    assert_eq!(
        app.post("/api/v1/projects/acme/web/alert-rules", &admin, foreign_action).await.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "channels of other orgs"
    );
    let bad_interval = json!({"name": "r", "conditions": [{"type": "event_frequency", "count": 1, "interval": "1y"}], "actions": [{"type": "email", "team_ids": []}]});
    assert_eq!(
        app.post("/api/v1/projects/acme/web/alert-rules", &admin, bad_interval).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let ok = json!({"name": "r", "conditions": [{"type": "reappeared"}], "actions": [{"type": "email", "user_ids": []}], "frequency_minutes": 5});
    let mut ok2 = ok.clone();
    ok2["actions"] = json!([{"type": "channel", "channel_id": app.post("/api/v1/orgs/acme/channels", &admin, json!({"kind": "slack", "name": "s", "config": {"webhook_url": "https://hooks.example.com/x"}})).await.body["id"]}]);
    assert_eq!(
        app.post("/api/v1/projects/acme/web/alert-rules", &member, ok2.clone()).await.status,
        StatusCode::FORBIDDEN
    );
    let r = app.post("/api/v1/projects/acme/web/alert-rules", &admin, ok2).await;
    assert_eq!(r.status, StatusCode::CREATED);
    let id = r.body["id"].as_str().unwrap();
    assert_eq!(app.get(&format!("/api/v1/projects/acme/web/alert-rules/{id}"), &member).await.status, StatusCode::OK);
    assert_eq!(app.get("/api/v1/projects/acme/web/alert-rules", &member).await.body.as_array().unwrap().len(), 1);
    assert_eq!(
        app.delete(&format!("/api/v1/projects/acme/web/alert-rules/{id}"), &admin).await.status,
        StatusCode::NO_CONTENT
    );
    let _ = ok;
}

#[tokio::test]
async fn channels_to_internal_addresses_are_refused_by_default() {
    let (url, got) = receiver().await;
    let app = TestApp::new();
    let org = app.org("acme");
    let (_, admin) = app.member(org, "admin@example.com", Role::Admin);
    for (kind, config) in [
        ("webhook", json!({"url": format!("{url}/hook")})),
        ("slack", json!({"webhook_url": format!("{url}/slack")})),
        ("teams", json!({"webhook_url": format!("{url}/teams")})),
    ] {
        let r =
            app.post("/api/v1/orgs/acme/channels", &admin, json!({"kind": kind, "name": kind, "config": config})).await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
        let id = r.body["id"].as_str().unwrap().to_string();
        let t = app.post(&format!("/api/v1/orgs/acme/channels/{id}/test"), &admin, json!({})).await;
        assert!(
            t.body.to_string().to_lowercase().contains("internal")
                || t.body.to_string().contains("RUPTA_INTEGRATIONS_ALLOW_PRIVATE_HOSTS"),
            "{kind}: {}",
            t.body
        );
    }
    assert!(got.0.lock().unwrap().is_empty(), "nothing reached the internal receiver");
}

#[tokio::test]
async fn alert_fires_are_listed_newest_first_per_project() {
    let app = TestApp::new();
    let org = app.org("acme");
    let web = app.project(org, "web");
    let api = app.project(org, "api");
    let (_, viewer) = app.member(org, "viewer@example.com", Role::Viewer);
    let other = app.org("other");
    let (_, outsider) = app.member(other, "outsider@example.com", Role::Owner);
    let rid = |name: &str, project: i64| -> i64 {
        app.conn()
            .query_one("INSERT INTO alert_rules (project_id, name) VALUES ($1, $2) RETURNING id", &[&project, &name])
            .unwrap()
            .get(0)
    };
    let (r1, r2, r3) = (rid("New errors", web), rid("Regressions", web), rid("API", api));
    let i1 = app.issue(web, "TypeError: a", "error");
    let i2 = app.issue(web, "TypeError: b", "warning");
    let i3 = app.issue(api, "TypeError: c", "error");
    let now = Utc::now();
    for (rule, issue, minutes) in [(r1, i1, 30), (r2, i1, 20), (r1, i2, 10), (r3, i3, 5)] {
        app.conn()
            .execute(
                "INSERT INTO alert_fires (rule_id, issue_id, fired_at) VALUES ($1, $2, $3)",
                &[&rule, &issue, &(now - chrono::Duration::minutes(minutes))],
            )
            .unwrap();
    }
    // An issue deleted since.
    app.conn()
        .execute(
            "INSERT INTO alert_fires (rule_id, issue_id, fired_at) VALUES ($1, 999999, $2)",
            &[&r2, &(now - chrono::Duration::hours(2))],
        )
        .unwrap();

    let r = app.get("/api/v1/projects/acme/web/alert-fires?limit=2", &viewer).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let data = r.body["data"].as_array().unwrap();
    assert_eq!(data.len(), 2);
    assert_eq!(data[0]["rule"]["name"], "New errors");
    assert_eq!(data[0]["issue"]["id"], i2.to_string());
    assert_eq!(data[0]["issue"]["short_id"], "WEB-2");
    assert_eq!(data[0]["issue"]["level"], "warning");
    assert_eq!(data[1]["rule"]["name"], "Regressions");
    let next = r.body["next_cursor"].as_str().unwrap();
    let r = app.get(&format!("/api/v1/projects/acme/web/alert-fires?limit=2&cursor={next}"), &viewer).await;
    let data = r.body["data"].as_array().unwrap();
    assert_eq!(data.len(), 2);
    assert_eq!(data[0]["issue"]["id"], i1.to_string());
    assert_eq!(data[0]["rule"]["id"], r1.to_string());
    assert!(data[1]["issue"].is_null(), "deleted issue: {}", data[1]);
    assert!(r.body["next_cursor"].is_null());

    let r = app.get(&format!("/api/v1/projects/acme/web/alert-fires?rule={r1}"), &viewer).await;
    assert_eq!(r.body["data"].as_array().unwrap().len(), 2);
    let r = app.get(&format!("/api/v1/projects/acme/web/alert-fires?rule={r3}"), &viewer).await;
    assert_eq!(r.body["data"].as_array().unwrap().len(), 0, "another project's rule");
    assert_eq!(
        app.get("/api/v1/projects/acme/web/alert-fires?rule=x", &viewer).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(app.get("/api/v1/projects/acme/web/alert-fires", &outsider).await.status, StatusCode::NOT_FOUND);
    let token = app.org_token(org, &["member:read"]);
    assert_eq!(
        app.get("/api/v1/projects/acme/web/alert-fires", &common::As::token(&token)).await.status,
        StatusCode::FORBIDDEN
    );
}
