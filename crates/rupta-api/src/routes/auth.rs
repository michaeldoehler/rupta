//! `/api/v1/auth/*` (setup, login, logout, session, configuration) and the
//! single sign-on redirects under `/auth/sso/{provider}`.

use std::collections::HashMap;

use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use rupta_auth::audit::{self, action};
use rupta_auth::oidc::{INSTANCE_PROVIDER_ID, OidcSettings, STATE_COOKIE, STATE_TTL_SECS};
use rupta_auth::provision::{self, Policy, Provisioned};
use rupta_auth::{password, session, users};
use rupta_core::ids::Id;
use rupta_core::role::Role;
use rupta_store::squad::Connection;
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::error::{ApiError, ApiResult};
use crate::extract::{Auth, ClientIp, Json, MaybeAuth, Path, Query};
use crate::state::{ApiMetrics, ApiState};

pub fn routes() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(config))
        .routes(routes!(setup))
        .routes(routes!(login))
        .routes(routes!(logout))
        .routes(routes!(get_session))
        .routes(routes!(sso_start))
        .routes(routes!(sso_callback))
}

#[derive(Serialize, ToSchema)]
pub struct SsoProvider {
    pub id: String,
    pub name: String,
}

#[derive(Serialize, ToSchema)]
pub struct AuthConfig {
    /// Email and password login is available.
    pub local_login: bool,
    /// Single sign-on providers to offer as login buttons.
    pub sso: Vec<SsoProvider>,
    /// No user exists yet: the setup form should be shown.
    pub setup_required: bool,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ConfigParams {
    /// Also list this organization's single sign-on provider.
    pub org: Option<String>,
}

/// Login options.
#[utoipa::path(get, path = "/api/v1/auth/config", tag = "auth", params(ConfigParams),
    responses((status = 200, body = AuthConfig)))]
async fn config(State(state): State<ApiState>, Query(q): Query<ConfigParams>) -> ApiResult<axum::Json<AuthConfig>> {
    let instance = state.config.oidc.as_ref().map(|o| SsoProvider { id: o.id.clone(), name: o.name.clone() });
    let (count, org_sso) = state
        .db(move |c| {
            let n = users::count(c)?;
            let org = match q.org {
                Some(slug) => c
                    .query_opt(
                        "SELECT o.slug, s.provider_name FROM sso_configs s JOIN organizations o ON o.id = s.org_id WHERE o.slug = $1",
                        &[&slug],
                    )?
                    .map(|r| SsoProvider { id: format!("org-{}", r.get::<String>(0)), name: r.get(1) }),
                None => None,
            };
            Ok((n, org))
        })
        .await?;
    let sso = instance.into_iter().chain(org_sso).collect();
    Ok(axum::Json(AuthConfig { local_login: state.config.local_login, sso, setup_required: count == 0 }))
}

#[derive(Serialize, ToSchema, Clone)]
pub struct SessionUser {
    #[schema(value_type = String)]
    pub id: Id,
    pub email: String,
    pub name: String,
    pub avatar_url: Option<String>,
    pub is_superuser: bool,
}

#[derive(Serialize, ToSchema)]
pub struct SessionOrg {
    #[schema(value_type = String)]
    pub id: Id,
    pub slug: String,
    pub name: String,
    pub role: String,
    /// URL of the organization's icon, when it has one.
    pub icon_url: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct SessionInfo {
    pub user: SessionUser,
    /// Send as `X-CSRF-Token` on mutating requests; `null` for token auth.
    pub csrf_token: Option<String>,
    pub orgs: Vec<SessionOrg>,
}

pub(crate) fn session_info(conn: &Connection, user_id: i64, csrf_token: Option<String>) -> ApiResult<SessionInfo> {
    let user = users::get(conn, user_id)?.ok_or_else(ApiError::unauthorized)?;
    let orgs = if user.is_superuser {
        conn.query(
            "SELECT o.id, o.slug, o.name, coalesce(m.role, 'owner'), o.icon_updated_at FROM organizations o
             LEFT JOIN members m ON m.org_id = o.id AND m.user_id = $1 ORDER BY o.name, o.id",
            &[&user_id],
        )?
    } else {
        conn.query(
            "SELECT o.id, o.slug, o.name, m.role, o.icon_updated_at FROM members m JOIN organizations o ON o.id = m.org_id
             WHERE m.user_id = $1 ORDER BY o.name, o.id",
            &[&user_id],
        )?
    };
    Ok(SessionInfo {
        user: SessionUser {
            id: Id(user.id),
            email: user.email,
            name: user.name,
            avatar_url: user.avatar_url,
            is_superuser: user.is_superuser,
        },
        csrf_token,
        orgs: orgs
            .iter()
            .map(|r| {
                let slug: String = r.get(1);
                SessionOrg {
                    id: Id(r.get(0)),
                    icon_url: crate::routes::orgs::icon_url(&slug, r.get(4)),
                    slug,
                    name: r.get(2),
                    role: r.get(3),
                }
            })
            .collect(),
    })
}

fn with_session_cookie(state: &ApiState, token: &str, status: StatusCode, body: SessionInfo) -> Response {
    let mut resp = (status, axum::Json(body)).into_response();
    resp.headers_mut()
        .append(header::SET_COOKIE, HeaderValue::from_str(&session::cookie(token, state.secure_cookies())).unwrap());
    resp
}

#[derive(Deserialize, ToSchema)]
pub struct SetupRequest {
    pub email: String,
    #[serde(default)]
    pub name: String,
    pub password: String,
    pub org_name: String,
}

/// First start: creates the superuser and the first organization. Works
/// only while there are no users.
#[utoipa::path(post, path = "/api/v1/auth/setup", tag = "auth", request_body = SetupRequest,
    responses((status = 201, body = SessionInfo), (status = 409, body = crate::error::ErrorBody)))]
async fn setup(
    State(state): State<ApiState>,
    ip: ClientIp,
    headers: HeaderMap,
    Json(req): Json<SetupRequest>,
) -> ApiResult<Response> {
    let email =
        users::normalize_email(&req.email).ok_or_else(|| ApiError::invalid("email", "not a valid email address"))?;
    password::check_policy(&req.password).map_err(|e| ApiError::invalid("password", e))?;
    let org_name = req.org_name.trim().to_string();
    if org_name.is_empty() || org_name.len() > 100 {
        return Err(ApiError::invalid("org_name", "must have 1 to 100 characters"));
    }
    if req.name.len() > 100 {
        return Err(ApiError::invalid("name", "must have at most 100 characters"));
    }
    // Refuse cheaply before spending time on hashing.
    if state.db(|c| Ok(users::count(c)?)).await? > 0 {
        return Err(ApiError::conflict("setup is already done"));
    }
    let pw = req.password.clone();
    let hash = tokio::task::spawn_blocking(move || password::hash(&pw)).await.map_err(ApiError::internal)?;
    let ua = user_agent(&headers);
    let ip = ip.string();
    let name = req.name.trim().to_string();
    let (token, info) = state
        .db(move |c| {
            let slug = users::slugify(&org_name);
            let b = users::bootstrap(c, &email, &name, &hash, &slug, &org_name)?
                .ok_or_else(|| ApiError::conflict("setup is already done"))?;
            let s = session::create(c, b.user_id, ip.as_deref(), ua.as_deref())?;
            audit::record(
                c,
                &audit::Entry {
                    org_id: Some(b.org_id),
                    actor_user_id: Some(b.user_id),
                    action: action::SETUP,
                    target_type: Some("user"),
                    target_id: Some(b.user_id.to_string()),
                    ip: ip.clone(),
                    ..Default::default()
                },
            )?;
            Ok((s.token, session_info(c, b.user_id, Some(s.csrf_token))?))
        })
        .await?;
    Ok(with_session_cookie(&state, &token, StatusCode::CREATED, info))
}

fn user_agent(headers: &HeaderMap) -> Option<String> {
    headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).map(|s| s.chars().take(512).collect())
}

#[derive(Deserialize, ToSchema)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

enum LoginOutcome {
    Ok(String, SessionInfo),
    Invalid,
    SsoRequired(String),
}

/// Email and password login.
#[utoipa::path(post, path = "/api/v1/auth/login", tag = "auth", request_body = LoginRequest,
    responses((status = 200, body = SessionInfo), (status = 401, body = crate::error::ErrorBody),
        (status = 403, description = "local login disabled, or the organization requires single sign-on", body = crate::error::ErrorBody),
        (status = 429, body = crate::error::ErrorBody)))]
async fn login(
    State(state): State<ApiState>,
    ip: ClientIp,
    headers: HeaderMap,
    Json(req): Json<LoginRequest>,
) -> ApiResult<Response> {
    if !state.config.local_login {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "local_login_disabled",
            "email and password login is disabled; use single sign-on",
        ));
    }
    let email = req.email.trim().to_lowercase();
    if email.len() > 254 || req.password.len() > password::MAX_PASSWORD_LEN {
        return Err(invalid_credentials());
    }
    let ip_key = ip.string().unwrap_or_else(|| "unknown".into());
    if let Err(retry) = state.login_guard.check(&email, &ip_key) {
        return Err(ApiError::rate_limited(retry.as_secs().max(1)));
    }
    let ua = user_agent(&headers);
    let ipv = ip.string();
    let pw = req.password;
    let email2 = email.clone();
    let outcome = state
        .db(move |c| {
            let found = users::find_by_email(c, &email2)?;
            let user = match found {
                Some((u, Some(hash))) if password::verify(&pw, &hash) && !u.disabled => u,
                Some((u, _)) => {
                    audit::record(
                        c,
                        &audit::Entry {
                            actor_user_id: Some(u.id),
                            action: action::LOGIN_FAILED,
                            target_type: Some("user"),
                            target_id: Some(u.id.to_string()),
                            data: json!({ "email": email2 }),
                            ip: ipv.clone(),
                            ..Default::default()
                        },
                    )?;
                    return Ok(LoginOutcome::Invalid);
                }
                None => {
                    password::verify_dummy(&pw);
                    audit::record(
                        c,
                        &audit::Entry { action: action::LOGIN_FAILED, data: json!({ "email": email2 }), ip: ipv.clone(), ..Default::default() },
                    )?;
                    return Ok(LoginOutcome::Invalid);
                }
            };
            // Organizations that enforce single sign-on refuse passwords,
            // except for owners when break-glass login is allowed.
            let enforced = c.query_opt(
                "SELECT o.slug FROM sso_configs s JOIN members m ON m.org_id = s.org_id JOIN organizations o ON o.id = s.org_id
                 WHERE m.user_id = $1 AND s.enforce AND NOT (m.role = 'owner' AND s.owner_local_login)
                 ORDER BY o.slug LIMIT 1",
                &[&user.id],
            )?;
            if let Some(row) = enforced {
                return Ok(LoginOutcome::SsoRequired(row.get(0)));
            }
            let s = session::create(c, user.id, ipv.as_deref(), ua.as_deref())?;
            audit::record(
                c,
                &audit::Entry {
                    actor_user_id: Some(user.id),
                    action: action::LOGIN,
                    target_type: Some("user"),
                    target_id: Some(user.id.to_string()),
                    data: json!({ "method": "password" }),
                    ip: ipv.clone(),
                    ..Default::default()
                },
            )?;
            Ok(LoginOutcome::Ok(s.token, session_info(c, user.id, Some(s.csrf_token))?))
        })
        .await?;
    match outcome {
        LoginOutcome::Ok(token, info) => {
            state.login_guard.success(&email);
            ApiMetrics::inc(&state.metrics.logins);
            Ok(with_session_cookie(&state, &token, StatusCode::OK, info))
        }
        LoginOutcome::Invalid => {
            ApiMetrics::inc(&state.metrics.login_failures);
            if state.login_guard.failure(&email, &ip_key) {
                ApiMetrics::inc(&state.metrics.login_lockouts);
                let ipv = ip.string();
                let email = email.clone();
                state
                    .db(move |c| {
                        Ok(audit::record(
                            c,
                            &audit::Entry {
                                action: action::LOGIN_LOCKED,
                                data: json!({ "email": email }),
                                ip: ipv,
                                ..Default::default()
                            },
                        )?)
                    })
                    .await?;
            }
            Err(invalid_credentials())
        }
        LoginOutcome::SsoRequired(org) => {
            Err(ApiError::new(StatusCode::FORBIDDEN, "sso_required", "your organization requires single sign-on")
                .with_details(json!({ "provider": format!("org-{org}"), "org": org })))
        }
    }
}

fn invalid_credentials() -> ApiError {
    ApiError::new(StatusCode::UNAUTHORIZED, "invalid_credentials", "wrong email or password")
}

/// Ends the session.
#[utoipa::path(post, path = "/api/v1/auth/logout", tag = "auth", responses((status = 204)))]
async fn logout(State(state): State<ApiState>, MaybeAuth(auth): MaybeAuth) -> ApiResult<Response> {
    if let Some(auth) = auth
        && let Some(sid) = auth.principal.session_id.clone()
    {
        let entry = audit::Entry {
            target_type: Some("user"),
            target_id: auth.user_id().map(|u| u.to_string()),
            ..auth.audit(None, action::LOGOUT)
        };
        state
            .db(move |c| {
                session::delete(c, &sid)?;
                audit::record(c, &entry)?;
                Ok(())
            })
            .await?;
    }
    let mut resp = StatusCode::NO_CONTENT.into_response();
    resp.headers_mut()
        .append(header::SET_COOKIE, HeaderValue::from_str(&session::clear_cookie(state.secure_cookies())).unwrap());
    Ok(resp)
}

/// The signed-in user, the CSRF token and the user's organizations.
#[utoipa::path(get, path = "/api/v1/auth/session", tag = "auth",
    responses((status = 200, body = SessionInfo), (status = 401, body = crate::error::ErrorBody)))]
async fn get_session(State(state): State<ApiState>, auth: Auth) -> ApiResult<axum::Json<SessionInfo>> {
    let uid = auth.require_user()?;
    let csrf = auth.csrf_token.clone();
    Ok(axum::Json(state.db(move |c| session_info(c, uid, csrf)).await?))
}

/// A provider with what provisioning needs.
pub(crate) struct ProviderInfo {
    pub settings: OidcSettings,
    pub provider_key: String,
    pub org_id: Option<i64>,
}

/// Loads a provider: `oidc` (environment) or `org-{slug}` (database).
pub(crate) fn load_provider(state: &ApiState, conn: &Connection, id: &str) -> ApiResult<Option<ProviderInfo>> {
    if id == INSTANCE_PROVIDER_ID {
        return Ok(state.config.oidc.clone().map(|settings| ProviderInfo {
            settings,
            provider_key: INSTANCE_PROVIDER_ID.to_string(),
            org_id: None,
        }));
    }
    let Some(slug) = id.strip_prefix("org-") else { return Ok(None) };
    let row = conn.query_opt(
        "SELECT s.org_id, s.provider_name, s.issuer, s.client_id, s.client_secret_enc, s.scopes, s.role_claim,
                s.role_mapping, s.default_role, s.jit_provisioning
         FROM sso_configs s JOIN organizations o ON o.id = s.org_id WHERE o.slug = $1",
        &[&slug],
    )?;
    let Some(row) = row else { return Ok(None) };
    let org_id: i64 = row.get(0);
    let enc: String = row.get(4);
    let secret = if enc.is_empty() {
        None
    } else {
        Some(state.keys.decrypt(&format!("sso:{org_id}"), &enc).and_then(|b| String::from_utf8(b).ok()).ok_or_else(
            || ApiError::internal("cannot decrypt the SSO client secret (was RUPTA_SECRET_KEY changed?)"),
        )?)
    };
    let mapping: serde_json::Value = row.get(7);
    let role_mapping = mapping
        .as_object()
        .map(|m| m.iter().filter_map(|(k, v)| Some((k.clone(), Role::parse(v.as_str()?)?))).collect())
        .unwrap_or_default();
    Ok(Some(ProviderInfo {
        settings: OidcSettings {
            id: id.to_string(),
            name: row.get(1),
            issuer: row.get(2),
            client_id: row.get(3),
            client_secret: secret,
            scopes: row.get::<String>(5).split_whitespace().map(str::to_string).collect(),
            role_claim: row.get::<Option<String>>(6).filter(|s| !s.is_empty()),
            role_mapping,
            default_role: rupta_auth::oidc::parse_default_role(&row.get::<String>(8)).unwrap_or(Some(Role::Member)),
            jit_provisioning: row.get(9),
            // Organization providers accept any additional audience; the
            // authorized party (azp) check still applies.
            allowed_audiences: Vec::new(),
        },
        provider_key: format!("org:{org_id}"),
        org_id: Some(org_id),
    }))
}

fn redirect_uri(state: &ApiState, provider: &str) -> String {
    format!("{}/auth/sso/{provider}/callback", state.config.public_url)
}

fn state_cookie(state: &ApiState, value: &str, max_age: i64) -> HeaderValue {
    HeaderValue::from_str(&format!(
        "{STATE_COOKIE}={value}; Path=/auth/sso; Max-Age={max_age}; HttpOnly; SameSite=Lax{}",
        if state.secure_cookies() { "; Secure" } else { "" }
    ))
    .unwrap()
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct StartParams {
    /// Local path to open after signing in.
    pub next: Option<String>,
}

/// Redirects to the provider.
#[utoipa::path(get, path = "/auth/sso/{provider}/start", tag = "auth",
    params(("provider" = String, Path, description = "`oidc` or `org-{slug}`"), StartParams),
    responses((status = 302, description = "to the provider"), (status = 404, body = crate::error::ErrorBody)))]
async fn sso_start(
    State(state): State<ApiState>,
    Path(provider): Path<String>,
    Query(q): Query<StartParams>,
) -> ApiResult<Response> {
    let st = state.clone();
    let id = provider.clone();
    let info = state.db(move |c| load_provider(&st, c, &id)).await?.ok_or_else(ApiError::not_found)?;
    let started = state
        .oidc
        .start(&state.keys, &info.settings, &redirect_uri(&state, &provider), q.next.as_deref())
        .await
        .map_err(|e| {
            tracing::warn!(provider, error = %e, "single sign-on start failed");
            ApiError::new(StatusCode::BAD_GATEWAY, "sso_unavailable", "the identity provider is not reachable")
        })?;
    let mut resp = Redirect::to(&started.authorize_url).into_response();
    *resp.status_mut() = StatusCode::FOUND;
    resp.headers_mut().append(header::SET_COOKIE, state_cookie(&state, &started.state_cookie, STATE_TTL_SECS));
    Ok(resp)
}

/// Completes single sign-on and redirects into the app.
#[utoipa::path(get, path = "/auth/sso/{provider}/callback", tag = "auth",
    params(("provider" = String, Path, description = "`oidc` or `org-{slug}`")),
    responses((status = 302, description = "into the app; on failure to `/login?error=sso_no_access` when the user's claims map to no role and the default role is `none`, else to `/login?error=sso_failed`")))]
async fn sso_callback(
    State(state): State<ApiState>,
    Path(provider): Path<String>,
    ip: ClientIp,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<HashMap<String, String>>,
) -> Response {
    let cookie = headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find_map(|h| session::read_cookie(h, STATE_COOKIE).map(str::to_string));
    let result = sso_finish(&state, &provider, &ip, &headers, &query, cookie.as_deref()).await;
    let (location, session_cookie) = match result {
        Ok((next, token)) => (next, Some(token)),
        Err(failure) => {
            ApiMetrics::inc(&state.metrics.sso_failures);
            let SsoFailure { reason, error, org_id, user_id, subject, email } = failure;
            tracing::warn!(provider, reason, "single sign-on failed");
            let ipv = ip.string();
            let provider2 = provider.clone();
            let mut data = json!({ "provider": provider2, "reason": reason });
            if let Some(s) = subject {
                data["subject"] = json!(s);
            }
            if let Some(e) = email {
                data["email"] = json!(e);
            }
            let _ = state
                .db(move |c| {
                    Ok(audit::record(
                        c,
                        &audit::Entry {
                            org_id,
                            action: action::SSO_LOGIN_FAILED,
                            target_type: user_id.map(|_| "user"),
                            target_id: user_id.map(|u| u.to_string()),
                            data,
                            ip: ipv,
                            ..Default::default()
                        },
                    )?)
                })
                .await;
            (format!("/login?error={error}"), None)
        }
    };
    let mut resp = Redirect::to(&location).into_response();
    *resp.status_mut() = StatusCode::FOUND;
    resp.headers_mut().append(header::SET_COOKIE, state_cookie(&state, "", 0));
    if let Some(token) = session_cookie {
        resp.headers_mut().append(
            header::SET_COOKIE,
            HeaderValue::from_str(&session::cookie(&token, state.secure_cookies())).unwrap(),
        );
    }
    resp
}

/// Why a single sign-on callback failed, for the log, the audit log and
/// the login page.
struct SsoFailure {
    reason: String,
    /// `error` query parameter of the login page.
    error: &'static str,
    org_id: Option<i64>,
    user_id: Option<i64>,
    subject: Option<String>,
    email: Option<String>,
}

impl From<String> for SsoFailure {
    fn from(reason: String) -> SsoFailure {
        SsoFailure { reason, error: "sso_failed", org_id: None, user_id: None, subject: None, email: None }
    }
}

/// Returns (next path, session token) or why it failed.
async fn sso_finish(
    state: &ApiState,
    provider: &str,
    ip: &ClientIp,
    headers: &HeaderMap,
    query: &HashMap<String, String>,
    cookie: Option<&str>,
) -> Result<(String, String), SsoFailure> {
    let st = state.clone();
    let id = provider.to_string();
    let info = state
        .db(move |c| load_provider(&st, c, &id))
        .await
        .map_err(|e| e.message)?
        .ok_or_else(|| "unknown provider".to_string())?;
    let identity = state
        .oidc
        .finish(&state.keys, &info.settings, &redirect_uri(state, provider), query, cookie)
        .await
        .map_err(|e| e.to_string())?;
    let default_org = state.config.default_org.clone();
    let ipv = ip.string();
    let ua = user_agent(headers);
    let provider = provider.to_string();
    let (next, token) = state
        .db(move |c| {
            let org_id = match info.org_id {
                Some(id) => id,
                None => match users::org_id_by_slug(c, &default_org)? {
                    Some(id) => id,
                    None => users::create_org(c, &default_org, &default_org)?,
                },
            };
            let policy = Policy {
                provider_key: &info.provider_key,
                org_id,
                role_mapping: &info.settings.role_mapping,
                default_role: info.settings.default_role,
                jit_provisioning: info.settings.jit_provisioning,
            };
            match provision::provision(c, &policy, &identity)? {
                Provisioned::Refused(reason) => Ok(Err(SsoFailure::from(reason.to_string()))),
                Provisioned::NoRole { user_id } => Ok(Err(SsoFailure {
                    reason: "no role is mapped for the user's claims and the default role is none".to_string(),
                    error: "sso_no_access",
                    org_id: Some(org_id),
                    user_id,
                    subject: Some(identity.subject.clone()),
                    email: identity.email.clone(),
                })),
                Provisioned::User { user_id, created, linked, role, role_change } => {
                    if let Some((old, new)) = role_change {
                        audit::record(
                            c,
                            &audit::Entry {
                                org_id: Some(org_id),
                                action: if old.is_none() { action::MEMBER_JOIN } else { action::MEMBER_ROLE_CHANGE },
                                target_type: Some("user"),
                                target_id: Some(user_id.to_string()),
                                data: json!({ "old_role": old.map(|r| r.as_str()), "role": new.as_str(), "via": provider }),
                                ip: ipv.clone(),
                                ..Default::default()
                            },
                        )?;
                    }
                    let s = session::create(c, user_id, ipv.as_deref(), ua.as_deref())?;
                    audit::record(
                        c,
                        &audit::Entry {
                            org_id: Some(org_id),
                            actor_user_id: Some(user_id),
                            action: action::SSO_LOGIN,
                            target_type: Some("user"),
                            target_id: Some(user_id.to_string()),
                            data: json!({ "provider": provider, "created": created, "linked": linked, "role": role.as_str() }),
                            ip: ipv.clone(),
                            ..Default::default()
                        },
                    )?;
                    Ok(Ok((identity.next.clone(), s.token)))
                }
            }
        })
        .await
        .map_err(|e| SsoFailure::from(e.message))??;
    ApiMetrics::inc(&state.metrics.sso_logins);
    Ok((next, token))
}
