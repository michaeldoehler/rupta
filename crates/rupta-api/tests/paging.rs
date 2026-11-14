//! PagerDuty (Events API v2) and Opsgenie (Alert API v2) channels against
//! local receivers that answer like the services.

mod common;

use std::sync::{Arc, Mutex};

use axum::http::{HeaderMap, StatusCode};
use common::{TestApp, wait_for};
use rupta_core::role::Role;
use rupta_core::signal::IssueSignal;
use serde_json::{Value, json};

/// (path with query, headers, body)
type Got = Arc<Mutex<Vec<(String, HeaderMap, Value)>>>;

/// A receiver answering 202 like both services; the first `throttle`
/// requests get 429 with `Retry-After: 0`.
async fn receiver(throttle: u32) -> (String, Got) {
    let got: Got = Arc::default();
    let left = Arc::new(Mutex::new(throttle));
    let g = got.clone();
    let app = axum::Router::new().fallback(move |uri: axum::http::Uri, headers: HeaderMap, body: axum::body::Bytes| {
        let (g, left) = (g.clone(), left.clone());
        async move {
            let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            g.lock().unwrap().push((uri.to_string(), headers, v));
            let mut l = left.lock().unwrap();
            if *l > 0 {
                *l -= 1;
                return (
                    StatusCode::TOO_MANY_REQUESTS,
                    [("retry-after", "0")],
                    axum::Json(json!({"status": "throttle event"})),
                );
            }
            (
                StatusCode::ACCEPTED,
                [("x-receiver", "1")],
                axum::Json(
                    json!({"status": "success", "message": "Event processed", "result": "Request will be processed"}),
                ),
            )
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), got)
}

fn app() -> TestApp {
    TestApp::with_config(|c| c.integrations_allow_private_hosts = true)
}

#[tokio::test]
async fn pagerduty_triggers_with_the_issue_as_dedup_key_and_resolves() {
    let (url, got) = receiver(1).await;
    let app = app();
    let org = app.org("acme");
    let project = app.project(org, "web");
    let (_, admin) = app.member(org, "admin@example.com", Role::Admin);
    let issue = app.issue(project, "Boom", "fatal");

    let r = app
        .post(
            "/api/v1/orgs/acme/channels",
            &admin,
            json!({"kind": "pagerduty", "name": "PD", "config": {"region": "eu"}}),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "the routing key is required");
    let r = app
        .post(
            "/api/v1/orgs/acme/channels",
            &admin,
            json!({"kind": "pagerduty", "name": "PD", "config": {"region": "mars", "routing_key": "k"}}),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = app
        .post(
            "/api/v1/orgs/acme/channels",
            &admin,
            json!({"kind": "pagerduty", "name": "PD",
            "config": {"routing_key": "R0UT1NGKEY0123456789abcdefABCDEF", "api_url": format!("{url}/v2/enqueue")}}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["config"]["has_secret"], true);
    assert_eq!(r.body["config"]["region"], "us");
    assert!(!r.bytes.windows(10).any(|w| w == b"R0UT1NGKEY"), "the routing key is write-only");
    let channel = r.body["id"].as_str().unwrap().to_string();
    // Changing the name keeps the key.
    let r = app
        .patch(
            &format!("/api/v1/orgs/acme/channels/{channel}"),
            &admin,
            json!({"config": {"api_url": format!("{url}/v2/enqueue")}}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);

    // The test opens and resolves an incident (after one throttled try).
    let r = app.post(&format!("/api/v1/orgs/acme/channels/{channel}/test"), &admin, json!({})).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    {
        let g = got.lock().unwrap();
        assert_eq!(g.len(), 3, "429, trigger, resolve");
        assert_eq!(g[1].2["event_action"], "trigger");
        assert_eq!(g[2].2["event_action"], "resolve");
        assert_eq!(g[1].2["dedup_key"], g[2].2["dedup_key"]);
    }
    got.lock().unwrap().clear();

    app.post(
        "/api/v1/projects/acme/web/alert-rules",
        &admin,
        json!({"name": "page", "conditions": [{"type": "first_seen"}],
        "actions": [{"type": "channel", "channel_id": channel}]}),
    )
    .await;
    rupta_api::alerts::handle(&app.state, &IssueSignal::New { project_id: project, issue_id: issue }).await.unwrap();
    let ev = got.lock().unwrap()[0].2.clone();
    assert_eq!(ev["routing_key"], "R0UT1NGKEY0123456789abcdefABCDEF");
    assert_eq!(ev["event_action"], "trigger");
    assert_eq!(ev["dedup_key"], issue.to_string());
    assert_eq!(ev["payload"]["severity"], "critical");
    assert_eq!(ev["payload"]["source"], "web");
    assert!(ev["payload"]["summary"].as_str().unwrap().contains("Boom"));
    assert_eq!(ev["links"][0]["href"], format!("http://rupta.test/organizations/acme/issues/{issue}"));
    assert!(ev["payload"]["timestamp"].as_str().is_some());

    app.patch(&format!("/api/v1/issues/{issue}"), &admin, json!({"status": "resolved"})).await;
    let resolve = wait_for(|| got.lock().unwrap().get(1).map(|g| g.2.clone())).await;
    assert_eq!(
        resolve,
        json!({"routing_key": "R0UT1NGKEY0123456789abcdefABCDEF", "event_action": "resolve", "dedup_key": issue.to_string()})
    );
    wait_for(|| {
        let r = app
            .conn()
            .query_one("SELECT resolved_at IS NOT NULL FROM channel_incidents WHERE issue_id = $1", &[&issue])
            .unwrap();
        r.get::<bool>(0).then_some(())
    })
    .await;
    // Resolving again sends nothing more.
    app.patch(&format!("/api/v1/issues/{issue}"), &admin, json!({"status": "unresolved"})).await;
    app.patch(&format!("/api/v1/issues/{issue}"), &admin, json!({"status": "resolved"})).await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(got.lock().unwrap().len(), 2);
    assert!(
        app.state.metrics.get("rupta_alert_notifications_total", &[("channel", "pagerduty"), ("result", "success")])
            >= 2
    );
}

#[tokio::test]
async fn opsgenie_creates_alerts_by_alias_and_closes_them() {
    let (url, got) = receiver(0).await;
    let app = app();
    let org = app.org("acme");
    let project = app.project(org, "web");
    let (_, admin) = app.member(org, "admin@example.com", Role::Admin);
    let issue = app.issue(project, "Slow", "warning");
    let r = app
        .post(
            "/api/v1/orgs/acme/channels",
            &admin,
            json!({"kind": "opsgenie", "name": "OG",
            "config": {"api_key": "eb243592-faa2-4ba2-a551q-1afdf565c889", "region": "eu", "api_url": url}}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["config"]["region"], "eu");
    let channel = r.body["id"].as_str().unwrap().to_string();
    app.post(
        "/api/v1/projects/acme/web/alert-rules",
        &admin,
        json!({"name": "og", "conditions": [{"type": "first_seen"}],
        "actions": [{"type": "channel", "channel_id": channel}]}),
    )
    .await;
    rupta_api::alerts::handle(&app.state, &IssueSignal::New { project_id: project, issue_id: issue }).await.unwrap();
    let (path, headers, body) = got.lock().unwrap()[0].clone();
    assert_eq!(path, "/v2/alerts");
    assert_eq!(headers.get("authorization").unwrap(), "GenieKey eb243592-faa2-4ba2-a551q-1afdf565c889");
    assert_eq!(body["alias"], issue.to_string());
    assert_eq!(body["priority"], "P3");
    assert_eq!(body["source"], "rupta");
    assert!(body["message"].as_str().unwrap().chars().count() <= 130);
    assert!(body["tags"].as_array().unwrap().contains(&json!("web")));

    app.patch(&format!("/api/v1/issues/{issue}"), &admin, json!({"status": "resolved"})).await;
    let (path, _, body) = wait_for(|| got.lock().unwrap().get(1).cloned()).await;
    assert_eq!(path, format!("/v2/alerts/{issue}/close?identifierType=alias"));
    assert_eq!(body["source"], "rupta");
}
