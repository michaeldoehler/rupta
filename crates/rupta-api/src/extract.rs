//! Request extractors: authentication (session cookie with CSRF check, or
//! bearer token), JSON bodies and query strings with JSON errors, the
//! client address, and cursor pagination.

use std::net::{IpAddr, SocketAddr};

use axum::extract::{ConnectInfo, FromRef, FromRequest, FromRequestParts, Request};
use axum::http::request::Parts;
use axum::http::{Method, header};
use rupta_auth::crypto::ct_eq;
use rupta_auth::rbac::Principal;
use rupta_auth::{apitoken, session};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::error::{ApiError, ApiResult};
use crate::state::ApiState;

/// The client address (see [`ApiState::client_ip`]).
#[derive(Debug, Clone)]
pub struct ClientIp(pub Option<IpAddr>);

impl ClientIp {
    pub fn string(&self) -> Option<String> {
        self.0.map(|ip| ip.to_string())
    }
}

impl<S> FromRequestParts<S> for ClientIp
where
    ApiState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let state = ApiState::from_ref(state);
        let peer = parts.extensions.get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip());
        let xff = parts.headers.get("x-forwarded-for").and_then(|v| v.to_str().ok());
        Ok(ClientIp(state.client_ip(peer, xff)))
    }
}

/// An authenticated request.
#[derive(Debug, Clone)]
pub struct Auth {
    pub principal: Principal,
    /// The session's CSRF token (cookie authentication only).
    pub csrf_token: Option<String>,
    pub ip: Option<String>,
}

impl Auth {
    pub fn user_id(&self) -> Option<i64> {
        self.principal.user_id
    }

    /// The user, or 403 for organization tokens on user endpoints.
    pub fn require_user(&self) -> ApiResult<i64> {
        self.principal.user_id.ok_or_else(|| {
            ApiError::new(
                axum::http::StatusCode::FORBIDDEN,
                "forbidden",
                "this endpoint needs a user, not an organization token",
            )
        })
    }

    /// For the audit log.
    pub fn token_id(&self) -> Option<i64> {
        self.principal.token.as_ref().map(|t| t.id)
    }

    /// True when authenticated by a session cookie (not a token).
    pub fn is_session(&self) -> bool {
        self.principal.session_id.is_some()
    }

    pub fn audit(&self, org_id: Option<i64>, action: &'static str) -> rupta_auth::audit::Entry {
        rupta_auth::audit::Entry {
            org_id,
            actor_user_id: self.principal.user_id,
            actor_token_id: self.token_id(),
            action,
            ip: self.ip.clone(),
            ..Default::default()
        }
    }
}

fn bearer(parts: &Parts) -> Option<&str> {
    let v = parts.headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = v.split_once(' ')?;
    (scheme.eq_ignore_ascii_case("bearer") || scheme.eq_ignore_ascii_case("token")).then(|| token.trim())
}

async fn authenticate(parts: &mut Parts, state: &ApiState) -> ApiResult<Option<Auth>> {
    let ip = ClientIp::from_request_parts(parts, state).await?.string();
    if let Some(token) = bearer(parts) {
        let token = token.to_string();
        let found = state.db(move |c| Ok(apitoken::authenticate(c, &token)?)).await?;
        let Some(t) = found else { return Err(ApiError::unauthorized()) };
        let principal =
            Principal { user_id: t.user_id, is_superuser: t.is_superuser, token: Some(t.grant), session_id: None };
        return Ok(Some(Auth { principal, csrf_token: None, ip }));
    }
    let cookie = parts
        .headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find_map(|h| session::read_cookie(h, session::SESSION_COOKIE).map(str::to_string));
    let Some(cookie) = cookie else { return Ok(None) };
    let found = state.db(move |c| Ok(session::lookup(c, &cookie)?)).await?;
    let Some(s) = found else { return Ok(None) };
    let mutating = !matches!(parts.method, Method::GET | Method::HEAD | Method::OPTIONS);
    if mutating {
        let given = parts.headers.get(session::CSRF_HEADER).and_then(|v| v.to_str().ok()).unwrap_or("");
        if !ct_eq(given.as_bytes(), s.csrf_token.as_bytes()) {
            return Err(ApiError::new(
                axum::http::StatusCode::FORBIDDEN,
                "csrf_failed",
                "missing or wrong X-CSRF-Token header",
            ));
        }
    }
    let principal =
        Principal { user_id: Some(s.user_id), is_superuser: s.is_superuser, token: None, session_id: Some(s.id) };
    Ok(Some(Auth { principal, csrf_token: Some(s.csrf_token), ip }))
}

impl<S> FromRequestParts<S> for Auth
where
    ApiState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        if let Some(a) = parts.extensions.get::<Auth>() {
            return Ok(a.clone());
        }
        let state = ApiState::from_ref(state);
        let auth = authenticate(parts, &state).await?.ok_or_else(ApiError::unauthorized)?;
        parts.extensions.insert(auth.clone());
        Ok(auth)
    }
}

/// Authentication if present.
#[derive(Debug, Clone)]
pub struct MaybeAuth(pub Option<Auth>);

impl<S> FromRequestParts<S> for MaybeAuth
where
    ApiState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let state = ApiState::from_ref(state);
        match authenticate(parts, &state).await {
            Ok(a) => Ok(MaybeAuth(a)),
            Err(e) if e.code == "unauthorized" => Ok(MaybeAuth(None)),
            Err(e) => Err(e),
        }
    }
}

/// A JSON body; malformed bodies answer `400 bad_request` in the API format.
#[derive(Debug, Clone)]
pub struct Json<T>(pub T);

impl<T, S> FromRequest<S> for Json<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match axum::Json::<T>::from_request(req, state).await {
            Ok(axum::Json(v)) => Ok(Json(v)),
            Err(rejection) => {
                let status = rejection.status();
                if status == axum::http::StatusCode::PAYLOAD_TOO_LARGE {
                    return Err(ApiError::new(status, "payload_too_large", "the request body is too large"));
                }
                Err(ApiError::bad_request(rejection.body_text()))
            }
        }
    }
}

/// A query string; malformed ones answer `400 bad_request`.
#[derive(Debug, Clone)]
pub struct Query<T>(pub T);

impl<T, S> FromRequestParts<S> for Query<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match axum::extract::Query::<T>::from_request_parts(parts, state).await {
            Ok(axum::extract::Query(v)) => Ok(Query(v)),
            Err(rejection) => Err(ApiError::bad_request(rejection.body_text())),
        }
    }
}

/// A path; malformed parameters (e.g. a non-numeric id) answer `404`.
#[derive(Debug, Clone)]
pub struct Path<T>(pub T);

impl<T, S> FromRequestParts<S> for Path<T>
where
    T: DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match axum::extract::Path::<T>::from_request_parts(parts, state).await {
            Ok(axum::extract::Path(v)) => Ok(Path(v)),
            Err(_) => Err(ApiError::not_found()),
        }
    }
}

/// `?cursor=&limit=`.
#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PageParams {
    /// Opaque cursor from `next_cursor` of the previous page.
    pub cursor: Option<String>,
    /// Page size, 1 to 100 (default 50).
    pub limit: Option<u32>,
}

impl PageParams {
    pub fn limit(&self) -> i64 {
        self.limit.unwrap_or(50).clamp(1, 100) as i64
    }

    /// The decoded cursor, or 400 if it is malformed.
    pub fn cursor<T: DeserializeOwned>(&self) -> ApiResult<Option<T>> {
        match &self.cursor {
            None => Ok(None),
            Some(c) if c.is_empty() => Ok(None),
            Some(c) => decode_cursor(c).map(Some).ok_or_else(|| ApiError::bad_request("invalid cursor")),
        }
    }
}

pub fn encode_cursor<T: Serialize>(v: &T) -> String {
    rupta_auth::crypto::b64(&serde_json::to_vec(v).expect("cursor serializes"))
}

pub fn decode_cursor<T: DeserializeOwned>(c: &str) -> Option<T> {
    if c.len() > 512 {
        return None;
    }
    serde_json::from_slice(&rupta_auth::crypto::unb64(c)?).ok()
}

/// A page of results.
#[derive(Debug, Serialize, ToSchema)]
pub struct Page<T> {
    pub data: Vec<T>,
    /// Cursor of the next page; `null` on the last page.
    pub next_cursor: Option<String>,
}

impl<T> Page<T> {
    /// Builds a page from up to `limit + 1` rows: the extra row only
    /// signals that there is a next page; `key` gives its cursor value.
    pub fn from_rows<K: Serialize>(mut rows: Vec<T>, limit: i64, key: impl Fn(&T) -> K) -> Page<T> {
        let next_cursor = if rows.len() as i64 > limit {
            rows.truncate(limit as usize);
            rows.last().map(|r| encode_cursor(&key(r)))
        } else {
            None
        };
        Page { data: rows, next_cursor }
    }
}

/// For `Option<Option<T>>` fields: absent = `None`, `null` = `Some(None)`.
pub fn double_option<'de, T: Deserialize<'de>, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}
