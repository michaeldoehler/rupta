//! Notification channels (per organization) and alert rules (per project).

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use rupta_auth::audit::{self, action};
use rupta_auth::rbac::Permission;
use rupta_core::ids::Id;
use rupta_store::squad::{Connection, Row};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::access::{org_sync, project_sync};
use crate::error::{ApiError, ApiResult};
use crate::extract::{Auth, Json, Path};
use crate::notify::{self, Notification};
use crate::routes::orgs::check_name;
use crate::state::ApiState;

pub fn routes() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(list_channels, create_channel))
        .routes(routes!(update_channel, delete_channel))
        .routes(routes!(test_channel))
        .routes(routes!(list_rules, create_rule))
        .routes(routes!(get_rule, update_rule, delete_rule))
}

#[derive(Serialize, ToSchema)]
pub struct Channel {
    #[schema(value_type = String)]
    pub id: Id,
    /// `email`, `slack`, `webhook` or `teams`.
    pub kind: String,
    pub name: String,
    /// email: `{to: [..]}`; slack and teams: `{webhook_url}`; webhook:
    /// `{url, has_secret}` (the secret is write-only).
    #[schema(value_type = Object)]
    pub config: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Deserialize, ToSchema)]
pub struct ChannelBody {
    pub kind: Option<String>,
    pub name: Option<String>,
    /// email: `{to: ["a@b.c"]}`; slack and teams: `{webhook_url}`;
    /// webhook: `{url, secret?}`.
    #[schema(value_type = Option<Object>)]
    pub config: Option<Value>,
}

fn secret_ctx(org_id: i64) -> String {
    format!("channel:{org_id}")
}

fn check_url(field: &str, v: Option<&Value>) -> ApiResult<String> {
    let s = v.and_then(Value::as_str).ok_or_else(|| ApiError::invalid(field, "required"))?;
    match url::Url::parse(s) {
        Ok(u) if matches!(u.scheme(), "https" | "http") && u.host().is_some() && s.len() <= 2048 => Ok(s.to_string()),
        _ => Err(ApiError::invalid(field, "must be an http(s) URL")),
    }
}

/// Validates a channel configuration and encrypts its secret. `old` is the
/// stored configuration, whose secret is kept when none is given.
fn prepare_config(state: &ApiState, org_id: i64, kind: &str, config: &Value, old: Option<&Value>) -> ApiResult<Value> {
    match kind {
        "email" => {
            let to = config
                .get("to")
                .and_then(Value::as_array)
                .ok_or_else(|| ApiError::invalid("config.to", "a list of email addresses"))?;
            if to.is_empty() || to.len() > 50 {
                return Err(ApiError::invalid("config.to", "1 to 50 addresses"));
            }
            let mut out = Vec::new();
            for a in to {
                let e = a
                    .as_str()
                    .and_then(rupta_auth::users::normalize_email)
                    .ok_or_else(|| ApiError::invalid("config.to", "invalid email address"))?;
                out.push(e);
            }
            Ok(json!({ "to": out }))
        }
        "slack" | "teams" => Ok(json!({ "webhook_url": check_url("config.webhook_url", config.get("webhook_url"))? })),
        "webhook" => {
            let url = check_url("config.url", config.get("url"))?;
            let secret_enc = match config.get("secret").and_then(Value::as_str) {
                Some("") => None,
                Some(s) if s.len() > 256 => return Err(ApiError::invalid("config.secret", "at most 256 characters")),
                Some(s) => Some(state.keys.encrypt(&secret_ctx(org_id), s.as_bytes())),
                None => old.and_then(|o| o.get("secret_enc")).and_then(Value::as_str).map(str::to_string),
            };
            Ok(json!({ "url": url, "secret_enc": secret_enc }))
        }
        _ => Err(ApiError::invalid("kind", "email, slack, webhook or teams")),
    }
}

/// The configuration as shown to clients (no secrets).
fn public_config(config: &Value) -> Value {
    let mut c = config.clone();
    if let Some(obj) = c.as_object_mut()
        && let Some(enc) = obj.remove("secret_enc")
    {
        obj.insert("has_secret".into(), json!(!enc.is_null()));
    }
    c
}

/// The configuration for sending (secret decrypted).
pub(crate) fn sending_config(state: &ApiState, org_id: i64, config: &Value) -> Value {
    let mut c = config.clone();
    if let Some(obj) = c.as_object_mut()
        && let Some(enc) = obj.remove("secret_enc")
        && let Some(enc) = enc.as_str()
        && let Some(secret) = state.keys.decrypt(&secret_ctx(org_id), enc).and_then(|b| String::from_utf8(b).ok())
    {
        obj.insert("secret".into(), json!(secret));
    }
    c
}

fn channel_from(r: &Row) -> Channel {
    Channel {
        id: Id(r.get(0)),
        kind: r.get(1),
        name: r.get(2),
        config: public_config(&r.get::<Value>(3)),
        created_at: r.get(4),
    }
}

const CHANNEL_COLUMNS: &str = "id, kind, name, config, created_at";

/// Notification channels of an organization.
#[utoipa::path(get, path = "/api/v1/orgs/{org}/channels", tag = "alerts", params(("org" = String, Path)),
    responses((status = 200, body = Vec<Channel>)))]
async fn list_channels(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
) -> ApiResult<axum::Json<Vec<Channel>>> {
    let list = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ManageChannels)?;
            Ok(c.query(
                &format!("SELECT {CHANNEL_COLUMNS} FROM notification_channels WHERE org_id = $1 ORDER BY name, id"),
                &[&o.id],
            )?
            .iter()
            .map(channel_from)
            .collect())
        })
        .await?;
    Ok(axum::Json(list))
}

/// Creates a channel.
#[utoipa::path(post, path = "/api/v1/orgs/{org}/channels", tag = "alerts", params(("org" = String, Path)), request_body = ChannelBody,
    responses((status = 201, body = Channel)))]
async fn create_channel(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
    Json(b): Json<ChannelBody>,
) -> ApiResult<Response> {
    let name = check_name("name", b.name.as_deref().unwrap_or(""), 100)?;
    let kind = b.kind.clone().ok_or_else(|| ApiError::invalid("kind", "required"))?;
    let st = state.clone();
    let ch = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ManageChannels)?;
            let config = prepare_config(&st, o.id, &kind, b.config.as_ref().unwrap_or(&json!({})), None)?;
            let r = c.query_one(
                &format!("INSERT INTO notification_channels (org_id, kind, name, config) VALUES ($1, $2, $3, $4) RETURNING {CHANNEL_COLUMNS}"),
                &[&o.id, &kind, &name, &config],
            )?;
            let ch = channel_from(&r);
            audit::record(c, &audit::Entry { target_type: Some("channel"), target_id: Some(ch.id.0.to_string()), data: json!({"kind": kind, "name": name}), ..auth.audit(Some(o.id), action::CHANNEL_CREATE) })?;
            Ok(ch)
        })
        .await?;
    Ok((StatusCode::CREATED, axum::Json(ch)).into_response())
}

/// Changes a channel.
#[utoipa::path(patch, path = "/api/v1/orgs/{org}/channels/{id}", tag = "alerts",
    params(("org" = String, Path), ("id" = String, Path)), request_body = ChannelBody, responses((status = 200, body = Channel)))]
async fn update_channel(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, id)): Path<(String, i64)>,
    Json(b): Json<ChannelBody>,
) -> ApiResult<axum::Json<Channel>> {
    let name = b.name.as_deref().map(|n| check_name("name", n, 100)).transpose()?;
    if b.kind.is_some() {
        return Err(ApiError::invalid("kind", "cannot be changed; create a new channel"));
    }
    let st = state.clone();
    let ch = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ManageChannels)?;
            let r = c.query_opt("SELECT kind, config FROM notification_channels WHERE id = $1 AND org_id = $2", &[&id, &o.id])?.ok_or_else(ApiError::not_found)?;
            let kind: String = r.get(0);
            let old: Value = r.get(1);
            let config = match &b.config {
                Some(cfg) => prepare_config(&st, o.id, &kind, cfg, Some(&old))?,
                None => old,
            };
            let r = c.query_one(
                &format!("UPDATE notification_channels SET name = coalesce($1, name), config = $2 WHERE id = $3 RETURNING {CHANNEL_COLUMNS}"),
                &[&name, &config, &id],
            )?;
            audit::record(c, &audit::Entry { target_type: Some("channel"), target_id: Some(id.to_string()), data: json!({"name": name, "config_changed": b.config.is_some()}), ..auth.audit(Some(o.id), action::CHANNEL_UPDATE) })?;
            Ok(channel_from(&r))
        })
        .await?;
    Ok(axum::Json(ch))
}

/// Deletes a channel. Alert rules that use it skip it from then on.
#[utoipa::path(delete, path = "/api/v1/orgs/{org}/channels/{id}", tag = "alerts",
    params(("org" = String, Path), ("id" = String, Path)), responses((status = 204)))]
async fn delete_channel(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, id)): Path<(String, i64)>,
) -> ApiResult<StatusCode> {
    state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ManageChannels)?;
            if c.execute("DELETE FROM notification_channels WHERE id = $1 AND org_id = $2", &[&id, &o.id])? == 0 {
                return Err(ApiError::not_found());
            }
            audit::record(
                c,
                &audit::Entry {
                    target_type: Some("channel"),
                    target_id: Some(id.to_string()),
                    ..auth.audit(Some(o.id), action::CHANNEL_DELETE)
                },
            )?;
            Ok(())
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Sends a test notification through a channel.
#[utoipa::path(post, path = "/api/v1/orgs/{org}/channels/{id}/test", tag = "alerts",
    params(("org" = String, Path), ("id" = String, Path)),
    responses((status = 200, description = "`{\"ok\": true}`"), (status = 502, description = "delivery failed", body = crate::error::ErrorBody)))]
async fn test_channel(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, id)): Path<(String, i64)>,
) -> ApiResult<axum::Json<Value>> {
    let (org_id, slug, kind, config) = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ManageChannels)?;
            let r = c
                .query_opt(
                    "SELECT kind, config FROM notification_channels WHERE id = $1 AND org_id = $2",
                    &[&id, &o.id],
                )?
                .ok_or_else(ApiError::not_found)?;
            Ok((o.id, o.slug, r.get::<String>(0), r.get::<Value>(1)))
        })
        .await?;
    let n = Notification::test(&state, &slug);
    notify::send_channel(&state, &kind, &sending_config(&state, org_id, &config), &n)
        .await
        .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, "delivery_failed", e))?;
    Ok(axum::Json(json!({"ok": true})))
}

/// An alert rule.
#[derive(Serialize, ToSchema)]
pub struct AlertRule {
    #[schema(value_type = String)]
    pub id: Id,
    pub name: String,
    pub enabled: bool,
    /// Any of: `{type: first_seen}`, `{type: regression}`,
    /// `{type: reappeared}`, `{type: escalating}`,
    /// `{type: event_frequency, count, interval}`,
    /// `{type: user_frequency, count, interval}` (interval like `1h`).
    #[schema(value_type = Vec<Object>)]
    pub conditions: Value,
    /// All of: `{type: level, level, match: eq|gte}`,
    /// `{type: environment, value}`, `{type: tag, key, value}`.
    #[schema(value_type = Vec<Object>)]
    pub filters: Value,
    /// `{type: channel, channel_id}`, `{type: email, user_ids, team_ids}`.
    #[schema(value_type = Vec<Object>)]
    pub actions: Value,
    /// At most one notification per issue and rule in this many minutes.
    pub frequency_minutes: i32,
    pub environment: Option<String>,
    #[schema(value_type = Option<String>)]
    pub created_by: Option<Id>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Deserialize, ToSchema)]
pub struct AlertRuleBody {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    #[schema(value_type = Option<Vec<Object>>)]
    pub conditions: Option<Value>,
    #[schema(value_type = Option<Vec<Object>>)]
    pub filters: Option<Value>,
    #[schema(value_type = Option<Vec<Object>>)]
    pub actions: Option<Value>,
    pub frequency_minutes: Option<i32>,
    /// Only events of this environment (empty string: any).
    pub environment: Option<String>,
}

const LEVELS: &[&str] = &["debug", "info", "warning", "error", "fatal"];

fn check_interval(v: Option<&Value>) -> ApiResult<String> {
    let s = v.and_then(Value::as_str).ok_or_else(|| ApiError::invalid("conditions.interval", "required, e.g. 1h"))?;
    match rupta_core::query::parse_duration(s) {
        Some(d) if d >= chrono::Duration::minutes(1) && d <= chrono::Duration::days(30) => Ok(s.to_string()),
        _ => Err(ApiError::invalid("conditions.interval", "between 1m and 30d")),
    }
}

/// Validates rule parts against the organization (channels, users, teams).
fn check_rule(
    c: &Connection,
    org_id: i64,
    conditions: &Value,
    filters: &Value,
    actions: &Value,
) -> ApiResult<(Value, Value, Value)> {
    let conds = conditions.as_array().ok_or_else(|| ApiError::invalid("conditions", "a list"))?;
    if conds.is_empty() || conds.len() > 10 {
        return Err(ApiError::invalid("conditions", "1 to 10 conditions"));
    }
    let mut out_c = Vec::new();
    for cond in conds {
        let t = cond.get("type").and_then(Value::as_str).unwrap_or("");
        out_c.push(match t {
            "first_seen" | "regression" | "reappeared" | "escalating" => json!({"type": t}),
            "event_frequency" | "user_frequency" => {
                let count = cond
                    .get("count")
                    .and_then(Value::as_i64)
                    .filter(|n| *n >= 1 && *n <= 10_000_000)
                    .ok_or_else(|| ApiError::invalid("conditions.count", "a positive number"))?;
                json!({"type": t, "count": count, "interval": check_interval(cond.get("interval"))?})
            }
            _ => {
                return Err(ApiError::invalid(
                    "conditions.type",
                    "first_seen, regression, reappeared, escalating, event_frequency or user_frequency",
                ));
            }
        });
    }
    let filts = filters.as_array().ok_or_else(|| ApiError::invalid("filters", "a list"))?;
    let mut out_f = Vec::new();
    for f in filts.iter().take(20) {
        let s =
            |k: &str| f.get(k).and_then(Value::as_str).filter(|v| !v.is_empty() && v.len() <= 200).map(str::to_string);
        out_f.push(match f.get("type").and_then(Value::as_str).unwrap_or("") {
            "level" => {
                let level = s("level").filter(|l| LEVELS.contains(&l.as_str())).ok_or_else(|| ApiError::invalid("filters.level", "debug, info, warning, error or fatal"))?;
                let m = s("match").unwrap_or_else(|| "gte".into());
                if m != "gte" && m != "eq" {
                    return Err(ApiError::invalid("filters.match", "eq or gte"));
                }
                json!({"type": "level", "level": level, "match": m})
            }
            "environment" => json!({"type": "environment", "value": s("value").ok_or_else(|| ApiError::invalid("filters.value", "required"))?}),
            "tag" => json!({"type": "tag", "key": s("key").ok_or_else(|| ApiError::invalid("filters.key", "required"))?, "value": s("value").ok_or_else(|| ApiError::invalid("filters.value", "required"))?}),
            _ => return Err(ApiError::invalid("filters.type", "level, environment or tag")),
        });
    }
    let acts = actions.as_array().ok_or_else(|| ApiError::invalid("actions", "a list"))?;
    if acts.is_empty() || acts.len() > 20 {
        return Err(ApiError::invalid("actions", "1 to 20 actions"));
    }
    let id_of = |v: &Value| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok()));
    let mut out_a = Vec::new();
    for a in acts {
        out_a.push(match a.get("type").and_then(Value::as_str).unwrap_or("") {
            "channel" => {
                let cid = a.get("channel_id").and_then(id_of).ok_or_else(|| ApiError::invalid("actions.channel_id", "required"))?;
                if c.query_opt("SELECT 1 FROM notification_channels WHERE id = $1 AND org_id = $2", &[&cid, &org_id])?.is_none() {
                    return Err(ApiError::invalid("actions.channel_id", "no such channel"));
                }
                json!({"type": "channel", "channel_id": cid.to_string()})
            }
            "email" => {
                let users: Vec<i64> = a.get("user_ids").and_then(Value::as_array).map(|x| x.iter().filter_map(id_of).collect()).unwrap_or_default();
                let teams: Vec<i64> = a.get("team_ids").and_then(Value::as_array).map(|x| x.iter().filter_map(id_of).collect()).unwrap_or_default();
                if users.is_empty() && teams.is_empty() {
                    return Err(ApiError::invalid("actions.user_ids", "give users or teams"));
                }
                for u in &users {
                    if rupta_auth::users::membership(c, org_id, *u)?.is_none() {
                        return Err(ApiError::invalid("actions.user_ids", "not a member of the organization"));
                    }
                }
                for t in &teams {
                    if c.query_opt("SELECT 1 FROM teams WHERE id = $1 AND org_id = $2", &[t, &org_id])?.is_none() {
                        return Err(ApiError::invalid("actions.team_ids", "no such team"));
                    }
                }
                json!({"type": "email", "user_ids": users.iter().map(|u| u.to_string()).collect::<Vec<_>>(), "team_ids": teams.iter().map(|t| t.to_string()).collect::<Vec<_>>()})
            }
            _ => return Err(ApiError::invalid("actions.type", "channel or email")),
        });
    }
    Ok((Value::Array(out_c), Value::Array(out_f), Value::Array(out_a)))
}

fn rule_from(r: &Row) -> AlertRule {
    AlertRule {
        id: Id(r.get(0)),
        name: r.get(1),
        enabled: r.get(2),
        conditions: r.get(3),
        filters: r.get(4),
        actions: r.get(5),
        frequency_minutes: r.get(6),
        environment: r.get(7),
        created_by: r.get::<Option<i64>>(8).map(Id),
        created_at: r.get(9),
        updated_at: r.get(10),
    }
}

const RULE_COLUMNS: &str = "id, name, enabled, conditions, filters, actions, frequency_minutes, environment, created_by, created_at, updated_at";

/// Alert rules of a project.
#[utoipa::path(get, path = "/api/v1/projects/{org}/{project}/alert-rules", tag = "alerts",
    params(("org" = String, Path), ("project" = String, Path)), responses((status = 200, body = Vec<AlertRule>)))]
async fn list_rules(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, project)): Path<(String, String)>,
) -> ApiResult<axum::Json<Vec<AlertRule>>> {
    let list = state
        .db(move |c| {
            let p = project_sync(c, &auth, &org, &project, Permission::ReadProject)?;
            Ok(c.query(
                &format!("SELECT {RULE_COLUMNS} FROM alert_rules WHERE project_id = $1 ORDER BY name, id"),
                &[&p.id],
            )?
            .iter()
            .map(rule_from)
            .collect())
        })
        .await?;
    Ok(axum::Json(list))
}

fn check_frequency(f: Option<i32>) -> ApiResult<i32> {
    match f {
        None => Ok(30),
        Some(n) if (1..=43_200).contains(&n) => Ok(n),
        Some(_) => Err(ApiError::invalid("frequency_minutes", "1 to 43200")),
    }
}

/// Creates an alert rule.
#[utoipa::path(post, path = "/api/v1/projects/{org}/{project}/alert-rules", tag = "alerts",
    params(("org" = String, Path), ("project" = String, Path)), request_body = AlertRuleBody, responses((status = 201, body = AlertRule)))]
async fn create_rule(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, project)): Path<(String, String)>,
    Json(b): Json<AlertRuleBody>,
) -> ApiResult<Response> {
    let name = check_name("name", b.name.as_deref().unwrap_or(""), 200)?;
    let freq = check_frequency(b.frequency_minutes)?;
    let rule = state
        .db(move |c| {
            let p = project_sync(c, &auth, &org, &project, Permission::WriteProject)?;
            let (conds, filts, acts) = check_rule(
                c,
                p.org.id,
                b.conditions.as_ref().unwrap_or(&json!([])),
                b.filters.as_ref().unwrap_or(&json!([])),
                b.actions.as_ref().unwrap_or(&json!([])),
            )?;
            let env = b.environment.clone().filter(|e| !e.is_empty());
            let r = c.query_one(
                &format!(
                    "INSERT INTO alert_rules (project_id, name, enabled, conditions, filters, actions, frequency_minutes, environment, created_by)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING {RULE_COLUMNS}"
                ),
                &[&p.id, &name, &b.enabled.unwrap_or(true), &conds, &filts, &acts, &freq, &env, &auth.user_id()],
            )?;
            let rule = rule_from(&r);
            audit::record(c, &audit::Entry { target_type: Some("alert_rule"), target_id: Some(rule.id.0.to_string()), data: json!({"project": p.slug, "name": name}), ..auth.audit(Some(p.org.id), action::ALERT_RULE_CREATE) })?;
            Ok(rule)
        })
        .await?;
    Ok((StatusCode::CREATED, axum::Json(rule)).into_response())
}

/// An alert rule.
#[utoipa::path(get, path = "/api/v1/projects/{org}/{project}/alert-rules/{id}", tag = "alerts",
    params(("org" = String, Path), ("project" = String, Path), ("id" = String, Path)), responses((status = 200, body = AlertRule)))]
async fn get_rule(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, project, id)): Path<(String, String, i64)>,
) -> ApiResult<axum::Json<AlertRule>> {
    let rule = state
        .db(move |c| {
            let p = project_sync(c, &auth, &org, &project, Permission::ReadProject)?;
            let r = c.query_opt(
                &format!("SELECT {RULE_COLUMNS} FROM alert_rules WHERE id = $1 AND project_id = $2"),
                &[&id, &p.id],
            )?;
            Ok(rule_from(&r.ok_or_else(ApiError::not_found)?))
        })
        .await?;
    Ok(axum::Json(rule))
}

/// Changes an alert rule.
#[utoipa::path(patch, path = "/api/v1/projects/{org}/{project}/alert-rules/{id}", tag = "alerts",
    params(("org" = String, Path), ("project" = String, Path), ("id" = String, Path)), request_body = AlertRuleBody,
    responses((status = 200, body = AlertRule)))]
async fn update_rule(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, project, id)): Path<(String, String, i64)>,
    Json(b): Json<AlertRuleBody>,
) -> ApiResult<axum::Json<AlertRule>> {
    let name = b.name.as_deref().map(|n| check_name("name", n, 200)).transpose()?;
    let freq = b.frequency_minutes.map(|f| check_frequency(Some(f))).transpose()?;
    let rule = state
        .db(move |c| {
            let p = project_sync(c, &auth, &org, &project, Permission::WriteProject)?;
            let cur = c.query_opt(&format!("SELECT {RULE_COLUMNS} FROM alert_rules WHERE id = $1 AND project_id = $2"), &[&id, &p.id])?;
            let cur = rule_from(&cur.ok_or_else(ApiError::not_found)?);
            let (conds, filts, acts) = check_rule(
                c,
                p.org.id,
                b.conditions.as_ref().unwrap_or(&cur.conditions),
                b.filters.as_ref().unwrap_or(&cur.filters),
                b.actions.as_ref().unwrap_or(&cur.actions),
            )?;
            let env = match &b.environment {
                Some(e) if e.is_empty() => None,
                Some(e) => Some(e.clone()),
                None => cur.environment.clone(),
            };
            let r = c.query_one(
                &format!(
                    "UPDATE alert_rules SET name = $1, enabled = $2, conditions = $3, filters = $4, actions = $5, frequency_minutes = $6,
                            environment = $7, updated_at = now() WHERE id = $8 RETURNING {RULE_COLUMNS}"
                ),
                &[&name.unwrap_or(cur.name), &b.enabled.unwrap_or(cur.enabled), &conds, &filts, &acts, &freq.unwrap_or(cur.frequency_minutes), &env, &id],
            )?;
            audit::record(c, &audit::Entry { target_type: Some("alert_rule"), target_id: Some(id.to_string()), data: json!({"project": p.slug}), ..auth.audit(Some(p.org.id), action::ALERT_RULE_UPDATE) })?;
            Ok(rule_from(&r))
        })
        .await?;
    Ok(axum::Json(rule))
}

/// Deletes an alert rule.
#[utoipa::path(delete, path = "/api/v1/projects/{org}/{project}/alert-rules/{id}", tag = "alerts",
    params(("org" = String, Path), ("project" = String, Path), ("id" = String, Path)), responses((status = 204)))]
async fn delete_rule(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, project, id)): Path<(String, String, i64)>,
) -> ApiResult<StatusCode> {
    state
        .db(move |c| {
            let p = project_sync(c, &auth, &org, &project, Permission::WriteProject)?;
            if c.execute("DELETE FROM alert_rules WHERE id = $1 AND project_id = $2", &[&id, &p.id])? == 0 {
                return Err(ApiError::not_found());
            }
            audit::record(
                c,
                &audit::Entry {
                    target_type: Some("alert_rule"),
                    target_id: Some(id.to_string()),
                    data: json!({"project": p.slug}),
                    ..auth.audit(Some(p.org.id), action::ALERT_RULE_DELETE)
                },
            )?;
            Ok(())
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
