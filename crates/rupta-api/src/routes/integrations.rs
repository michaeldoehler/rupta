//! Issue tracker integrations: connections per organization, defaults
//! per project, links between issues and tickets, and the webhook
//! receiver at `/api/integrations/{kind}/{integration_id}/webhook`.

use axum::body::Bytes;
use axum::extract::{RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use rupta_auth::audit::{self, action};
use rupta_auth::rbac::Permission;
use rupta_core::ids::Id;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::access::{issue_sync, org_sync, project_sync};
use crate::error::{ApiError, ApiResult};
use crate::extract::{Auth, Json, Path};
use crate::integrations::content::{ticket_body, ticket_title};
use crate::integrations::store::{self, DefaultsRow, IntegrationRow, LinkRow, secret_ctx};
use crate::integrations::sync::{self, NewLink, WebhookOutcome};
use crate::integrations::{Account, Category, IssueTracker, Kind, Target, TicketTarget, WebhookRequest};
use crate::routes::orgs::check_name;
use crate::state::ApiState;

pub fn routes() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(list_integrations, create_integration))
        .routes(routes!(get_integration, update_integration, delete_integration))
        .routes(routes!(test_integration))
        .routes(routes!(integration_targets))
        .routes(routes!(list_project_integrations))
        .routes(routes!(put_project_integration, delete_project_integration))
        .routes(routes!(list_links, create_link))
        .routes(routes!(delete_link))
        .routes(routes!(refresh_link))
        .routes(routes!(ticket_draft))
        .routes(routes!(webhook))
}

/// Who the stored credentials belong to, as the tracker reported at the
/// last successful check.
#[derive(Serialize, Deserialize, ToSchema, Clone, Debug)]
pub struct AccountRef {
    pub id: String,
    pub name: String,
}

/// A connection to an issue tracker.
#[derive(Serialize, ToSchema)]
pub struct Integration {
    #[schema(value_type = String)]
    pub id: Id,
    /// `jira_cloud`, `jira_server`, `linear`, `github` or `gitlab`.
    pub kind: Kind,
    pub name: String,
    pub enabled: bool,
    /// Non-secret settings: `base_url`, `email` (Jira Cloud),
    /// `resolve_transition`/`reopen_transition` (Jira),
    /// `resolve_state`/`reopen_state` (Linear).
    #[schema(value_type = Object)]
    pub config: Value,
    /// The token, masked: `****` and its last four characters.
    pub token_hint: Option<String>,
    pub has_webhook_secret: bool,
    /// Where the tracker sends webhooks.
    pub webhook_url: String,
    pub account: Option<AccountRef>,
    #[schema(value_type = Option<String>)]
    pub created_by: Option<Id>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// The last failed call or check, cleared by the next success.
    pub last_error: Option<String>,
    pub last_error_at: Option<DateTime<Utc>>,
}

/// An integration, with the webhook secret when rupta generated one in
/// this request (shown once).
#[derive(Serialize, ToSchema)]
pub struct IntegrationWithSecret {
    #[serde(flatten)]
    pub integration: Integration,
    /// Set only when rupta generated a webhook secret in this request.
    pub webhook_secret: Option<String>,
}

#[derive(Deserialize, ToSchema, Default)]
pub struct IntegrationBody {
    pub kind: Option<String>,
    pub name: Option<String>,
    pub enabled: Option<bool>,
    /// Replaces the non-secret settings (see `Integration.config`).
    #[schema(value_type = Option<Object>)]
    pub config: Option<Value>,
    /// API token, API key or personal access token. Write-only.
    pub token: Option<String>,
    /// The webhook secret. Write-only. For Linear, the signing secret
    /// Linear shows for the webhook; for the others rupta generates one at
    /// creation when none is given.
    pub webhook_secret: Option<String>,
    /// Generates a new webhook secret, returned once.
    pub rotate_webhook_secret: Option<bool>,
    /// Check the credentials against the tracker before saving (default
    /// true).
    pub validate: Option<bool>,
}

fn public_url(state: &ApiState, i: &IntegrationRow) -> String {
    format!("{}/api/integrations/{}/{}/webhook", state.config.public_url, i.kind.as_str(), i.id)
}

fn mask(token: &str) -> String {
    let n = token.chars().count();
    if n >= 12 { format!("****{}", token.chars().skip(n - 4).collect::<String>()) } else { "****".into() }
}

fn to_api(state: &ApiState, i: &IntegrationRow) -> Integration {
    let mut config = i.config.clone();
    let account = config.as_object_mut().and_then(|o| o.remove("account")).and_then(|a| serde_json::from_value(a).ok());
    Integration {
        id: Id(i.id),
        kind: i.kind,
        name: i.name.clone(),
        enabled: i.enabled,
        config,
        token_hint: i.token(state).map(|t| mask(&t)),
        has_webhook_secret: i.webhook_secret_enc.is_some(),
        webhook_url: public_url(state, i),
        account,
        created_by: i.created_by.map(Id),
        created_at: i.created_at,
        updated_at: i.updated_at,
        last_error: i.last_error.clone(),
        last_error_at: i.last_error_at,
    }
}

/// Settings keys allowed per kind.
fn allowed_keys(kind: Kind) -> &'static [&'static str] {
    match kind {
        Kind::JiraCloud => &["base_url", "email", "resolve_transition", "reopen_transition"],
        Kind::JiraServer => &["base_url", "resolve_transition", "reopen_transition"],
        Kind::Linear => &["base_url", "resolve_state", "reopen_state"],
        Kind::Github | Kind::Gitlab => &["base_url"],
    }
}

/// Validates the non-secret settings of a kind.
fn check_config(kind: Kind, config: &Value) -> ApiResult<Value> {
    let obj = match config {
        Value::Null => Map::new(),
        Value::Object(o) => o.clone(),
        _ => return Err(ApiError::invalid("config", "an object")),
    };
    let mut out = Map::new();
    for (k, v) in obj {
        if !allowed_keys(kind).contains(&k.as_str()) {
            return Err(ApiError::invalid(&format!("config.{k}"), format!("not a setting of {}", kind.as_str())));
        }
        let s = match &v {
            Value::Null => continue,
            Value::String(s) if s.trim().is_empty() => continue,
            Value::String(s) if s.len() <= 500 => s.trim().to_string(),
            _ => return Err(ApiError::invalid(&format!("config.{k}"), "a string of at most 500 characters")),
        };
        if k == "base_url" {
            let u = url::Url::parse(s.trim_end_matches('/'))
                .map_err(|_| ApiError::invalid("config.base_url", "must be an http(s) URL"))?;
            if !matches!(u.scheme(), "http" | "https") || u.host_str().is_none() {
                return Err(ApiError::invalid("config.base_url", "must be an http(s) URL"));
            }
            if !u.username().is_empty() || u.password().is_some() || u.query().is_some() || u.fragment().is_some() {
                return Err(ApiError::invalid(
                    "config.base_url",
                    "must not contain credentials, a query or a fragment",
                ));
            }
            out.insert(k, json!(s.trim_end_matches('/')));
            continue;
        }
        if k == "email" && rupta_auth::users::normalize_email(&s).is_none() {
            return Err(ApiError::invalid("config.email", "an email address"));
        }
        out.insert(k, json!(s));
    }
    if matches!(kind, Kind::JiraCloud | Kind::JiraServer) && !out.contains_key("base_url") {
        return Err(ApiError::invalid("config.base_url", "required for Jira, e.g. https://example.atlassian.net"));
    }
    if kind == Kind::JiraCloud && !out.contains_key("email") {
        return Err(ApiError::invalid("config.email", "required for Jira Cloud: the email of the token's account"));
    }
    Ok(Value::Object(out))
}

fn check_secret(field: &str, v: &str) -> ApiResult<String> {
    let v = v.trim();
    if v.is_empty() || v.len() > 2000 || v.chars().any(char::is_control) {
        return Err(ApiError::invalid(field, "1 to 2000 characters"));
    }
    Ok(v.to_string())
}

fn generate_secret() -> String {
    let mut b = [0u8; 24];
    rand::fill(&mut b);
    hex::encode(b)
}

/// Checks credentials against the tracker; the account on success.
async fn check_credentials(state: &ApiState, kind: Kind, config: &Value, token: &str) -> ApiResult<Account> {
    let tracker =
        store::tracker_from(kind, config, token, state.outbound.clone()).map_err(|e| sync::tracker_error(&e))?;
    let r = tracker.validate().await;
    state.metrics.count(
        "rupta_integration_requests_total",
        &[
            ("kind", kind.as_str()),
            ("operation", "validate"),
            ("result", r.as_ref().map(|_| "success").unwrap_or_else(|e| e.reason())),
        ],
    );
    r.map_err(|e| {
        let mut err = sync::tracker_error(&e);
        err.status = StatusCode::UNPROCESSABLE_ENTITY;
        err.code = "validation_failed";
        err.message = format!("checking the settings against the tracker failed: {e}");
        // Name the form field the problem is most likely in, so clients
        // can show the message next to it.
        let field = match e {
            crate::integrations::TrackerError::Network(_)
            | crate::integrations::TrackerError::Blocked(_)
            | crate::integrations::TrackerError::NotFound(_) => "base_url",
            _ => "token",
        };
        if let Some(d) = err.details.as_object_mut() {
            d.insert("field".into(), serde_json::json!(field));
        }
        err
    })
}

const ORG_COLUMNS: &str = store::INTEGRATION_COLUMNS;

/// Issue tracker integrations of an organization. Members can list them
/// (to link tickets); secrets are never returned.
#[utoipa::path(get, path = "/api/v1/orgs/{org}/integrations", tag = "integrations", params(("org" = String, Path)),
    responses((status = 200, body = Vec<Integration>)))]
async fn list_integrations(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
) -> ApiResult<axum::Json<Vec<Integration>>> {
    let st = state.clone();
    let list = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ReadOrg)?;
            let rows = c.query(
                &format!("SELECT {ORG_COLUMNS} FROM integrations WHERE org_id = $1 ORDER BY name, id"),
                &[&o.id],
            )?;
            rows.iter().map(|r| store::integration_from(r).map(|i| to_api(&st, &i))).collect()
        })
        .await?;
    Ok(axum::Json(list))
}

/// Connects an issue tracker. The credentials are checked against the
/// tracker first unless `validate` is false. Admins only.
#[utoipa::path(post, path = "/api/v1/orgs/{org}/integrations", tag = "integrations", params(("org" = String, Path)),
    request_body = IntegrationBody,
    responses((status = 201, body = IntegrationWithSecret), (status = 422, description = "invalid settings or refused credentials", body = crate::error::ErrorBody)))]
async fn create_integration(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
    Json(b): Json<IntegrationBody>,
) -> ApiResult<Response> {
    let o = state.org(&auth, &org, Permission::ManageIntegrations).await?;
    let kind_s = b.kind.clone().ok_or_else(|| ApiError::invalid("kind", "required"))?;
    let kind = Kind::parse(&kind_s)
        .ok_or_else(|| ApiError::invalid("kind", "jira_cloud, jira_server, linear, github or gitlab"))?;
    let name = check_name("name", b.name.as_deref().unwrap_or(""), 100)?;
    let mut config = check_config(kind, b.config.as_ref().unwrap_or(&Value::Null))?;
    let token = check_secret("token", b.token.as_deref().ok_or_else(|| ApiError::invalid("token", "required"))?)?;
    let mut generated = None;
    let webhook_secret = match b.webhook_secret.as_deref() {
        Some(s) => Some(check_secret("webhook_secret", s)?),
        None if kind.rupta_generates_webhook_secret() => {
            let s = generate_secret();
            generated = Some(s.clone());
            Some(s)
        }
        None => None,
    };
    if b.validate.unwrap_or(true) {
        let account = check_credentials(&state, kind, &config, &token).await?;
        config["account"] = json!({"id": account.id, "name": account.name});
    } else {
        let settings = crate::integrations::Settings { kind, config: config.clone(), token: String::new() };
        let u = settings.base_url().map_err(|e| ApiError::invalid("config.base_url", e.to_string()))?;
        state.outbound.check_host(&u).await.map_err(|e| ApiError::invalid("config.base_url", e.to_string()))?;
    }
    let st = state.clone();
    let enabled = b.enabled.unwrap_or(true);
    let row = state
        .db(move |c| {
            let txn = c.transaction()?;
            let id: i64 = txn
                .query_one(
                    "INSERT INTO integrations (org_id, kind, name, config, enabled, created_by) VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
                    &[&o.id, &kind.as_str(), &name, &config, &enabled, &auth.user_id()],
                )?
                .get(0);
            let token_enc = st.keys.encrypt(&secret_ctx(id, "token"), token.as_bytes());
            let hook_enc = webhook_secret.map(|w| st.keys.encrypt(&secret_ctx(id, "webhook"), w.as_bytes()));
            txn.execute("UPDATE integrations SET secret_enc = $1, webhook_secret_enc = $2 WHERE id = $3", &[&token_enc, &hook_enc, &id])?;
            audit::record(
                &txn,
                &audit::Entry {
                    target_type: Some("integration"),
                    target_id: Some(id.to_string()),
                    data: json!({"kind": kind.as_str(), "name": name}),
                    ..auth.audit(Some(o.id), action::INTEGRATION_CREATE)
                },
            )?;
            let row = store::load(&txn, o.id, id)?.ok_or_else(ApiError::not_found)?;
            txn.commit()?;
            Ok(row)
        })
        .await?;
    let body = IntegrationWithSecret { integration: to_api(&state, &row), webhook_secret: generated };
    Ok((StatusCode::CREATED, axum::Json(body)).into_response())
}

async fn load_integration(
    state: &ApiState,
    auth: &Auth,
    org: &str,
    id: i64,
    perm: Permission,
) -> ApiResult<IntegrationRow> {
    let (auth, org) = (auth.clone(), org.to_string());
    state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, perm)?;
            store::load(c, o.id, id)?.ok_or_else(ApiError::not_found)
        })
        .await
}

/// One integration.
#[utoipa::path(get, path = "/api/v1/orgs/{org}/integrations/{id}", tag = "integrations",
    params(("org" = String, Path), ("id" = String, Path)), responses((status = 200, body = Integration)))]
async fn get_integration(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, id)): Path<(String, i64)>,
) -> ApiResult<axum::Json<Integration>> {
    let row = load_integration(&state, &auth, &org, id, Permission::ReadOrg).await?;
    Ok(axum::Json(to_api(&state, &row)))
}

/// Changes an integration. Leave out `token` and `webhook_secret` to keep
/// them. Changed settings or a new token are checked against the tracker
/// unless `validate` is false. Admins only.
#[utoipa::path(patch, path = "/api/v1/orgs/{org}/integrations/{id}", tag = "integrations",
    params(("org" = String, Path), ("id" = String, Path)), request_body = IntegrationBody,
    responses((status = 200, body = IntegrationWithSecret), (status = 422, body = crate::error::ErrorBody)))]
async fn update_integration(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, id)): Path<(String, i64)>,
    Json(b): Json<IntegrationBody>,
) -> ApiResult<axum::Json<IntegrationWithSecret>> {
    let cur = load_integration(&state, &auth, &org, id, Permission::ManageIntegrations).await?;
    if b.kind.as_deref().is_some_and(|k| k != cur.kind.as_str()) {
        return Err(ApiError::invalid("kind", "cannot be changed; create a new integration"));
    }
    let name = b.name.as_deref().map(|n| check_name("name", n, 100)).transpose()?;
    let account = cur.config.get("account").cloned();
    let mut config = match &b.config {
        Some(c) => check_config(cur.kind, c)?,
        None => {
            let mut c = cur.config.clone();
            if let Some(o) = c.as_object_mut() {
                o.remove("account");
            }
            c
        }
    };
    let token = b.token.as_deref().map(|t| check_secret("token", t)).transpose()?;
    let mut generated = None;
    let webhook_secret = if b.rotate_webhook_secret == Some(true) {
        let s = generate_secret();
        generated = Some(s.clone());
        Some(s)
    } else {
        b.webhook_secret.as_deref().map(|s| check_secret("webhook_secret", s)).transpose()?
    };
    let changed_access = b.config.is_some() || token.is_some();
    if changed_access && b.validate.unwrap_or(true) {
        let t = match &token {
            Some(t) => t.clone(),
            None => cur
                .token(&state)
                .ok_or_else(|| ApiError::invalid("token", "the stored token cannot be read; enter it again"))?,
        };
        let a = check_credentials(&state, cur.kind, &config, &t).await?;
        config["account"] = json!({"id": a.id, "name": a.name});
    } else if let Some(a) = account {
        config["account"] = a;
    }
    let st = state.clone();
    let enabled = b.enabled;
    let changed: Vec<&str> = [
        ("name", name.is_some()),
        ("enabled", enabled.is_some()),
        ("config", b.config.is_some()),
        ("token", token.is_some()),
        ("webhook_secret", webhook_secret.is_some()),
    ]
    .into_iter()
    .filter(|(_, c)| *c)
    .map(|(n, _)| n)
    .collect();
    let changed = json!(changed);
    let row = state
        .db(move |c| {
            let txn = c.transaction()?;
            let token_enc = token.map(|t| st.keys.encrypt(&secret_ctx(id, "token"), t.as_bytes()));
            let hook_enc = webhook_secret.map(|w| st.keys.encrypt(&secret_ctx(id, "webhook"), w.as_bytes()));
            txn.execute(
                "UPDATE integrations SET name = coalesce($1, name), enabled = coalesce($2, enabled), config = $3,
                     secret_enc = coalesce($4, secret_enc), webhook_secret_enc = coalesce($5, webhook_secret_enc), updated_at = now()
                 WHERE id = $6",
                &[&name, &enabled, &config, &token_enc, &hook_enc, &id],
            )?;
            if changed_access {
                store::set_error(&txn, id, None)?;
            }
            audit::record(
                &txn,
                &audit::Entry {
                    target_type: Some("integration"),
                    target_id: Some(id.to_string()),
                    data: json!({"kind": cur.kind.as_str(), "changed": changed}),
                    ..auth.audit(Some(cur.org_id), action::INTEGRATION_UPDATE)
                },
            )?;
            let row = store::load(&txn, cur.org_id, id)?.ok_or_else(ApiError::not_found)?;
            txn.commit()?;
            Ok(row)
        })
        .await?;
    Ok(axum::Json(IntegrationWithSecret { integration: to_api(&state, &row), webhook_secret: generated }))
}

/// Deletes an integration with its project defaults and issue links
/// (the tickets stay in the tracker). Admins only.
#[utoipa::path(delete, path = "/api/v1/orgs/{org}/integrations/{id}", tag = "integrations",
    params(("org" = String, Path), ("id" = String, Path)), responses((status = 204)))]
async fn delete_integration(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, id)): Path<(String, i64)>,
) -> ApiResult<StatusCode> {
    state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ManageIntegrations)?;
            let txn = c.transaction()?;
            let row = store::load(&txn, o.id, id)?.ok_or_else(ApiError::not_found)?;
            txn.execute("DELETE FROM issue_links WHERE integration_id = $1", &[&id])?;
            txn.execute("DELETE FROM project_integration_defaults WHERE integration_id = $1", &[&id])?;
            txn.execute("DELETE FROM integration_deliveries WHERE integration_id = $1", &[&id])?;
            txn.execute("DELETE FROM integrations WHERE id = $1", &[&id])?;
            audit::record(
                &txn,
                &audit::Entry {
                    target_type: Some("integration"),
                    target_id: Some(id.to_string()),
                    data: json!({"kind": row.kind.as_str(), "name": row.name}),
                    ..auth.audit(Some(o.id), action::INTEGRATION_DELETE)
                },
            )?;
            txn.commit()?;
            Ok(())
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The result of a connection test.
#[derive(Serialize, ToSchema)]
pub struct TestResult {
    pub ok: bool,
    pub account: Option<Account>,
    /// What went wrong, in plain words.
    pub error: Option<String>,
    /// `network`, `blocked_host`, `unauthorized`, `not_found`,
    /// `rate_limited`, `error_response` or `invalid`.
    pub reason: Option<String>,
}

/// Checks the stored credentials against the tracker. Admins only.
#[utoipa::path(post, path = "/api/v1/orgs/{org}/integrations/{id}/test", tag = "integrations",
    params(("org" = String, Path), ("id" = String, Path)), responses((status = 200, body = TestResult)))]
async fn test_integration(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, id)): Path<(String, i64)>,
) -> ApiResult<axum::Json<TestResult>> {
    let row = load_integration(&state, &auth, &org, id, Permission::ManageIntegrations).await?;
    let r = match row.tracker(&state) {
        Ok(t) => t.validate().await,
        Err(e) => Err(e),
    };
    sync::count_request(&state, &row, "validate", &r);
    let result = match r {
        Ok(a) => {
            let acc = json!({"id": a.id, "name": a.name});
            state
                .db(move |c| {
                    c.execute(
                        "UPDATE integrations SET config = jsonb_set(config, '{account}', $1) WHERE id = $2",
                        &[&acc, &id],
                    )?;
                    store::set_error(c, id, None)
                })
                .await?;
            TestResult { ok: true, account: Some(a), error: None, reason: None }
        }
        Err(e) => {
            let msg = e.to_string();
            let m = msg.clone();
            state.db(move |c| store::set_error(c, id, Some(&m))).await?;
            TestResult { ok: false, account: None, error: Some(msg), reason: Some(e.reason().to_string()) }
        }
    };
    Ok(axum::Json(result))
}

/// Where tickets can be created: Jira projects with issue types, Linear
/// teams with states, labels and projects, GitHub repositories, GitLab
/// projects. Fetched live from the tracker; members only.
#[utoipa::path(get, path = "/api/v1/orgs/{org}/integrations/{id}/targets", tag = "integrations",
    params(("org" = String, Path), ("id" = String, Path)),
    responses((status = 200, body = Vec<Target>), (status = 502, description = "the tracker failed", body = crate::error::ErrorBody)))]
async fn integration_targets(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, id)): Path<(String, i64)>,
) -> ApiResult<axum::Json<Vec<Target>>> {
    let row = load_integration(&state, &auth, &org, id, Permission::TriageIssues).await?;
    let tracker = row.tracker(&state).map_err(|e| sync::tracker_error(&e))?;
    let r = tracker.targets().await;
    sync::count_request(&state, &row, "targets", &r);
    r.map(axum::Json).map_err(|e| sync::tracker_error(&e))
}

/// A short reference to an integration.
#[derive(Serialize, ToSchema)]
pub struct IntegrationRef {
    #[schema(value_type = String)]
    pub id: Id,
    pub kind: Kind,
    pub name: String,
    pub enabled: bool,
}

/// How a project uses an integration.
#[derive(Serialize, ToSchema)]
pub struct ProjectIntegration {
    pub integration: IntegrationRef,
    /// Jira project key, Linear team id, GitHub `owner/repo`, GitLab project path.
    pub target: String,
    pub target_name: Option<String>,
    /// Jira issue type (id or name).
    pub issue_type: Option<String>,
    /// Linear project id.
    pub project: Option<String>,
    pub labels: Vec<String>,
    /// Tracker → rupta: resolve the issue when the ticket is done, unresolve when it is reopened.
    pub incoming_sync: bool,
    /// rupta → tracker: move the ticket to done/open when the issue is resolved/unresolved.
    pub outgoing_transition: bool,
    /// rupta → tracker: comment on resolve, unresolve and regression.
    pub outgoing_comment: bool,
    pub updated_at: DateTime<Utc>,
}

#[derive(Deserialize, ToSchema)]
pub struct ProjectIntegrationBody {
    pub target: String,
    pub target_name: Option<String>,
    pub issue_type: Option<String>,
    pub project: Option<String>,
    pub labels: Option<Vec<String>>,
    pub incoming_sync: Option<bool>,
    pub outgoing_transition: Option<bool>,
    pub outgoing_comment: Option<bool>,
}

fn project_integration(d: DefaultsRow, i: &IntegrationRow) -> ProjectIntegration {
    ProjectIntegration {
        integration: IntegrationRef { id: Id(i.id), kind: i.kind, name: i.name.clone(), enabled: i.enabled },
        target: d.target,
        target_name: d.target_name,
        issue_type: d.issue_type,
        project: d.project,
        labels: d.labels,
        incoming_sync: d.incoming_sync,
        outgoing_transition: d.outgoing_transition,
        outgoing_comment: d.outgoing_comment,
        updated_at: d.updated_at,
    }
}

/// The integration defaults of a project.
#[utoipa::path(get, path = "/api/v1/projects/{org}/{project}/integrations", tag = "integrations",
    params(("org" = String, Path), ("project" = String, Path)), responses((status = 200, body = Vec<ProjectIntegration>)))]
async fn list_project_integrations(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, project)): Path<(String, String)>,
) -> ApiResult<axum::Json<Vec<ProjectIntegration>>> {
    let list = state
        .db(move |c| {
            let p = project_sync(c, &auth, &org, &project, Permission::ReadProject)?;
            let rows = c.query(
                &format!(
                    "SELECT {} FROM project_integration_defaults WHERE project_id = $1 ORDER BY integration_id",
                    store::DEFAULTS_COLUMNS
                ),
                &[&p.id],
            )?;
            let mut out = Vec::new();
            for r in rows {
                let d = store::defaults_from(&r);
                if let Some(i) = store::load(c, p.org.id, d.integration_id)? {
                    out.push(project_integration(d, &i));
                }
            }
            Ok(out)
        })
        .await?;
    Ok(axum::Json(list))
}

fn opt_text(field: &str, v: &Option<String>, max: usize) -> ApiResult<Option<String>> {
    match v.as_deref().map(str::trim) {
        None | Some("") => Ok(None),
        Some(s) if s.chars().count() <= max && !s.chars().any(char::is_control) => Ok(Some(s.to_string())),
        Some(_) => Err(ApiError::invalid(field, format!("at most {max} characters"))),
    }
}

fn check_labels(labels: &Option<Vec<String>>) -> ApiResult<Vec<String>> {
    let labels: Vec<String> = labels.iter().flatten().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
    if labels.len() > 20 || labels.iter().any(|l| l.chars().count() > 100) {
        return Err(ApiError::invalid("labels", "at most 20 labels of at most 100 characters"));
    }
    Ok(labels)
}

/// Sets how a project uses an integration (creates or replaces).
#[utoipa::path(put, path = "/api/v1/projects/{org}/{project}/integrations/{integration_id}", tag = "integrations",
    params(("org" = String, Path), ("project" = String, Path), ("integration_id" = String, Path)),
    request_body = ProjectIntegrationBody, responses((status = 200, body = ProjectIntegration)))]
async fn put_project_integration(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, project, integration_id)): Path<(String, String, i64)>,
    Json(b): Json<ProjectIntegrationBody>,
) -> ApiResult<axum::Json<ProjectIntegration>> {
    let target =
        opt_text("target", &Some(b.target.clone()), 300)?.ok_or_else(|| ApiError::invalid("target", "required"))?;
    let target_name = opt_text("target_name", &b.target_name, 300)?;
    let issue_type = opt_text("issue_type", &b.issue_type, 200)?;
    let tproject = opt_text("project", &b.project, 200)?;
    let labels = check_labels(&b.labels)?;
    let out = state
        .db(move |c| {
            let p = project_sync(c, &auth, &org, &project, Permission::WriteProject)?;
            let i = store::load(c, p.org.id, integration_id)?.ok_or_else(ApiError::not_found)?;
            let txn = c.transaction()?;
            let r = txn.query_one(
                &format!(
                    "INSERT INTO project_integration_defaults (project_id, integration_id, target, target_name, issue_type, project, labels,
                         incoming_sync, outgoing_transition, outgoing_comment)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                     ON CONFLICT (project_id, integration_id) DO UPDATE SET target = $3, target_name = $4, issue_type = $5, project = $6,
                         labels = $7, incoming_sync = $8, outgoing_transition = $9, outgoing_comment = $10, updated_at = now()
                     RETURNING {}",
                    store::DEFAULTS_COLUMNS
                ),
                &[
                    &p.id,
                    &integration_id,
                    &target,
                    &target_name,
                    &issue_type,
                    &tproject,
                    &json!(labels),
                    &b.incoming_sync.unwrap_or(false),
                    &b.outgoing_transition.unwrap_or(false),
                    &b.outgoing_comment.unwrap_or(false),
                ],
            )?;
            let d = store::defaults_from(&r);
            audit::record(
                &txn,
                &audit::Entry {
                    target_type: Some("integration"),
                    target_id: Some(integration_id.to_string()),
                    data: json!({"project": p.slug, "target": d.target, "incoming_sync": d.incoming_sync,
                                 "outgoing_transition": d.outgoing_transition, "outgoing_comment": d.outgoing_comment}),
                    ..auth.audit(Some(p.org.id), action::INTEGRATION_DEFAULTS_CHANGE)
                },
            )?;
            txn.commit()?;
            Ok(project_integration(d, &i))
        })
        .await?;
    Ok(axum::Json(out))
}

/// Removes a project's defaults for an integration (links stay).
#[utoipa::path(delete, path = "/api/v1/projects/{org}/{project}/integrations/{integration_id}", tag = "integrations",
    params(("org" = String, Path), ("project" = String, Path), ("integration_id" = String, Path)), responses((status = 204)))]
async fn delete_project_integration(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, project, integration_id)): Path<(String, String, i64)>,
) -> ApiResult<StatusCode> {
    state
        .db(move |c| {
            let p = project_sync(c, &auth, &org, &project, Permission::WriteProject)?;
            if c.execute(
                "DELETE FROM project_integration_defaults WHERE project_id = $1 AND integration_id = $2",
                &[&p.id, &integration_id],
            )? == 0
            {
                return Err(ApiError::not_found());
            }
            audit::record(
                c,
                &audit::Entry {
                    target_type: Some("integration"),
                    target_id: Some(integration_id.to_string()),
                    data: json!({"project": p.slug, "removed": true}),
                    ..auth.audit(Some(p.org.id), action::INTEGRATION_DEFAULTS_CHANGE)
                },
            )?;
            Ok(())
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// A ticket linked to an issue.
#[derive(Serialize, ToSchema)]
pub struct IssueLink {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub issue_id: Id,
    pub integration: IntegrationRef,
    /// The tracker's stable id (Jira issue id, Linear UUID, `owner/repo#17`, `group/project#17`).
    pub external_id: String,
    /// `PROJ-123`, `ENG-42`, `#17`.
    pub external_key: String,
    pub url: String,
    pub title: Option<String>,
    /// The tracker's status name (`In Progress`, `closed`, …), `deleted`
    /// when the ticket was deleted.
    pub status: Option<String>,
    pub status_category: Category,
    pub assignee: Option<String>,
    #[schema(value_type = Option<String>)]
    pub created_by: Option<Id>,
    pub created_at: DateTime<Utc>,
    /// When rupta last learned the ticket's state.
    pub synced_at: Option<DateTime<Utc>>,
}

fn link_to_api(l: LinkRow, i: &IntegrationRow) -> IssueLink {
    IssueLink {
        id: Id(l.id),
        issue_id: Id(l.issue_id),
        integration: IntegrationRef { id: Id(i.id), kind: i.kind, name: i.name.clone(), enabled: i.enabled },
        external_id: l.external_id,
        external_key: l.external_key,
        url: l.url,
        title: l.title,
        status: l.status,
        status_category: l.status_category,
        assignee: l.assignee,
        created_by: l.created_by.map(Id),
        created_at: l.created_at,
        synced_at: l.synced_at,
    }
}

/// Tickets linked to an issue.
#[utoipa::path(get, path = "/api/v1/issues/{id}/links", tag = "issues", params(("id" = String, Path)),
    responses((status = 200, body = Vec<IssueLink>)))]
async fn list_links(
    State(state): State<ApiState>,
    auth: Auth,
    Path(id): Path<i64>,
) -> ApiResult<axum::Json<Vec<IssueLink>>> {
    let list = state
        .db(move |c| {
            let p = issue_sync(c, &auth, id, Permission::ReadProject)?;
            let rows = c.query(
                &format!("SELECT {} FROM issue_links WHERE issue_id = $1 ORDER BY id", store::LINK_COLUMNS),
                &[&id],
            )?;
            let mut out = Vec::new();
            for r in rows {
                let l = store::link_from(&r);
                if let Some(i) = store::load(c, p.org.id, l.integration_id)? {
                    out.push(link_to_api(l, &i));
                }
            }
            Ok(out)
        })
        .await?;
    Ok(axum::Json(list))
}

#[derive(Deserialize, ToSchema)]
pub struct LinkBody {
    pub integration_id: String,
    /// `create` a new ticket (default) or `link` an existing one.
    pub mode: Option<String>,
    /// With `create`: where (defaults to the project's defaults).
    pub target: Option<String>,
    pub issue_type: Option<String>,
    /// Linear project id.
    pub project: Option<String>,
    pub labels: Option<Vec<String>>,
    /// With `create`: replaces the generated title.
    pub title: Option<String>,
    /// With `create`: replaces the generated description (plain text or
    /// Markdown; a link back to the issue is added).
    pub description: Option<String>,
    /// With `link`: a key (`PROJ-123`, `ENG-42`, `owner/repo#17`, `#17`
    /// with a default target) or the ticket's URL.
    pub reference: Option<String>,
}

/// Creates a ticket for the issue or links an existing one.
#[utoipa::path(post, path = "/api/v1/issues/{id}/links", tag = "issues", params(("id" = String, Path)), request_body = LinkBody,
    responses((status = 201, body = IssueLink), (status = 409, description = "already linked", body = crate::error::ErrorBody),
              (status = 422, body = crate::error::ErrorBody), (status = 502, description = "the tracker failed", body = crate::error::ErrorBody)))]
async fn create_link(
    State(state): State<ApiState>,
    auth: Auth,
    Path(id): Path<i64>,
    Json(b): Json<LinkBody>,
) -> ApiResult<Response> {
    let integration_id: i64 =
        b.integration_id.trim().parse().map_err(|_| ApiError::invalid("integration_id", "an integration id"))?;
    let mode = b.mode.clone().unwrap_or_else(|| "create".into());
    let a = auth.clone();
    let (row, defaults) = state
        .db(move |c| {
            let p = issue_sync(c, &a, id, Permission::TriageIssues)?;
            let i = store::load(c, p.org.id, integration_id)?
                .ok_or_else(|| ApiError::invalid("integration_id", "no such integration"))?;
            Ok((i, store::defaults(c, p.id, integration_id)?))
        })
        .await?;
    if !row.enabled {
        return Err(ApiError::invalid("integration_id", "the integration is disabled"));
    }
    let what = match mode.as_str() {
        "create" => {
            let target = opt_text("target", &b.target, 300)?
                .or_else(|| defaults.as_ref().map(|d| d.target.clone()))
                .ok_or_else(|| {
                    ApiError::invalid("target", "required: the project has no defaults for this integration")
                })?;
            let labels = match &b.labels {
                Some(_) => check_labels(&b.labels)?,
                None => defaults.as_ref().map(|d| d.labels.clone()).unwrap_or_default(),
            };
            NewLink::Create {
                target: TicketTarget {
                    target,
                    issue_type: opt_text("issue_type", &b.issue_type, 200)?
                        .or_else(|| defaults.as_ref().and_then(|d| d.issue_type.clone())),
                    project: opt_text("project", &b.project, 200)?
                        .or_else(|| defaults.as_ref().and_then(|d| d.project.clone())),
                    labels,
                },
                title: opt_text("title", &b.title, 255)?,
                description: match b.description.as_deref() {
                    Some(d) if d.len() > 30_000 => {
                        return Err(ApiError::invalid("description", "at most 30000 characters"));
                    }
                    d => d.map(str::to_string),
                },
            }
        }
        "link" => NewLink::Existing {
            reference: opt_text("reference", &b.reference, 500)?
                .ok_or_else(|| ApiError::invalid("reference", "required"))?,
        },
        _ => return Err(ApiError::invalid("mode", "create or link")),
    };
    let link = sync::link(&state, auth.user_id(), id, &row, what).await?;
    Ok((StatusCode::CREATED, axum::Json(link_to_api(link, &row))).into_response())
}

async fn load_link(
    state: &ApiState,
    auth: &Auth,
    issue: i64,
    link: i64,
    perm: Permission,
) -> ApiResult<(LinkRow, IntegrationRow)> {
    let auth = auth.clone();
    state
        .db(move |c| {
            let p = issue_sync(c, &auth, issue, perm)?;
            let r = c
                .query_opt(
                    &format!("SELECT {} FROM issue_links WHERE id = $1 AND issue_id = $2", store::LINK_COLUMNS),
                    &[&link, &issue],
                )?
                .ok_or_else(ApiError::not_found)?;
            let l = store::link_from(&r);
            let i = store::load(c, p.org.id, l.integration_id)?.ok_or_else(ApiError::not_found)?;
            Ok((l, i))
        })
        .await
}

/// Unlinks a ticket (the ticket stays in the tracker).
#[utoipa::path(delete, path = "/api/v1/issues/{id}/links/{link_id}", tag = "issues",
    params(("id" = String, Path), ("link_id" = String, Path)), responses((status = 204)))]
async fn delete_link(
    State(state): State<ApiState>,
    auth: Auth,
    Path((id, link_id)): Path<(i64, i64)>,
) -> ApiResult<StatusCode> {
    let (l, i) = load_link(&state, &auth, id, link_id, Permission::TriageIssues).await?;
    let user = auth.user_id();
    state
        .db(move |c| {
            let txn = c.transaction()?;
            txn.execute("DELETE FROM issue_links WHERE id = $1", &[&l.id])?;
            store::activity(&txn, id, user, "ticket_unlinked", store::ticket_json(&i, &l.external_key, &l.url))?;
            txn.commit()?;
            Ok(())
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Fetches the ticket again from the tracker.
#[utoipa::path(post, path = "/api/v1/issues/{id}/links/{link_id}/refresh", tag = "issues",
    params(("id" = String, Path), ("link_id" = String, Path)),
    responses((status = 200, body = IssueLink), (status = 502, description = "the tracker failed", body = crate::error::ErrorBody)))]
async fn refresh_link(
    State(state): State<ApiState>,
    auth: Auth,
    Path((id, link_id)): Path<(i64, i64)>,
) -> ApiResult<axum::Json<IssueLink>> {
    let (l, i) = load_link(&state, &auth, id, link_id, Permission::TriageIssues).await?;
    let fresh = sync::refresh(&state, &i, &l).await?;
    Ok(axum::Json(link_to_api(fresh, &i)))
}

/// The generated ticket for an issue, to prefill the create dialog.
#[derive(Serialize, ToSchema)]
pub struct TicketDraftView {
    pub title: String,
    /// Markdown (Jira receives the same content as ADF or wiki markup).
    pub description: String,
}

/// The ticket title and description rupta would generate for the issue.
#[utoipa::path(get, path = "/api/v1/issues/{id}/ticket-draft", tag = "issues", params(("id" = String, Path)),
    responses((status = 200, body = TicketDraftView)))]
async fn ticket_draft(
    State(state): State<ApiState>,
    auth: Auth,
    Path(id): Path<i64>,
) -> ApiResult<axum::Json<TicketDraftView>> {
    let st = state.clone();
    let s = state
        .db(move |c| {
            issue_sync(c, &auth, id, Permission::ReadProject)?;
            store::summary(&st, c, id)
        })
        .await?;
    Ok(axum::Json(TicketDraftView { title: ticket_title(&s), description: ticket_body(&s).markdown() }))
}

/// Receives a tracker's webhook. Verified with the integration's webhook
/// secret (see docs/integrations.md); unknown integrations and failed
/// verification answer 401. Redeliveries (same delivery id) answer 200
/// without effect.
#[utoipa::path(post, path = "/api/integrations/{kind}/{integration_id}/webhook", tag = "integrations",
    params(("kind" = String, Path), ("integration_id" = String, Path)), security(()),
    request_body(content = Object, description = "The tracker's webhook payload, as the tracker sends it"),
    responses((status = 200, description = "`{\"ok\": true, \"duplicate\": false, \"updated_links\": n, \"changed_issues\": n}`"),
              (status = 401, body = crate::error::ErrorBody), (status = 400, body = crate::error::ErrorBody)))]
async fn webhook(
    State(state): State<ApiState>,
    Path((kind, integration_id)): Path<(String, String)>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<axum::Json<Value>> {
    let Ok(integration_id) = integration_id.parse::<i64>() else { return Err(ApiError::unauthorized()) };
    let req = WebhookRequest { headers: &headers, query: query.as_deref(), body: &body };
    let out = sync::webhook(&state, &kind, integration_id, req).await?;
    Ok(axum::Json(match out {
        WebhookOutcome::Duplicate => json!({"ok": true, "duplicate": true}),
        WebhookOutcome::Processed { updated_links, changed_issues, .. } => {
            json!({"ok": true, "duplicate": false, "updated_links": updated_links, "changed_issues": changed_issues})
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_validation() {
        assert!(check_config(Kind::JiraCloud, &json!({"base_url": "https://x.atlassian.net"})).is_err());
        let c =
            check_config(Kind::JiraCloud, &json!({"base_url": "https://x.atlassian.net/", "email": "a@b.co"})).unwrap();
        assert_eq!(c["base_url"], "https://x.atlassian.net");
        assert!(check_config(Kind::Github, &json!({"email": "a@b.co"})).is_err());
        assert!(check_config(Kind::Gitlab, &json!({"base_url": "https://u:p@git.example.com"})).is_err());
        assert!(check_config(Kind::Gitlab, &json!({"base_url": "ftp://git.example.com"})).is_err());
        assert_eq!(check_config(Kind::Linear, &Value::Null).unwrap(), json!({}));
    }

    #[test]
    fn masking() {
        assert_eq!(mask("ghp_1234567890abcdef"), "****cdef");
        assert_eq!(mask("short"), "****");
    }
}
