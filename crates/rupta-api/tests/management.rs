mod common;

use axum::http::{Method, StatusCode};
use chrono::{Duration, Utc};
use common::{As, TestApp};
use rupta_core::role::Role;
use serde_json::{Value, json};

/// Every role against representative endpoints: status codes per role.
#[tokio::test]
async fn rbac_matrix_over_http() {
    let app = TestApp::new();
    let org = app.org("acme");
    app.project(org, "web");
    let roles = [Role::Viewer, Role::Member, Role::Admin, Role::Owner];
    let mut who = Vec::new();
    for r in roles {
        who.push(app.member(org, &format!("{}@example.com", r.as_str()), r).1);
    }
    let other = app.org("other");
    let (_, outsider) = app.member(other, "outsider@example.com", Role::Owner);
    let issue = app.issue(
        app.conn().query_one("SELECT id FROM projects WHERE slug = 'web'", &[]).unwrap().get(0),
        "Boom",
        "error",
    );

    // (method, path, body, expected status for viewer, member, admin, owner)
    let cases: Vec<(Method, String, Value, [u16; 4])> = vec![
        (Method::GET, "/api/v1/orgs/acme".into(), json!(null), [200, 200, 200, 200]),
        (Method::GET, "/api/v1/orgs/acme/projects".into(), json!(null), [200, 200, 200, 200]),
        (Method::GET, "/api/v1/orgs/acme/members".into(), json!(null), [200, 200, 200, 200]),
        (Method::GET, "/api/v1/orgs/acme/issues".into(), json!(null), [200, 200, 200, 200]),
        (Method::PATCH, format!("/api/v1/issues/{issue}"), json!({"priority": "low"}), [403, 200, 200, 200]),
        (Method::POST, "/api/v1/orgs/acme/teams".into(), json!({"name": "T"}), [403, 403, 201, 409]),
        (Method::PATCH, "/api/v1/projects/acme/web".into(), json!({"name": "Web"}), [403, 403, 200, 200]),
        (Method::GET, "/api/v1/orgs/acme/audit-log".into(), json!(null), [403, 403, 200, 200]),
        (Method::GET, "/api/v1/orgs/acme/tokens".into(), json!(null), [403, 403, 200, 200]),
        (Method::GET, "/api/v1/orgs/acme/channels".into(), json!(null), [403, 403, 200, 200]),
        (Method::GET, "/api/v1/orgs/acme/sso".into(), json!(null), [403, 403, 403, 404]),
        (Method::PATCH, "/api/v1/orgs/acme".into(), json!({"name": "Acme 2"}), [403, 403, 200, 200]),
    ];
    for (method, path, body, expected) in &cases {
        for (i, w) in who.iter().enumerate() {
            let b = if body.is_null() { None } else { Some(body.clone()) };
            let r = app.request(method.clone(), path, w, b).await;
            assert_eq!(r.status.as_u16(), expected[i], "{method} {path} as {:?}: {:?}", roles[i], r.body);
        }
        let b = if body.is_null() { None } else { Some(body.clone()) };
        let r = app.request(method.clone(), path, &outsider, b).await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "{method} {path} as outsider");
        let r = app.request(method.clone(), path, &As::anonymous(), None).await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{method} {path} anonymous");
    }
    // Only owners delete organizations.
    assert_eq!(app.delete("/api/v1/orgs/acme", &who[2]).await.status, StatusCode::FORBIDDEN);
    assert_eq!(app.delete("/api/v1/orgs/acme", &who[3]).await.status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn orgs_members_invitations_and_teams() {
    let app = TestApp::new();
    let (uid, owner) = app.user("owner@example.com");
    let r = app.post("/api/v1/orgs", &owner, json!({"name": "Acme Corp"})).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["slug"], "acme-corp");
    assert_eq!(r.body["role"], "owner");
    assert_eq!(
        app.post("/api/v1/orgs", &owner, json!({"name": "x", "slug": "acme-corp"})).await.status,
        StatusCode::CONFLICT
    );
    assert_eq!(
        app.post("/api/v1/orgs", &owner, json!({"name": "x", "slug": "Bad Slug"})).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let list = app.get("/api/v1/orgs", &owner).await;
    assert_eq!(list.body.as_array().unwrap().len(), 1);

    // Invite, then accept by creating an account.
    let inv =
        app.post("/api/v1/orgs/acme-corp/members", &owner, json!({"email": "New@Example.com", "role": "member"})).await;
    assert_eq!(inv.status, StatusCode::CREATED, "{:?}", inv.body);
    assert_eq!(inv.body["email_sent"], false);
    let url = inv.body["accept_url"].as_str().unwrap();
    assert!(url.starts_with("http://rupta.test/accept-invite/"));
    let token = url.rsplit('/').next().unwrap();
    let info = app.get(&format!("/api/v1/invites/{token}"), &As::anonymous()).await;
    assert_eq!(info.body["email"], "new@example.com");
    assert_eq!(info.body["user_exists"], false);
    let acc = app
        .post(
            &format!("/api/v1/invites/{token}/accept"),
            &As::anonymous(),
            json!({"name": "New", "password": "a long password"}),
        )
        .await;
    assert_eq!(acc.status, StatusCode::OK, "{:?}", acc.body);
    assert_eq!(acc.body["orgs"][0]["role"], "member");
    assert_eq!(
        app.get(&format!("/api/v1/invites/{token}"), &As::anonymous()).await.status,
        StatusCode::NOT_FOUND,
        "used once"
    );
    let new_uid: i64 = acc.body["user"]["id"].as_str().unwrap().parse().unwrap();

    // Accepting as a signed-in user.
    let inv =
        app.post("/api/v1/orgs/acme-corp/members", &owner, json!({"email": "x@example.com", "role": "viewer"})).await;
    let token = inv.body["accept_url"].as_str().unwrap().rsplit('/').next().unwrap().to_string();
    let (_, x) = app.user("x@example.com");
    let acc = app.post(&format!("/api/v1/invites/{token}/accept"), &x, json!({})).await;
    assert_eq!(acc.status, StatusCode::OK, "{:?}", acc.body);

    let members = app.get("/api/v1/orgs/acme-corp/members?limit=2", &owner).await;
    assert_eq!(members.body["data"].as_array().unwrap().len(), 2);
    let next = members.body["next_cursor"].as_str().unwrap();
    let rest = app.get(&format!("/api/v1/orgs/acme-corp/members?limit=2&cursor={next}"), &owner).await;
    assert_eq!(rest.body["data"].as_array().unwrap().len(), 1);
    assert!(rest.body["next_cursor"].is_null());

    // Roles: the last owner stays.
    assert_eq!(
        app.patch(&format!("/api/v1/orgs/acme-corp/members/{uid}"), &owner, json!({"role": "admin"})).await.status,
        StatusCode::CONFLICT
    );
    assert_eq!(app.delete(&format!("/api/v1/orgs/acme-corp/members/{uid}"), &owner).await.status, StatusCode::CONFLICT);
    let r = app.patch(&format!("/api/v1/orgs/acme-corp/members/{new_uid}"), &owner, json!({"role": "admin"})).await;
    assert_eq!(r.body["role"], "admin");
    // Admins cannot make owners.
    let (_, admin) = app.user("new@example.com");
    assert_eq!(
        app.patch(&format!("/api/v1/orgs/acme-corp/members/{uid}"), &admin, json!({"role": "member"})).await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.post("/api/v1/orgs/acme-corp/members", &admin, json!({"email": "o2@example.com", "role": "owner"}))
            .await
            .status,
        StatusCode::FORBIDDEN
    );

    // Teams.
    let t = app.post("/api/v1/orgs/acme-corp/teams", &admin, json!({"name": "Backend"})).await;
    assert_eq!(t.status, StatusCode::CREATED);
    let t = app.put(&format!("/api/v1/orgs/acme-corp/teams/backend/members/{new_uid}"), &admin, json!({})).await;
    assert_eq!(t.body["members"][0]["email"], "new@example.com");
    let other = app.user("stranger@example.com").0;
    assert_eq!(
        app.put(&format!("/api/v1/orgs/acme-corp/teams/backend/members/{other}"), &admin, json!({})).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let r = app
        .patch("/api/v1/orgs/acme-corp/teams/backend", &admin, json!({"name": "Platform", "slug": "platform"}))
        .await;
    assert_eq!(r.body["slug"], "platform");
    assert_eq!(
        app.delete(&format!("/api/v1/orgs/acme-corp/teams/platform/members/{new_uid}"), &admin).await.status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(app.delete("/api/v1/orgs/acme-corp/teams/platform", &admin).await.status, StatusCode::NO_CONTENT);

    // Leaving and removal.
    assert_eq!(
        app.delete(&format!("/api/v1/orgs/acme-corp/members/{new_uid}"), &admin).await.status,
        StatusCode::NO_CONTENT
    );
    let audit = app.get("/api/v1/orgs/acme-corp/audit-log", &owner).await;
    let actions: Vec<&str> =
        audit.body["data"].as_array().unwrap().iter().map(|e| e["action"].as_str().unwrap()).collect();
    for a in ["org.create", "member.invite", "member.join", "member.role_change", "team.create", "member.remove"] {
        assert!(actions.contains(&a), "{a} in {actions:?}");
    }
    let filtered = app.get("/api/v1/orgs/acme-corp/audit-log?action=member.join", &owner).await;
    assert!(filtered.body["data"].as_array().unwrap().iter().all(|e| e["action"] == "member.join"));
}

#[tokio::test]
async fn tokens_are_shown_once_scoped_and_revocable() {
    let app = TestApp::new();
    let org = app.org("acme");
    let (_, admin) = app.member(org, "admin@example.com", Role::Admin);
    app.project(org, "web");
    let r = app
        .post("/api/v1/orgs/acme/tokens", &admin, json!({"name": "CI", "scopes": ["project:releases", "project:read"]}))
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let token = r.body["token"].as_str().unwrap().to_string();
    assert!(token.starts_with("rpt_") && token.len() == 44);
    let list = app.get("/api/v1/orgs/acme/tokens", &admin).await;
    assert!(list.body[0].get("token").is_none(), "never listed");
    assert_eq!(list.body[0]["token_hint"], &token[token.len() - 4..]);
    let id = list.body[0]["id"].as_str().unwrap().to_string();

    let t = As::token(&token);
    assert_eq!(app.get("/api/v1/projects/acme/web", &t).await.status, StatusCode::OK);
    assert_eq!(app.patch("/api/v1/projects/acme/web", &t, json!({"name": "x"})).await.status, StatusCode::FORBIDDEN);
    assert_eq!(
        app.post("/api/v1/orgs/acme/tokens", &t, json!({"name": "x", "scopes": ["org:read"]})).await.status,
        StatusCode::FORBIDDEN,
        "tokens cannot mint tokens"
    );
    assert_eq!(
        app.post("/api/v1/orgs/acme/tokens", &admin, json!({"name": "x", "scopes": ["root"]})).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        app.post("/api/v1/orgs/acme/tokens", &admin, json!({"name": "x", "scopes": []})).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    assert_eq!(app.delete(&format!("/api/v1/orgs/acme/tokens/{id}"), &admin).await.status, StatusCode::NO_CONTENT);
    assert_eq!(app.get("/api/v1/projects/acme/web", &t).await.status, StatusCode::UNAUTHORIZED);

    let r = app
        .post("/api/v1/user/tokens", &admin, json!({"name": "Mine", "scopes": ["org:read"], "expires_in_days": 30}))
        .await;
    assert_eq!(r.status, StatusCode::CREATED);
    assert!(r.body["expires_at"].is_string());
    let mine = app.get("/api/v1/user/tokens", &admin).await;
    assert_eq!(mine.body.as_array().unwrap().len(), 1);
    let (_, other) = app.user("other@example.com");
    let tid = mine.body[0]["id"].as_str().unwrap();
    assert_eq!(app.delete(&format!("/api/v1/user/tokens/{tid}"), &other).await.status, StatusCode::NOT_FOUND);
    assert_eq!(app.delete(&format!("/api/v1/user/tokens/{tid}"), &admin).await.status, StatusCode::NO_CONTENT);
    let actions = app.audit_actions();
    assert!(actions.contains(&"token.create".into()) && actions.contains(&"token.delete".into()));
    assert_eq!(app.get("/api/v1/scopes", &admin).await.body.as_array().unwrap().len(), rupta_auth::rbac::SCOPES.len());
}

#[tokio::test]
async fn user_profile_and_password_change() {
    let app = TestApp::new();
    app.user_with_password("ada@example.com", "old password 123");
    let login = app
        .post(
            "/api/v1/auth/login",
            &As::anonymous(),
            json!({"email": "ada@example.com", "password": "old password 123"}),
        )
        .await;
    let cookie = login
        .headers
        .get_all("set-cookie")
        .iter()
        .map(|v| v.to_str().unwrap())
        .find(|c| c.starts_with("rupta_session="))
        .unwrap();
    let who = As {
        cookie: Some(cookie.split(';').next().unwrap()[14..].to_string()),
        csrf: login.body["csrf_token"].as_str().map(str::to_string),
        token: None,
    };
    let (_, second) = app.user("ada@example.com");
    let r = app.patch("/api/v1/user", &who, json!({"name": "Ada L."})).await;
    assert_eq!(r.body["name"], "Ada L.");
    assert_eq!(
        app.patch("/api/v1/user", &who, json!({"current_password": "wrong", "new_password": "new password 456"}))
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        app.patch(
            "/api/v1/user",
            &who,
            json!({"current_password": "old password 123", "new_password": "new password 456"})
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(app.get("/api/v1/user", &second).await.status, StatusCode::UNAUTHORIZED, "other sessions end");
    assert_eq!(app.get("/api/v1/user", &who).await.status, StatusCode::OK, "this session stays");
    let r = app
        .post(
            "/api/v1/auth/login",
            &As::anonymous(),
            json!({"email": "ada@example.com", "password": "new password 456"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
}

#[tokio::test]
async fn projects_keys_settings_and_stats() {
    let app = TestApp::new();
    let org = app.org("acme");
    let (_, admin) = app.member(org, "admin@example.com", Role::Admin);
    let gen0 = app.state.store.config_generation();
    let r = app
        .post("/api/v1/orgs/acme/projects", &admin, json!({"name": "Web Shop", "platform": "javascript-react"}))
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["slug"], "web-shop");
    let pid = r.body["id"].as_str().unwrap().to_string();
    let dsn = r.body["dsn"].as_str().unwrap();
    assert!(dsn.starts_with("http://") && dsn.ends_with(&format!("@rupta.test/{pid}")), "{dsn}");
    assert_eq!(r.body["settings"]["scrubbing"]["enabled"], true);
    assert!(app.state.store.config_generation() > gen0, "ingestion is told to reload");

    let keys = app.get("/api/v1/projects/acme/web-shop/keys", &admin).await;
    assert_eq!(keys.body.as_array().unwrap().len(), 1);
    let k = app
        .post("/api/v1/projects/acme/web-shop/keys", &admin, json!({"label": "Backend", "rate_limit_per_min": 100}))
        .await;
    assert_eq!(k.status, StatusCode::CREATED);
    assert_eq!(k.body["public_key"].as_str().unwrap().len(), 32);
    let kid = k.body["id"].as_str().unwrap();
    let k2 = app
        .patch(
            &format!("/api/v1/projects/acme/web-shop/keys/{kid}"),
            &admin,
            json!({"is_active": false, "rate_limit_per_min": null}),
        )
        .await;
    assert_eq!(k2.body["is_active"], false);
    assert!(k2.body["rate_limit_per_min"].is_null());
    assert_eq!(
        app.delete(&format!("/api/v1/projects/acme/web-shop/keys/{kid}"), &admin).await.status,
        StatusCode::NO_CONTENT
    );

    let s = app
        .patch(
            "/api/v1/projects/acme/web-shop",
            &admin,
            json!({"settings": {"retention_days": 30, "filters": {"localhost": true}}}),
        )
        .await;
    assert_eq!(s.status, StatusCode::OK, "{:?}", s.body);
    assert_eq!(s.body["settings"]["retention_days"], 30);
    assert_eq!(s.body["settings"]["filters"]["localhost"], true);
    assert_eq!(s.body["settings"]["filters"]["web_crawlers"], false);
    assert_eq!(
        app.patch("/api/v1/projects/acme/web-shop", &admin, json!({"settings": {"retention_days": -1}})).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    app.post("/api/v1/orgs/acme/teams", &admin, json!({"name": "Front"})).await;
    let s = app.patch("/api/v1/projects/acme/web-shop", &admin, json!({"team": "front"})).await;
    assert_eq!(s.body["team"]["slug"], "front");

    // Usage statistics from outcomes.
    let pid: i64 = pid.parse().unwrap();
    let hour = Utc::now() - Duration::hours(2);
    let conn = app.conn();
    conn.execute("INSERT INTO outcomes (project_id, hour, category, outcome, quantity) VALUES ($1, date_trunc('hour', $2::timestamptz), 'error', 'accepted', 7)", &[&pid, &hour]).unwrap();
    conn.execute("INSERT INTO outcomes (project_id, hour, category, outcome, reason, quantity) VALUES ($1, date_trunc('hour', $2::timestamptz), 'error', 'rate_limited', 'key', 3)", &[&pid, &hour]).unwrap();
    let st = app.get("/api/v1/projects/acme/web-shop/stats?stat=received", &admin).await;
    assert_eq!(st.status, StatusCode::OK, "{:?}", st.body);
    assert_eq!(st.body.as_array().unwrap().iter().map(|p| p[1].as_i64().unwrap()).sum::<i64>(), 7);
    let st = app.get("/api/v1/projects/acme/web-shop/stats?stat=rejected&resolution=1d", &admin).await;
    assert_eq!(st.body.as_array().unwrap().iter().map(|p| p[1].as_i64().unwrap()).sum::<i64>(), 3);

    let gen1 = app.state.store.config_generation();
    assert_eq!(app.delete("/api/v1/projects/acme/web-shop", &admin).await.status, StatusCode::NO_CONTENT);
    assert!(app.state.store.config_generation() > gen1);
    assert_eq!(app.get("/api/v1/projects/acme/web-shop", &admin).await.status, StatusCode::NOT_FOUND);
    let n: i64 = conn.query_one("SELECT count(*) FROM outcomes WHERE project_id = $1", &[&pid]).unwrap().get(0);
    assert_eq!(n, 0);
    assert!(app.audit_actions().contains(&"project.delete".into()));
}

#[tokio::test]
async fn releases_and_health() {
    let app = TestApp::new();
    let org = app.org("acme");
    let pid = app.project(org, "web");
    let (_, who) = app.member(org, "v@example.com", Role::Viewer);
    let conn = app.conn();
    for v in ["web@1.0.0", "web@1.1.0"] {
        let rid: i64 = conn
            .query_one("INSERT INTO releases (org_id, version) VALUES ($1, $2) RETURNING id", &[&org, &v])
            .unwrap()
            .get(0);
        conn.execute(
            "INSERT INTO release_projects (release_id, project_id, new_issues) VALUES ($1, $2, 2)",
            &[&rid, &pid],
        )
        .unwrap();
    }
    conn.execute("INSERT INTO deploys (release_id, environment, name) SELECT id, 'production', 'deploy 1' FROM releases WHERE version = 'web@1.1.0'", &[]).unwrap();
    let hour = Utc::now() - Duration::hours(1);
    conn.execute("INSERT INTO session_counts (project_id, release, environment, hour, total, crashed, errored) VALUES ($1, 'web@1.1.0', 'production', date_trunc('hour', $2::timestamptz), 200, 4, 10)", &[&pid, &hour]).unwrap();
    conn.execute("INSERT INTO session_counts (project_id, release, environment, hour, total, crashed) VALUES ($1, 'web@1.0.0', 'production', date_trunc('hour', $2::timestamptz), 50, 0)", &[&pid, &hour]).unwrap();
    // Four users of 1.1.0, one of them crashed (it has two rows: before and after the crash).
    for (user, crashed) in [(1, false), (2, false), (3, false), (4, false), (4, true)] {
        conn.execute("INSERT INTO session_users (project_id, release, environment, hour, user_hash, crashed) VALUES ($1, 'web@1.1.0', 'production', date_trunc('hour', $2::timestamptz), $3, $4)", &[&pid, &hour, &(user as i64), &crashed]).unwrap();
    }
    conn.execute("INSERT INTO session_users (project_id, release, environment, hour, user_hash, crashed) VALUES ($1, 'web@1.0.0', 'staging', date_trunc('hour', $2::timestamptz), 9, false)", &[&pid, &hour]).unwrap();

    let list = app.get("/api/v1/orgs/acme/releases", &who).await;
    assert_eq!(list.status, StatusCode::OK, "{:?}", list.body);
    let first = &list.body["data"][0];
    assert_eq!(first["version"], "web@1.1.0");
    assert_eq!(first["short_version"], "1.1.0");
    assert_eq!(first["health"]["crash_free_sessions"], 98.0);
    assert_eq!(first["health"]["adoption"], 80.0);
    assert_eq!(first["health"]["crash_free_users"], 75.0);
    assert_eq!(
        (first["health"]["total_users"].as_i64(), first["health"]["crashed_users"].as_i64()),
        (Some(4), Some(1))
    );
    assert_eq!(first["last_deploy"]["environment"], "production");
    assert_eq!(first["new_issues"], 2);
    // Projects come with their platform, for the platform icons.
    assert_eq!(first["projects"][0]["platform"], "javascript", "{:?}", first["projects"]);
    let page = app.get("/api/v1/orgs/acme/releases?limit=1", &who).await;
    assert!(page.body["next_cursor"].is_string());
    let q = app.get("/api/v1/orgs/acme/releases?query=1.0.0", &who).await;
    assert_eq!(q.body["data"].as_array().unwrap().len(), 1);

    let d = app.get("/api/v1/orgs/acme/releases/web%401.1.0", &who).await;
    assert_eq!(d.status, StatusCode::OK, "{:?}", d.body);
    assert_eq!(d.body["deploys"].as_array().unwrap().len(), 1);
    assert_eq!(d.body["project_health"][0]["health"]["total_sessions"], 200);
    assert_eq!(d.body["sessions"].as_array().unwrap().iter().map(|p| p[1].as_i64().unwrap()).sum::<i64>(), 200);
    assert_eq!(app.get("/api/v1/orgs/acme/releases/nope", &who).await.status, StatusCode::NOT_FOUND);

    let h = app.get("/api/v1/projects/acme/web/health?period=24h", &who).await;
    assert_eq!(h.status, StatusCode::OK, "{:?}", h.body);
    assert_eq!(h.body["health"]["total_sessions"], 250);
    assert_eq!(h.body["health"]["crash_free_sessions"], 98.4);
    assert_eq!(h.body["releases"][0]["version"], "web@1.1.0");
    assert_eq!(h.body["releases"][0]["health"]["adoption"], 80.0);
    assert_eq!(h.body["series"].as_array().unwrap().len(), 24);
    // Five distinct users over both releases, one crashed.
    assert_eq!(h.body["health"]["total_users"], 5);
    assert_eq!(h.body["health"]["crash_free_users"], 80.0);
    assert_eq!(h.body["releases"][0]["health"]["crash_free_users"], 75.0);
    let series = h.body["series"].as_array().unwrap();
    assert_eq!(series.iter().map(|p| p[3].as_i64().unwrap()).sum::<i64>(), 5);
    assert_eq!(series.iter().map(|p| p[4].as_i64().unwrap()).sum::<i64>(), 1);
    let prod = app.get("/api/v1/projects/acme/web/health?period=24h&environment=production", &who).await;
    assert_eq!(prod.body["health"]["total_users"], 4);
    // Longer periods count approximately; small sets are still exact.
    let long = app.get("/api/v1/projects/acme/web/health?period=90d", &who).await;
    assert_eq!(long.status, StatusCode::OK, "{:?}", long.body);
    assert_eq!(long.body["health"]["total_users"], 5);
    assert_eq!(d.body["project_health"][0]["health"]["crash_free_users"], 75.0);
    assert_eq!(d.body["sessions"].as_array().unwrap().iter().map(|p| p[3].as_i64().unwrap()).sum::<i64>(), 4);
}

#[tokio::test]
async fn monitors_crud_with_checkins() {
    let app = TestApp::new();
    let org = app.org("acme");
    app.project(org, "web");
    let (_, admin) = app.member(org, "admin@example.com", Role::Admin);
    let (_, viewer) = app.member(org, "viewer@example.com", Role::Viewer);
    let body = json!({"name": "Nightly backup", "schedule": {"type": "crontab", "value": "0 3 * * *"}, "checkin_margin": 5, "timezone": "Europe/Berlin"});
    assert_eq!(
        app.post("/api/v1/projects/acme/web/monitors", &viewer, body.clone()).await.status,
        StatusCode::FORBIDDEN
    );
    let r = app.post("/api/v1/projects/acme/web/monitors", &admin, body.clone()).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["slug"], "nightly-backup");
    assert_eq!(app.post("/api/v1/projects/acme/web/monitors", &admin, body).await.status, StatusCode::CONFLICT);
    let bad = json!({"name": "x", "schedule": {"type": "crontab", "value": "every day"}});
    assert_eq!(
        app.post("/api/v1/projects/acme/web/monitors", &admin, bad).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let mid: i64 = r.body["id"].as_str().unwrap().parse().unwrap();
    let conn = app.conn();
    for (i, status) in ["ok", "error", "ok"].iter().enumerate() {
        let id = uuid::Uuid::new_v4();
        let at = Utc::now() - Duration::minutes(10 - i as i64);
        conn.execute("INSERT INTO checkins (id, monitor_id, status, duration_ms, created_at, updated_at) VALUES ($1, $2, $3, 1200, $4, $4)", &[id.as_bytes(), &mid, status, &at]).unwrap();
    }
    let d = app.get("/api/v1/projects/acme/web/monitors/nightly-backup", &viewer).await;
    assert_eq!(d.body["checkins"].as_array().unwrap().len(), 3);
    assert_eq!(d.body["checkins"][0]["status"], "ok");
    let page = app.get("/api/v1/projects/acme/web/monitors/nightly-backup/checkins?limit=2", &viewer).await;
    assert_eq!(page.body["data"].as_array().unwrap().len(), 2);
    let next = page.body["next_cursor"].as_str().unwrap();
    let rest = app
        .get(&format!("/api/v1/projects/acme/web/monitors/nightly-backup/checkins?limit=2&cursor={next}"), &viewer)
        .await;
    assert_eq!(rest.body["data"][0]["status"], "ok");
    let u = app
        .patch(
            "/api/v1/projects/acme/web/monitors/nightly-backup",
            &admin,
            json!({"status": "disabled", "schedule": {"type": "interval", "value": 2, "unit": "hour"}}),
        )
        .await;
    assert_eq!(u.body["status"], "disabled");
    assert_eq!(u.body["schedule"]["unit"], "hour");
    assert_eq!(app.get("/api/v1/projects/acme/web/monitors", &viewer).await.body.as_array().unwrap().len(), 1);
    assert_eq!(
        app.delete("/api/v1/projects/acme/web/monitors/nightly-backup", &admin).await.status,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn bootstrap_from_environment_runs_once() {
    let app = TestApp::new();
    let id =
        rupta_api::bootstrap::admin_from_env(&app.state, "Root@Example.com", "a long password", "main").await.unwrap();
    assert!(id.is_some());
    assert_eq!(
        rupta_api::bootstrap::admin_from_env(&app.state, "other@example.com", "a long password", "main").await.unwrap(),
        None
    );
    let r = app
        .post(
            "/api/v1/auth/login",
            &As::anonymous(),
            json!({"email": "root@example.com", "password": "a long password"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["orgs"][0]["slug"], "main");
    assert_eq!(r.body["orgs"][0]["role"], "owner");
    assert_eq!(r.body["user"]["is_superuser"], true);
    assert!(
        rupta_api::bootstrap::admin_from_env(&TestApp::new().state, "x@example.com", "short", "main").await.is_err()
    );
    assert!(
        rupta_api::bootstrap::admin_from_env(&TestApp::new().state, "x@example.com", "a long password", "Bad Org")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn maintenance_removes_expired_sessions_and_invitations() {
    let app = TestApp::new();
    let org = app.org("acme");
    let (uid, who) = app.user("a@example.com");
    let past = Utc::now() - Duration::hours(1);
    let conn = app.conn();
    conn.execute("UPDATE web_sessions SET expires_at = $1", &[&past]).unwrap();
    conn.execute(
        "INSERT INTO invitations (org_id, email, role, token_hash, invited_by, expires_at) VALUES ($1, 'x@example.com', 'member', 'h', $2, $3)",
        &[&org, &uid, &past],
    )
    .unwrap();
    assert_eq!(rupta_api::maintenance::run_once(&app.state).await.unwrap(), 2);
    assert_eq!(app.get("/api/v1/auth/session", &who).await.status, StatusCode::UNAUTHORIZED);
}
