//! The signed-in user (`/api/v1/user`) and API tokens of users and
//! organizations.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Duration, Utc};
use rupta_auth::audit::{self, action};
use rupta_auth::rbac::{self, Permission};
use rupta_auth::{apitoken, password, session, users};
use rupta_core::ids::Id;
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::access::org_sync;
use crate::error::{ApiError, ApiResult};
use crate::extract::{Auth, Json, Path};
use crate::preferences::{Preferences, PreferencesPatch};
use crate::routes::auth::SessionUser;
use crate::routes::orgs::check_name;
use crate::state::ApiState;

pub fn routes() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(get_user, update_user))
        .routes(routes!(list_user_tokens, create_user_token))
        .routes(routes!(delete_user_token))
        .routes(routes!(list_org_tokens, create_org_token))
        .routes(routes!(delete_org_token))
        .routes(routes!(list_scopes))
}

/// The signed-in user with their preferences.
#[derive(Serialize, ToSchema)]
pub struct CurrentUser {
    #[serde(flatten)]
    pub user: SessionUser,
    pub preferences: Preferences,
}

fn current_user(c: &rupta_store::squad::Connection, uid: i64) -> ApiResult<CurrentUser> {
    let u = users::get(c, uid)?.ok_or_else(ApiError::unauthorized)?;
    let prefs: serde_json::Value = c.query_one("SELECT preferences FROM users WHERE id = $1", &[&uid])?.get(0);
    Ok(CurrentUser {
        user: SessionUser {
            id: Id(u.id),
            email: u.email,
            name: u.name,
            avatar_url: u.avatar_url,
            is_superuser: u.is_superuser,
        },
        preferences: Preferences::from_value(&prefs),
    })
}

/// The signed-in user.
#[utoipa::path(get, path = "/api/v1/user", tag = "user", responses((status = 200, body = CurrentUser)))]
async fn get_user(State(state): State<ApiState>, auth: Auth) -> ApiResult<axum::Json<CurrentUser>> {
    let uid = auth.require_user()?;
    Ok(axum::Json(state.db(move |c| current_user(c, uid)).await?))
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateUser {
    pub name: Option<String>,
    pub avatar_url: Option<String>,
    /// To change the password, give the current and the new one.
    pub current_password: Option<String>,
    pub new_password: Option<String>,
    /// Only the given fields change; each is validated.
    pub preferences: Option<PreferencesPatch>,
}

/// Changes name, avatar, preferences or password. A password change ends
/// all other sessions of the user.
#[utoipa::path(patch, path = "/api/v1/user", tag = "user", request_body = UpdateUser,
    responses((status = 200, body = CurrentUser), (status = 422, body = crate::error::ErrorBody)))]
async fn update_user(
    State(state): State<ApiState>,
    auth: Auth,
    Json(req): Json<UpdateUser>,
) -> ApiResult<axum::Json<CurrentUser>> {
    let uid = auth.require_user()?;
    if auth.principal.token.is_some() && req.new_password.is_some() {
        return Err(ApiError::forbidden().with_details(json!({"reason": "passwords can only be changed in a session"})));
    }
    let name = req.name.as_deref().map(|n| n.trim().chars().take(100).collect::<String>());
    if let Some(a) = &req.avatar_url
        && !(a.is_empty() || a.starts_with("https://") || a.starts_with("http://"))
    {
        return Err(ApiError::invalid("avatar_url", "must be an http(s) URL"));
    }
    // Validate the preferences before anything is written.
    if let Some(p) = &req.preferences {
        Preferences::default().apply(p)?;
    }
    let new_hash = match (&req.current_password, &req.new_password) {
        (_, None) => None,
        (current, Some(new)) => {
            password::check_policy(new).map_err(|e| ApiError::invalid("new_password", e))?;
            let stored: Option<String> = state
                .db(move |c| Ok(c.query_one("SELECT password_hash FROM users WHERE id = $1", &[&uid])?.get(0)))
                .await?;
            let (current, new) = (current.clone().unwrap_or_default(), new.clone());
            let hash = tokio::task::spawn_blocking(move || {
                // Accounts created through single sign-on have no password yet.
                let ok = match &stored {
                    Some(h) => password::verify(&current, h),
                    None => true,
                };
                ok.then(|| password::hash(&new))
            })
            .await
            .map_err(ApiError::internal)?;
            Some(hash.ok_or_else(|| ApiError::invalid("current_password", "is wrong"))?)
        }
    };
    let avatar = req.avatar_url.clone().map(|a| if a.is_empty() { None } else { Some(a) });
    let prefs = req.preferences;
    let u = state
        .db(move |c| {
            let txn = c.transaction()?;
            if let Some(n) = &name {
                txn.execute("UPDATE users SET name = $1 WHERE id = $2", &[n, &uid])?;
            }
            if let Some(a) = &avatar {
                txn.execute("UPDATE users SET avatar_url = $1 WHERE id = $2", &[a, &uid])?;
            }
            if let Some(p) = &prefs {
                let stored: serde_json::Value =
                    txn.query_one("SELECT preferences FROM users WHERE id = $1", &[&uid])?.get(0);
                let merged = Preferences::from_value(&stored).apply(p)?;
                txn.execute("UPDATE users SET preferences = $1 WHERE id = $2", &[&merged.to_value(), &uid])?;
            }
            if let Some(h) = &new_hash {
                txn.execute("UPDATE users SET password_hash = $1 WHERE id = $2", &[h, &uid])?;
                session::delete_for_user(&txn, uid, auth.principal.session_id.as_deref())?;
                audit::record(
                    &txn,
                    &audit::Entry {
                        target_type: Some("user"),
                        target_id: Some(uid.to_string()),
                        ..auth.audit(None, action::PASSWORD_CHANGE)
                    },
                )?;
            }
            let u = current_user(&txn, uid)?;
            txn.commit()?;
            Ok(u)
        })
        .await?;
    Ok(axum::Json(u))
}

#[derive(Serialize, ToSchema)]
pub struct ApiToken {
    #[schema(value_type = String)]
    pub id: Id,
    pub name: String,
    /// Last four characters, for recognition.
    pub token_hint: String,
    pub scopes: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, ToSchema)]
pub struct CreatedToken {
    #[serde(flatten)]
    pub info: ApiToken,
    /// The token. Shown only once.
    pub token: String,
}

#[derive(Deserialize, ToSchema)]
pub struct CreateToken {
    pub name: String,
    pub scopes: Vec<String>,
    /// Days until the token expires; none = never.
    pub expires_in_days: Option<u32>,
}

#[derive(Serialize, ToSchema)]
pub struct ScopeInfo {
    pub scope: String,
    pub description: String,
}

/// Every token scope.
#[utoipa::path(get, path = "/api/v1/scopes", tag = "user", responses((status = 200, body = Vec<ScopeInfo>)))]
async fn list_scopes() -> axum::Json<Vec<ScopeInfo>> {
    axum::Json(
        rbac::SCOPES.iter().map(|(s, d)| ScopeInfo { scope: s.to_string(), description: d.to_string() }).collect(),
    )
}

fn check_create(req: &CreateToken) -> ApiResult<(String, Vec<String>, Option<DateTime<Utc>>)> {
    let name = check_name("name", &req.name, 100)?;
    if req.scopes.is_empty() {
        return Err(ApiError::invalid("scopes", "give at least one scope"));
    }
    let mut scopes = req.scopes.clone();
    scopes.sort();
    scopes.dedup();
    if let Some(bad) = scopes.iter().find(|s| !rbac::is_scope(s)) {
        return Err(ApiError::invalid("scopes", format!("unknown scope {bad:?}")));
    }
    let expires_at = match req.expires_in_days {
        Some(0) | None => None,
        Some(d) if d > 3650 => return Err(ApiError::invalid("expires_in_days", "at most 3650")),
        Some(d) => Some(Utc::now() + Duration::days(d as i64)),
    };
    Ok((name, scopes, expires_at))
}

fn token_rows(rows: Vec<rupta_store::squad::Row>) -> Vec<ApiToken> {
    rows.iter()
        .map(|r| ApiToken {
            id: Id(r.get(0)),
            name: r.get(1),
            token_hint: r.get(2),
            scopes: serde_json::from_value(r.get::<serde_json::Value>(3)).unwrap_or_default(),
            created_at: r.get(4),
            last_used_at: r.get(5),
            expires_at: r.get(6),
        })
        .collect()
}

const TOKEN_COLUMNS: &str = "id, name, token_hint, scopes, created_at, last_used_at, expires_at";

fn require_session(auth: &Auth) -> ApiResult<i64> {
    let uid = auth.require_user()?;
    // Tokens cannot mint tokens: a leaked token must not be able to
    // extend its own lifetime or scopes.
    if auth.principal.token.is_some() {
        return Err(ApiError::forbidden().with_details(json!({"reason": "tokens are managed in a browser session"})));
    }
    Ok(uid)
}

/// The caller's personal tokens.
#[utoipa::path(get, path = "/api/v1/user/tokens", tag = "user", responses((status = 200, body = Vec<ApiToken>)))]
async fn list_user_tokens(State(state): State<ApiState>, auth: Auth) -> ApiResult<axum::Json<Vec<ApiToken>>> {
    let uid = auth.require_user()?;
    let rows = state
        .db(move |c| {
            Ok(c.query(&format!("SELECT {TOKEN_COLUMNS} FROM api_tokens WHERE user_id = $1 ORDER BY id"), &[&uid])?)
        })
        .await?;
    Ok(axum::Json(token_rows(rows)))
}

/// Creates a personal token. It acts as the user, limited by its scopes.
#[utoipa::path(post, path = "/api/v1/user/tokens", tag = "user", request_body = CreateToken,
    responses((status = 201, body = CreatedToken)))]
async fn create_user_token(
    State(state): State<ApiState>,
    auth: Auth,
    Json(req): Json<CreateToken>,
) -> ApiResult<Response> {
    let uid = require_session(&auth)?;
    let (name, scopes, expires_at) = check_create(&req)?;
    let created = state
        .db(move |c| {
            let t = apitoken::create(c, &name, Some(uid), None, &scopes, expires_at)?;
            audit::record(
                c,
                &audit::Entry {
                    target_type: Some("token"),
                    target_id: Some(t.id.to_string()),
                    data: json!({"name": name, "scopes": scopes, "kind": "user"}),
                    ..auth.audit(None, action::TOKEN_CREATE)
                },
            )?;
            Ok(CreatedToken {
                info: ApiToken {
                    id: Id(t.id),
                    name,
                    token_hint: t.hint,
                    scopes,
                    created_at: t.created_at,
                    last_used_at: None,
                    expires_at,
                },
                token: t.token,
            })
        })
        .await?;
    Ok((StatusCode::CREATED, axum::Json(created)).into_response())
}

/// Revokes a personal token.
#[utoipa::path(delete, path = "/api/v1/user/tokens/{id}", tag = "user", params(("id" = String, Path)), responses((status = 204)))]
async fn delete_user_token(State(state): State<ApiState>, auth: Auth, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    let uid = auth.require_user()?;
    state
        .db(move |c| {
            let n = c.execute("DELETE FROM api_tokens WHERE id = $1 AND user_id = $2", &[&id, &uid])?;
            if n == 0 {
                return Err(ApiError::not_found());
            }
            audit::record(
                c,
                &audit::Entry {
                    target_type: Some("token"),
                    target_id: Some(id.to_string()),
                    data: json!({"kind": "user"}),
                    ..auth.audit(None, action::TOKEN_DELETE)
                },
            )?;
            Ok(())
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Organization tokens (for CI and the upload CLI).
#[utoipa::path(get, path = "/api/v1/orgs/{org}/tokens", tag = "orgs", params(("org" = String, Path)),
    responses((status = 200, body = Vec<ApiToken>)))]
async fn list_org_tokens(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
) -> ApiResult<axum::Json<Vec<ApiToken>>> {
    let rows = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ManageOrgTokens)?;
            Ok(c.query(&format!("SELECT {TOKEN_COLUMNS} FROM api_tokens WHERE org_id = $1 ORDER BY id"), &[&o.id])?)
        })
        .await?;
    Ok(axum::Json(token_rows(rows)))
}

/// Creates an organization token. It acts in this organization only, with
/// at most the admin role.
#[utoipa::path(post, path = "/api/v1/orgs/{org}/tokens", tag = "orgs", params(("org" = String, Path)), request_body = CreateToken,
    responses((status = 201, body = CreatedToken)))]
async fn create_org_token(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
    Json(req): Json<CreateToken>,
) -> ApiResult<Response> {
    require_session(&auth)?;
    let (name, scopes, expires_at) = check_create(&req)?;
    let created = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ManageOrgTokens)?;
            let t = apitoken::create(c, &name, None, Some(o.id), &scopes, expires_at)?;
            audit::record(
                c,
                &audit::Entry {
                    target_type: Some("token"),
                    target_id: Some(t.id.to_string()),
                    data: json!({"name": name, "scopes": scopes, "kind": "org"}),
                    ..auth.audit(Some(o.id), action::TOKEN_CREATE)
                },
            )?;
            Ok(CreatedToken {
                info: ApiToken {
                    id: Id(t.id),
                    name,
                    token_hint: t.hint,
                    scopes,
                    created_at: t.created_at,
                    last_used_at: None,
                    expires_at,
                },
                token: t.token,
            })
        })
        .await?;
    Ok((StatusCode::CREATED, axum::Json(created)).into_response())
}

/// Revokes an organization token.
#[utoipa::path(delete, path = "/api/v1/orgs/{org}/tokens/{id}", tag = "orgs",
    params(("org" = String, Path), ("id" = String, Path)), responses((status = 204)))]
async fn delete_org_token(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, id)): Path<(String, i64)>,
) -> ApiResult<StatusCode> {
    state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ManageOrgTokens)?;
            let n = c.execute("DELETE FROM api_tokens WHERE id = $1 AND org_id = $2", &[&id, &o.id])?;
            if n == 0 {
                return Err(ApiError::not_found());
            }
            audit::record(
                c,
                &audit::Entry {
                    target_type: Some("token"),
                    target_id: Some(id.to_string()),
                    data: json!({"kind": "org"}),
                    ..auth.audit(Some(o.id), action::TOKEN_DELETE)
                },
            )?;
            Ok(())
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
