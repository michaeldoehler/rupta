//! The audit log and per-organization single sign-on settings.

use axum::extract::State;
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use rupta_auth::audit::{self, action};
use rupta_auth::rbac::Permission;
use rupta_core::ids::Id;
use rupta_core::role::Role;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::access::org_sync;
use crate::error::{ApiError, ApiResult};
use crate::extract::{Auth, Json, Page, PageParams, Path, Query};
use crate::routes::orgs::check_name;
use crate::state::ApiState;

pub fn routes() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new().routes(routes!(audit_log)).routes(routes!(get_sso, put_sso, delete_sso))
}

#[derive(Serialize, ToSchema)]
pub struct AuditActor {
    #[schema(value_type = Option<String>)]
    pub user_id: Option<Id>,
    pub email: Option<String>,
    #[schema(value_type = Option<String>)]
    pub token_id: Option<Id>,
}

#[derive(Serialize, ToSchema)]
pub struct AuditEntry {
    #[schema(value_type = String)]
    pub id: Id,
    pub action: String,
    pub actor: AuditActor,
    pub target_type: Option<String>,
    pub target_id: Option<String>,
    #[schema(value_type = Object)]
    pub data: Value,
    pub ip: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AuditParams {
    /// Only this action, e.g. `auth.login_failed`.
    pub action: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

/// The organization's audit log, newest first. Includes the logins,
/// logouts and failed logins of its members.
#[utoipa::path(get, path = "/api/v1/orgs/{org}/audit-log", tag = "orgs", params(("org" = String, Path), AuditParams),
    responses((status = 200, body = Page<AuditEntry>)))]
async fn audit_log(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
    Query(q): Query<AuditParams>,
) -> ApiResult<axum::Json<Page<AuditEntry>>> {
    let page = PageParams { cursor: q.cursor.clone(), limit: q.limit };
    let before: Option<i64> = page.cursor()?;
    let limit = page.limit();
    let rows = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ReadAuditLog)?;
            Ok(c.query(
                "SELECT a.id, a.action, a.actor_user_id, u.email, a.actor_token_id, a.target_type, a.target_id, a.data, a.ip, a.created_at
                 FROM audit_log a LEFT JOIN users u ON u.id = a.actor_user_id
                 WHERE (a.org_id = $1 OR (a.org_id IS NULL AND a.actor_user_id IN (SELECT user_id FROM members WHERE org_id = $1)))
                   AND ($2::bigint IS NULL OR a.id < $2) AND ($3::text IS NULL OR a.action = $3)
                 ORDER BY a.id DESC LIMIT $4",
                &[&o.id, &before, &q.action, &(limit + 1)],
            )?)
        })
        .await?;
    let entries = rows
        .iter()
        .map(|r| AuditEntry {
            id: Id(r.get(0)),
            action: r.get(1),
            actor: AuditActor {
                user_id: r.get::<Option<i64>>(2).map(Id),
                email: r.get(3),
                token_id: r.get::<Option<i64>>(4).map(Id),
            },
            target_type: r.get(5),
            target_id: r.get(6),
            data: r.get(7),
            ip: r.get(8),
            created_at: r.get(9),
        })
        .collect();
    Ok(axum::Json(Page::from_rows(entries, limit, |e| e.id.0)))
}

#[derive(Serialize, ToSchema)]
pub struct SsoConfig {
    /// Provider id for `/auth/sso/{id}/start`: `org-{slug}`.
    pub provider_id: String,
    pub provider_name: String,
    pub issuer: String,
    pub client_id: String,
    /// The secret is never returned.
    pub has_client_secret: bool,
    pub scopes: String,
    pub role_claim: Option<String>,
    /// Provider role name → rupta role.
    pub role_mapping: BTreeMap<String, String>,
    pub default_role: String,
    pub jit_provisioning: bool,
    /// Members must use single sign-on; password login is refused.
    pub enforce: bool,
    /// With `enforce`, owners may still use their password (break-glass).
    pub owner_local_login: bool,
    pub redirect_uri: String,
    pub updated_at: DateTime<Utc>,
}

fn load_sso(
    state: &ApiState,
    c: &rupta_store::squad::Connection,
    org_id: i64,
    slug: &str,
) -> ApiResult<Option<SsoConfig>> {
    let row = c.query_opt(
        "SELECT provider_name, issuer, client_id, client_secret_enc <> '', scopes, role_claim, role_mapping, default_role,
                jit_provisioning, enforce, owner_local_login, updated_at FROM sso_configs WHERE org_id = $1",
        &[&org_id],
    )?;
    Ok(row.map(|r| {
        let mapping: Value = r.get(6);
        let provider_id = format!("org-{slug}");
        SsoConfig {
            redirect_uri: format!("{}/auth/sso/{provider_id}/callback", state.config.public_url),
            provider_id,
            provider_name: r.get(0),
            issuer: r.get(1),
            client_id: r.get(2),
            has_client_secret: r.get(3),
            scopes: r.get(4),
            role_claim: r.get(5),
            role_mapping: mapping
                .as_object()
                .map(|m| m.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string())).collect())
                .unwrap_or_default(),
            default_role: r.get(7),
            jit_provisioning: r.get(8),
            enforce: r.get(9),
            owner_local_login: r.get(10),
            updated_at: r.get(11),
        }
    }))
}

/// The organization's single sign-on configuration (404 if none).
#[utoipa::path(get, path = "/api/v1/orgs/{org}/sso", tag = "orgs", params(("org" = String, Path)),
    responses((status = 200, body = SsoConfig), (status = 404, body = crate::error::ErrorBody)))]
async fn get_sso(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
) -> ApiResult<axum::Json<SsoConfig>> {
    let st = state.clone();
    let cfg = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ManageSso)?;
            load_sso(&st, c, o.id, &o.slug)?.ok_or_else(ApiError::not_found)
        })
        .await?;
    Ok(axum::Json(cfg))
}

#[derive(Deserialize, ToSchema)]
pub struct PutSso {
    #[serde(default = "default_provider_name")]
    pub provider_name: String,
    pub issuer: String,
    pub client_id: String,
    /// Omit to keep the stored secret; empty string for a public client.
    pub client_secret: Option<String>,
    pub scopes: Option<String>,
    pub role_claim: Option<String>,
    #[serde(default)]
    pub role_mapping: BTreeMap<String, String>,
    #[serde(default = "default_role")]
    pub default_role: String,
    #[serde(default = "yes")]
    pub jit_provisioning: bool,
    #[serde(default)]
    pub enforce: bool,
    #[serde(default = "yes")]
    pub owner_local_login: bool,
}

fn default_provider_name() -> String {
    "Single sign-on".into()
}
fn default_role() -> String {
    "member".into()
}
fn yes() -> bool {
    true
}

/// Creates or replaces the single sign-on configuration. The issuer's
/// discovery document is fetched to check the settings. Owners only.
#[utoipa::path(put, path = "/api/v1/orgs/{org}/sso", tag = "orgs", params(("org" = String, Path)), request_body = PutSso,
    responses((status = 200, body = SsoConfig), (status = 422, body = crate::error::ErrorBody)))]
async fn put_sso(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
    Json(req): Json<PutSso>,
) -> ApiResult<axum::Json<SsoConfig>> {
    // Authorize before fetching anything from the given issuer.
    state.org(&auth, &org, Permission::ManageSso).await?;
    let name = check_name("provider_name", &req.provider_name, 100)?;
    let issuer = req.issuer.trim().trim_end_matches('/').to_string();
    if !(issuer.starts_with("https://") || issuer.starts_with("http://")) || url::Url::parse(&issuer).is_err() {
        return Err(ApiError::invalid("issuer", "must be an http(s) URL"));
    }
    let client_id = check_name("client_id", &req.client_id, 255)?;
    let default_role =
        Role::parse(&req.default_role).ok_or_else(|| ApiError::invalid("default_role", "unknown role"))?;
    if default_role == Role::Owner {
        return Err(ApiError::invalid("default_role", "the default role cannot be owner"));
    }
    let mut mapping = serde_json::Map::new();
    for (k, v) in &req.role_mapping {
        let r = Role::parse(v).ok_or_else(|| ApiError::invalid("role_mapping", format!("unknown role {v:?}")))?;
        mapping.insert(k.clone(), json!(r.as_str()));
    }
    let scopes = req.scopes.clone().unwrap_or_else(|| "openid email profile".into());
    if !scopes.split_whitespace().any(|s| s == "openid") {
        return Err(ApiError::invalid("scopes", "must include openid"));
    }
    // Check reachability and validity of the issuer before saving.
    state.oidc.forget(&issuer);
    let probe = rupta_auth::oidc::OidcSettings {
        id: "probe".into(),
        name: name.clone(),
        issuer: issuer.clone(),
        client_id: client_id.clone(),
        client_secret: None,
        scopes: vec!["openid".into()],
        role_claim: None,
        role_mapping: vec![],
        default_role,
        jit_provisioning: false,
    };
    state
        .oidc
        .start(&state.keys, &probe, &format!("{}/auth/sso/probe/callback", state.config.public_url), None)
        .await
        .map_err(|e| ApiError::invalid("issuer", format!("discovery failed: {e}")))?;

    let st = state.clone();
    let cfg = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ManageSso)?;
            let ctx = format!("sso:{}", o.id);
            let secret_enc: Option<String> = match &req.client_secret {
                None => None,
                Some(s) if s.is_empty() => Some(String::new()),
                Some(s) => Some(st.keys.encrypt(&ctx, s.as_bytes())),
            };
            let existing = c.query_opt("SELECT client_secret_enc FROM sso_configs WHERE org_id = $1", &[&o.id])?;
            let secret_enc = match (secret_enc, existing) {
                (Some(s), _) => s,
                (None, Some(r)) => r.get(0),
                (None, None) => String::new(),
            };
            let role_claim = req.role_claim.clone().filter(|s| !s.trim().is_empty());
            let mapping = Value::Object(mapping);
            c.execute(
                "INSERT INTO sso_configs (org_id, provider_name, issuer, client_id, client_secret_enc, scopes, role_claim, role_mapping,
                                          default_role, jit_provisioning, enforce, owner_local_login, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, now())
                 ON CONFLICT (org_id) DO UPDATE SET provider_name = excluded.provider_name, issuer = excluded.issuer,
                   client_id = excluded.client_id, client_secret_enc = excluded.client_secret_enc, scopes = excluded.scopes,
                   role_claim = excluded.role_claim, role_mapping = excluded.role_mapping, default_role = excluded.default_role,
                   jit_provisioning = excluded.jit_provisioning, enforce = excluded.enforce,
                   owner_local_login = excluded.owner_local_login, updated_at = now()",
                &[
                    &o.id, &name, &issuer, &client_id, &secret_enc, &scopes, &role_claim, &mapping, &default_role.as_str(),
                    &req.jit_provisioning, &req.enforce, &req.owner_local_login,
                ],
            )?;
            audit::record(
                c,
                &audit::Entry {
                    target_type: Some("sso"),
                    target_id: Some(o.id.to_string()),
                    data: json!({"issuer": issuer, "client_id": client_id, "enforce": req.enforce, "jit": req.jit_provisioning,
                                 "secret_changed": req.client_secret.is_some()}),
                    ..auth.audit(Some(o.id), action::SSO_CONFIG_CHANGE)
                },
            )?;
            load_sso(&st, c, o.id, &o.slug)?.ok_or_else(|| ApiError::internal("SSO configuration vanished"))
        })
        .await?;
    Ok(axum::Json(cfg))
}

/// Removes the single sign-on configuration. Linked identities are kept.
#[utoipa::path(delete, path = "/api/v1/orgs/{org}/sso", tag = "orgs", params(("org" = String, Path)), responses((status = 204)))]
async fn delete_sso(State(state): State<ApiState>, auth: Auth, Path(org): Path<String>) -> ApiResult<StatusCode> {
    state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ManageSso)?;
            if c.execute("DELETE FROM sso_configs WHERE org_id = $1", &[&o.id])? == 0 {
                return Err(ApiError::not_found());
            }
            audit::record(
                c,
                &audit::Entry {
                    target_type: Some("sso"),
                    target_id: Some(o.id.to_string()),
                    ..auth.audit(Some(o.id), action::SSO_CONFIG_DELETE)
                },
            )?;
            Ok(())
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
