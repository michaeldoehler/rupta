//! Calls the REST API in process, as the MCP caller.
//!
//! Every tool goes through the `/api/v1` routes instead of reading the
//! database itself, so the API's authorization guard, validation and
//! limits apply unchanged. The caller's [`Auth`] travels in the request
//! extensions (the API's extractor takes it from there) together with the
//! original `Authorization` header and client address.

use std::net::SocketAddr;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use rupta_api::extract::Auth;
use serde_json::Value;
use tower::ServiceExt;

/// A failed call, worded for the agent that made it.
#[derive(Debug, Clone)]
pub(crate) struct ToolError(pub String);

impl ToolError {
    pub fn new(msg: impl Into<String>) -> ToolError {
        ToolError(msg.into())
    }
}

pub(crate) type ToolResult<T> = Result<T, ToolError>;

/// The caller of one MCP request.
#[derive(Clone)]
pub(crate) struct Rest {
    api: Router,
    auth: Auth,
    authorization: Option<String>,
    forwarded_for: Option<String>,
    peer: Option<ConnectInfo<SocketAddr>>,
}

/// Query parameters; `None` values are left out.
pub(crate) type Params<'a> = &'a [(&'a str, Option<String>)];

impl Rest {
    pub fn new(api: Router, auth: Auth, parts: &http::request::Parts) -> Rest {
        let h = |name: &str| parts.headers.get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
        Rest {
            api,
            auth,
            authorization: h(header::AUTHORIZATION.as_str()),
            forwarded_for: h("x-forwarded-for"),
            peer: parts.extensions.get::<ConnectInfo<SocketAddr>>().cloned(),
        }
    }

    pub async fn get(&self, path: &str, params: Params<'_>) -> ToolResult<Value> {
        self.call(Method::GET, path, params, None).await
    }

    pub async fn post(&self, path: &str, body: Value) -> ToolResult<Value> {
        self.call(Method::POST, path, &[], Some(body)).await
    }

    pub async fn patch(&self, path: &str, body: Value) -> ToolResult<Value> {
        self.call(Method::PATCH, path, &[], Some(body)).await
    }

    pub async fn call(&self, method: Method, path: &str, params: Params<'_>, body: Option<Value>) -> ToolResult<Value> {
        let mut uri = path.to_string();
        let query = {
            let mut q = form_urlencoded::Serializer::new(String::new());
            for (k, v) in params {
                if let Some(v) = v {
                    q.append_pair(k, v);
                }
            }
            q.finish()
        };
        if !query.is_empty() {
            uri.push('?');
            uri.push_str(&query);
        }
        let mut b = Request::builder().method(method).uri(&uri);
        if let Some(a) = &self.authorization {
            b = b.header(header::AUTHORIZATION, a);
        }
        if let Some(x) = &self.forwarded_for {
            b = b.header("x-forwarded-for", x);
        }
        let mut req = match body {
            Some(v) => b.header(header::CONTENT_TYPE, "application/json").body(Body::from(v.to_string())),
            None => b.body(Body::empty()),
        }
        .map_err(|e| ToolError::new(format!("invalid request: {e}")))?;
        req.extensions_mut().insert(self.auth.clone());
        if let Some(p) = &self.peer {
            req.extensions_mut().insert(*p);
        }
        let resp = self.api.clone().oneshot(req).await.map_err(|e| ToolError::new(format!("internal error: {e}")))?;
        let status = resp.status();
        let bytes =
            resp.into_body().collect().await.map_err(|e| ToolError::new(format!("internal error: {e}")))?.to_bytes();
        let value: Value =
            if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap_or(Value::Null) };
        if status.is_success() {
            return Ok(value);
        }
        Err(describe_error(status, &value))
    }
}

/// Turns an API error into a message that tells the agent what to do.
fn describe_error(status: StatusCode, body: &Value) -> ToolError {
    let e = &body["error"];
    let code = e["code"].as_str().unwrap_or("");
    let message = e["message"].as_str().unwrap_or("");
    let text = match status {
        StatusCode::NOT_FOUND => {
            "Not found: it does not exist, or it belongs to an organization this token cannot access. \
             Check the ids and slugs (list_organizations, list_projects, search_issues)."
                .to_string()
        }
        StatusCode::FORBIDDEN => format!(
            "Forbidden: {message}. The token's scopes or the user's role in the organization do not allow this action \
             (see the tool description for what it needs). Do not retry; ask the user for a token with more rights if needed."
        ),
        StatusCode::UNPROCESSABLE_ENTITY | StatusCode::BAD_REQUEST => {
            let field = e["details"]["field"].as_str().map(|f| format!(" (argument {f})")).unwrap_or_default();
            format!("Invalid request{field}: {message}")
        }
        StatusCode::CONFLICT => format!("Conflict: {message}"),
        StatusCode::BAD_GATEWAY => {
            let reason = e["details"]["reason"].as_str().map(|r| format!(" ({r})")).unwrap_or_default();
            format!("The external service failed{reason}: {message}")
        }
        StatusCode::TOO_MANY_REQUESTS => format!("Rate limited: {message}"),
        _ => format!(
            "Request failed with status {}{}: {message}",
            status.as_u16(),
            if code.is_empty() { String::new() } else { format!(" {code}") }
        ),
    };
    ToolError(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn errors_name_the_field_and_the_cause() {
        let e = describe_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            &json!({"error": {"code": "validation_failed", "message": "query: unknown value is:sideways", "details": {"field": "query"}}}),
        );
        assert_eq!(e.0, "Invalid request (argument query): query: unknown value is:sideways");
        assert!(describe_error(StatusCode::NOT_FOUND, &Value::Null).0.starts_with("Not found"));
        assert!(describe_error(StatusCode::FORBIDDEN, &json!({"error": {"message": "no"}})).0.contains("Do not retry"));
    }
}
