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
    if path.starts_with("api/") || path.starts_with("auth/") || path.contains('.') {
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
