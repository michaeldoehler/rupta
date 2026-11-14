//! The single-page web app, embedded at build time from `web/dist`.

use axum::body::Body;
use axum::http::{HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};

#[derive(rust_embed::RustEmbed)]
#[folder = "../../web/dist"]
struct Assets;

/// Serves a file of the app, or `index.html` for client-side routes.
pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if let Some(file) = Assets::get(path).filter(|_| !path.is_empty()) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        // Vite puts content hashes in asset names: cache them for a year.
        let cache = if path.starts_with("assets/") { "public, max-age=31536000, immutable" } else { "no-cache" };
        return (
            [
                (header::CONTENT_TYPE, HeaderValue::from_str(mime.as_ref()).unwrap()),
                (header::CACHE_CONTROL, HeaderValue::from_static(cache)),
            ],
            Body::from(file.data.into_owned()),
        )
            .into_response();
    }
    // Missing API routes and build assets are real 404s. Every other path
    // is a client-side route, also when it contains a dot (release
    // versions such as `app@1.2.3`, tag keys such as `browser.name`).
    if path.starts_with("api/") || path.starts_with("auth/") || path.starts_with("assets/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    match Assets::get("index.html") {
        Some(index) => (
            [
                (header::CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8")),
                (header::CACHE_CONTROL, HeaderValue::from_static("no-cache")),
            ],
            Body::from(index.data.into_owned()),
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn status(path: &str) -> (StatusCode, String) {
        let res = serve(path.parse().unwrap()).await;
        let ct = res.headers().get(header::CONTENT_TYPE).map(|v| v.to_str().unwrap().to_string()).unwrap_or_default();
        (res.status(), ct)
    }

    #[tokio::test]
    async fn client_routes_with_dots_get_the_app() {
        for path in ["/", "/acme/issues", "/acme/releases/web-app@1.3.8", "/acme/issues/42/tags/browser.name"] {
            let (code, ct) = status(path).await;
            assert_eq!(code, StatusCode::OK, "{path}");
            assert!(ct.starts_with("text/html"), "{path}: {ct}");
        }
    }

    #[tokio::test]
    async fn missing_assets_and_api_routes_are_not_found() {
        for path in ["/assets/missing-abc123.js", "/api/v1/nope", "/auth/nope"] {
            assert_eq!(status(path).await.0, StatusCode::NOT_FOUND, "{path}");
        }
    }
}
