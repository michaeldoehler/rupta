//! Authentication of `/mcp`: an API token in `Authorization: Bearer`.
//!
//! The token is checked by the REST API's own extractor, so the same rules
//! apply (expiry, revocation, `last_used_at`). Session cookies are not
//! accepted: the endpoint is for programs, and a cookie would be sent by a
//! browser on its own.

use axum::extract::{FromRequestParts, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use rupta_api::ApiState;
use rupta_api::extract::Auth;
use serde_json::json;

pub(crate) async fn require_token(State(state): State<ApiState>, req: Request, next: Next) -> Response {
    let (mut parts, body) = req.into_parts();
    parts.headers.remove(header::COOKIE);
    let given = parts.headers.contains_key(header::AUTHORIZATION);
    match Auth::from_request_parts(&mut parts, &state).await {
        Ok(_) => next.run(Request::from_parts(parts, body)).await,
        Err(e) if e.status == StatusCode::UNAUTHORIZED => unauthorized(&state, given),
        Err(e) => e.into_response(),
    }
}

/// `401` with the challenge that points MCP clients to the metadata.
fn unauthorized(state: &ApiState, token_given: bool) -> Response {
    let mut challenge = format!("Bearer resource_metadata=\"{}\"", crate::metadata_url(state));
    if token_given {
        challenge.push_str(", error=\"invalid_token\"");
    }
    let mut r = (
        StatusCode::UNAUTHORIZED,
        axum::Json(json!({"error": {
            "code": "unauthorized",
            "message": "send a rupta API token: Authorization: Bearer rpt_...",
            "details": {}
        }})),
    )
        .into_response();
    if let Ok(v) = HeaderValue::from_str(&challenge) {
        r.headers_mut().insert(header::WWW_AUTHENTICATE, v);
    }
    r
}
