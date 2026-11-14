mod common;

use axum::http::{StatusCode, header};
use common::{As, TestApp};
use rupta_core::role::Role;
use serde_json::json;

fn session_cookie(resp: &common::Resp) -> String {
    let c = resp
        .headers
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap().to_string())
        .find(|v| v.starts_with("rupta_session="))
        .expect("session cookie");
    c.split(';').next().unwrap().trim_start_matches("rupta_session=").to_string()
}

#[tokio::test]
async fn setup_works_once_and_signs_in() {
    let app = TestApp::new();
    let cfg = app.get("/api/v1/auth/config", &As::anonymous()).await;
    assert_eq!(cfg.body, json!({"local_login": true, "sso": [], "setup_required": true}));

    let r = app
        .post(
            "/api/v1/auth/setup",
            &As::anonymous(),
            json!({"email": "Admin@Example.com", "name": "Admin", "password": "a long password", "org_name": "Acme Corp"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["user"]["email"], "admin@example.com");
    assert_eq!(r.body["user"]["is_superuser"], true);
    assert_eq!(r.body["orgs"][0]["slug"], "acme-corp");
    assert_eq!(r.body["orgs"][0]["role"], "owner");
    let set_cookie = r.headers.get(header::SET_COOKIE).unwrap().to_str().unwrap();
    assert!(set_cookie.contains("HttpOnly") && set_cookie.contains("SameSite=Lax") && !set_cookie.contains("Secure"));

    let who =
        As { cookie: Some(session_cookie(&r)), csrf: r.body["csrf_token"].as_str().map(str::to_string), token: None };
    let s = app.get("/api/v1/auth/session", &who).await;
    assert_eq!(s.status, StatusCode::OK);
    assert_eq!(s.body["user"]["email"], "admin@example.com");

    let again = app
        .post(
            "/api/v1/auth/setup",
            &As::anonymous(),
            json!({"email": "x@example.com", "password": "a long password", "org_name": "X"}),
        )
        .await;
    assert_eq!(again.status, StatusCode::CONFLICT);
    assert_eq!(app.get("/api/v1/auth/config", &As::anonymous()).await.body["setup_required"], false);
    assert!(app.audit_actions().contains(&"auth.setup".to_string()));
}

#[tokio::test]
async fn setup_validates_input() {
    let app = TestApp::new();
    for body in [
        json!({"email": "nope", "password": "a long password", "org_name": "X"}),
        json!({"email": "a@x.org", "password": "short", "org_name": "X"}),
        json!({"email": "a@x.org", "password": "a long password", "org_name": ""}),
    ] {
        let r = app.post("/api/v1/auth/setup", &As::anonymous(), body).await;
        assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(r.code(), "validation_failed");
    }
    let r = app.request(axum::http::Method::POST, "/api/v1/auth/setup", &As::anonymous(), None).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn login_logout_and_session_cookies() {
    let app = TestApp::with_config(|c| c.public_url = "https://errors.example.com".into());
    app.user_with_password("ada@example.com", "correct horse battery");
    let r = app
        .post(
            "/api/v1/auth/login",
            &As::anonymous(),
            json!({"email": "ADA@example.com", "password": "correct horse battery"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert!(r.headers.get(header::SET_COOKIE).unwrap().to_str().unwrap().contains("; Secure"));
    let who =
        As { cookie: Some(session_cookie(&r)), csrf: r.body["csrf_token"].as_str().map(str::to_string), token: None };
    assert_eq!(app.get("/api/v1/auth/session", &who).await.status, StatusCode::OK);

    // Logout needs the CSRF token like every mutating request.
    assert_eq!(app.post("/api/v1/auth/logout", &who.without_csrf(), json!({})).await.code(), "csrf_failed");
    let out = app.post("/api/v1/auth/logout", &who, json!({})).await;
    assert_eq!(out.status, StatusCode::NO_CONTENT);
    assert!(out.headers.get(header::SET_COOKIE).unwrap().to_str().unwrap().contains("Max-Age=0"));
    assert_eq!(app.get("/api/v1/auth/session", &who).await.status, StatusCode::UNAUTHORIZED);
    let actions = app.audit_actions();
    assert!(actions.contains(&"auth.login".into()) && actions.contains(&"auth.logout".into()), "{actions:?}");
}

#[tokio::test]
async fn wrong_passwords_and_unknown_users_look_the_same() {
    let app = TestApp::new();
    app.user_with_password("ada@example.com", "correct horse battery");
    let wrong = app
        .post(
            "/api/v1/auth/login",
            &As::anonymous(),
            json!({"email": "ada@example.com", "password": "wrong password!"}),
        )
        .await;
    let unknown = app
        .post(
            "/api/v1/auth/login",
            &As::anonymous(),
            json!({"email": "bob@example.com", "password": "wrong password!"}),
        )
        .await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong.body, unknown.body);
    assert_eq!(app.audit_actions().iter().filter(|a| *a == "auth.login_failed").count(), 2);
}

#[tokio::test]
async fn repeated_failures_lock_the_account() {
    let app = TestApp::new();
    app.user_with_password("ada@example.com", "correct horse battery");
    for _ in 0..10 {
        let r = app
            .post(
                "/api/v1/auth/login",
                &As::anonymous(),
                json!({"email": "ada@example.com", "password": "nope nope nope"}),
            )
            .await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    }
    // Locked: even the right password is refused for now.
    let r = app
        .post(
            "/api/v1/auth/login",
            &As::anonymous(),
            json!({"email": "ada@example.com", "password": "correct horse battery"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(r.headers.get(header::RETRY_AFTER).is_some());
    assert!(app.audit_actions().contains(&"auth.login_locked".into()));
}

#[tokio::test]
async fn local_login_can_be_disabled() {
    let app = TestApp::with_config(|c| c.local_login = false);
    app.user_with_password("ada@example.com", "correct horse battery");
    let r = app
        .post(
            "/api/v1/auth/login",
            &As::anonymous(),
            json!({"email": "ada@example.com", "password": "correct horse battery"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert_eq!(r.code(), "local_login_disabled");
}

#[tokio::test]
async fn disabled_users_cannot_log_in() {
    let app = TestApp::new();
    let uid = app.user_with_password("ada@example.com", "correct horse battery");
    app.conn().execute("UPDATE users SET disabled = true WHERE id = $1", &[&uid]).unwrap();
    let r = app
        .post(
            "/api/v1/auth/login",
            &As::anonymous(),
            json!({"email": "ada@example.com", "password": "correct horse battery"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn enforced_sso_refuses_passwords_except_owner_break_glass() {
    let app = TestApp::new();
    let org = app.org("acme");
    let member = app.user_with_password("m@example.com", "correct horse battery");
    let owner = app.user_with_password("o@example.com", "correct horse battery");
    rupta_auth::users::set_member(&app.conn(), org, member, Role::Member).unwrap();
    rupta_auth::users::set_member(&app.conn(), org, owner, Role::Owner).unwrap();
    app.conn()
        .execute(
            "INSERT INTO sso_configs (org_id, provider_name, issuer, client_id, client_secret_enc, enforce) VALUES ($1, 'IdP', 'https://idp', 'c', '', true)",
            &[&org],
        )
        .unwrap();
    let r = app
        .post(
            "/api/v1/auth/login",
            &As::anonymous(),
            json!({"email": "m@example.com", "password": "correct horse battery"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert_eq!(r.code(), "sso_required");
    assert_eq!(r.body["error"]["details"]["provider"], "org-acme");
    let r = app
        .post(
            "/api/v1/auth/login",
            &As::anonymous(),
            json!({"email": "o@example.com", "password": "correct horse battery"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    app.conn().execute("UPDATE sso_configs SET owner_local_login = false", &[]).unwrap();
    let r = app
        .post(
            "/api/v1/auth/login",
            &As::anonymous(),
            json!({"email": "o@example.com", "password": "correct horse battery"}),
        )
        .await;
    assert_eq!(r.code(), "sso_required");
}

#[tokio::test]
async fn auth_config_lists_an_orgs_provider_on_request() {
    let app = TestApp::new();
    let org = app.org("acme");
    app.conn()
        .execute("INSERT INTO sso_configs (org_id, provider_name, issuer, client_id, client_secret_enc) VALUES ($1, 'Acme IdP', 'https://idp', 'c', '')", &[&org])
        .unwrap();
    assert_eq!(app.get("/api/v1/auth/config", &As::anonymous()).await.body["sso"], json!([]));
    assert_eq!(
        app.get("/api/v1/auth/config?org=acme", &As::anonymous()).await.body["sso"],
        json!([{"id": "org-acme", "name": "Acme IdP"}])
    );
}

#[tokio::test]
async fn tokens_authenticate_and_bad_tokens_are_refused() {
    let app = TestApp::new();
    let org = app.org("acme");
    let (uid, _) = app.member(org, "ada@example.com", Role::Member);
    let token = app.user_token(uid, &["org:read"]);
    let r = app.get("/api/v1/orgs", &As::token(&token)).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body[0]["slug"], "acme");
    let r = app.get("/api/v1/orgs", &As::token("rpt_0000000000000000000000000000000000000000")).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    // Token requests need no CSRF header.
    let r = app.post("/api/v1/orgs/acme/teams", &As::token(&token), json!({"name": "x"})).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "member without org:write");
}

#[tokio::test]
async fn api_responses_carry_security_headers_and_json_404s() {
    let app = TestApp::new();
    let r = app.get("/api/v1/does-not-exist", &As::anonymous()).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert_eq!(r.code(), "not_found");
    for h in
        ["x-content-type-options", "x-frame-options", "content-security-policy", "cache-control", "referrer-policy"]
    {
        assert!(r.headers.get(h).is_some(), "{h}");
    }
}

#[tokio::test]
async fn oversized_bodies_are_refused() {
    let app = TestApp::with_config(|c| c.max_body_bytes = 1024);
    let big = "x".repeat(4096);
    let r = app.post("/api/v1/auth/login", &As::anonymous(), json!({"email": big, "password": "x"})).await;
    assert_eq!(r.status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn cors_only_for_configured_origins() {
    let app = TestApp::with_config(|c| c.cors_allowed_origins = vec!["https://app.example.com".into()]);
    let req = |origin: &str| {
        axum::http::Request::builder()
            .method("OPTIONS")
            .uri("/api/v1/auth/config")
            .header("origin", origin)
            .header("access-control-request-method", "GET")
            .body(axum::body::Body::empty())
            .unwrap()
    };
    let ok = app.send(req("https://app.example.com")).await;
    assert_eq!(ok.headers.get("access-control-allow-origin").unwrap(), "https://app.example.com");
    assert_eq!(ok.headers.get("access-control-allow-credentials").unwrap(), "true");
    let other = app.send(req("https://evil.example.com")).await;
    assert!(other.headers.get("access-control-allow-origin").is_none());
}
