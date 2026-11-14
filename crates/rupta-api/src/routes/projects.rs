//! Projects, their client keys (DSNs), settings and usage statistics.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Duration, Utc};
use rupta_auth::audit::{self, action};
use rupta_auth::rbac::Permission;
use rupta_auth::users;
use rupta_core::ids::Id;
use rupta_core::settings::ProjectSettings;
use rupta_store::squad::{Connection, Row};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::access::{ProjectCtx, org_sync, project_sync};
use crate::error::{ApiError, ApiResult};
use crate::extract::{Auth, Json, Path, Query};
use crate::routes::orgs::{check_name, check_slug};
use crate::state::ApiState;

pub fn routes() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(list_projects, create_project))
        .routes(routes!(get_project, update_project, delete_project))
        .routes(routes!(list_keys, create_key))
        .routes(routes!(update_key, delete_key))
        .routes(routes!(project_stats))
}

#[derive(Serialize, ToSchema)]
pub struct TeamRef {
    #[schema(value_type = String)]
    pub id: Id,
    pub slug: String,
    pub name: String,
}

#[derive(Serialize, ToSchema)]
pub struct Project {
    #[schema(value_type = String)]
    pub id: Id,
    pub slug: String,
    pub name: String,
    pub platform: Option<String>,
    pub team: Option<TeamRef>,
    pub created_at: DateTime<Utc>,
    /// See `rupta_core::settings::ProjectSettings` and docs/api.md.
    #[schema(value_type = Object)]
    pub settings: Value,
    /// DSN of the first active key, for quick setup.
    pub dsn: Option<String>,
}

const PROJECT_SELECT: &str = "SELECT p.id, p.slug, p.name, p.platform, t.id, t.slug, t.name, p.created_at, p.settings,
    (SELECT k.public_key FROM project_keys k WHERE k.project_id = p.id AND k.is_active ORDER BY k.id LIMIT 1)
    FROM projects p LEFT JOIN teams t ON t.id = p.team_id";

fn project_from(state: &ApiState, r: &Row) -> Project {
    let id: i64 = r.get(0);
    let settings: Value = r.get(8);
    Project {
        id: Id(id),
        slug: r.get(1),
        name: r.get(2),
        platform: r.get(3),
        team: r.get::<Option<i64>>(4).map(|tid| TeamRef { id: Id(tid), slug: r.get(5), name: r.get(6) }),
        created_at: r.get(7),
        settings: serde_json::to_value(ProjectSettings::from_json(&settings)).unwrap_or(settings),
        dsn: r.get::<Option<String>>(9).map(|k| dsn(&state.config.public_url, &k, id)),
    }
}

/// `{scheme}://{public_key}@{host}{path}/{project_id}` from the public URL.
pub fn dsn(public_url: &str, public_key: &str, project_id: i64) -> String {
    let (scheme, rest) = public_url.split_once("://").unwrap_or(("http", public_url));
    format!("{scheme}://{public_key}@{}/{project_id}", rest.trim_end_matches('/'))
}

fn load_project(state: &ApiState, c: &Connection, id: i64) -> ApiResult<Project> {
    let r = c.query_opt(&format!("{PROJECT_SELECT} WHERE p.id = $1"), &[&id])?.ok_or_else(ApiError::not_found)?;
    Ok(project_from(state, &r))
}

/// Projects of an organization.
#[utoipa::path(get, path = "/api/v1/orgs/{org}/projects", tag = "projects", params(("org" = String, Path)),
    responses((status = 200, body = Vec<Project>)))]
async fn list_projects(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
) -> ApiResult<axum::Json<Vec<Project>>> {
    let st = state.clone();
    let list = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ReadProject)?;
            let rows = c.query(&format!("{PROJECT_SELECT} WHERE p.org_id = $1 ORDER BY p.slug"), &[&o.id])?;
            Ok(rows.iter().map(|r| project_from(&st, r)).collect())
        })
        .await?;
    Ok(axum::Json(list))
}

#[derive(Deserialize, ToSchema)]
pub struct CreateProject {
    pub name: String,
    pub slug: Option<String>,
    /// SDK platform, e.g. `javascript-react`, `python`, `php`.
    pub platform: Option<String>,
    /// Team slug.
    pub team: Option<String>,
}

fn check_platform(p: &Option<String>) -> ApiResult<()> {
    if let Some(p) = p
        && (p.len() > 64
            || !p.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.' || b == b'_'))
    {
        return Err(ApiError::invalid("platform", "lowercase letters, digits, `-`, `.` and `_`, at most 64"));
    }
    Ok(())
}

fn new_key_pair() -> (String, String) {
    (hex::encode(rupta_auth::crypto::random_bytes::<16>()), hex::encode(rupta_auth::crypto::random_bytes::<16>()))
}

/// Creates a project with one client key.
#[utoipa::path(post, path = "/api/v1/orgs/{org}/projects", tag = "projects", params(("org" = String, Path)),
    request_body = CreateProject, responses((status = 201, body = Project), (status = 409, body = crate::error::ErrorBody)))]
async fn create_project(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
    Json(req): Json<CreateProject>,
) -> ApiResult<Response> {
    let name = check_name("name", &req.name, 100)?;
    let slug = req.slug.clone().unwrap_or_else(|| users::slugify(&name));
    check_slug("slug", &slug)?;
    check_platform(&req.platform)?;
    let st = state.clone();
    let p = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::CreateProject)?;
            let txn = c.transaction()?;
            let team_id: Option<i64> = match &req.team {
                Some(t) => Some(
                    txn.query_opt("SELECT id FROM teams WHERE org_id = $1 AND slug = $2", &[&o.id, t])?
                        .ok_or_else(|| ApiError::invalid("team", "no such team"))?
                        .get(0),
                ),
                None => None,
            };
            let settings = serde_json::to_value(ProjectSettings::default()).expect("settings serialize");
            let id: i64 = txn
                .query_one(
                    "INSERT INTO projects (org_id, slug, name, platform, team_id, settings) VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
                    &[&o.id, &slug, &name, &req.platform, &team_id, &settings],
                )
                .map_err(|e| if e.code() == "23505" { ApiError::conflict("a project with this slug exists") } else { e.into() })?
                .get(0);
            let (public, secret) = new_key_pair();
            txn.execute("INSERT INTO project_keys (project_id, public_key, secret_key) VALUES ($1, $2, $3)", &[&id, &public, &secret])?;
            audit::record(&txn, &audit::Entry { target_type: Some("project"), target_id: Some(id.to_string()), data: json!({"slug": slug}), ..auth.audit(Some(o.id), action::PROJECT_CREATE) })?;
            txn.commit()?;
            load_project(&st, c, id)
        })
        .await?;
    state.store.bump_config();
    Ok((StatusCode::CREATED, axum::Json(p)).into_response())
}

/// A project.
#[utoipa::path(get, path = "/api/v1/projects/{org}/{project}", tag = "projects",
    params(("org" = String, Path), ("project" = String, Path)), responses((status = 200, body = Project)))]
async fn get_project(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, project)): Path<(String, String)>,
) -> ApiResult<axum::Json<Project>> {
    let st = state.clone();
    let p = state
        .db(move |c| {
            let p = project_sync(c, &auth, &org, &project, Permission::ReadProject)?;
            load_project(&st, c, p.id)
        })
        .await?;
    Ok(axum::Json(p))
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateProject {
    pub name: Option<String>,
    pub slug: Option<String>,
    pub platform: Option<String>,
    /// Team slug; empty string removes the team.
    pub team: Option<String>,
    /// Partial settings: given top-level keys replace the stored ones;
    /// `filters` and `scrubbing` are merged key by key.
    #[schema(value_type = Option<Object>)]
    pub settings: Option<Value>,
}

/// Merges a settings patch and validates the result.
pub fn merge_settings(current: &Value, patch: &Value) -> ApiResult<ProjectSettings> {
    let Some(patch) = patch.as_object() else { return Err(ApiError::invalid("settings", "must be an object")) };
    let mut merged = serde_json::to_value(ProjectSettings::from_json(current)).expect("settings serialize");
    let obj = merged.as_object_mut().expect("settings are an object");
    for (k, v) in patch {
        match (k.as_str(), obj.get_mut(k), v) {
            ("filters" | "scrubbing", Some(Value::Object(cur)), Value::Object(p)) => {
                for (kk, vv) in p {
                    cur.insert(kk.clone(), vv.clone());
                }
            }
            _ => {
                obj.insert(k.clone(), v.clone());
            }
        }
    }
    let s: ProjectSettings =
        serde_json::from_value(merged).map_err(|e| ApiError::invalid("settings", e.to_string()))?;
    if let Some(d) = s.retention_days
        && !(1..=3650).contains(&d)
    {
        return Err(ApiError::invalid("settings.retention_days", "between 1 and 3650"));
    }
    if s.allowed_origins.iter().any(|o| o.is_empty() || o.len() > 255) {
        return Err(ApiError::invalid("settings.allowed_origins", "entries must have 1 to 255 characters"));
    }
    for (field, list) in [
        ("settings.filters.error_messages", &s.filters.error_messages),
        ("settings.filters.releases", &s.filters.releases),
        ("settings.scrubbing.extra_fields", &s.scrubbing.extra_fields),
        ("settings.scrubbing.safe_fields", &s.scrubbing.safe_fields),
    ] {
        if list.len() > 200 || list.iter().any(|x| x.len() > 500) {
            return Err(ApiError::invalid(field, "at most 200 entries of at most 500 characters"));
        }
    }
    Ok(s)
}

/// Changes a project: name, slug, platform, team, settings.
#[utoipa::path(patch, path = "/api/v1/projects/{org}/{project}", tag = "projects",
    params(("org" = String, Path), ("project" = String, Path)), request_body = UpdateProject,
    responses((status = 200, body = Project)))]
async fn update_project(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, project)): Path<(String, String)>,
    Json(req): Json<UpdateProject>,
) -> ApiResult<axum::Json<Project>> {
    let name = req.name.as_deref().map(|n| check_name("name", n, 100)).transpose()?;
    if let Some(s) = &req.slug {
        check_slug("slug", s)?;
    }
    check_platform(&req.platform)?;
    let st = state.clone();
    let p = state
        .db(move |c| {
            let p = project_sync(c, &auth, &org, &project, Permission::WriteProject)?;
            let txn = c.transaction()?;
            if let Some(n) = &name {
                txn.execute("UPDATE projects SET name = $1 WHERE id = $2", &[n, &p.id])?;
            }
            if let Some(s) = &req.slug {
                txn.execute("UPDATE projects SET slug = $1 WHERE id = $2", &[s, &p.id])
                    .map_err(|e| if e.code() == "23505" { ApiError::conflict("a project with this slug exists") } else { e.into() })?;
            }
            if let Some(pl) = &req.platform {
                txn.execute("UPDATE projects SET platform = $1 WHERE id = $2", &[pl, &p.id])?;
            }
            if let Some(t) = &req.team {
                let team_id: Option<i64> = if t.is_empty() {
                    None
                } else {
                    Some(
                        txn.query_opt("SELECT id FROM teams WHERE org_id = $1 AND slug = $2", &[&p.org.id, t])?
                            .ok_or_else(|| ApiError::invalid("team", "no such team"))?
                            .get(0),
                    )
                };
                txn.execute("UPDATE projects SET team_id = $1 WHERE id = $2", &[&team_id, &p.id])?;
            }
            if let Some(patch) = &req.settings {
                let current: Value = txn.query_one("SELECT settings FROM projects WHERE id = $1", &[&p.id])?.get(0);
                let merged = serde_json::to_value(merge_settings(&current, patch)?).expect("settings serialize");
                txn.execute("UPDATE projects SET settings = $1 WHERE id = $2", &[&merged, &p.id])?;
            }
            audit::record(
                &txn,
                &audit::Entry {
                    target_type: Some("project"),
                    target_id: Some(p.id.to_string()),
                    data: json!({"name": name, "slug": req.slug, "platform": req.platform, "team": req.team, "settings": req.settings}),
                    ..auth.audit(Some(p.org.id), action::PROJECT_UPDATE)
                },
            )?;
            txn.commit()?;
            load_project(&st, c, p.id)
        })
        .await?;
    state.store.bump_config();
    Ok(axum::Json(p))
}

/// Deletes event data of a removed project: events, payloads, attachments
/// (rows and objects), outcomes and session counts. Best effort: failures
/// are logged, and the retention job removes what is left.
pub async fn purge_project_data(state: &ApiState, project_id: i64) {
    let objects = state.store.objects().clone();
    let result = state
        .db(move |c| {
            let keys: Vec<String> = c
                .query("SELECT object_key FROM attachments WHERE project_id = $1", &[&project_id])?
                .iter()
                .map(|r| r.get(0))
                .collect();
            for sql in [
                "DELETE FROM events WHERE project_id = $1",
                "DELETE FROM event_payloads WHERE project_id = $1",
                "DELETE FROM attachments WHERE project_id = $1",
                "DELETE FROM outcomes WHERE project_id = $1",
                "DELETE FROM session_counts WHERE project_id = $1",
            ] {
                c.execute(sql, &[&project_id])?;
            }
            for k in keys {
                if let Err(e) = objects.delete(&k) {
                    tracing::warn!(key = k, error = %e, "deleting an attachment object failed");
                }
            }
            Ok(())
        })
        .await;
    if let Err(e) = result {
        tracing::warn!(project_id, error = %e.message, "purging project data failed");
    }
}

/// Deletes a project and all its data.
#[utoipa::path(delete, path = "/api/v1/projects/{org}/{project}", tag = "projects",
    params(("org" = String, Path), ("project" = String, Path)), responses((status = 204)))]
async fn delete_project(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, project)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    let pid = state
        .db(move |c| {
            let p = project_sync(c, &auth, &org, &project, Permission::DeleteProject)?;
            let txn = c.transaction()?;
            txn.execute("DELETE FROM projects WHERE id = $1", &[&p.id])?;
            audit::record(
                &txn,
                &audit::Entry {
                    target_type: Some("project"),
                    target_id: Some(p.id.to_string()),
                    data: json!({"slug": p.slug}),
                    ..auth.audit(Some(p.org.id), action::PROJECT_DELETE)
                },
            )?;
            txn.commit()?;
            Ok(p.id)
        })
        .await?;
    state.store.bump_config();
    purge_project_data(&state, pid).await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, ToSchema)]
pub struct ProjectKey {
    #[schema(value_type = String)]
    pub id: Id,
    pub label: String,
    pub public_key: String,
    pub secret_key: String,
    pub is_active: bool,
    pub rate_limit_per_min: Option<i32>,
    pub dsn: String,
    pub created_at: DateTime<Utc>,
}

fn key_from(state: &ApiState, project_id: i64, r: &Row) -> ProjectKey {
    let public_key: String = r.get(2);
    ProjectKey {
        id: Id(r.get(0)),
        label: r.get(1),
        dsn: dsn(&state.config.public_url, &public_key, project_id),
        public_key,
        secret_key: r.get(3),
        is_active: r.get(4),
        rate_limit_per_min: r.get(5),
        created_at: r.get(6),
    }
}

const KEY_COLUMNS: &str = "id, label, public_key, secret_key, is_active, rate_limit_per_min, created_at";

/// Client keys (DSNs) of a project.
#[utoipa::path(get, path = "/api/v1/projects/{org}/{project}/keys", tag = "projects",
    params(("org" = String, Path), ("project" = String, Path)), responses((status = 200, body = Vec<ProjectKey>)))]
async fn list_keys(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, project)): Path<(String, String)>,
) -> ApiResult<axum::Json<Vec<ProjectKey>>> {
    let st = state.clone();
    let keys = state
        .db(move |c| {
            let p = project_sync(c, &auth, &org, &project, Permission::ReadProject)?;
            let rows = c.query(
                &format!("SELECT {KEY_COLUMNS} FROM project_keys WHERE project_id = $1 ORDER BY id"),
                &[&p.id],
            )?;
            Ok(rows.iter().map(|r| key_from(&st, p.id, r)).collect())
        })
        .await?;
    Ok(axum::Json(keys))
}

#[derive(Deserialize, ToSchema, Default)]
pub struct KeyRequest {
    pub label: Option<String>,
    pub is_active: Option<bool>,
    /// Events per minute for this key; `null` = unlimited.
    pub rate_limit_per_min: Option<Option<i32>>,
}

fn check_key(req: &KeyRequest) -> ApiResult<Option<String>> {
    if let Some(Some(n)) = req.rate_limit_per_min
        && n < 1
    {
        return Err(ApiError::invalid("rate_limit_per_min", "must be positive"));
    }
    req.label.as_deref().map(|l| check_name("label", l, 64)).transpose()
}

/// Creates a client key.
#[utoipa::path(post, path = "/api/v1/projects/{org}/{project}/keys", tag = "projects",
    params(("org" = String, Path), ("project" = String, Path)), request_body = KeyRequest,
    responses((status = 201, body = ProjectKey)))]
async fn create_key(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, project)): Path<(String, String)>,
    Json(req): Json<KeyRequest>,
) -> ApiResult<Response> {
    let label = check_key(&req)?.unwrap_or_else(|| "Default".into());
    let st = state.clone();
    let key = state
        .db(move |c| {
            let p = project_sync(c, &auth, &org, &project, Permission::WriteProject)?;
            let (public, secret) = new_key_pair();
            let rate = req.rate_limit_per_min.flatten();
            let r = c.query_one(
                &format!(
                    "INSERT INTO project_keys (project_id, public_key, secret_key, label, is_active, rate_limit_per_min)
                     VALUES ($1, $2, $3, $4, $5, $6) RETURNING {KEY_COLUMNS}"
                ),
                &[&p.id, &public, &secret, &label, &req.is_active.unwrap_or(true), &rate],
            )?;
            let k = key_from(&st, p.id, &r);
            audit::record(
                c,
                &audit::Entry {
                    target_type: Some("key"),
                    target_id: Some(k.id.0.to_string()),
                    data: json!({"project": p.slug, "label": label}),
                    ..auth.audit(Some(p.org.id), action::KEY_CREATE)
                },
            )?;
            Ok(k)
        })
        .await?;
    state.store.bump_config();
    Ok((StatusCode::CREATED, axum::Json(key)).into_response())
}

/// Changes a client key (label, active, rate limit).
#[utoipa::path(patch, path = "/api/v1/projects/{org}/{project}/keys/{id}", tag = "projects",
    params(("org" = String, Path), ("project" = String, Path), ("id" = String, Path)), request_body = KeyRequest,
    responses((status = 200, body = ProjectKey)))]
async fn update_key(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, project, id)): Path<(String, String, i64)>,
    Json(req): Json<KeyRequest>,
) -> ApiResult<axum::Json<ProjectKey>> {
    let label = check_key(&req)?;
    let st = state.clone();
    let key = state
        .db(move |c| {
            let p = project_sync(c, &auth, &org, &project, Permission::WriteProject)?;
            let txn = c.transaction()?;
            let exists = txn.query_opt("SELECT 1 FROM project_keys WHERE id = $1 AND project_id = $2", &[&id, &p.id])?;
            exists.ok_or_else(ApiError::not_found)?;
            if let Some(l) = &label {
                txn.execute("UPDATE project_keys SET label = $1 WHERE id = $2", &[l, &id])?;
            }
            if let Some(a) = req.is_active {
                txn.execute("UPDATE project_keys SET is_active = $1 WHERE id = $2", &[&a, &id])?;
            }
            if let Some(r) = req.rate_limit_per_min {
                txn.execute("UPDATE project_keys SET rate_limit_per_min = $1 WHERE id = $2", &[&r, &id])?;
            }
            let r = txn.query_one(&format!("SELECT {KEY_COLUMNS} FROM project_keys WHERE id = $1"), &[&id])?;
            audit::record(
                &txn,
                &audit::Entry { target_type: Some("key"), target_id: Some(id.to_string()), data: json!({"label": label, "is_active": req.is_active, "rate_limit_per_min": req.rate_limit_per_min}), ..auth.audit(Some(p.org.id), action::KEY_UPDATE) },
            )?;
            txn.commit()?;
            Ok(key_from(&st, p.id, &r))
        })
        .await?;
    state.store.bump_config();
    Ok(axum::Json(key))
}

/// Deletes a client key; SDKs using it are refused from then on.
#[utoipa::path(delete, path = "/api/v1/projects/{org}/{project}/keys/{id}", tag = "projects",
    params(("org" = String, Path), ("project" = String, Path), ("id" = String, Path)), responses((status = 204)))]
async fn delete_key(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, project, id)): Path<(String, String, i64)>,
) -> ApiResult<StatusCode> {
    state
        .db(move |c| {
            let p = project_sync(c, &auth, &org, &project, Permission::WriteProject)?;
            if c.execute("DELETE FROM project_keys WHERE id = $1 AND project_id = $2", &[&id, &p.id])? == 0 {
                return Err(ApiError::not_found());
            }
            audit::record(
                c,
                &audit::Entry {
                    target_type: Some("key"),
                    target_id: Some(id.to_string()),
                    data: json!({"project": p.slug}),
                    ..auth.audit(Some(p.org.id), action::KEY_DELETE)
                },
            )?;
            Ok(())
        })
        .await?;
    state.store.bump_config();
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct StatsParams {
    /// `received` (accepted errors), `rejected` (filtered, rate limited,
    /// invalid, discarded) or `filtered`.
    pub stat: Option<String>,
    /// Start: Unix seconds or RFC 3339 (default: 24 hours ago).
    pub since: Option<String>,
    /// End: Unix seconds or RFC 3339 (default: now).
    pub until: Option<String>,
    /// `1h` (default) or `1d`.
    pub resolution: Option<String>,
    /// Item category (default `error`).
    pub category: Option<String>,
}

pub(crate) fn parse_time(field: &str, s: &str) -> ApiResult<DateTime<Utc>> {
    if let Ok(secs) = s.parse::<i64>() {
        return DateTime::from_timestamp(secs, 0).ok_or_else(|| ApiError::invalid(field, "out of range"));
    }
    DateTime::parse_from_rfc3339(s)
        .map(|t| t.with_timezone(&Utc))
        .map_err(|_| ApiError::invalid(field, "Unix seconds or RFC 3339"))
}

/// Fills empty buckets with zero: `[[unix_seconds, count], ...]`.
pub(crate) fn fill_series(
    since: DateTime<Utc>,
    until: DateTime<Utc>,
    step: Duration,
    points: &[(DateTime<Utc>, i64)],
) -> Vec<(i64, i64)> {
    let step_s = step.num_seconds().max(1);
    let start = since.timestamp() - since.timestamp().rem_euclid(step_s);
    let end = until.timestamp();
    let mut out = Vec::new();
    let mut t = start;
    while t <= end && out.len() < 10_000 {
        let n: i64 = points
            .iter()
            .filter(|(ts, _)| ts.timestamp() - ts.timestamp().rem_euclid(step_s) == t)
            .map(|(_, n)| *n)
            .sum();
        out.push((t, n));
        t += step_s;
    }
    out
}

/// Accepted or rejected items over time, from the outcomes table.
#[utoipa::path(get, path = "/api/v1/projects/{org}/{project}/stats", tag = "projects",
    params(("org" = String, Path), ("project" = String, Path), StatsParams),
    responses((status = 200, description = "`[[unix_seconds, count], ...]`", body = Vec<Vec<i64>>)))]
async fn project_stats(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, project)): Path<(String, String)>,
    Query(q): Query<StatsParams>,
) -> ApiResult<axum::Json<Vec<(i64, i64)>>> {
    let until = q.until.as_deref().map(|s| parse_time("until", s)).transpose()?.unwrap_or_else(Utc::now);
    let since = q.since.as_deref().map(|s| parse_time("since", s)).transpose()?.unwrap_or(until - Duration::hours(24));
    let (unit, step) = match q.resolution.as_deref().unwrap_or("1h") {
        "1h" => ("hour", Duration::hours(1)),
        "1d" => ("day", Duration::days(1)),
        _ => return Err(ApiError::invalid("resolution", "1h or 1d")),
    };
    if since >= until || until - since > Duration::days(366) {
        return Err(ApiError::invalid("since", "the range must be positive and at most a year"));
    }
    let outcome_filter = match q.stat.as_deref().unwrap_or("received") {
        "received" | "accepted" => "outcome = 'accepted'",
        "rejected" => "outcome <> 'accepted'",
        "filtered" => "outcome = 'filtered'",
        "rate_limited" => "outcome = 'rate_limited'",
        _ => return Err(ApiError::invalid("stat", "received, rejected, filtered or rate_limited")),
    };
    let category = q.category.unwrap_or_else(|| "error".into());
    let points = state
        .db(move |c| {
            let p: ProjectCtx = project_sync(c, &auth, &org, &project, Permission::ReadProject)?;
            let rows = c.query(
                &format!(
                    "SELECT date_trunc('{unit}', hour), sum(quantity)::bigint FROM outcomes
                     WHERE project_id = $1 AND category = $2 AND hour >= $3 AND hour <= $4 AND {outcome_filter}
                     GROUP BY 1 ORDER BY 1"
                ),
                &[&p.id, &category, &since, &until],
            )?;
            Ok(rows.iter().map(|r| (r.get::<DateTime<Utc>>(0), r.get::<i64>(1))).collect::<Vec<_>>())
        })
        .await?;
    Ok(axum::Json(fill_series(since, until, step, &points)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dsns_follow_the_public_url() {
        assert_eq!(dsn("https://errors.example.com", "abc", 4), "https://abc@errors.example.com/4");
        assert_eq!(dsn("http://localhost:8080/rupta/", "abc", 4), "http://abc@localhost:8080/rupta/4");
    }

    #[test]
    fn settings_merge_and_validate() {
        let s = merge_settings(
            &json!({"retention_days": 30}),
            &json!({"filters": {"localhost": true}, "allowed_origins": ["https://a.com"]}),
        )
        .unwrap();
        assert_eq!(s.retention_days, Some(30));
        assert!(s.filters.localhost);
        assert_eq!(s.allowed_origins, vec!["https://a.com"]);
        assert!(merge_settings(&json!({}), &json!({"retention_days": 0})).is_err());
        assert!(merge_settings(&json!({}), &json!({"retention_days": "x"})).is_err());
        assert!(merge_settings(&json!({}), &json!([1])).is_err());
    }

    #[test]
    fn series_are_filled() {
        let since = DateTime::from_timestamp(3600 * 10 + 5, 0).unwrap();
        let until = DateTime::from_timestamp(3600 * 13, 0).unwrap();
        let pts = [(DateTime::from_timestamp(3600 * 11, 0).unwrap(), 4)];
        assert_eq!(
            fill_series(since, until, Duration::hours(1), &pts),
            vec![(36000, 0), (39600, 4), (43200, 0), (46800, 0)]
        );
    }
}
