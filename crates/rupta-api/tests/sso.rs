//! Single sign-on against an OpenID provider running in the test: it
//! serves discovery, JWKS, a token endpoint with PKCE checks and user
//! info, and signs ID tokens with an RSA key generated here.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::{Form, State};
use axum::http::{HeaderMap, StatusCode, header};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use common::{As, TestApp};
use openidconnect::core::{CoreJwsSigningAlgorithm, CoreRsaPrivateSigningKey};
use openidconnect::{JsonWebKeyId, PrivateSigningKey};
use rupta_auth::oidc::OidcSettings;
use rupta_core::role::Role;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(Clone)]
struct Pending {
    nonce: String,
    challenge: String,
    redirect_uri: String,
    claims: Value,
    userinfo: Value,
}

#[derive(Clone)]
struct Idp {
    issuer: String,
    key: Arc<CoreRsaPrivateSigningKey>,
    codes: Arc<Mutex<HashMap<String, Pending>>>,
    tokens: Arc<Mutex<HashMap<String, Value>>>,
    basic_auth_seen: Arc<Mutex<Vec<String>>>,
}

fn rsa_pem() -> String {
    use rsa::pkcs1::EncodeRsaPrivateKey;
    let key = rsa::RsaPrivateKey::new(&mut rand_core06::OsRng, 2048).unwrap();
    key.to_pkcs1_pem(rsa::pkcs1::LineEnding::LF).unwrap().to_string()
}

impl Idp {
    fn sign(&self, claims: &Value) -> String {
        let header = json!({"alg": "RS256", "typ": "JWT", "kid": "k1"});
        let input =
            format!("{}.{}", URL_SAFE_NO_PAD.encode(header.to_string()), URL_SAFE_NO_PAD.encode(claims.to_string()));
        let sig = self.key.sign(&CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256, input.as_bytes()).unwrap();
        format!("{input}.{}", URL_SAFE_NO_PAD.encode(sig))
    }

    /// What the provider does when the user signs in: a code for the
    /// authorization request in `authorize_url`.
    fn authorize(&self, authorize_url: &str, claims: Value, userinfo: Value) -> (String, String) {
        let url = url::Url::parse(authorize_url).unwrap();
        let q: HashMap<String, String> = url.query_pairs().into_owned().collect();
        assert_eq!(q["response_type"], "code");
        assert_eq!(q["code_challenge_method"], "S256");
        assert!(q["scope"].contains("openid"));
        let code = format!("code-{}", self.codes.lock().unwrap().len());
        self.codes.lock().unwrap().insert(
            code.clone(),
            Pending {
                nonce: q["nonce"].clone(),
                challenge: q["code_challenge"].clone(),
                redirect_uri: q["redirect_uri"].clone(),
                claims,
                userinfo,
            },
        );
        (code, q["state"].clone())
    }
}

async fn discovery(State(idp): State<Idp>) -> axum::Json<Value> {
    let i = &idp.issuer;
    axum::Json(json!({
        "issuer": i,
        "authorization_endpoint": format!("{i}/authorize"),
        "token_endpoint": format!("{i}/token"),
        "userinfo_endpoint": format!("{i}/userinfo"),
        "jwks_uri": format!("{i}/jwks"),
        "response_types_supported": ["code"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["RS256"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["client_secret_basic", "none"],
    }))
}

async fn jwks(State(idp): State<Idp>) -> axum::Json<Value> {
    axum::Json(json!({ "keys": [idp.key.as_verification_key()] }))
}

async fn token(
    State(idp): State<Idp>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> Result<axum::Json<Value>, (StatusCode, axum::Json<Value>)> {
    let fail = |e: &str| (StatusCode::BAD_REQUEST, axum::Json(json!({"error": e})));
    if let Some(auth) = headers.get(header::AUTHORIZATION) {
        idp.basic_auth_seen.lock().unwrap().push(auth.to_str().unwrap().to_string());
    }
    let p = idp
        .codes
        .lock()
        .unwrap()
        .remove(form.get("code").ok_or_else(|| fail("invalid_request"))?)
        .ok_or_else(|| fail("invalid_grant"))?;
    let verifier = form.get("code_verifier").ok_or_else(|| fail("invalid_request"))?;
    if URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())) != p.challenge {
        return Err(fail("invalid_grant"));
    }
    if form.get("redirect_uri") != Some(&p.redirect_uri) {
        return Err(fail("invalid_grant"));
    }
    let now = chrono::Utc::now().timestamp();
    let mut claims = json!({"iss": idp.issuer, "aud": "rupta-client", "iat": now, "exp": now + 300, "nonce": p.nonce});
    for (k, v) in p.claims.as_object().unwrap() {
        claims[k] = v.clone();
    }
    let access = format!("at-{}", rand_core06::RngCore::next_u64(&mut rand_core06::OsRng));
    idp.tokens.lock().unwrap().insert(access.clone(), p.userinfo);
    Ok(axum::Json(
        json!({"access_token": access, "token_type": "Bearer", "expires_in": 300, "id_token": idp.sign(&claims)}),
    ))
}

async fn userinfo(State(idp): State<Idp>, headers: HeaderMap) -> Result<axum::Json<Value>, StatusCode> {
    let t = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;
    idp.tokens.lock().unwrap().get(t).cloned().map(axum::Json).ok_or(StatusCode::UNAUTHORIZED)
}

async fn start_idp() -> Idp {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let issuer = format!("http://{}", listener.local_addr().unwrap());
    let pem = tokio::task::spawn_blocking(rsa_pem).await.unwrap();
    let key = CoreRsaPrivateSigningKey::from_pem(&pem, Some(JsonWebKeyId::new("k1".into()))).unwrap();
    let idp = Idp {
        issuer,
        key: Arc::new(key),
        codes: Arc::default(),
        tokens: Arc::default(),
        basic_auth_seen: Arc::default(),
    };
    let app = axum::Router::new()
        .route("/.well-known/openid-configuration", axum::routing::get(discovery))
        .route("/jwks", axum::routing::get(jwks))
        .route("/token", axum::routing::post(token))
        .route("/userinfo", axum::routing::get(userinfo))
        .with_state(idp.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    idp
}

fn settings(idp: &Idp) -> OidcSettings {
    OidcSettings {
        id: "oidc".into(),
        name: "Zitadel".into(),
        issuer: idp.issuer.clone(),
        client_id: "rupta-client".into(),
        client_secret: Some("client-secret".into()),
        scopes: vec!["openid".into(), "email".into(), "profile".into(), "urn:zitadel:iam:org:project:roles".into()],
        role_claim: Some("urn:zitadel:iam:org:project:roles".into()),
        role_mapping: vec![("rupta-admin".into(), Role::Admin), ("rupta-viewer".into(), Role::Viewer)],
        default_role: Role::Member,
        jit_provisioning: true,
    }
}

fn cookie(resp: &common::Resp, name: &str) -> Option<String> {
    resp.headers
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap().to_string())
        .find(|c| c.starts_with(&format!("{name}=")))
        .map(|c| c.split(';').next().unwrap()[name.len() + 1..].to_string())
}

/// Runs the browser side of a sign-in; returns the callback response.
async fn sign_in(
    app: &TestApp,
    idp: &Idp,
    provider: &str,
    claims: Value,
    userinfo: Value,
    tamper_state: bool,
) -> common::Resp {
    let start =
        app.get(&format!("/auth/sso/{provider}/start?next=/organizations/default/issues"), &As::anonymous()).await;
    assert_eq!(start.status, StatusCode::FOUND, "{:?}", start.body);
    let location = start.headers.get(header::LOCATION).unwrap().to_str().unwrap().to_string();
    assert!(location.starts_with(&format!("{}/authorize?", idp.issuer)), "{location}");
    assert!(
        location.contains(&format!("redirect_uri=http%3A%2F%2Frupta.test%2Fauth%2Fsso%2F{provider}%2Fcallback")),
        "{location}"
    );
    let state_cookie = cookie(&start, "rupta_sso_state").expect("state cookie");
    let (code, mut state) = idp.authorize(&location, claims, userinfo);
    if tamper_state {
        state.push('x');
    }
    let req = axum::http::Request::builder()
        .uri(format!("/auth/sso/{provider}/callback?code={code}&state={state}"))
        .header(header::COOKIE, format!("rupta_sso_state={state_cookie}"))
        .body(axum::body::Body::empty())
        .unwrap();
    app.send(req).await
}

#[tokio::test]
async fn zitadel_style_sign_in_provisions_and_maps_roles() {
    let idp = start_idp().await;
    let s = settings(&idp);
    let app = TestApp::with_config(|c| c.oidc = Some(s));
    let cfg = app.get("/api/v1/auth/config", &As::anonymous()).await;
    assert_eq!(cfg.body["sso"], json!([{"id": "oidc", "name": "Zitadel"}]));

    let claims = json!({
        "sub": "zitadel-user-1", "email": "ada@example.com", "email_verified": true, "name": "Ada Lovelace",
        "urn:zitadel:iam:org:project:roles": {"rupta-viewer": {"1234": "acme.zitadel.cloud"}, "rupta-admin": {"1234": "acme.zitadel.cloud"}},
    });
    let r = sign_in(&app, &idp, "oidc", claims.clone(), json!({}), false).await;
    assert_eq!(r.status, StatusCode::FOUND);
    assert_eq!(r.headers.get(header::LOCATION).unwrap(), "/organizations/default/issues");
    let session = cookie(&r, "rupta_session").expect("session cookie");
    assert!(idp.basic_auth_seen.lock().unwrap()[0].starts_with("Basic "), "client_secret_basic");

    let who = As { cookie: Some(session), ..As::default() };
    let s = app.get("/api/v1/auth/session", &who).await;
    assert_eq!(s.status, StatusCode::OK);
    assert_eq!(s.body["user"]["email"], "ada@example.com");
    assert_eq!(s.body["user"]["name"], "Ada Lovelace");
    assert_eq!(
        s.body["orgs"],
        json!([{"id": s.body["orgs"][0]["id"], "slug": "default", "name": "default", "role": "admin"}]),
        "highest role wins"
    );

    // The next sign-in follows the provider's roles; same user by subject.
    let mut claims2 = claims.clone();
    claims2["urn:zitadel:iam:org:project:roles"] = json!({"rupta-viewer": {"1234": "x"}});
    claims2["email"] = json!("changed@example.com");
    let r = sign_in(&app, &idp, "oidc", claims2, json!({}), false).await;
    let who = As { cookie: cookie(&r, "rupta_session"), ..As::default() };
    let s = app.get("/api/v1/auth/session", &who).await;
    assert_eq!(s.body["user"]["email"], "ada@example.com");
    assert_eq!(s.body["orgs"][0]["role"], "viewer");
    let users: i64 = app.conn().query_one("SELECT count(*) FROM users", &[]).unwrap().get(0);
    assert_eq!(users, 1);
    let actions = app.audit_actions();
    assert!(
        actions.contains(&"auth.sso_login".into()) && actions.contains(&"member.role_change".into()),
        "{actions:?}"
    );
}

#[tokio::test]
async fn tampered_state_and_wrong_nonce_are_refused() {
    let idp = start_idp().await;
    let s = settings(&idp);
    let app = TestApp::with_config(|c| c.oidc = Some(s));
    let claims = json!({"sub": "u1", "email": "a@example.com", "email_verified": true});
    let r = sign_in(&app, &idp, "oidc", claims.clone(), json!({}), true).await;
    assert_eq!(r.status, StatusCode::FOUND);
    assert_eq!(r.headers.get(header::LOCATION).unwrap(), "/login?error=sso_failed");
    assert!(cookie(&r, "rupta_session").is_none());

    // A token minted for another nonce.
    let mut bad = claims.clone();
    bad["nonce"] = json!("not-the-nonce");
    let r = sign_in(&app, &idp, "oidc", bad, json!({}), false).await;
    assert_eq!(r.headers.get(header::LOCATION).unwrap(), "/login?error=sso_failed");
    // A token for another audience.
    let mut bad = claims;
    bad["aud"] = json!("someone-else");
    let r = sign_in(&app, &idp, "oidc", bad, json!({}), false).await;
    assert_eq!(r.headers.get(header::LOCATION).unwrap(), "/login?error=sso_failed");
    assert_eq!(app.audit_actions().iter().filter(|a| *a == "auth.sso_login_failed").count(), 3);

    // Without the state cookie.
    let req = axum::http::Request::builder()
        .uri("/auth/sso/oidc/callback?code=x&state=y")
        .body(axum::body::Body::empty())
        .unwrap();
    assert_eq!(app.send(req).await.headers.get(header::LOCATION).unwrap(), "/login?error=sso_failed");
    assert_eq!(app.get("/auth/sso/nope/start", &As::anonymous()).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn existing_users_link_only_with_verified_email_and_roles_can_come_from_userinfo() {
    let idp = start_idp().await;
    let s = settings(&idp);
    let app = TestApp::with_config(|c| c.oidc = Some(s));
    app.user_with_password("bob@example.com", "a long password");
    let r = sign_in(
        &app,
        &idp,
        "oidc",
        json!({"sub": "b1", "email": "bob@example.com", "email_verified": false}),
        json!({"sub": "b1"}),
        false,
    )
    .await;
    assert_eq!(
        r.headers.get(header::LOCATION).unwrap(),
        "/login?error=sso_failed",
        "unverified email must not take over an account"
    );
    let r = sign_in(
        &app,
        &idp,
        "oidc",
        json!({"sub": "b1", "email": "bob@example.com", "email_verified": true}),
        json!({"sub": "b1", "urn:zitadel:iam:org:project:roles": {"rupta-admin": {}}}),
        false,
    )
    .await;
    let who = As { cookie: cookie(&r, "rupta_session"), ..As::default() };
    let s = app.get("/api/v1/auth/session", &who).await;
    assert_eq!(s.body["user"]["email"], "bob@example.com");
    assert_eq!(s.body["orgs"][0]["role"], "admin", "role claim read from user info");
    let linked: i64 = app
        .conn()
        .query_one("SELECT count(*) FROM identities WHERE provider = 'oidc' AND subject = 'b1'", &[])
        .unwrap()
        .get(0);
    assert_eq!(linked, 1);
}

#[tokio::test]
async fn organization_providers_with_encrypted_secrets() {
    let idp = start_idp().await;
    // The test provider listens on loopback, which is refused by default.
    let app = TestApp::with_config(|c| c.sso_allow_private_issuers = true);
    let org = app.org("acme");
    let (_, owner) = app.member(org, "owner@example.com", Role::Owner);
    let (_, admin) = app.member(org, "admin@example.com", Role::Admin);
    let body = json!({
        "provider_name": "Acme IdP", "issuer": idp.issuer, "client_id": "rupta-client", "client_secret": "org-secret",
        "role_claim": "groups", "role_mapping": {"rupta-admins": "admin"}, "default_role": "viewer", "enforce": true,
    });
    assert_eq!(
        app.put("/api/v1/orgs/acme/sso", &admin, body.clone()).await.status,
        StatusCode::FORBIDDEN,
        "owners only"
    );
    let r = app.put("/api/v1/orgs/acme/sso", &owner, body).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["provider_id"], "org-acme");
    assert_eq!(r.body["has_client_secret"], true);
    assert!(r.body.get("client_secret").is_none());
    assert_eq!(r.body["redirect_uri"], "http://rupta.test/auth/sso/org-acme/callback");
    let stored: String = app.conn().query_one("SELECT client_secret_enc FROM sso_configs", &[]).unwrap().get(0);
    assert!(!stored.contains("org-secret"));
    assert!(app.audit_actions().contains(&"sso.config_change".into()));
    let bad = app.put("/api/v1/orgs/acme/sso", &owner, json!({"issuer": "http://127.0.0.1:1", "client_id": "x"})).await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY, "discovery must work");

    let r = sign_in(
        &app,
        &idp,
        "org-acme",
        json!({"sub": "c1", "email": "carol@example.com", "email_verified": true, "groups": ["staff"]}),
        json!({}),
        false,
    )
    .await;
    let who = As { cookie: cookie(&r, "rupta_session"), ..As::default() };
    let s = app.get("/api/v1/auth/session", &who).await;
    assert_eq!(s.body["orgs"][0]["slug"], "acme");
    assert_eq!(s.body["orgs"][0]["role"], "viewer", "the default role");
    let basic = idp.basic_auth_seen.lock().unwrap()[0].clone();
    let decoded = String::from_utf8(
        base64::engine::general_purpose::STANDARD.decode(basic.trim_start_matches("Basic ")).unwrap(),
    )
    .unwrap();
    assert_eq!(decoded, "rupta-client:org-secret", "the decrypted secret is used");
    let cfg = app.get("/api/v1/auth/config?org=acme", &As::anonymous()).await;
    assert_eq!(cfg.body["sso"][0]["id"], "org-acme");
    assert_eq!(app.delete("/api/v1/orgs/acme/sso", &owner).await.status, StatusCode::NO_CONTENT);
    assert_eq!(app.get("/auth/sso/org-acme/start", &As::anonymous()).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn organization_providers_on_internal_addresses_are_refused_by_default() {
    let idp = start_idp().await;
    let app = TestApp::new();
    let org = app.org("acme");
    let (_, owner) = app.member(org, "owner@example.com", Role::Owner);
    let body =
        json!({"provider_name": "Internal", "issuer": idp.issuer, "client_id": "rupta-client", "client_secret": "s"});
    let r = app.put("/api/v1/orgs/acme/sso", &owner, body).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", r.body);
    assert!(r.body.to_string().contains("RUPTA_SSO_ALLOW_PRIVATE_ISSUERS"), "{}", r.body);
    let stored: i64 = app.conn().query_one("SELECT count(*) FROM sso_configs", &[]).unwrap().get(0);
    assert_eq!(stored, 0);
}
