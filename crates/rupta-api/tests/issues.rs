mod common;

use axum::http::StatusCode;
use chrono::{Duration, Utc};
use common::{As, TestApp};
use rupta_core::role::Role;
use serde_json::{Value, json};

struct Fixture {
    app: TestApp,
    org: i64,
    project: i64,
    member: (i64, As),
    viewer: As,
    a: i64,
    b: i64,
    c: i64,
}

fn titles(v: &Value) -> Vec<String> {
    v["data"].as_array().unwrap().iter().map(|i| i["title"].as_str().unwrap().to_string()).collect()
}

fn fixture() -> Fixture {
    let app = TestApp::new();
    let org = app.org("acme");
    let project = app.project(org, "web");
    let member = app.member(org, "m@example.com", Role::Member);
    let (_, viewer) = app.member(org, "v@example.com", Role::Viewer);
    let now = Utc::now();
    let a = app.issue(project, "TypeError: x is undefined", "error");
    let b = app.issue(project, "Connection reset", "fatal");
    let c = app.issue(project, "Slow query", "warning");
    for i in 0..5 {
        app.event(project, a, now - Duration::minutes(i), json!({"environment": "production", "browser": "Chrome 120.0", "os": "Windows 11", "user_email": "ada@example.com", "tags": {"region": "eu-west"}}));
    }
    app.event(project, b, now - Duration::hours(30), json!({"environment": "staging", "browser": "Firefox 119", "release": "1.0.0", "tags": [["region", "us-east"]]}));
    app.event(
        project,
        c,
        now - Duration::days(3),
        json!({"environment": "production", "browser": "Safari 17", "user_id": "42"}),
    );
    app.conn().execute("UPDATE issues SET first_seen = $1 WHERE id = $2", &[&(now - Duration::days(10)), &c]).unwrap();
    Fixture { app, org, project, member, viewer, a, b, c }
}

#[tokio::test]
async fn search_language_end_to_end() {
    let f = fixture();
    let who = &f.member.1;
    let q = |query: &str| format!("/api/v1/orgs/acme/issues?query={}", urlencode(query));
    let cases: &[(&str, &[&str])] = &[
        ("", &["TypeError: x is undefined", "Connection reset", "Slow query"]),
        ("is:unresolved", &["TypeError: x is undefined", "Connection reset", "Slow query"]),
        ("level:fatal", &["Connection reset"]),
        ("!level:fatal", &["TypeError: x is undefined", "Slow query"]),
        ("level:[error,warning]", &["TypeError: x is undefined", "Slow query"]),
        ("environment:production", &["TypeError: x is undefined", "Slow query"]),
        ("browser:Chrome", &["TypeError: x is undefined"]),
        ("os:\"Windows 11\"", &["TypeError: x is undefined"]),
        ("user.email:ada@example.com", &["TypeError: x is undefined"]),
        ("user.id:42", &["Slow query"]),
        ("release:1.0.0", &["Connection reset"]),
        ("tags[region]:eu-west", &["TypeError: x is undefined"]),
        ("region:us-east", &["Connection reset"]),
        ("region:eu*", &["TypeError: x is undefined"]),
        ("times_seen:>3", &["TypeError: x is undefined"]),
        ("first_seen:-24h", &["TypeError: x is undefined"]),
        ("first_seen:-2d", &["TypeError: x is undefined", "Connection reset"]),
        ("first_seen:>7d", &["Slow query"]),
        ("last_seen:-1h", &["TypeError: x is undefined"]),
        ("reset", &["Connection reset"]),
        ("\"x is undefined\"", &["TypeError: x is undefined"]),
        ("is:assigned", &[]),
        ("has:release", &["Connection reset"]),
    ];
    for (query, want) in cases {
        let r = f.app.get(&q(query), who).await;
        assert_eq!(r.status, StatusCode::OK, "{query}: {:?}", r.body);
        let mut got = titles(&r.body);
        let mut want: Vec<String> = want.iter().map(|s| s.to_string()).collect();
        got.sort();
        want.sort();
        assert_eq!(got, want, "{query}");
    }
    let bad = f.app.get(&q("is:sideways"), who).await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(bad.body["error"]["details"]["field"], "query");
}

fn urlencode(s: &str) -> String {
    form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

#[tokio::test]
async fn sorting_stats_and_cursor_pagination() {
    let f = fixture();
    let who = &f.member.1;
    let r = f.app.get("/api/v1/orgs/acme/issues?sort=freq", who).await;
    assert_eq!(titles(&r.body)[0], "TypeError: x is undefined");
    let first = &r.body["data"][0];
    assert_eq!(first["count"], 5);
    assert_eq!(first["short_id"], "WEB-1");
    assert_eq!(first["project"]["slug"], "web");
    assert_eq!(first["metadata"]["type"], "TypeError");
    let stats = first["stats"].as_array().unwrap();
    assert_eq!(stats.len(), 24, "hourly buckets for 24h");
    assert_eq!(stats.iter().map(|p| p[1].as_i64().unwrap()).sum::<i64>(), 5);
    let r14 = f.app.get("/api/v1/orgs/acme/issues?sort=freq&period=14d", who).await;
    assert_eq!(r14.body["data"][0]["stats"].as_array().unwrap().len(), 14);

    // Pages of one, following the cursor, give every issue once in order.
    let mut seen = Vec::new();
    let mut url = "/api/v1/orgs/acme/issues?sort=last_seen&limit=1".to_string();
    loop {
        let r = f.app.get(&url, who).await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
        seen.extend(titles(&r.body));
        match r.body["next_cursor"].as_str() {
            Some(c) => url = format!("/api/v1/orgs/acme/issues?sort=last_seen&limit=1&cursor={c}"),
            None => break,
        }
    }
    assert_eq!(seen, vec!["TypeError: x is undefined", "Connection reset", "Slow query"]);
    assert_eq!(f.app.get("/api/v1/orgs/acme/issues?cursor=garbage!", who).await.status, StatusCode::BAD_REQUEST);
    assert_eq!(f.app.get("/api/v1/orgs/acme/issues?sort=random", who).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    for sort in ["first_seen", "users", "priority"] {
        assert_eq!(f.app.get(&format!("/api/v1/orgs/acme/issues?sort={sort}"), who).await.status, StatusCode::OK);
    }
    // period filters by last seen.
    let r = f.app.get("/api/v1/orgs/acme/issues?period=24h", who).await;
    assert_eq!(titles(&r.body), vec!["TypeError: x is undefined"]);
    let r = f.app.get("/api/v1/orgs/acme/issues?environment=staging", who).await;
    assert_eq!(titles(&r.body), vec!["Connection reset"]);
    let _ = (f.org, f.project);
}

#[tokio::test]
async fn triage_records_activity_and_respects_roles() {
    let f = fixture();
    let (mid, who) = (&f.member.0, &f.member.1);
    let path = format!("/api/v1/issues/{}", f.a);
    let r = f.app.patch(&path, &f.viewer, json!({"status": "resolved"})).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "viewers cannot triage");
    // Viewers may bookmark and mark seen (personal state).
    let r = f.app.patch(&path, &f.viewer, json!({"is_bookmarked": true, "has_seen": true})).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["is_bookmarked"], true);
    assert_eq!(r.body["has_seen"], true);

    let r = f
        .app
        .patch(
            &path,
            who,
            json!({"status": "resolved", "resolved_in_release": "1.2.0", "assignee": "me", "priority": "high"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["status"], "resolved");
    assert_eq!(r.body["resolved_in_release"], "1.2.0");
    assert_eq!(r.body["assignee"]["type"], "user");
    assert_eq!(r.body["assignee"]["id"], mid.to_string());
    assert_eq!(r.body["priority"], "high");
    assert!(r.body["permalink"].as_str().unwrap().ends_with(&format!("/organizations/acme/issues/{}", f.a)));

    let r = f.app.get(&format!("/api/v1/orgs/acme/issues?query={}", urlencode("assigned:me is:resolved")), who).await;
    assert_eq!(titles(&r.body), vec!["TypeError: x is undefined"]);
    let r = f.app.get(&format!("/api/v1/orgs/acme/issues?query={}", urlencode("is:bookmarked")), &f.viewer).await;
    assert_eq!(titles(&r.body), vec!["TypeError: x is undefined"]);

    let r = f.app.patch(&path, who, json!({"status": "ignored", "ignore_duration": 60})).await;
    assert_eq!(r.body["substatus"], "archived_until_condition_met");
    assert!(r.body["ignore_until"].is_string());
    let r = f.app.patch(&path, who, json!({"assignee": null})).await;
    assert!(r.body["assignee"].is_null());
    assert_eq!(
        f.app.patch(&path, who, json!({"assignee": "user:999999"})).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(f.app.patch(&path, who, json!({"status": "maybe"})).await.status, StatusCode::UNPROCESSABLE_ENTITY);

    let comment = f.app.post(&format!("{path}/comments"), who, json!({"text": "Looking into it"})).await;
    assert_eq!(comment.status, StatusCode::CREATED);
    let cid = comment.body["id"].as_str().unwrap().to_string();
    let act = f.app.get(&format!("{path}/activity"), who).await;
    let kinds: Vec<&str> = act.body["data"].as_array().unwrap().iter().map(|a| a["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, vec!["comment", "unassigned", "status_change", "assigned", "priority_change", "status_change"]);
    let (_, other) = f.app.member(f.org, "o@example.com", Role::Member);
    assert_eq!(
        f.app.patch(&format!("{path}/comments/{cid}"), &other, json!({"text": "hijack"})).await.status,
        StatusCode::FORBIDDEN
    );
    let edited = f.app.patch(&format!("{path}/comments/{cid}"), who, json!({"text": "Fixed"})).await;
    assert_eq!(edited.body["data"]["text"], "Fixed");
    assert!(edited.body["updated_at"].is_string());
    assert_eq!(f.app.delete(&format!("{path}/comments/{cid}"), who).await.status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn bulk_changes_and_deletion() {
    let f = fixture();
    let who = &f.member.1;
    let r =
        f.app.put(&format!("/api/v1/orgs/acme/issues?id={}&id={}", f.a, f.b), who, json!({"status": "resolved"})).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["updated"], 2);
    let r = f.app.get("/api/v1/orgs/acme/issues?query=is:resolved", who).await;
    assert_eq!(titles(&r.body).len(), 2);

    // Members cannot delete; admins can.
    assert_eq!(f.app.delete(&format!("/api/v1/orgs/acme/issues?id={}", f.c), who).await.status, StatusCode::FORBIDDEN);
    let (_, admin) = f.app.member(f.org, "a@example.com", Role::Admin);
    assert_eq!(
        f.app.delete(&format!("/api/v1/orgs/acme/issues?id={}", f.c), &admin).await.status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(f.app.get(&format!("/api/v1/issues/{}", f.c), &admin).await.status, StatusCode::NOT_FOUND);
    let n: i64 = f.app.conn().query_one("SELECT count(*) FROM events WHERE issue_id = $1", &[&f.c]).unwrap().get(0);
    assert_eq!(n, 0);
    assert!(f.app.audit_actions().contains(&"issue.delete".into()));
}

#[tokio::test]
async fn other_orgs_issues_are_not_found() {
    let f = fixture();
    let other_org = f.app.org("other");
    let (_, outsider) = f.app.member(other_org, "x@example.com", Role::Owner);
    let path = format!("/api/v1/issues/{}", f.a);
    let r = f.app.get(&path, &outsider).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    let missing = f.app.get("/api/v1/issues/99999999", &outsider).await;
    assert_eq!(r.body, missing.body, "no difference between foreign and missing");
    assert_eq!(f.app.patch(&path, &outsider, json!({"status": "resolved"})).await.status, StatusCode::NOT_FOUND);
    assert_eq!(f.app.get("/api/v1/orgs/acme/issues", &outsider).await.status, StatusCode::NOT_FOUND);
    // Bulk updates silently skip issues of other organizations.
    let r = f.app.put(&format!("/api/v1/orgs/other/issues?id={}", f.a), &outsider, json!({"status": "resolved"})).await;
    assert_eq!(r.body["updated"], 0);
    // Unknown project filter: nothing.
    let r = f.app.get(&format!("/api/v1/orgs/acme/issues?project={}", f.project + 1000), &f.member.1).await;
    assert_eq!(titles(&r.body).len(), 0);
}

#[tokio::test]
async fn saved_searches() {
    let f = fixture();
    let who = &f.member.1;
    let r = f.app.post("/api/v1/orgs/acme/saved-searches", who, json!({"name": "Mine", "query": "assigned:me"})).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let id = r.body["id"].as_str().unwrap().to_string();
    assert_eq!(
        f.app
            .post("/api/v1/orgs/acme/saved-searches", who, json!({"name": "Shared", "query": "x", "shared": true}))
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        f.app.post("/api/v1/orgs/acme/saved-searches", who, json!({"name": "Bad", "query": "\"open"})).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(f.app.get("/api/v1/orgs/acme/saved-searches", &f.viewer).await.body, json!([]), "private to its owner");
    let r = f.app.patch(&format!("/api/v1/orgs/acme/saved-searches/{id}"), who, json!({"name": "Renamed"})).await;
    assert_eq!(r.body["name"], "Renamed");
    assert_eq!(
        f.app.delete(&format!("/api/v1/orgs/acme/saved-searches/{id}"), &f.viewer).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        f.app.delete(&format!("/api/v1/orgs/acme/saved-searches/{id}"), who).await.status,
        StatusCode::NO_CONTENT
    );
}
