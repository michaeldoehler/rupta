mod common;

use axum::http::{StatusCode, header};
use chrono::{Duration, Utc};
use common::{As, TestApp};
use rupta_core::role::Role;
use serde_json::json;

#[tokio::test]
async fn events_navigation_tags_and_stats() {
    let app = TestApp::new();
    let org = app.org("acme");
    let project = app.project(org, "web");
    let (_, who) = app.member(org, "v@example.com", Role::Viewer);
    let issue = app.issue(project, "TypeError", "error");
    let now = Utc::now();
    let e1 = app.event(project, issue, now - Duration::minutes(30), json!({"environment": "production", "browser": "Chrome 120", "message": "first", "user_hash": 1, "tags": {"region": "eu", "browser": "dup"}}));
    let e2 = app.event(project, issue, now - Duration::minutes(20), json!({"environment": "production", "browser": "Chrome 120", "message": "second", "user_hash": 2, "tags": [["region", "us"]]}));
    let e3 = app.event(project, issue, now - Duration::minutes(10), json!({"environment": "staging", "browser": "Firefox 119", "message": "third", "user_hash": 1, "tags": {"region": "eu"}}));
    let hex = |u: uuid::Uuid| u.simple().to_string();

    let list = app.get(&format!("/api/v1/issues/{issue}/events?limit=2"), &who).await;
    assert_eq!(list.status, StatusCode::OK, "{:?}", list.body);
    let ids: Vec<&str> =
        list.body["data"].as_array().unwrap().iter().map(|e| e["event_id"].as_str().unwrap()).collect();
    assert_eq!(ids, vec![hex(e3), hex(e2)]);
    let cursor = list.body["next_cursor"].as_str().unwrap();
    let rest = app.get(&format!("/api/v1/issues/{issue}/events?limit=2&cursor={cursor}"), &who).await;
    assert_eq!(rest.body["data"][0]["event_id"], hex(e1));
    assert!(rest.body["next_cursor"].is_null());
    let filtered = app.get(&format!("/api/v1/issues/{issue}/events?query=environment:staging"), &who).await;
    assert_eq!(filtered.body["data"].as_array().unwrap().len(), 1);
    let text = app.get(&format!("/api/v1/issues/{issue}/events?query=second"), &who).await;
    assert_eq!(text.body["data"][0]["event_id"], hex(e2));

    let latest = app.get(&format!("/api/v1/issues/{issue}/events/latest"), &who).await;
    assert_eq!(latest.status, StatusCode::OK, "{:?}", latest.body);
    assert_eq!(latest.body["event_id"], hex(e3));
    assert_eq!(latest.body["message"], "third");
    assert_eq!(latest.body["previous_event_id"], hex(e2));
    assert!(latest.body["next_event_id"].is_null());
    assert_eq!(latest.body["issue_id"], issue.to_string());
    let middle = app.get(&format!("/api/v1/issues/{issue}/events/{}", e2), &who).await;
    assert_eq!(middle.body["previous_event_id"], hex(e1));
    assert_eq!(middle.body["next_event_id"], hex(e3));
    assert_eq!(app.get(&format!("/api/v1/issues/{issue}/events/oldest"), &who).await.body["event_id"], hex(e1));
    assert_eq!(app.get(&format!("/api/v1/issues/{issue}/events/recommended"), &who).await.status, StatusCode::OK);
    assert_eq!(
        app.get(&format!("/api/v1/issues/{issue}/events/{}", uuid::Uuid::new_v4()), &who).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(app.get(&format!("/api/v1/issues/{issue}/events/not-an-id"), &who).await.status, StatusCode::NOT_FOUND);
    let by_org = app.get(&format!("/api/v1/orgs/acme/events/web/{}", hex(e1)), &who).await;
    assert_eq!(by_org.body["message"], "first");

    let tags = app.get(&format!("/api/v1/issues/{issue}/tags"), &who).await;
    assert_eq!(tags.status, StatusCode::OK, "{:?}", tags.body);
    let find = |k: &str| tags.body.as_array().unwrap().iter().find(|t| t["key"] == k).cloned().unwrap();
    assert_eq!(find("environment")["top_values"][0], json!({"value": "production", "count": 2}));
    assert_eq!(find("browser")["unique_values"], 2, "the first-class column wins over a tag of the same name");
    let region = find("region");
    assert_eq!(region["total_values"], 3);
    assert_eq!(region["top_values"][0], json!({"value": "eu", "count": 2}));
    let values = app.get(&format!("/api/v1/issues/{issue}/tags/region/values"), &who).await;
    assert_eq!(values.body["data"], json!([{"value": "eu", "count": 2}, {"value": "us", "count": 1}]));

    let stats = app.get(&format!("/api/v1/issues/{issue}/stats?period=24h"), &who).await;
    assert_eq!(stats.status, StatusCode::OK, "{:?}", stats.body);
    assert_eq!(stats.body["events"].as_array().unwrap().iter().map(|p| p[1].as_i64().unwrap()).sum::<i64>(), 3);
    assert_eq!(stats.body["resolution"], 3600);
    let d = app.get(&format!("/api/v1/issues/{issue}/stats?period=30d"), &who).await;
    assert_eq!(d.body["resolution"], 86400);

    let q = app.get("/api/v1/orgs/acme/events?group_by=environment", &who).await;
    assert_eq!(q.status, StatusCode::OK, "{:?}", q.body);
    assert_eq!(q.body["data"][0], json!({"environment": "production", "count": 2}));
    let q = app.get("/api/v1/orgs/acme/events?fields=event_id,message&query=browser:Firefox", &who).await;
    assert_eq!(q.body["data"], json!([{"event_id": hex(e3), "message": "third"}]));
    assert_eq!(
        app.get("/api/v1/orgs/acme/events?fields=password_hash", &who).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
}

#[tokio::test]
async fn feedback_and_attachments() {
    let app = TestApp::new();
    let org = app.org("acme");
    let project = app.project(org, "web");
    let (_, who) = app.member(org, "v@example.com", Role::Viewer);
    let issue = app.issue(project, "TypeError", "error");
    let e = app.event(project, issue, Utc::now(), json!({}));
    let conn = app.conn();
    conn.execute(
        "INSERT INTO user_feedback (project_id, event_id, issue_id, name, email, message) VALUES ($1, $2, $3, 'Ada', 'ada@x.org', 'It broke')",
        &[&project, e.as_bytes(), &issue],
    )
    .unwrap();
    app.state.store.objects().put("attachments/test/1", b"<html>evil</html>").unwrap();
    let aid: i64 = conn
        .query_one(
            "INSERT INTO attachments (project_id, event_id, name, content_type, size, object_key, day) VALUES ($1, $2, 'page.html', 'text/html', 17, 'attachments/test/1', current_date) RETURNING id",
            &[&project, e.as_bytes()],
        )
        .unwrap()
        .get(0);
    let fb = app.get(&format!("/api/v1/issues/{issue}/feedback"), &who).await;
    assert_eq!(fb.body["data"][0]["message"], "It broke");
    assert_eq!(fb.body["data"][0]["event_id"], e.simple().to_string());
    let pf = app.get("/api/v1/projects/acme/web/feedback", &who).await;
    assert_eq!(pf.body["data"].as_array().unwrap().len(), 1);

    let list = app.get(&format!("/api/v1/issues/{issue}/attachments"), &who).await;
    assert_eq!(list.body["data"][0]["name"], "page.html");
    let dl = app.get(&format!("/api/v1/attachments/{aid}/download"), &who).await;
    assert_eq!(dl.status, StatusCode::OK);
    assert_eq!(dl.bytes, b"<html>evil</html>");
    assert_eq!(
        dl.headers.get(header::CONTENT_TYPE).unwrap(),
        "application/octet-stream",
        "HTML is never served as HTML"
    );
    assert!(dl.headers.get(header::CONTENT_DISPOSITION).unwrap().to_str().unwrap().starts_with("attachment"));
    let other = app.org("other");
    let (_, outsider) = app.member(other, "x@example.com", Role::Owner);
    assert_eq!(app.get(&format!("/api/v1/attachments/{aid}/download"), &outsider).await.status, StatusCode::NOT_FOUND);
    assert_eq!(app.get(&format!("/api/v1/issues/{issue}/feedback"), &outsider).await.status, StatusCode::NOT_FOUND);
    let _ = As::anonymous();
}
