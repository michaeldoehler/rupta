//! Releases, deploys and release health: session counts from
//! `session_counts`, distinct users from `session_users` (one row per
//! user, hour, release and environment, keyed by a hash of the session's
//! `did`). Users are counted exactly with `count(DISTINCT user_hash)`;
//! project health over periods longer than 31 days uses
//! `approx_count_distinct` (HyperLogLog, typically within 1–2 %).

use std::collections::HashMap;

use axum::extract::{RawQuery, State};
use chrono::{DateTime, Duration, Utc};
use rupta_auth::rbac::Permission;
use rupta_core::ids::Id;
use rupta_store::squad::{Connection, Row};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::access::{org_sync, project_sync};
use crate::error::{ApiError, ApiResult};
use crate::extract::{Auth, Page, PageParams, Path, Query};
use crate::routes::issues::{bucket_for, parse_period};
use crate::sql::id_list;
use crate::state::ApiState;

pub fn routes() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new().routes(routes!(list_releases)).routes(routes!(get_release)).routes(routes!(project_health))
}

#[derive(Serialize, ToSchema, Clone)]
pub struct ReleaseProject {
    #[schema(value_type = String)]
    pub id: Id,
    pub slug: String,
    pub name: String,
    /// The project's platform, e.g. `javascript-react`.
    pub platform: Option<String>,
    pub new_issues: i64,
}

#[derive(Serialize, ToSchema, Clone, Default)]
pub struct Health {
    pub total_sessions: i64,
    pub crashed_sessions: i64,
    pub errored_sessions: i64,
    pub abnormal_sessions: i64,
    /// Percent of sessions that did not crash; `null` without sessions.
    pub crash_free_sessions: Option<f64>,
    /// Distinct users (sessions with a `did`).
    pub total_users: i64,
    /// Distinct users with at least one crashed session.
    pub crashed_users: i64,
    /// Percent of users without a crashed session; `null` without
    /// sessions that carry a `did`.
    pub crash_free_users: Option<f64>,
    /// Share (percent) of the projects' sessions in the last 24 hours
    /// that used this release.
    pub adoption: Option<f64>,
}

#[derive(Serialize, ToSchema, Clone)]
pub struct Deploy {
    #[schema(value_type = String)]
    pub id: Id,
    pub environment: String,
    pub name: Option<String>,
    pub url: Option<String>,
    pub date_started: Option<DateTime<Utc>>,
    pub date_finished: DateTime<Utc>,
}

#[derive(Serialize, ToSchema)]
pub struct Release {
    #[schema(value_type = String)]
    pub id: Id,
    pub version: String,
    /// The version without a `package@` prefix, shortened if it is a
    /// commit hash.
    pub short_version: String,
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    pub url: Option<String>,
    pub date_released: Option<DateTime<Utc>>,
    pub date_created: DateTime<Utc>,
    pub first_event: Option<DateTime<Utc>>,
    pub last_event: Option<DateTime<Utc>>,
    pub projects: Vec<ReleaseProject>,
    pub new_issues: i64,
    pub health: Health,
    pub last_deploy: Option<Deploy>,
}

#[derive(Serialize, ToSchema)]
pub struct ReleaseDetail {
    #[serde(flatten)]
    pub release: Release,
    pub deploys: Vec<Deploy>,
    /// Health per project.
    pub project_health: Vec<ProjectHealthRef>,
    /// Daily `[[unix_seconds, sessions, crashed, users, crashed_users]]`
    /// of the last 14 days (users distinct per day).
    pub sessions: Vec<(i64, i64, i64, i64, i64)>,
}

#[derive(Serialize, ToSchema)]
pub struct ProjectHealthRef {
    #[schema(value_type = String)]
    pub project_id: Id,
    pub slug: String,
    pub health: Health,
}

pub(crate) fn short_version(v: &str) -> String {
    let v = v.rsplit_once('@').map(|(_, rest)| rest).unwrap_or(v);
    if v.len() >= 32 && v.bytes().all(|b| b.is_ascii_hexdigit()) { v[..12].to_string() } else { v.to_string() }
}

fn percent_free(total: i64, bad: i64) -> Option<f64> {
    (total > 0).then(|| ((total - bad).max(0) as f64 / total as f64 * 10000.0).round() / 100.0)
}

fn health(total: i64, crashed: i64, errored: i64, abnormal: i64, users: (i64, i64)) -> Health {
    Health {
        total_sessions: total,
        crashed_sessions: crashed,
        errored_sessions: errored,
        abnormal_sessions: abnormal,
        crash_free_sessions: percent_free(total, crashed),
        total_users: users.0,
        crashed_users: users.1,
        crash_free_users: percent_free(users.0, users.1),
        adoption: None,
    }
}

/// The distinct-count aggregate for a time range: exact up to 31 days.
fn distinct_users(period: Option<Duration>) -> &'static str {
    match period {
        Some(p) if p > Duration::days(31) => "approx_count_distinct(user_hash)",
        _ => "count(DISTINCT user_hash)",
    }
}

/// `(users, crashed users)` of `session_users` rows matching `cond`
/// (an SQL condition over the parameters `params`).
fn user_counts(
    c: &Connection,
    cond: &str,
    params: &[&dyn rupta_store::squad::ToSql],
    period: Option<Duration>,
) -> ApiResult<(i64, i64)> {
    let agg = distinct_users(period);
    let users: i64 = c.query_one(&format!("SELECT {agg} FROM session_users WHERE {cond}"), params)?.get(0);
    if users == 0 {
        return Ok((0, 0));
    }
    let crashed: i64 =
        c.query_one(&format!("SELECT {agg} FROM session_users WHERE {cond} AND crashed"), params)?.get(0);
    Ok((users, crashed))
}

fn deploy_from(r: &Row, off: usize) -> Deploy {
    Deploy {
        id: Id(r.get(off)),
        environment: r.get(off + 1),
        name: r.get(off + 2),
        url: r.get(off + 3),
        date_started: r.get(off + 4),
        date_finished: r.get(off + 5),
    }
}

/// Builds releases with projects, health and last deploy, limited to `projects`.
fn decorate(c: &Connection, rows: &[Row], projects: &[i64]) -> ApiResult<Vec<Release>> {
    let mut out = Vec::new();
    let since = Utc::now() - Duration::hours(24);
    let recent_total: i64 = c
        .query_one(
            &format!(
                "SELECT coalesce(sum(total), 0)::bigint FROM session_counts WHERE project_id IN ({}) AND hour >= $1",
                id_list(projects)
            ),
            &[&since],
        )?
        .get(0);
    for r in rows {
        let id: i64 = r.get(0);
        let version: String = r.get(1);
        let rp: Vec<ReleaseProject> = c
            .query(
                &format!(
                    "SELECT p.id, p.slug, p.name, rp.new_issues, p.platform FROM release_projects rp JOIN projects p ON p.id = rp.project_id
                     WHERE rp.release_id = $1 AND p.id IN ({}) ORDER BY p.slug",
                    id_list(projects)
                ),
                &[&id],
            )?
            .iter()
            .map(|x| ReleaseProject { id: Id(x.get(0)), slug: x.get(1), name: x.get(2), platform: x.get(4), new_issues: x.get(3) })
            .collect();
        let hrow = c.query_one(
            &format!(
                "SELECT coalesce(sum(total), 0)::bigint, coalesce(sum(crashed), 0)::bigint, coalesce(sum(errored), 0)::bigint,
                        coalesce(sum(abnormal), 0)::bigint, coalesce(sum(total) FILTER (WHERE hour >= $2), 0)::bigint
                 FROM session_counts WHERE release = $1 AND project_id IN ({})",
                id_list(projects)
            ),
            &[&version, &since],
        )?;
        let users =
            user_counts(c, &format!("release = $1 AND project_id IN ({})", id_list(projects)), &[&version], None)?;
        let mut h = health(hrow.get(0), hrow.get(1), hrow.get(2), hrow.get(3), users);
        let recent: i64 = hrow.get(4);
        h.adoption = (recent_total > 0).then(|| (recent as f64 / recent_total as f64 * 10000.0).round() / 100.0);
        let last_deploy = c
            .query_opt(
                "SELECT id, environment, name, url, date_started, date_finished FROM deploys WHERE release_id = $1 ORDER BY date_finished DESC, id DESC LIMIT 1",
                &[&id],
            )?
            .map(|d| deploy_from(&d, 0));
        out.push(Release {
            id: Id(id),
            short_version: short_version(&version),
            version,
            reference: r.get(2),
            url: r.get(3),
            date_released: r.get(4),
            date_created: r.get(5),
            first_event: r.get(6),
            last_event: r.get(7),
            new_issues: rp.iter().map(|p| p.new_issues).sum(),
            projects: rp,
            health: h,
            last_deploy,
        });
    }
    Ok(out)
}

const RELEASE_COLUMNS: &str =
    "r.id, r.version, r.ref, r.url, r.date_released, r.created_at, r.first_event, r.last_event";

fn org_projects(c: &Connection, org_id: i64, raw: &Option<String>) -> ApiResult<Vec<i64>> {
    let all: Vec<i64> =
        c.query("SELECT id FROM projects WHERE org_id = $1", &[&org_id])?.iter().map(|r| r.get(0)).collect();
    let wanted: Vec<i64> = raw
        .as_deref()
        .map(|raw| {
            form_urlencoded::parse(raw.as_bytes())
                .filter(|(k, _)| k == "project")
                .flat_map(|(_, v)| v.split(',').filter_map(|s| s.trim().parse().ok()).collect::<Vec<i64>>())
                .collect()
        })
        .unwrap_or_default();
    Ok(if wanted.is_empty() { all } else { wanted.into_iter().filter(|p| all.contains(p)).collect() })
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ReleaseListParams {
    /// Substring of the version.
    pub query: Option<String>,
    /// `date` (newest first; default).
    pub sort: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

/// Releases of the organization, newest first.
#[utoipa::path(get, path = "/api/v1/orgs/{org}/releases", tag = "releases",
    params(("org" = String, Path), ("project" = Option<Vec<String>>, Query, description = "project ids"), ReleaseListParams),
    responses((status = 200, body = Page<Release>)))]
async fn list_releases(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
    RawQuery(raw): RawQuery,
    Query(q): Query<ReleaseListParams>,
) -> ApiResult<axum::Json<Page<Release>>> {
    if q.sort.as_deref().is_some_and(|s| s != "date") {
        return Err(ApiError::invalid("sort", "only date is supported"));
    }
    let page = PageParams { cursor: q.cursor.clone(), limit: q.limit };
    let before: Option<i64> = page.cursor()?;
    let limit = page.limit();
    let pattern = q
        .query
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| format!("%{}%", s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")));
    let releases = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ReadProject)?;
            let projects = org_projects(c, o.id, &raw)?;
            if projects.is_empty() {
                return Ok(Vec::new());
            }
            let rows = c.query(
                &format!(
                    "SELECT {RELEASE_COLUMNS} FROM releases r WHERE r.org_id = $1
                     AND EXISTS (SELECT 1 FROM release_projects rp WHERE rp.release_id = r.id AND rp.project_id IN ({}))
                     AND ($2::text IS NULL OR r.version ILIKE $2) AND ($3::bigint IS NULL OR r.id < $3)
                     ORDER BY r.id DESC LIMIT $4",
                    id_list(&projects)
                ),
                &[&o.id, &pattern, &before, &(limit + 1)],
            )?;
            decorate(c, &rows, &projects)
        })
        .await?;
    Ok(axum::Json(Page::from_rows(releases, limit, |r| r.id.0)))
}

/// A release with deploys, per-project health and daily sessions.
#[utoipa::path(get, path = "/api/v1/orgs/{org}/releases/{version}", tag = "releases",
    params(("org" = String, Path), ("version" = String, Path, description = "URL-encoded version")),
    responses((status = 200, body = ReleaseDetail), (status = 404, body = crate::error::ErrorBody)))]
async fn get_release(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, version)): Path<(String, String)>,
) -> ApiResult<axum::Json<ReleaseDetail>> {
    let detail = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ReadProject)?;
            let projects = org_projects(c, o.id, &None)?;
            let row = c
                .query_opt(&format!("SELECT {RELEASE_COLUMNS} FROM releases r WHERE r.org_id = $1 AND r.version = $2"), &[&o.id, &version])?
                .ok_or_else(ApiError::not_found)?;
            let id: i64 = row.get(0);
            let release = decorate(c, std::slice::from_ref(&row), &projects)?.remove(0);
            let deploys = c
                .query("SELECT id, environment, name, url, date_started, date_finished FROM deploys WHERE release_id = $1 ORDER BY date_finished DESC, id DESC", &[&id])?
                .iter()
                .map(|d| deploy_from(d, 0))
                .collect();
            let mut project_health = Vec::new();
            for p in &release.projects {
                let h = c.query_one(
                    "SELECT coalesce(sum(total), 0)::bigint, coalesce(sum(crashed), 0)::bigint, coalesce(sum(errored), 0)::bigint, coalesce(sum(abnormal), 0)::bigint
                     FROM session_counts WHERE release = $1 AND project_id = $2",
                    &[&version, &p.id.0],
                )?;
                let users = user_counts(c, "release = $1 AND project_id = $2", &[&version, &p.id.0], None)?;
                project_health.push(ProjectHealthRef { project_id: p.id, slug: p.slug.clone(), health: health(h.get(0), h.get(1), h.get(2), h.get(3), users) });
            }
            let now = Utc::now();
            let since = now - Duration::days(14);
            let pts = c.query(
                &format!(
                    "SELECT date_trunc('day', hour), sum(total)::bigint, sum(crashed)::bigint FROM session_counts
                     WHERE release = $1 AND project_id IN ({}) AND hour >= $2 GROUP BY 1",
                    id_list(&projects)
                ),
                &[&version, &since],
            )?;
            let totals: Vec<(DateTime<Utc>, i64)> = pts.iter().map(|r| (r.get(0), r.get(1))).collect();
            let crashed: Vec<(DateTime<Utc>, i64)> = pts.iter().map(|r| (r.get(0), r.get(2))).collect();
            let users = user_series(
                c,
                "day",
                &format!("release = $1 AND project_id IN ({}) AND hour >= $2", id_list(&projects)),
                &[&version, &since],
                None,
            )?;
            let sessions = combine(since + Duration::days(1), now, Duration::days(1), &totals, &crashed, &users);
            Ok(ReleaseDetail { release, deploys, project_health, sessions })
        })
        .await?;
    Ok(axum::Json(detail))
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct HealthParams {
    /// `24h` (default), `7d`, `14d`, `30d`, `90d`.
    pub period: Option<String>,
    pub environment: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct ReleaseHealth {
    pub version: String,
    pub health: Health,
}

#[derive(Serialize, ToSchema)]
pub struct ProjectHealth {
    pub period: String,
    pub resolution: i64,
    pub health: Health,
    /// `[[unix_seconds, sessions, crashed, users, crashed_users]]`; users
    /// are distinct per bucket.
    pub series: Vec<(i64, i64, i64, i64, i64)>,
    /// Releases with sessions in the period, most sessions first; `adoption`
    /// is the share of the period's sessions.
    pub releases: Vec<ReleaseHealth>,
}

/// Session health of a project over a period.
#[utoipa::path(get, path = "/api/v1/projects/{org}/{project}/health", tag = "releases",
    params(("org" = String, Path), ("project" = String, Path), HealthParams), responses((status = 200, body = ProjectHealth)))]
async fn project_health(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, project)): Path<(String, String)>,
    Query(q): Query<HealthParams>,
) -> ApiResult<axum::Json<ProjectHealth>> {
    let period = parse_period("period", q.period.as_deref(), Duration::hours(24))?;
    let (unit, step) = bucket_for(period);
    let env = q.environment.clone().filter(|e| !e.is_empty());
    let now = Utc::now();
    let since = now - period;
    let (rows, per_release, users, user_points, release_users) = state
        .db(move |c| {
            let p = project_sync(c, &auth, &org, &project, Permission::ReadProject)?;
            let rows = c.query(
                &format!(
                    "SELECT date_trunc('{unit}', hour), sum(total)::bigint, sum(crashed)::bigint, sum(errored)::bigint, sum(abnormal)::bigint
                     FROM session_counts WHERE project_id = $1 AND hour >= $2 AND ($3::text IS NULL OR environment = $3) GROUP BY 1"
                ),
                &[&p.id, &since, &env],
            )?;
            let per_release = c.query(
                "SELECT release, sum(total)::bigint, sum(crashed)::bigint, sum(errored)::bigint, sum(abnormal)::bigint
                 FROM session_counts WHERE project_id = $1 AND hour >= $2 AND ($3::text IS NULL OR environment = $3)
                 GROUP BY 1 ORDER BY 2 DESC LIMIT 50",
                &[&p.id, &since, &env],
            )?;
            // The column engine's fast path wants plain comparisons, so the
            // environment condition is added only when there is one.
            let (cond, env_cond) = match &env {
                Some(_) => ("project_id = $1 AND hour >= $2 AND environment = $3", true),
                None => ("project_id = $1 AND hour >= $2", false),
            };
            let e = env.clone().unwrap_or_default();
            let params: Vec<&dyn rupta_store::squad::ToSql> =
                if env_cond { vec![&p.id, &since, &e] } else { vec![&p.id, &since] };
            let users = user_counts(c, cond, &params, Some(period))?;
            let user_points = user_series(c, unit, cond, &params, Some(period))?;
            let agg = distinct_users(Some(period));
            let mut release_users: HashMap<String, (i64, i64)> = HashMap::new();
            for r in c.query(&format!("SELECT release, {agg} FROM session_users WHERE {cond} GROUP BY 1"), &params)? {
                release_users.entry(r.get(0)).or_default().0 = r.get(1);
            }
            for r in c.query(&format!("SELECT release, {agg} FROM session_users WHERE {cond} AND crashed GROUP BY 1"), &params)? {
                release_users.entry(r.get(0)).or_default().1 = r.get(1);
            }
            Ok((rows, per_release, users, user_points, release_users))
        })
        .await?;
    let sum = |i: usize| rows.iter().map(|r| r.get::<i64>(i)).sum::<i64>();
    let total = sum(1);
    let h = health(total, sum(2), sum(3), sum(4), users);
    let tot: Vec<(DateTime<Utc>, i64)> = rows.iter().map(|r| (r.get(0), r.get(1))).collect();
    let cr: Vec<(DateTime<Utc>, i64)> = rows.iter().map(|r| (r.get(0), r.get(2))).collect();
    let series = combine(since + step, now, step, &tot, &cr, &user_points);
    let releases = per_release
        .iter()
        .map(|r| {
            let version: String = r.get(0);
            let u = release_users.get(&version).copied().unwrap_or_default();
            let mut rh = health(r.get(1), r.get(2), r.get(3), r.get(4), u);
            rh.adoption = (total > 0).then(|| (r.get::<i64>(1) as f64 / total as f64 * 10000.0).round() / 100.0);
            ReleaseHealth { version, health: rh }
        })
        .collect();
    Ok(axum::Json(ProjectHealth {
        period: q.period.unwrap_or_else(|| "24h".into()),
        resolution: step.num_seconds(),
        health: h,
        series,
        releases,
    }))
}

/// Distinct users and crashed users per bucket: `(bucket, users, crashed)`.
type UserPoint = (DateTime<Utc>, i64, i64);

fn user_series(
    c: &Connection,
    unit: &str,
    cond: &str,
    params: &[&dyn rupta_store::squad::ToSql],
    period: Option<Duration>,
) -> ApiResult<Vec<UserPoint>> {
    let agg = distinct_users(period);
    let mut points: HashMap<DateTime<Utc>, (i64, i64)> = HashMap::new();
    for r in c.query(
        &format!("SELECT date_trunc('{unit}', hour), {agg} FROM session_users WHERE {cond} GROUP BY 1"),
        params,
    )? {
        points.entry(r.get(0)).or_default().0 = r.get(1);
    }
    for r in c.query(
        &format!("SELECT date_trunc('{unit}', hour), {agg} FROM session_users WHERE {cond} AND crashed GROUP BY 1"),
        params,
    )? {
        points.entry(r.get(0)).or_default().1 = r.get(1);
    }
    Ok(points.into_iter().map(|(t, (u, x))| (t, u, x)).collect())
}

/// `[[unix_seconds, sessions, crashed, users, crashed_users]]` over filled buckets.
fn combine(
    first: DateTime<Utc>,
    now: DateTime<Utc>,
    step: Duration,
    totals: &[(DateTime<Utc>, i64)],
    crashed: &[(DateTime<Utc>, i64)],
    users: &[UserPoint],
) -> Vec<(i64, i64, i64, i64, i64)> {
    use crate::routes::projects::fill_series;
    let u: Vec<(DateTime<Utc>, i64)> = users.iter().map(|p| (p.0, p.1)).collect();
    let cu: Vec<(DateTime<Utc>, i64)> = users.iter().map(|p| (p.0, p.2)).collect();
    let b: HashMap<i64, i64> = fill_series(first, now, step, crashed).into_iter().collect();
    let c: HashMap<i64, i64> = fill_series(first, now, step, &u).into_iter().collect();
    let d: HashMap<i64, i64> = fill_series(first, now, step, &cu).into_iter().collect();
    fill_series(first, now, step, totals)
        .into_iter()
        .map(|(t, n)| {
            (t, n, b.get(&t).copied().unwrap_or(0), c.get(&t).copied().unwrap_or(0), d.get(&t).copied().unwrap_or(0))
        })
        .collect()
}
