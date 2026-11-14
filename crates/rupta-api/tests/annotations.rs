//! `GET /api/v1/orgs/{org}/annotations`.

mod common;

use axum::http::StatusCode;
use chrono::{Duration, DurationRound, Utc};
use common::TestApp;
use rupta_core::role::Role;
use serde_json::json;

#[tokio::test]
async fn releases_deploys_and_new_issue_spikes() {
    let app = TestApp::new();
    let org = app.org("acme");
    let web = app.project(org, "web");
    let api = app.project(org, "api");
    let (_, viewer) = app.member(org, "viewer@example.com", Role::Viewer);
    let c = app.conn();
    let now = Utc::now();
    let rel: i64 = c
        .query_one(
            "INSERT INTO releases (org_id, version, ref, date_released) VALUES ($1, '1.4.0', 'abc123', $2) RETURNING id",
            &[&org, &(now - Duration::hours(3))],
        )
        .unwrap()
        .get(0);
    c.execute("INSERT INTO release_projects (release_id, project_id) VALUES ($1, $2)", &[&rel, &web]).unwrap();
    c.execute(
        "INSERT INTO deploys (release_id, environment, date_started, date_finished) VALUES ($1, 'production', $2, $3)",
        &[&rel, &(now - Duration::minutes(150)), &(now - Duration::minutes(140))],
    )
    .unwrap();
    let old: i64 = c
        .query_one(
            "INSERT INTO releases (org_id, version, created_at) VALUES ($1, '0.9.0', $2) RETURNING id",
            &[&org, &(now - Duration::days(20))],
        )
        .unwrap()
        .get(0);
    c.execute("INSERT INTO release_projects (release_id, project_id) VALUES ($1, $2)", &[&old, &web]).unwrap();
    // Eight new issues in one hour of the api project, one in another hour.
    let spike_hour = (now - Duration::hours(5)).duration_trunc(Duration::hours(1)).unwrap();
    for i in 0..8 {
        let id = app.issue(api, &format!("spike {i}"), "error");
        c.execute("UPDATE issues SET first_seen = $1 WHERE id = $2", &[&(spike_hour + Duration::minutes(i)), &id])
            .unwrap();
    }
    let lone = app.issue(api, "lone", "error");
    c.execute("UPDATE issues SET first_seen = $1 WHERE id = $2", &[&(now - Duration::hours(10)), &lone]).unwrap();

    let from = (now - Duration::hours(24)).timestamp_millis();
    let to = now.timestamp_millis();
    let r = app.get(&format!("/api/v1/orgs/acme/annotations?from={from}&to={to}"), &viewer).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let list = r.body.as_array().unwrap();
    assert_eq!(list.len(), 3, "{list:?}");
    let kinds: Vec<&str> = list.iter().map(|a| a["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, vec!["deploy", "release", "new_issues"], "newest first");
    assert_eq!(list[0]["title"], "Deploy 1.4.0 to production");
    assert_eq!(list[0]["time_end"].as_i64().unwrap() - list[0]["time"].as_i64().unwrap(), 10 * 60 * 1000);
    assert_eq!(list[1]["title"], "Release 1.4.0");
    assert_eq!(list[1]["tags"], json!(["release", "web"]));
    assert_eq!(list[1]["url"], "http://rupta.test/organizations/acme/releases/1.4.0");
    assert_eq!(list[2]["title"], "8 new issues in api");
    assert_eq!(list[2]["time"], spike_hour.timestamp_millis());
    assert_eq!(list[2]["project"], "api");

    let r = app.get(&format!("/api/v1/orgs/acme/annotations?from={from}&to={to}&project=api"), &viewer).await;
    assert_eq!(r.body.as_array().unwrap().len(), 1);
    let r = app.get(&format!("/api/v1/orgs/acme/annotations?from={from}&to={to}&kind=release"), &viewer).await;
    assert_eq!(r.body.as_array().unwrap().len(), 1);
    let r = app.get("/api/v1/orgs/acme/annotations?from=2026-01-01T00:00:00Z&to=2026-06-01T00:00:00Z", &viewer).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "more than 90 days");
    let r = app.get("/api/v1/orgs/acme/annotations?kind=commits", &viewer).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = app.get("/api/v1/orgs/acme/annotations?project=nope", &viewer).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let other = app.org("other");
    let (_, outsider) = app.member(other, "o@example.com", Role::Owner);
    assert_eq!(app.get("/api/v1/orgs/acme/annotations", &outsider).await.status, StatusCode::NOT_FOUND);
}
