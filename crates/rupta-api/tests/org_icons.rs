//! The organization icon: fetching a site's favicon, setting and removing
//! the domain, and serving the image.

mod common;

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use common::TestApp;
use rupta_api::favicon;
use rupta_api::integrations::http::Outbound;
use rupta_core::role::Role;
use serde_json::json;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n-not-really-a-png-but-the-magic-is-right";

async fn site() -> String {
    let app = axum::Router::new()
        // The start page redirects, as many do (to www., or a language).
        .route("/", get(|| async { (StatusCode::MOVED_PERMANENTLY, [("location", "/en/")]).into_response() }))
        .route(
            "/en/",
            get(|| async {
                axum::response::Html(
                    r#"<html><head><link rel="icon" href="/missing.png" sizes="16x16"><link rel="apple-touch-icon" href="/touch.png"></head></html>"#,
                )
            }),
        )
        .route("/touch.png", get(|| async { ([("content-type", "image/png")], PNG) }))
        .route("/missing.png", get(|| async { StatusCode::NOT_FOUND }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}/")
}

#[tokio::test]
async fn the_best_icon_of_a_site_is_fetched_through_redirects() {
    let base = url::Url::parse(&site().await).unwrap();
    let (icon, kind) = favicon::fetch_from(&Outbound::new(true), base.clone(), "example").await.unwrap();
    assert_eq!((icon.as_slice(), kind), (PNG, "image/png"));
    // Internal hosts are refused unless allowed.
    let err = favicon::fetch_from(&Outbound::new(false), base, "example").await.unwrap_err();
    assert!(err.contains("loopback"), "{err}");
}

#[tokio::test]
async fn a_site_without_an_icon_is_an_error() {
    let app = axum::Router::new().route("/", get(|| async { axum::response::Html("<html></html>") }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let base = url::Url::parse(&format!("http://{addr}/")).unwrap();
    let err = favicon::fetch_from(&Outbound::new(true), base, "example").await.unwrap_err();
    assert!(err.contains("favicon.ico"), "{err}");
}

#[tokio::test]
async fn domain_and_icon_of_an_organization() {
    let app = TestApp::new();
    let org = app.org("acme");
    let (_, owner) = app.member(org, "o@example.com", Role::Owner);
    let (_, member) = app.member(org, "m@example.com", Role::Member);

    // No icon yet.
    let r = app.get("/api/v1/orgs/acme", &owner).await;
    assert_eq!((r.body["domain"].clone(), r.body["icon_url"].clone()), (json!(null), json!(null)));
    assert_eq!(app.get("/api/v1/orgs/acme/icon", &owner).await.status, StatusCode::NOT_FOUND);

    // Not a domain; and loopback is refused before any request.
    for bad in ["localhost", "10.0.0.1", "not a domain"] {
        let r = app.patch("/api/v1/orgs/acme", &owner, json!({"domain": bad})).await;
        assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}: {:?}", r.body);
        assert_eq!(r.body["error"]["details"]["field"], "domain");
    }
    // Members may not change it.
    let r = app.patch("/api/v1/orgs/acme", &member, json!({"domain": "example.com"})).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{:?}", r.body);

    // An icon in place (as a successful fetch leaves it) is listed and served.
    app.conn()
        .execute(
            "UPDATE organizations SET domain = 'example.com', icon = $1, icon_type = 'image/svg+xml', icon_updated_at = now() WHERE id = $2",
            &[&b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>".to_vec(), &org],
        )
        .unwrap();
    let r = app.get("/api/v1/orgs/acme", &owner).await;
    assert_eq!(r.body["domain"], "example.com");
    let url = r.body["icon_url"].as_str().unwrap().to_string();
    assert!(url.starts_with("/api/v1/orgs/acme/icon?v="), "{url}");
    let listed = app.get("/api/v1/orgs", &owner).await;
    assert_eq!(listed.body[0]["icon_url"], url.as_str());
    let session = app.get("/api/v1/auth/session", &owner).await;
    assert_eq!(session.body["orgs"][0]["icon_url"], url.as_str(), "{:?}", session.body);

    let icon = app.get(&url, &member).await;
    assert_eq!(icon.status, StatusCode::OK);
    assert_eq!(icon.headers["content-type"], "image/svg+xml");
    assert!(icon.headers["content-security-policy"].to_str().unwrap().contains("sandbox"));
    assert_eq!(icon.headers["x-content-type-options"], "nosniff");
    // Other organizations' icons are not visible.
    app.org("other");
    assert_eq!(app.get("/api/v1/orgs/other/icon", &member).await.status, StatusCode::NOT_FOUND);

    // An empty domain removes domain and icon.
    let r = app.patch("/api/v1/orgs/acme", &owner, json!({"domain": ""})).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!((r.body["domain"].clone(), r.body["icon_url"].clone()), (json!(null), json!(null)));
    assert_eq!(app.get("/api/v1/orgs/acme/icon", &owner).await.status, StatusCode::NOT_FOUND);
    assert!(app.audit_actions().iter().any(|a| a == "org.update"));
}
