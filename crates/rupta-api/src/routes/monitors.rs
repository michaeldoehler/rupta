//! Cron monitors and their check-ins. Check-ins are written by ingestion
//! (`check_in` envelope items); the API manages the monitors.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use rupta_auth::rbac::Permission;
use rupta_core::ids::Id;
use rupta_store::squad::{Connection, Row};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::access::project_sync;
use crate::error::{ApiError, ApiResult};
use crate::extract::{Auth, Json, Page, PageParams, Path, Query};
use crate::routes::orgs::{check_name, check_slug};
use crate::state::ApiState;

pub fn routes() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(list_monitors, create_monitor))
        .routes(routes!(get_monitor, update_monitor, delete_monitor))
        .routes(routes!(list_checkins))
}

#[derive(Serialize, ToSchema)]
pub struct CheckIn {
    pub id: String,
    /// `in_progress`, `ok`, `error`, `missed` or `timeout`.
    pub status: String,
    pub environment: Option<String>,
    pub duration_ms: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Serialize, ToSchema)]
pub struct Monitor {
    #[schema(value_type = String)]
    pub id: Id,
    pub slug: String,
    pub name: String,
    /// `{type: "crontab", value: "0 * * * *"}` or
    /// `{type: "interval", value: 10, unit: "minute"}`.
    #[schema(value_type = Object)]
    pub schedule: Value,
    pub timezone: String,
    /// Minutes a check-in may be late.
    pub checkin_margin: Option<i32>,
    /// Minutes a job may run.
    pub max_runtime: Option<i32>,
    /// `active` or `disabled`.
    pub status: String,
    /// `ok`, `missed`, `error`, `timeout` or `unknown`.
    pub health: String,
    pub last_checkin: Option<DateTime<Utc>>,
    pub next_checkin: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    /// The latest check-ins (detail only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkins: Option<Vec<CheckIn>>,
}

const MONITOR_COLUMNS: &str = "id, slug, name, schedule, timezone, checkin_margin, max_runtime, status, health, last_checkin, next_checkin, created_at";

fn monitor_from(r: &Row) -> Monitor {
    Monitor {
        id: Id(r.get(0)),
        slug: r.get(1),
        name: r.get(2),
        schedule: r.get(3),
        timezone: r.get(4),
        checkin_margin: r.get(5),
        max_runtime: r.get(6),
        status: r.get(7),
        health: r.get(8),
        last_checkin: r.get(9),
        next_checkin: r.get(10),
        created_at: r.get(11),
        checkins: None,
    }
}

fn checkin_from(r: &Row) -> CheckIn {
    CheckIn {
        id: Uuid::from_bytes(r.get(0)).to_string(),
        status: r.get(1),
        environment: r.get(2),
        duration_ms: r.get(3),
        created_at: r.get(4),
        updated_at: r.get(5),
    }
}

#[derive(Deserialize, ToSchema)]
pub struct MonitorBody {
    pub name: Option<String>,
    pub slug: Option<String>,
    #[schema(value_type = Option<Object>)]
    pub schedule: Option<Value>,
    pub timezone: Option<String>,
    pub checkin_margin: Option<i32>,
    pub max_runtime: Option<i32>,
    pub status: Option<String>,
}

/// Checks a crontab field list: five fields of digits, `*`, `/`, `-`, `,`
/// (or a nickname such as `@daily`).
fn valid_crontab(s: &str) -> bool {
    let s = s.trim();
    if matches!(s, "@yearly" | "@annually" | "@monthly" | "@weekly" | "@daily" | "@hourly") {
        return true;
    }
    let fields: Vec<&str> = s.split_whitespace().collect();
    fields.len() == 5
        && fields.iter().all(|f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_alphanumeric() || b"*/-,".contains(&b)))
}

fn check_schedule(v: &Value) -> ApiResult<Value> {
    match v.get("type").and_then(Value::as_str) {
        Some("crontab") => {
            let c = v.get("value").and_then(Value::as_str).filter(|c| valid_crontab(c));
            let c =
                c.ok_or_else(|| ApiError::invalid("schedule.value", "a crontab expression such as `*/5 * * * *`"))?;
            Ok(json!({"type": "crontab", "value": c.trim()}))
        }
        Some("interval") => {
            let n = v.get("value").and_then(Value::as_i64).filter(|n| *n >= 1 && *n <= 100_000);
            let n = n.ok_or_else(|| ApiError::invalid("schedule.value", "a positive number"))?;
            let unit = v.get("unit").and_then(Value::as_str).unwrap_or("minute");
            if !matches!(unit, "minute" | "hour" | "day" | "week" | "month" | "year") {
                return Err(ApiError::invalid("schedule.unit", "minute, hour, day, week, month or year"));
            }
            Ok(json!({"type": "interval", "value": n, "unit": unit}))
        }
        _ => Err(ApiError::invalid("schedule.type", "crontab or interval")),
    }
}

fn check_body(b: &MonitorBody) -> ApiResult<()> {
    for (field, v) in [("checkin_margin", b.checkin_margin), ("max_runtime", b.max_runtime)] {
        if v.is_some_and(|n| !(1..=40_320).contains(&n)) {
            return Err(ApiError::invalid(field, "1 to 40320 minutes"));
        }
    }
    if let Some(s) = &b.status
        && s != "active"
        && s != "disabled"
    {
        return Err(ApiError::invalid("status", "active or disabled"));
    }
    if let Some(tz) = &b.timezone
        && (tz.is_empty() || tz.len() > 64 || !tz.bytes().all(|c| c.is_ascii_alphanumeric() || b"/_+-".contains(&c)))
    {
        return Err(ApiError::invalid("timezone", "an IANA time zone such as Europe/Berlin"));
    }
    Ok(())
}

fn recent_checkins(c: &Connection, monitor: i64, limit: i64) -> ApiResult<Vec<CheckIn>> {
    Ok(c.query(
        "SELECT id, status, environment, duration_ms, created_at, updated_at FROM checkins WHERE monitor_id = $1 ORDER BY created_at DESC LIMIT $2",
        &[&monitor, &limit],
    )?
    .iter()
    .map(checkin_from)
    .collect())
}

/// Monitors of a project.
#[utoipa::path(get, path = "/api/v1/projects/{org}/{project}/monitors", tag = "monitors",
    params(("org" = String, Path), ("project" = String, Path)), responses((status = 200, body = Vec<Monitor>)))]
async fn list_monitors(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, project)): Path<(String, String)>,
) -> ApiResult<axum::Json<Vec<Monitor>>> {
    let list = state
        .db(move |c| {
            let p = project_sync(c, &auth, &org, &project, Permission::ReadProject)?;
            Ok(c.query(
                &format!("SELECT {MONITOR_COLUMNS} FROM monitors WHERE project_id = $1 ORDER BY slug"),
                &[&p.id],
            )?
            .iter()
            .map(monitor_from)
            .collect())
        })
        .await?;
    Ok(axum::Json(list))
}

/// Creates a monitor. SDKs can also create monitors with check-ins that
/// carry a monitor configuration (handled by ingestion).
#[utoipa::path(post, path = "/api/v1/projects/{org}/{project}/monitors", tag = "monitors",
    params(("org" = String, Path), ("project" = String, Path)), request_body = MonitorBody, responses((status = 201, body = Monitor)))]
async fn create_monitor(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, project)): Path<(String, String)>,
    Json(b): Json<MonitorBody>,
) -> ApiResult<Response> {
    let name = check_name("name", b.name.as_deref().unwrap_or(""), 100)?;
    let slug = b.slug.clone().unwrap_or_else(|| rupta_auth::users::slugify(&name));
    check_slug("slug", &slug)?;
    let schedule = check_schedule(b.schedule.as_ref().ok_or_else(|| ApiError::invalid("schedule", "required"))?)?;
    check_body(&b)?;
    let m = state
        .db(move |c| {
            let p = project_sync(c, &auth, &org, &project, Permission::WriteProject)?;
            let r = c
                .query_one(
                    &format!(
                        "INSERT INTO monitors (project_id, slug, name, schedule, timezone, checkin_margin, max_runtime, status)
                         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING {MONITOR_COLUMNS}"
                    ),
                    &[
                        &p.id, &slug, &name, &schedule, &b.timezone.clone().unwrap_or_else(|| "UTC".into()), &b.checkin_margin, &b.max_runtime,
                        &b.status.clone().unwrap_or_else(|| "active".into()),
                    ],
                )
                .map_err(|e| if e.code() == "23505" { ApiError::conflict("a monitor with this slug exists") } else { e.into() })?;
            Ok(monitor_from(&r))
        })
        .await?;
    state.store.bump_config();
    Ok((StatusCode::CREATED, axum::Json(m)).into_response())
}

/// A monitor with its latest 50 check-ins.
#[utoipa::path(get, path = "/api/v1/projects/{org}/{project}/monitors/{monitor}", tag = "monitors",
    params(("org" = String, Path), ("project" = String, Path), ("monitor" = String, Path, description = "slug")),
    responses((status = 200, body = Monitor)))]
async fn get_monitor(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, project, slug)): Path<(String, String, String)>,
) -> ApiResult<axum::Json<Monitor>> {
    let m = state
        .db(move |c| {
            let p = project_sync(c, &auth, &org, &project, Permission::ReadProject)?;
            let r = c.query_opt(
                &format!("SELECT {MONITOR_COLUMNS} FROM monitors WHERE project_id = $1 AND slug = $2"),
                &[&p.id, &slug],
            )?;
            let mut m = monitor_from(&r.ok_or_else(ApiError::not_found)?);
            m.checkins = Some(recent_checkins(c, m.id.0, 50)?);
            Ok(m)
        })
        .await?;
    Ok(axum::Json(m))
}

/// Changes a monitor.
#[utoipa::path(patch, path = "/api/v1/projects/{org}/{project}/monitors/{monitor}", tag = "monitors",
    params(("org" = String, Path), ("project" = String, Path), ("monitor" = String, Path)), request_body = MonitorBody,
    responses((status = 200, body = Monitor)))]
async fn update_monitor(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, project, slug)): Path<(String, String, String)>,
    Json(b): Json<MonitorBody>,
) -> ApiResult<axum::Json<Monitor>> {
    let name = b.name.as_deref().map(|n| check_name("name", n, 100)).transpose()?;
    if let Some(s) = &b.slug {
        check_slug("slug", s)?;
    }
    let schedule = b.schedule.as_ref().map(check_schedule).transpose()?;
    check_body(&b)?;
    let m = state
        .db(move |c| {
            let p = project_sync(c, &auth, &org, &project, Permission::WriteProject)?;
            let r = c
                .query_opt(
                    &format!(
                        "UPDATE monitors SET name = coalesce($1, name), slug = coalesce($2, slug), schedule = coalesce($3, schedule),
                           timezone = coalesce($4, timezone), checkin_margin = coalesce($5, checkin_margin),
                           max_runtime = coalesce($6, max_runtime), status = coalesce($7, status)
                         WHERE project_id = $8 AND slug = $9 RETURNING {MONITOR_COLUMNS}"
                    ),
                    &[&name, &b.slug, &schedule, &b.timezone, &b.checkin_margin, &b.max_runtime, &b.status, &p.id, &slug],
                )
                .map_err(|e| if e.code() == "23505" { ApiError::conflict("a monitor with this slug exists") } else { e.into() })?;
            Ok(monitor_from(&r.ok_or_else(ApiError::not_found)?))
        })
        .await?;
    state.store.bump_config();
    Ok(axum::Json(m))
}

/// Deletes a monitor and its check-ins.
#[utoipa::path(delete, path = "/api/v1/projects/{org}/{project}/monitors/{monitor}", tag = "monitors",
    params(("org" = String, Path), ("project" = String, Path), ("monitor" = String, Path)), responses((status = 204)))]
async fn delete_monitor(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, project, slug)): Path<(String, String, String)>,
) -> ApiResult<StatusCode> {
    state
        .db(move |c| {
            let p = project_sync(c, &auth, &org, &project, Permission::WriteProject)?;
            if c.execute("DELETE FROM monitors WHERE project_id = $1 AND slug = $2", &[&p.id, &slug])? == 0 {
                return Err(ApiError::not_found());
            }
            Ok(())
        })
        .await?;
    state.store.bump_config();
    Ok(StatusCode::NO_CONTENT)
}

/// Check-ins of a monitor, newest first.
#[utoipa::path(get, path = "/api/v1/projects/{org}/{project}/monitors/{monitor}/checkins", tag = "monitors",
    params(("org" = String, Path), ("project" = String, Path), ("monitor" = String, Path), PageParams),
    responses((status = 200, body = Page<CheckIn>)))]
async fn list_checkins(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, project, slug)): Path<(String, String, String)>,
    Query(page): Query<PageParams>,
) -> ApiResult<axum::Json<Page<CheckIn>>> {
    let before: Option<(i64, String)> = page.cursor()?;
    let limit = page.limit();
    let rows = state
        .db(move |c| {
            let p = project_sync(c, &auth, &org, &project, Permission::ReadProject)?;
            let mid: i64 = c
                .query_opt("SELECT id FROM monitors WHERE project_id = $1 AND slug = $2", &[&p.id, &slug])?
                .ok_or_else(ApiError::not_found)?
                .get(0);
            let (ts, id) = match &before {
                Some((micros, id)) => (
                    Some(
                        DateTime::from_timestamp_micros(*micros)
                            .ok_or_else(|| ApiError::bad_request("invalid cursor"))?,
                    ),
                    Some(*Uuid::parse_str(id).map_err(|_| ApiError::bad_request("invalid cursor"))?.as_bytes()),
                ),
                None => (None, None),
            };
            Ok(c.query(
                "SELECT id, status, environment, duration_ms, created_at, updated_at FROM checkins
                 WHERE monitor_id = $1 AND ($2::timestamptz IS NULL OR created_at < $2 OR (created_at = $2 AND id < $3))
                 ORDER BY created_at DESC, id DESC LIMIT $4",
                &[&mid, &ts, &id, &(limit + 1)],
            )?)
        })
        .await?;
    let list: Vec<CheckIn> = rows.iter().map(checkin_from).collect();
    Ok(axum::Json(Page::from_rows(list, limit, |c| (c.created_at.timestamp_micros(), c.id.clone()))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crontab_shapes() {
        assert!(valid_crontab("*/5 * * * *"));
        assert!(valid_crontab("0 9 * * MON-FRI"));
        assert!(valid_crontab("@daily"));
        assert!(!valid_crontab("* * * *"));
        assert!(!valid_crontab("rm -rf / * * *"));
    }
}
