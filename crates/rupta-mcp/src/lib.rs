//! The Model Context Protocol (MCP) server of rupta: lets MCP clients
//! (agents in editors, chat applications, command-line tools) search
//! issues, read events, traces, logs, metrics, releases, monitors and
//! alerts, and triage issues.
//!
//! The endpoint `/mcp` speaks the Streamable HTTP transport through the
//! official Rust SDK (`rmcp`), statelessly: every request carries a rupta
//! API token (`Authorization: Bearer rpt_...`). Each tool is a thin layer
//! over the REST API: it calls the `/api/v1` routes in process with the
//! caller's authentication, so organizations, roles and token scopes limit
//! a tool exactly like the corresponding REST request.

mod auth;
mod format;
mod prompts;
mod resources;
mod rest;
mod server;
mod tools;

use std::sync::Arc;

use axum::Router;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use rmcp::transport::streamable_http_server::session::never::NeverSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rupta_api::ApiState;
use serde_json::json;

pub use server::RuptaMcp;

/// Path of the MCP endpoint.
pub const PATH: &str = "/mcp";
/// RFC 9728 metadata of the protected resource.
pub const METADATA_PATH: &str = "/.well-known/oauth-protected-resource";

/// Largest JSON-RPC request body.
const MAX_BODY_BYTES: usize = 1 << 20;

/// The MCP endpoint and its OAuth protected resource metadata, or, when
/// `enabled` is false, `404` for both paths.
pub fn router(state: ApiState, enabled: bool) -> Router {
    if !enabled {
        let gone = || async {
            (
                StatusCode::NOT_FOUND,
                axum::Json(
                    json!({"error": {"code": "not_found", "message": "the MCP endpoint is disabled (RUPTA_MCP=false)", "details": {}}}),
                ),
            )
        };
        return Router::new()
            .route(PATH, axum::routing::any(gone))
            .route(METADATA_PATH, axum::routing::any(gone))
            .route(&format!("{METADATA_PATH}{PATH}"), axum::routing::any(gone));
    }
    let server = RuptaMcp::new(state.clone());
    let config = StreamableHttpServerConfig::default()
        // No sessions: every POST is answered on its own, authenticated by
        // its own token. Server-initiated messages are not used.
        .with_legacy_session_mode(false)
        .with_json_response(true)
        .with_sse_keep_alive(None)
        .with_max_request_body_bytes(MAX_BODY_BYTES)
        // Hosts differ behind proxies and inside container networks; the
        // Origin check below is what protects against DNS rebinding, and
        // every request needs a token besides.
        .disable_allowed_hosts()
        .with_allowed_origins(allowed_origins(&state))
        .enforce_origin_validation();
    let service =
        StreamableHttpService::new(move || Ok(server.clone()), Arc::new(NeverSessionManager::default()), config);
    let metadata = {
        let state = state.clone();
        move || {
            let state = state.clone();
            async move { protected_resource_metadata(&state) }
        }
    };
    Router::new()
        .route_service(PATH, service)
        .layer(axum::middleware::from_fn_with_state(state.clone(), auth::require_token))
        .route(METADATA_PATH, axum::routing::get(metadata.clone()))
        .route(&format!("{METADATA_PATH}{PATH}"), axum::routing::get(metadata))
}

/// Browser origins that may call `/mcp`: the instance itself and
/// `RUPTA_CORS_ALLOWED_ORIGINS`. Clients outside a browser send no
/// `Origin` header and are not affected.
fn allowed_origins(state: &ApiState) -> Vec<String> {
    let mut out = Vec::new();
    for o in std::iter::once(&state.config.public_url).chain(state.config.cors_allowed_origins.iter()) {
        if let Some(origin) = explicit_origin(o) {
            out.push(origin);
        }
    }
    out
}

/// `scheme://host:port` with the port spelled out, as the SDK compares
/// origins with explicit ports.
fn explicit_origin(url: &str) -> Option<String> {
    let u = url::Url::parse(url.trim_end_matches('/')).ok()?;
    let host = u.host_str()?;
    let port = u.port_or_known_default()?;
    let host = if host.contains(':') && !host.starts_with('[') { format!("[{host}]") } else { host.to_string() };
    Some(format!("{}://{host}:{port}", u.scheme()))
}

/// The URL of the MCP endpoint.
pub fn resource_url(state: &ApiState) -> String {
    format!("{}{PATH}", state.config.public_url)
}

/// The URL of the metadata document for the MCP endpoint (RFC 9728,
/// section 3.1: the well-known path followed by the resource's path).
pub fn metadata_url(state: &ApiState) -> String {
    format!("{}{METADATA_PATH}{PATH}", state.config.public_url)
}

/// OAuth 2.0 Protected Resource Metadata (RFC 9728) of `/mcp`.
fn protected_resource_metadata(state: &ApiState) -> axum::response::Response {
    let mut doc = json!({
        "resource": resource_url(state),
        "resource_name": "rupta",
        "bearer_methods_supported": ["header"],
        "scopes_supported": rupta_auth::rbac::SCOPES.iter().map(|(s, _)| *s).collect::<Vec<_>>(),
        "resource_documentation": "https://github.com/michaeldoehler/rupta/blob/main/docs/mcp.md",
    });
    if let Some(oidc) = &state.config.oidc {
        doc["authorization_servers"] = json!([oidc.issuer]);
    }
    ([(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "max-age=300")], doc.to_string())
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_get_explicit_ports() {
        assert_eq!(explicit_origin("https://errors.example.com/").as_deref(), Some("https://errors.example.com:443"));
        assert_eq!(explicit_origin("http://localhost:8080").as_deref(), Some("http://localhost:8080"));
        assert_eq!(explicit_origin("http://[::1]:8080").as_deref(), Some("http://[::1]:8080"));
        assert_eq!(explicit_origin("not a url"), None);
    }
}
