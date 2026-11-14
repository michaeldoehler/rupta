//! Issue lookup by short id, artifact bundles, the SSO connection test,
//! user preferences and workflow emails, resolution in a release, ignoring
//! until more users, and the escalating alert condition.

mod common;

use axum::http::StatusCode;
use chrono::{Duration, Utc};
use common::{TestApp, smtp_sink, wait_for};
use rupta_core::role::Role;
use rupta_core::signal::IssueSignal;
use serde_json::{Value, json};

#[tokio::test]
async fn issue_by_short_id() {
    let app = TestApp::new();
    let org = app.org("acme");
    let other = app.org("other");
    let web = app.project(org, "web-app");
    app.project(other, "web-app");
    let (_, who) = app.member(org, "m@example.com", Role::Member);
    let first = app.issue(web, "first", "error");
    let second = app.issue(web, "second", "error");
    let r = app.get("/api/v1/orgs/acme/issues/by-short-id/WEB-APP-2", &who).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["id"], second.to_string());
    assert_eq!(r.body["short_id"], "WEB-APP-2");
    let r = app.get("/api/v1/orgs/acme/issues/by-short-id/web-app-1", &who).await;
    assert_eq!(r.body["id"], first.to_string());
    for missing in ["WEB-APP-3", "WEB-1", "WEBAPP", "WEB-APP-x", "WEB-APP-0"] {
        let r = app.get(&format!("/api/v1/orgs/acme/issues/by-short-id/{missing}"), &who).await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "{missing}");
    }
    // Not a member of the other organization.
    assert_eq!(app.get("/api/v1/orgs/other/issues/by-short-id/WEB-APP-1", &who).await.status, StatusCode::NOT_FOUND);
}

/// A bundle row with index entries and its object, linked to projects.
fn bundle(app: &TestApp, org: i64, projects: &[i64], checksum: &str) -> (i64, String) {
    let conn = app.conn();
    let key = format!("artifact-bundles/{org}/{checksum}.zip");
    app.state.store.objects().put(&key, b"zip").unwrap();
    let id: i64 = conn
        .query_one(
            "INSERT INTO artifact_bundles (org_id, checksum, release, dist, object_key, file_count, size)
             VALUES ($1, $2, 'web@1.0', NULL, $3, 3, 1234) RETURNING id",
            &[&org, &checksum, &key],
        )
        .unwrap()
        .get(0);
    for p in projects {
        conn.execute("INSERT INTO artifact_bundle_projects (bundle_id, project_id) VALUES ($1, $2)", &[&id, p])
            .unwrap();
    }
    for (i, debug_id) in [Some(uuid::Uuid::new_v4()), Some(uuid::Uuid::new_v4()), None].iter().enumerate() {
        conn.execute(
            "INSERT INTO artifact_index (org_id, bundle_id, debug_id, release, url, file_path, file_type)
             VALUES ($1, $2, $3, 'web@1.0', $4, $4, 'source_map')",
            &[&org, &id, &debug_id.map(|u| *u.as_bytes()), &format!("~/f{i}.js")],
        )
        .unwrap();
    }
    (id, key)
}

#[tokio::test]
async fn artifact_bundles_list_and_delete() {
    let app = TestApp::new();
    let org = app.org("acme");
    let web = app.project(org, "web");
    let api = app.project(org, "api");
    let (_, admin) = app.member(org, "admin@example.com", Role::Admin);
    let (_, member) = app.member(org, "m@example.com", Role::Member);
    let (shared, shared_key) = bundle(&app, org, &[web, api], &"a".repeat(40));
    let (own, own_key) = bundle(&app, org, &[web], &"b".repeat(40));
    bundle(&app, org, &[api], &"c".repeat(40));

    let r = app.get("/api/v1/projects/acme/web/artifact-bundles?limit=1", &member).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let b = &r.body["data"][0];
    assert_eq!(b["id"], own.to_string());
    assert_eq!(
        (b["file_count"].as_i64(), b["size"].as_i64(), b["debug_ids_count"].as_i64()),
        (Some(3), Some(1234), Some(2))
    );
    assert_eq!(b["release"], "web@1.0");
    assert!(b["dist"].is_null() && b["created_at"].is_string());
    let cursor = r.body["next_cursor"].as_str().unwrap().to_string();
    let r = app.get(&format!("/api/v1/projects/acme/web/artifact-bundles?cursor={cursor}"), &member).await;
    assert_eq!(r.body["data"].as_array().unwrap().len(), 1);
    assert_eq!(r.body["data"][0]["id"], shared.to_string());
    assert!(r.body["next_cursor"].is_null());

    // Members may not delete; admins may.
    let path = |id: i64| format!("/api/v1/projects/acme/web/artifact-bundles/{id}");
    assert_eq!(app.delete(&path(own), &member).await.status, StatusCode::FORBIDDEN);
    assert_eq!(app.delete(&path(own), &admin).await.status, StatusCode::NO_CONTENT);
    let objects = app.state.store.objects().clone();
    assert!(objects.get(&own_key).unwrap().is_none());
    let count = |sql: &str, id: i64| -> i64 { app.conn().query_one(sql, &[&id]).unwrap().get(0) };
    assert_eq!(count("SELECT count(*) FROM artifact_index WHERE bundle_id = $1", own), 0);
    assert_eq!(count("SELECT count(*) FROM artifact_bundles WHERE id = $1", own), 0);
    // A bundle another project still uses: only the link goes.
    assert_eq!(app.delete(&path(shared), &admin).await.status, StatusCode::NO_CONTENT);
    assert!(objects.get(&shared_key).unwrap().is_some());
    assert_eq!(count("SELECT count(*) FROM artifact_index WHERE bundle_id = $1", shared), 3);
    let r = app.get("/api/v1/projects/acme/web/artifact-bundles", &member).await;
    assert!(r.body["data"].as_array().unwrap().is_empty());
    let r = app.get("/api/v1/projects/acme/api/artifact-bundles", &member).await;
    assert_eq!(r.body["data"].as_array().unwrap().len(), 2);
    // Gone from this project now.
    assert_eq!(app.delete(&path(shared), &admin).await.status, StatusCode::NOT_FOUND);
    assert_eq!(app.audit_actions().iter().filter(|a| *a == "artifact_bundle.delete").count(), 2);
}

#[tokio::test]
async fn old_upload_chunks_are_purged() {
    let app = TestApp::new();
    let objects = app.state.store.objects().clone();
    let conn = app.conn();
    for (sum, age_hours) in [("1".repeat(40), 30), ("2".repeat(40), 1)] {
        let key = format!("chunks/{sum}");
        objects.put(&key, b"chunk").unwrap();
        conn.execute(
            "INSERT INTO file_chunks (checksum, object_key, size, created_at) VALUES ($1, $2, 5, $3)",
            &[&sum, &key, &(Utc::now() - Duration::hours(age_hours))],
        )
        .unwrap();
    }
    rupta_api::maintenance::run_once(&app.state).await.unwrap();
    let left: Vec<String> =
        conn.query("SELECT checksum FROM file_chunks", &[]).unwrap().iter().map(|r| r.get(0)).collect();
    assert_eq!(left, vec!["2".repeat(40)]);
    assert!(objects.get(&format!("chunks/{}", "1".repeat(40))).unwrap().is_none());
    assert!(objects.get(&format!("chunks/{}", "2".repeat(40))).unwrap().is_some());
}

/// A discovery endpoint on a local port; returns its issuer URL.
async fn discovery_server(doc: impl Fn(&str) -> Value + Send + Sync + 'static) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let issuer = format!("http://{}", listener.local_addr().unwrap());
    let body = doc(&issuer);
    let app = axum::Router::new()
        .route("/.well-known/openid-configuration", axum::routing::get(move || async move { axum::Json(body) }));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    issuer
}

fn good_doc(i: &str) -> Value {
    json!({
        "issuer": i, "authorization_endpoint": format!("{i}/authorize"), "token_endpoint": format!("{i}/token"),
        "userinfo_endpoint": format!("{i}/userinfo"), "jwks_uri": format!("{i}/jwks"),
        "response_types_supported": ["code"], "code_challenge_methods_supported": ["S256"],
        "scopes_supported": ["openid", "email", "profile"],
        "token_endpoint_auth_methods_supported": ["client_secret_basic"],
    })
}

#[tokio::test]
async fn sso_connection_test() {
    let good = discovery_server(good_doc).await;
    let bad = discovery_server(|i| {
        json!({"issuer": format!("{i}/elsewhere"), "authorization_endpoint": format!("{i}/a"),
                                          "response_types_supported": ["id_token"]})
    })
    .await;

    // Private addresses are refused by default.
    let strict = TestApp::new();
    let org = strict.org("acme");
    let (_, owner) = strict.member(org, "o@example.com", Role::Owner);
    let (_, admin) = strict.member(org, "a@example.com", Role::Admin);
    let body = json!({"issuer": good, "client_id": "rupta", "client_secret": "s"});
    assert_eq!(strict.post("/api/v1/orgs/acme/sso/test", &admin, body.clone()).await.status, StatusCode::FORBIDDEN);
    let r = strict.post("/api/v1/orgs/acme/sso/test", &owner, body.clone()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["ok"], false);
    assert!(r.body["problems"][0].as_str().unwrap().contains("private"), "{:?}", r.body);
    assert!(r.body["token_endpoint"].is_null());
    let r = strict.post("/api/v1/orgs/acme/sso/test", &owner, json!({"issuer": "ftp://x"})).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);

    // ... unless the issuer is the instance's own provider.
    let instance = TestApp::with_config(|c| {
        c.oidc = Some(rupta_auth::oidc::OidcSettings {
            id: "oidc".into(),
            name: "Instance".into(),
            issuer: good.clone(),
            client_id: "x".into(),
            client_secret: None,
            scopes: vec!["openid".into()],
            role_claim: None,
            role_mapping: vec![],
            default_role: Role::Member,
            jit_provisioning: true,
        })
    });
    let org = instance.org("acme");
    let (_, owner) = instance.member(org, "o@example.com", Role::Owner);
    let r = instance.post("/api/v1/orgs/acme/sso/test", &owner, body.clone()).await;
    assert_eq!(r.body["ok"], true, "{:?}", r.body);

    // ... or private issuers are allowed.
    let app = TestApp::with_config(|c| c.sso_allow_private_issuers = true);
    let org = app.org("acme");
    let (_, owner) = app.member(org, "o@example.com", Role::Owner);
    let r = app.post("/api/v1/orgs/acme/sso/test", &owner, body).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["ok"], true, "{:?}", r.body);
    assert_eq!(r.body["issuer"], good);
    assert_eq!(r.body["token_endpoint"], format!("{good}/token"));
    assert_eq!(r.body["userinfo_endpoint"], format!("{good}/userinfo"));
    assert_eq!(r.body["jwks_uri"], format!("{good}/jwks"));
    assert_eq!(r.body["scopes_supported"], json!(["openid", "email", "profile"]));
    assert_eq!(r.body["problems"], json!([]));
    // A public client needs the auth method `none`.
    let r = app.post("/api/v1/orgs/acme/sso/test", &owner, json!({"issuer": format!("{good}/")})).await;
    assert_eq!(r.body["ok"], false);
    assert_eq!(r.body["problems"].as_array().unwrap().len(), 1, "{:?}", r.body);

    let r = app.post("/api/v1/orgs/acme/sso/test", &owner, json!({"issuer": bad, "client_secret": "s"})).await;
    assert_eq!(r.body["ok"], false);
    let problems: Vec<String> =
        r.body["problems"].as_array().unwrap().iter().map(|p| p.as_str().unwrap().to_string()).collect();
    for want in ["differs", "token_endpoint is missing", "jwks_uri is missing", "response type code", "PKCE"] {
        assert!(problems.iter().any(|p| p.contains(want)), "{want}: {problems:?}");
    }
    // Nothing listens there.
    let r = app
        .post("/api/v1/orgs/acme/sso/test", &owner, json!({"issuer": "http://127.0.0.1:9", "client_secret": "s"}))
        .await;
    assert_eq!(r.body["ok"], false);
    assert!(r.body["problems"][0].as_str().unwrap().contains("failed"), "{:?}", r.body);
}

#[tokio::test]
async fn preferences_are_stored_and_validated() {
    let app = TestApp::new();
    let (_, who) = app.user("u@example.com");
    let r = app.get("/api/v1/user", &who).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(
        r.body["preferences"],
        json!({"timezone": null, "clock": "24h", "theme": "system",
               "notifications": {"issue_alerts": true, "workflow": true, "weekly_report": false}})
    );
    assert_eq!(r.body["email"], "u@example.com");
    let r = app
        .patch(
            "/api/v1/user",
            &who,
            json!({"preferences": {"timezone": "Europe/Berlin", "clock": "12h", "notifications": {"workflow": false}}}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["preferences"]["timezone"], "Europe/Berlin");
    assert_eq!(r.body["preferences"]["notifications"]["workflow"], false);
    assert_eq!(r.body["preferences"]["notifications"]["issue_alerts"], true);
    // Partial changes keep the rest.
    let r = app.patch("/api/v1/user", &who, json!({"preferences": {"theme": "dark"}})).await;
    assert_eq!(
        (r.body["preferences"]["theme"].as_str(), r.body["preferences"]["clock"].as_str()),
        (Some("dark"), Some("12h"))
    );
    for bad in [
        json!({"timezone": "Berlin/Europe"}),
        json!({"clock": "25h"}),
        json!({"theme": "blue"}),
        json!({"notifications": {"workflow": "yes"}}),
    ] {
        let r = app.patch("/api/v1/user", &who, json!({"preferences": bad, "name": "Changed"})).await;
        assert!(r.status.is_client_error(), "{bad}: {:?}", r.body);
    }
    let r = app.get("/api/v1/user", &who).await;
    assert_eq!(r.body["preferences"]["theme"], "dark");
    assert_ne!(r.body["name"], "Changed");
}

#[tokio::test]
async fn workflow_and_alert_emails_follow_preferences() {
    let (smtp, mails) = smtp_sink().await;
    let app = TestApp::with_config(|c| {
        c.smtp_url = Some(smtp.clone());
        c.smtp_from = Some("rupta <rupta@example.com>".into());
    });
    let org = app.org("acme");
    let project = app.project(org, "web");
    let (_, lead) = app.member(org, "lead@example.com", Role::Admin);
    let (dev_id, dev) = app.member(org, "dev@example.com", Role::Member);
    let (quiet_id, quiet) = app.member(org, "quiet@example.com", Role::Member);
    let issue = app.issue(project, "TypeError: boom", "error");
    let r = app
        .patch(
            "/api/v1/user",
            &quiet,
            json!({"preferences": {"notifications": {"workflow": false, "issue_alerts": false}}}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);

    // Assigned to dev by the lead: dev gets an email.
    let r = app.patch(&format!("/api/v1/issues/{issue}"), &lead, json!({"assignee": format!("user:{dev_id}")})).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let m = wait_for(|| mails.lock().unwrap().first().cloned()).await;
    assert_eq!(m.0, vec!["dev@example.com"]);
    assert!(m.1.contains("Assigned to you"), "{}", m.1);
    // A comment by the lead on dev's issue: dev gets an email; dev's own comment sends nothing.
    let r = app.post(&format!("/api/v1/issues/{issue}/comments"), &lead, json!({"text": "please look"})).await;
    assert_eq!(r.status, StatusCode::CREATED);
    let m = wait_for(|| mails.lock().unwrap().get(1).cloned()).await;
    assert_eq!(m.0, vec!["dev@example.com"]);
    assert!(m.1.contains("please look"), "{}", m.1);
    app.post(&format!("/api/v1/issues/{issue}/comments"), &dev, json!({"text": "on it"})).await;
    // Assigning yourself sends nothing, nor does assigning someone who opted out.
    app.patch(&format!("/api/v1/issues/{issue}"), &dev, json!({"assignee": "me"})).await;
    app.patch(&format!("/api/v1/issues/{issue}"), &lead, json!({"assignee": format!("user:{quiet_id}")})).await;
    app.post(&format!("/api/v1/issues/{issue}/comments"), &lead, json!({"text": "quiet one"})).await;

    // Alert emails skip users who turned issue alerts off.
    let rule = json!({"name": "new", "conditions": [{"type": "first_seen"}],
                      "actions": [{"type": "email", "user_ids": [dev_id.to_string(), quiet_id.to_string()]}]});
    assert_eq!(app.post("/api/v1/projects/acme/web/alert-rules", &lead, rule).await.status, StatusCode::CREATED);
    app.event(project, issue, Utc::now(), json!({}));
    rupta_api::alerts::handle(&app.state, &IssueSignal::New { project_id: project, issue_id: issue }).await.unwrap();
    let m = wait_for(|| mails.lock().unwrap().iter().find(|m| m.1.contains("New issue")).cloned()).await;
    assert_eq!(m.0, vec!["dev@example.com"]);
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let all = mails.lock().unwrap().clone();
    assert!(all.iter().all(|m| m.0 != vec!["quiet@example.com"]), "{all:?}");
    assert_eq!(all.len(), 3, "{all:?}");
}

#[tokio::test]
async fn resolve_in_next_release_and_ignore_until_users() {
    let app = TestApp::new();
    let org = app.org("acme");
    let project = app.project(org, "web");
    let (_, who) = app.member(org, "m@example.com", Role::Member);
    let conn = app.conn();
    for (v, age) in [("web@1.0", 2), ("web@1.1", 1)] {
        let rid: i64 = conn
            .query_one(
                "INSERT INTO releases (org_id, version, created_at) VALUES ($1, $2, $3) RETURNING id",
                &[&org, &v, &(Utc::now() - Duration::days(age))],
            )
            .unwrap()
            .get(0);
        conn.execute("INSERT INTO release_projects (release_id, project_id) VALUES ($1, $2)", &[&rid, &project])
            .unwrap();
    }
    let a = app.issue(project, "a", "error");
    let b = app.issue(project, "b", "error");
    let r = app
        .put(
            &format!("/api/v1/orgs/acme/issues?id={a}"),
            &who,
            json!({"status": "resolved", "substatus": "in_next_release"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = app.get(&format!("/api/v1/issues/{a}"), &who).await;
    assert_eq!(r.body["status"], "resolved");
    assert_eq!(r.body["substatus"], "in_next_release");
    assert_eq!(r.body["resolved_in_release"], "web@1.1");
    assert_eq!(r.body["resolved_in_mode"], "next_release");
    let r = app
        .patch(&format!("/api/v1/issues/{b}"), &who, json!({"status": "resolved", "resolved_in": "next_release"}))
        .await;
    assert_eq!(r.body["resolved_in_mode"], "next_release");
    let r = app
        .patch(&format!("/api/v1/issues/{b}"), &who, json!({"status": "resolved", "resolved_in_release": "web@1.0"}))
        .await;
    assert_eq!(
        (r.body["resolved_in_release"].as_str(), r.body["resolved_in_mode"].as_str()),
        (Some("web@1.0"), Some("release"))
    );
    let r = app
        .patch(&format!("/api/v1/issues/{b}"), &who, json!({"status": "resolved", "resolved_in": "last_release"}))
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = app
        .patch(
            &format!("/api/v1/issues/{b}"),
            &who,
            json!({"status": "resolved", "resolved_in": "next_release", "resolved_in_release": "x"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);

    // Ignore until 10 more users: an absolute threshold; with a window: the number itself.
    conn.execute("UPDATE issues SET users_seen = 7 WHERE id = $1", &[&b]).unwrap();
    let r =
        app.patch(&format!("/api/v1/issues/{b}"), &who, json!({"status": "ignored", "ignore_user_count": 10})).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        (r.body["ignore_user_count"].as_i64(), r.body["substatus"].as_str()),
        (Some(17), Some("archived_until_condition_met"))
    );
    assert!(r.body["resolved_in_mode"].is_null());
    let r = app
        .patch(
            &format!("/api/v1/issues/{b}"),
            &who,
            json!({"status": "ignored", "ignore_user_count": 10, "ignore_user_window": 60}),
        )
        .await;
    assert_eq!((r.body["ignore_user_count"].as_i64(), r.body["ignore_user_window"].as_i64()), (Some(10), Some(60)));
    for bad in
        [json!({"status": "ignored", "ignore_user_count": 0}), json!({"status": "ignored", "ignore_user_window": 60})]
    {
        assert_eq!(app.patch(&format!("/api/v1/issues/{b}"), &who, bad).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    }
    // Unresolving clears the conditions.
    let r = app.patch(&format!("/api/v1/issues/{b}"), &who, json!({"status": "unresolved"})).await;
    assert!(r.body["ignore_user_count"].is_null() && r.body["ignore_user_window"].is_null());
}

#[tokio::test]
async fn escalating_alert_condition() {
    let app = TestApp::new();
    let org = app.org("acme");
    let project = app.project(org, "web");
    let (_, admin) = app.member(org, "admin@example.com", Role::Admin);
    let (addr, got) = {
        let got: std::sync::Arc<std::sync::Mutex<Vec<Value>>> = Default::default();
        let g = got.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let router = axum::Router::new().route(
            "/hook",
            axum::routing::post(move |axum::Json(v): axum::Json<Value>| {
                let g = g.clone();
                async move {
                    g.lock().unwrap().push(v);
                    StatusCode::OK
                }
            }),
        );
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        (addr, got)
    };
    let ch = app
        .post(
            "/api/v1/orgs/acme/channels",
            &admin,
            json!({"kind": "webhook", "name": "hook", "config": {"url": format!("http://{addr}/hook")}}),
        )
        .await;
    assert_eq!(ch.status, StatusCode::CREATED, "{:?}", ch.body);
    let rule = json!({"name": "escalation", "conditions": [{"type": "escalating"}],
                      "actions": [{"type": "channel", "channel_id": ch.body["id"]}]});
    let r = app.post("/api/v1/projects/acme/web/alert-rules", &admin, rule).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let issue = app.issue(project, "spike", "error");
    app.event(project, issue, Utc::now(), json!({}));
    // Other signals do not match the condition.
    rupta_api::alerts::handle(&app.state, &IssueSignal::New { project_id: project, issue_id: issue }).await.unwrap();
    assert!(got.lock().unwrap().is_empty());
    rupta_api::alerts::handle(&app.state, &IssueSignal::Escalating { project_id: project, issue_id: issue })
        .await
        .unwrap();
    let body = wait_for(|| got.lock().unwrap().first().cloned()).await;
    assert_eq!(body["trigger"], "escalating");
}
