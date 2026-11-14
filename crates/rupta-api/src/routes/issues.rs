//! Issues: search and list, detail, triage (status, assignment, priority,
//! bookmarks, seen state), bulk changes, deletion, activity and comments,
//! and saved searches.

use std::collections::HashMap;

use axum::extract::{RawQuery, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Duration, Utc};
use rupta_auth::audit::{self, action};
use rupta_auth::rbac::Permission;
use rupta_core::ids::Id;
use rupta_core::query::{self, CompileContext};
use rupta_store::squad::{Connection, Row};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::access::{OrgCtx, ProjectCtx, issue_sync, org_sync};
use crate::error::{ApiError, ApiResult};
use crate::extract::{Auth, Json, Page, PageParams, Path, Query};
use crate::sql::{Params, id_list};
use crate::state::ApiState;

pub fn routes() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(list_issues, bulk_update, bulk_delete))
        .routes(routes!(get_issue, update_issue, delete_issue))
        .routes(routes!(issue_by_short_id))
        .routes(routes!(issue_activity))
        .routes(routes!(add_comment))
        .routes(routes!(edit_comment, delete_comment))
        .routes(routes!(list_saved_searches, create_saved_search))
        .routes(routes!(update_saved_search, delete_saved_search))
}

#[derive(Serialize, ToSchema, Clone)]
pub struct ProjectRef {
    #[schema(value_type = String)]
    pub id: Id,
    pub slug: String,
    pub name: String,
    pub platform: Option<String>,
}

#[derive(Serialize, ToSchema, Clone)]
pub struct Assignee {
    /// `user` or `team`.
    #[serde(rename = "type")]
    pub kind: String,
    #[schema(value_type = String)]
    pub id: Id,
    pub name: String,
    /// For users.
    pub email: Option<String>,
}

#[derive(Serialize, ToSchema, Clone, Default)]
pub struct IssueMetadata {
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub value: Option<String>,
    pub filename: Option<String>,
    pub function: Option<String>,
}

#[derive(Serialize, ToSchema, Clone)]
pub struct Issue {
    #[schema(value_type = String)]
    pub id: Id,
    /// `PROJECT-12`.
    pub short_id: String,
    pub title: String,
    pub culprit: Option<String>,
    pub level: String,
    /// `unresolved`, `resolved` or `ignored`.
    pub status: String,
    pub substatus: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub platform: Option<String>,
    pub project: ProjectRef,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    /// Number of events.
    pub count: i64,
    /// Number of distinct users.
    pub user_count: i64,
    pub assignee: Option<Assignee>,
    pub metadata: IssueMetadata,
    /// `[[unix_seconds, count], ...]` over the requested period.
    pub stats: Vec<(i64, i64)>,
    /// `high`, `medium` or `low`.
    pub priority: String,
    pub first_release: Option<String>,
    pub last_release: Option<String>,
    pub resolved_in_release: Option<String>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub ignore_until: Option<DateTime<Utc>>,
    /// The `times_seen` value at which an ignored issue reappears.
    pub ignore_count: Option<i64>,
    /// How `resolved_in_release` is meant: `release` (resolved in that
    /// version) or `next_release` (resolved in the release after it; it is
    /// the latest release at resolution time and may be `null` when the
    /// project had none). `null` when not resolved in a release.
    pub resolved_in_mode: Option<String>,
    /// Ignored until more users: without `ignore_user_window` the
    /// `users_seen` value at which the issue reappears; with it the number
    /// of distinct users within the window.
    pub ignore_user_count: Option<i64>,
    /// Minutes of the window for `ignore_user_count`.
    pub ignore_user_window: Option<i64>,
    pub is_bookmarked: bool,
    pub has_seen: bool,
    /// Link to the issue in the web app.
    pub permalink: String,
}

const ISSUE_COLUMNS: &str = "i.id, i.short_id, i.title, i.culprit, i.level, i.status, i.substatus, i.type, i.platform, i.project_id,
    i.first_seen, i.last_seen, i.times_seen, i.users_seen, i.assignee_user_id, i.assignee_team_id, i.metadata, i.priority,
    i.first_release, i.last_release, i.resolved_in_release, i.resolved_at, i.ignore_until, i.ignore_count,
    i.resolved_in_mode, i.ignore_user_count, i.ignore_user_window";

/// Loads everything around a set of issue rows: projects, assignees,
/// bookmarks, seen state.
pub(crate) struct Decorations {
    projects: HashMap<i64, ProjectRef>,
    users: HashMap<i64, (String, String)>,
    teams: HashMap<i64, String>,
    bookmarked: Vec<i64>,
    seen: Vec<i64>,
}

pub(crate) fn decorate(c: &Connection, rows: &[Row], user_id: Option<i64>) -> ApiResult<Decorations> {
    let ids: Vec<i64> = rows.iter().map(|r| r.get(0)).collect();
    let mut pids: Vec<i64> = rows.iter().map(|r| r.get(9)).collect();
    pids.sort();
    pids.dedup();
    let mut uids: Vec<i64> = rows.iter().filter_map(|r| r.get::<Option<i64>>(14)).collect();
    uids.sort();
    uids.dedup();
    let mut tids: Vec<i64> = rows.iter().filter_map(|r| r.get::<Option<i64>>(15)).collect();
    tids.sort();
    tids.dedup();
    let projects = if pids.is_empty() {
        HashMap::new()
    } else {
        c.query(&format!("SELECT id, slug, name, platform FROM projects WHERE id IN ({})", id_list(&pids)), &[])?
            .iter()
            .map(|r| (r.get(0), ProjectRef { id: Id(r.get(0)), slug: r.get(1), name: r.get(2), platform: r.get(3) }))
            .collect()
    };
    let users = if uids.is_empty() {
        HashMap::new()
    } else {
        c.query(&format!("SELECT id, name, email FROM users WHERE id IN ({})", id_list(&uids)), &[])?
            .iter()
            .map(|r| (r.get(0), (r.get(1), r.get(2))))
            .collect()
    };
    let teams = if tids.is_empty() {
        HashMap::new()
    } else {
        c.query(&format!("SELECT id, name FROM teams WHERE id IN ({})", id_list(&tids)), &[])?
            .iter()
            .map(|r| (r.get(0), r.get(1)))
            .collect()
    };
    let (bookmarked, seen) = match (user_id, ids.is_empty()) {
        (Some(uid), false) => (
            c.query(
                &format!("SELECT issue_id FROM issue_bookmarks WHERE user_id = $1 AND issue_id IN ({})", id_list(&ids)),
                &[&uid],
            )?
            .iter()
            .map(|r| r.get(0))
            .collect(),
            c.query(
                &format!("SELECT issue_id FROM issue_seen WHERE user_id = $1 AND issue_id IN ({})", id_list(&ids)),
                &[&uid],
            )?
            .iter()
            .map(|r| r.get(0))
            .collect(),
        ),
        _ => (Vec::new(), Vec::new()),
    };
    Ok(Decorations { projects, users, teams, bookmarked, seen })
}

pub(crate) fn issue_from(state: &ApiState, org_slug: &str, r: &Row, d: &Decorations) -> Issue {
    let id: i64 = r.get(0);
    let pid: i64 = r.get(9);
    let project = d.projects.get(&pid).cloned().unwrap_or(ProjectRef {
        id: Id(pid),
        slug: String::new(),
        name: String::new(),
        platform: None,
    });
    let assignee = match (r.get::<Option<i64>>(14), r.get::<Option<i64>>(15)) {
        (Some(u), _) => d.users.get(&u).map(|(name, email)| Assignee {
            kind: "user".into(),
            id: Id(u),
            name: if name.is_empty() { email.clone() } else { name.clone() },
            email: Some(email.clone()),
        }),
        (None, Some(t)) => {
            d.teams.get(&t).map(|name| Assignee { kind: "team".into(), id: Id(t), name: name.clone(), email: None })
        }
        _ => None,
    };
    let meta: Value = r.get(16);
    let s = |k: &str| meta.get(k).and_then(Value::as_str).map(str::to_string);
    Issue {
        id: Id(id),
        short_id: format!("{}-{}", project.slug.to_uppercase(), r.get::<i64>(1)),
        title: r.get(2),
        culprit: r.get(3),
        level: r.get(4),
        status: r.get(5),
        substatus: r.get(6),
        kind: r.get(7),
        platform: r.get(8),
        project,
        first_seen: r.get(10),
        last_seen: r.get(11),
        count: r.get(12),
        user_count: r.get(13),
        assignee,
        metadata: IssueMetadata {
            kind: s("type"),
            value: s("value"),
            filename: s("filename"),
            function: s("function"),
        },
        stats: Vec::new(),
        priority: r.get(17),
        first_release: r.get(18),
        last_release: r.get(19),
        resolved_in_release: r.get(20),
        resolved_at: r.get(21),
        ignore_until: r.get(22),
        ignore_count: r.get(23),
        resolved_in_mode: r.get(24),
        ignore_user_count: r.get(25),
        ignore_user_window: r.get::<Option<i32>>(26).map(i64::from),
        is_bookmarked: d.bookmarked.contains(&id),
        has_seen: d.seen.contains(&id),
        permalink: format!("{}/organizations/{org_slug}/issues/{id}", state.config.public_url),
    }
}

/// Event counts per issue and bucket, in one query for a whole page.
pub(crate) fn issue_stats(
    c: &Connection,
    project_ids: &[i64],
    issue_ids: &[i64],
    period: Duration,
    now: DateTime<Utc>,
) -> ApiResult<HashMap<i64, Vec<(i64, i64)>>> {
    let mut out = HashMap::new();
    if issue_ids.is_empty() {
        return Ok(out);
    }
    let (unit, step) = bucket_for(period);
    let since = now - period;
    let rows = c.query(
        &format!(
            "SELECT issue_id, date_trunc('{unit}', timestamp), count(*) FROM events
             WHERE project_id IN ({}) AND issue_id IN ({}) AND timestamp >= $1
             GROUP BY 1, 2",
            id_list(project_ids),
            id_list(issue_ids)
        ),
        &[&since],
    )?;
    let mut points: HashMap<i64, Vec<(DateTime<Utc>, i64)>> = HashMap::new();
    for r in rows {
        points.entry(r.get(0)).or_default().push((r.get(1), r.get(2)));
    }
    let first = since + step;
    for id in issue_ids {
        let p = points.get(id).map(Vec::as_slice).unwrap_or(&[]);
        out.insert(*id, crate::routes::projects::fill_series(first, now, step, p));
    }
    Ok(out)
}

/// Hourly buckets up to two days, daily beyond.
pub(crate) fn bucket_for(period: Duration) -> (&'static str, Duration) {
    if period <= Duration::hours(48) { ("hour", Duration::hours(1)) } else { ("day", Duration::days(1)) }
}

pub(crate) fn parse_period(field: &str, s: Option<&str>, default: Duration) -> ApiResult<Duration> {
    match s {
        None | Some("") => Ok(default),
        Some(p) => match query::parse_duration(p) {
            Some(d) if d > Duration::zero() && d <= Duration::days(366) => Ok(d),
            _ => Err(ApiError::invalid(field, "a duration such as 24h, 14d or 90d (at most 366d)")),
        },
    }
}

/// Query parameters of the issue list.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct IssueListParams {
    /// Search query, see docs/search.md.
    pub query: Option<String>,
    /// `last_seen` (default), `first_seen`, `freq`, `users` or `priority`.
    pub sort: Option<String>,
    /// Only issues seen within this period (e.g. `24h`, `14d`); also the
    /// range of `stats` (default 24h).
    pub period: Option<String>,
    /// Only issues with events in this environment.
    pub environment: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

/// Repeated query keys (`project=1&project=2`, `id=..&id=..`) and
/// comma-separated values.
fn multi(raw: &Option<String>, key: &str) -> Vec<String> {
    let Some(raw) = raw else { return Vec::new() };
    form_urlencoded::parse(raw.as_bytes())
        .filter(|(k, _)| k == key)
        .flat_map(|(_, v)| v.split(',').map(|s| s.trim().to_string()).collect::<Vec<_>>())
        .filter(|s| !s.is_empty())
        .collect()
}

fn ids(raw: &Option<String>, key: &str) -> ApiResult<Vec<i64>> {
    multi(raw, key)
        .iter()
        .map(|s| s.parse::<i64>().map_err(|_| ApiError::invalid(key, "must be numeric ids")))
        .collect()
}

fn sort_key(sort: &str) -> ApiResult<&'static str> {
    Ok(match sort {
        "last_seen" | "date" => "(extract(epoch from i.last_seen) * 1000000)::bigint",
        "first_seen" | "new" => "(extract(epoch from i.first_seen) * 1000000)::bigint",
        "freq" => "i.times_seen",
        "users" | "user" => "i.users_seen",
        "priority" => {
            "((CASE i.priority WHEN 'high' THEN 3 WHEN 'medium' THEN 2 ELSE 1 END) * 10000000000000000 + (extract(epoch from i.last_seen) * 1000000)::bigint)"
        }
        _ => return Err(ApiError::invalid("sort", "last_seen, first_seen, freq, users or priority")),
    })
}

/// The org's projects the caller selected (all if none).
fn select_projects(c: &Connection, org: &OrgCtx, wanted: &[i64]) -> ApiResult<Vec<i64>> {
    let all: Vec<i64> = c
        .query("SELECT id FROM projects WHERE org_id = $1 ORDER BY id", &[&org.id])?
        .iter()
        .map(|r| r.get(0))
        .collect();
    if wanted.is_empty() {
        return Ok(all);
    }
    // Projects of other organizations are silently not found.
    Ok(wanted.iter().copied().filter(|p| all.contains(p)).collect())
}

/// Issues matching a search, with per-issue statistics.
#[utoipa::path(get, path = "/api/v1/orgs/{org}/issues", tag = "issues", params(("org" = String, Path),
        ("project" = Option<Vec<String>>, Query, description = "project ids, comma-separated or repeated; default all"), IssueListParams),
    responses((status = 200, body = Page<Issue>), (status = 422, description = "invalid query", body = crate::error::ErrorBody)))]
async fn list_issues(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
    RawQuery(raw): RawQuery,
    Query(q): Query<IssueListParams>,
) -> ApiResult<axum::Json<Page<Issue>>> {
    let wanted = ids(&raw, "project")?;
    let page = PageParams { cursor: q.cursor.clone(), limit: q.limit };
    let limit = page.limit();
    let cursor: Option<(i64, i64)> = page.cursor()?;
    let sort = sort_key(q.sort.as_deref().unwrap_or("last_seen"))?;
    let stats_period = parse_period("period", q.period.as_deref(), Duration::hours(24))?;
    let filter_period = q.period.is_some();
    let mut parsed =
        query::parse(q.query.as_deref().unwrap_or("")).map_err(|e| ApiError::invalid("query", e.message))?;
    if let Some(env) = q.environment.clone().filter(|e| !e.is_empty()) {
        parsed.terms.push(query::Term {
            negated: false,
            kind: query::TermKind::Filter { key: "environment".into(), op: query::Op::Eq, values: vec![env] },
        });
    }
    let st = state.clone();
    let page = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ReadProject)?;
            let projects = select_projects(c, &o, &wanted)?;
            if projects.is_empty() {
                return Ok(Page { data: Vec::new(), next_cursor: None });
            }
            let now = Utc::now();
            let mut params = Params::new();
            let mut sql = format!(
                "SELECT {ISSUE_COLUMNS}, {sort} AS sort_key FROM issues i WHERE i.project_id IN ({})",
                id_list(&projects)
            );
            if filter_period {
                sql.push_str(&format!(" AND i.last_seen >= {}", params.bind(now - stats_period)));
            }
            if let Some((key, id)) = cursor {
                let (k, i) = (params.bind(key), params.bind(id));
                sql.push_str(&format!(" AND ({sort} < {k} OR ({sort} = {k} AND i.id < {i}))"));
            }
            let ctx = CompileContext {
                user_id: auth.user_id(),
                org_id: o.id,
                project_ids: projects.clone(),
                now,
                first_param: params.next_index(),
            };
            let compiled = query::compile(&parsed, &ctx).map_err(|e| ApiError::invalid("query", e.message))?;
            params.extend_query(&compiled.params);
            sql.push_str(&format!(" AND ({}) ORDER BY sort_key DESC, i.id DESC LIMIT {}", compiled.sql, limit + 1));
            let rows = c.query(&sql, &params.refs())?;
            let d = decorate(c, &rows, auth.user_id())?;
            let keys: Vec<(i64, i64)> = rows.iter().map(|r| (r.get::<i64>(27), r.get::<i64>(0))).collect();
            let mut issues: Vec<Issue> = rows.iter().map(|r| issue_from(&st, &o.slug, r, &d)).collect();
            let page_ids: Vec<i64> = issues.iter().take(limit as usize).map(|i| i.id.0).collect();
            let mut stats = issue_stats(c, &projects, &page_ids, stats_period, now)?;
            for i in issues.iter_mut() {
                i.stats = stats.remove(&i.id.0).unwrap_or_default();
            }
            let key_of: HashMap<i64, (i64, i64)> = keys.into_iter().map(|(k, id)| (id, (k, id))).collect();
            Ok(Page::from_rows(issues, limit, |i| key_of[&i.id.0]))
        })
        .await?;
    Ok(axum::Json(page))
}

/// Changes to one or more issues.
#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct IssueUpdate {
    /// `unresolved`, `resolved` or `ignored`.
    pub status: Option<String>,
    pub substatus: Option<String>,
    /// `user:{id}`, `team:{id}`, `me`, or `null` to unassign.
    #[serde(default, deserialize_with = "crate::extract::double_option")]
    #[schema(value_type = Option<String>)]
    pub assignee: Option<Option<String>>,
    /// `high`, `medium` or `low`.
    pub priority: Option<String>,
    /// With `status: ignored`: minutes until the issue reappears.
    pub ignore_duration: Option<i64>,
    /// With `status: ignored`: further events until it reappears.
    pub ignore_count: Option<i64>,
    /// With `status: ignored`: further distinct users until it reappears.
    pub ignore_user_count: Option<i64>,
    /// With `ignore_user_count`: count the users within this many minutes
    /// (1 to 525600) instead of in total.
    pub ignore_user_window: Option<i64>,
    /// With `status: resolved`: the release that fixes it. Events of that
    /// release and of releases first seen after it regress the issue.
    pub resolved_in_release: Option<String>,
    /// With `status: resolved`: `next_release` resolves the issue in the
    /// release after the latest one seen so far (same as `substatus:
    /// in_next_release`).
    pub resolved_in: Option<String>,
    /// Per user.
    pub is_bookmarked: Option<bool>,
    /// Per user.
    pub has_seen: Option<bool>,
}

impl IssueUpdate {
    /// Resolve in the next release (`resolved_in: next_release` or
    /// `substatus: in_next_release`).
    fn in_next_release(&self) -> bool {
        self.resolved_in.as_deref() == Some("next_release") || self.substatus.as_deref() == Some("in_next_release")
    }

    fn needs_triage(&self) -> bool {
        self.status.is_some() || self.substatus.is_some() || self.assignee.is_some() || self.priority.is_some()
    }
}

enum AssignTo {
    None,
    User(i64),
    Team(i64),
}

fn validate_update(c: &Connection, org: &OrgCtx, u: &IssueUpdate, me: Option<i64>) -> ApiResult<Option<AssignTo>> {
    if let Some(s) = &u.status
        && !matches!(s.as_str(), "unresolved" | "resolved" | "ignored")
    {
        return Err(ApiError::invalid("status", "unresolved, resolved or ignored"));
    }
    if let Some(s) = &u.substatus
        && (s.is_empty() || s.len() > 64)
    {
        return Err(ApiError::invalid("substatus", "1 to 64 characters"));
    }
    if let Some(p) = &u.priority
        && !matches!(p.as_str(), "high" | "medium" | "low")
    {
        return Err(ApiError::invalid("priority", "high, medium or low"));
    }
    if u.ignore_duration.is_some_and(|d| d <= 0 || d > 525_600) {
        return Err(ApiError::invalid("ignore_duration", "1 to 525600 minutes"));
    }
    if u.ignore_count.is_some_and(|n| n <= 0) {
        return Err(ApiError::invalid("ignore_count", "must be positive"));
    }
    if u.ignore_user_count.is_some_and(|n| n <= 0) {
        return Err(ApiError::invalid("ignore_user_count", "must be positive"));
    }
    if u.ignore_user_window.is_some_and(|d| d <= 0 || d > 525_600) {
        return Err(ApiError::invalid("ignore_user_window", "1 to 525600 minutes"));
    }
    if u.ignore_user_window.is_some() && u.ignore_user_count.is_none() {
        return Err(ApiError::invalid("ignore_user_window", "only together with ignore_user_count"));
    }
    if u.resolved_in_release.as_ref().is_some_and(|r| r.is_empty() || r.len() > 250) {
        return Err(ApiError::invalid("resolved_in_release", "1 to 250 characters"));
    }
    if u.resolved_in.as_deref().is_some_and(|r| r != "next_release") {
        return Err(ApiError::invalid("resolved_in", "next_release"));
    }
    if u.in_next_release() && u.resolved_in_release.is_some() {
        return Err(ApiError::invalid("resolved_in_release", "not together with the next release"));
    }
    let Some(a) = &u.assignee else { return Ok(None) };
    let Some(a) = a else { return Ok(Some(AssignTo::None)) };
    let bad = || ApiError::invalid("assignee", "user:{id}, team:{id}, me or null");
    let target = if a == "me" {
        AssignTo::User(me.ok_or_else(bad)?)
    } else if let Some(id) = a.strip_prefix("user:") {
        AssignTo::User(id.parse().map_err(|_| bad())?)
    } else if let Some(id) = a.strip_prefix("team:") {
        AssignTo::Team(id.parse().map_err(|_| bad())?)
    } else {
        return Err(bad());
    };
    match target {
        AssignTo::User(uid) => {
            if rupta_auth::users::membership(c, org.id, uid)?.is_none() {
                return Err(ApiError::invalid("assignee", "not a member of the organization"));
            }
        }
        AssignTo::Team(tid) => {
            if c.query_opt("SELECT 1 FROM teams WHERE id = $1 AND org_id = $2", &[&tid, &org.id])?.is_none() {
                return Err(ApiError::invalid("assignee", "no such team"));
            }
        }
        AssignTo::None => {}
    }
    Ok(Some(target))
}

fn activity(c: &Connection, issue: i64, user: Option<i64>, kind: &str, data: Value) -> ApiResult<()> {
    c.execute(
        "INSERT INTO issue_activity (issue_id, user_id, kind, data) VALUES ($1, $2, $3, $4)",
        &[&issue, &user, &kind, &data],
    )?;
    Ok(())
}

/// The status columns an update writes.
#[derive(Default)]
struct StatusFields {
    substatus: String,
    resolved_at: Option<DateTime<Utc>>,
    ignore_until: Option<DateTime<Utc>>,
    ignore_count: Option<i64>,
    ignore_user_count: Option<i64>,
    ignore_user_window: Option<i32>,
    resolved_in_release: Option<String>,
    resolved_in_mode: Option<&'static str>,
}

/// The latest release of a project (by first sight), if any.
fn latest_release(c: &Connection, project_id: i64) -> ApiResult<Option<String>> {
    Ok(c.query_opt(
        "SELECT r.version FROM releases r JOIN release_projects rp ON rp.release_id = r.id
         WHERE rp.project_id = $1 ORDER BY r.created_at DESC, r.id DESC LIMIT 1",
        &[&project_id],
    )?
    .map(|r| r.get(0)))
}

/// Applies an update to issues already authorized. Records activity.
/// Returns the issues newly assigned (for notifications).
fn apply_update(
    c: &Connection,
    org: &OrgCtx,
    issue_ids: &[i64],
    u: &IssueUpdate,
    me: Option<i64>,
) -> ApiResult<Vec<Assigned>> {
    let assign = validate_update(c, org, u, me)?;
    let now = Utc::now();
    let mut assigned = Vec::new();
    for &id in issue_ids {
        let cur = c.query_one(
            "SELECT status, substatus, priority, times_seen, users_seen, project_id, assignee_user_id, assignee_team_id
             FROM issues WHERE id = $1",
            &[&id],
        )?;
        let (old_status, old_sub, old_prio, times_seen, users_seen, project_id): (
            String,
            String,
            String,
            i64,
            i64,
            i64,
        ) = (cur.get(0), cur.get(1), cur.get(2), cur.get(3), cur.get(4), cur.get(5));
        let (old_user, old_team): (Option<i64>, Option<i64>) = (cur.get(6), cur.get(7));
        if let Some(status) = &u.status {
            let f = match status.as_str() {
                "resolved" => {
                    let (release, mode, sub) = if u.in_next_release() {
                        (latest_release(c, project_id)?, Some("next_release"), "in_next_release".to_string())
                    } else {
                        let mode = u.resolved_in_release.as_ref().map(|_| "release");
                        (u.resolved_in_release.clone(), mode, u.substatus.clone().unwrap_or(old_sub.clone()))
                    };
                    StatusFields {
                        substatus: sub,
                        resolved_at: Some(now),
                        resolved_in_release: release,
                        resolved_in_mode: mode,
                        ..StatusFields::default()
                    }
                }
                "ignored" => {
                    let until = u.ignore_duration.map(|m| now + Duration::minutes(m));
                    // Stored as the absolute event count at which it reappears.
                    let count = u.ignore_count.map(|n| times_seen + n);
                    // Without a window also absolute; with one, the number of users.
                    let users =
                        u.ignore_user_count.map(|n| if u.ignore_user_window.is_some() { n } else { users_seen + n });
                    let sub = u.substatus.clone().unwrap_or_else(|| {
                        if until.is_some() || count.is_some() || users.is_some() {
                            "archived_until_condition_met".into()
                        } else {
                            "archived_forever".into()
                        }
                    });
                    StatusFields {
                        substatus: sub,
                        ignore_until: until,
                        ignore_count: count,
                        ignore_user_count: users,
                        ignore_user_window: u.ignore_user_window.map(|w| w as i32),
                        ..StatusFields::default()
                    }
                }
                _ => StatusFields {
                    substatus: u.substatus.clone().unwrap_or_else(|| "ongoing".into()),
                    ..StatusFields::default()
                },
            };
            c.execute(
                "UPDATE issues SET status = $1, substatus = $2, resolved_at = $3, ignore_until = $4, ignore_count = $5,
                     resolved_in_release = $6, resolved_in_mode = $7, ignore_user_count = $8, ignore_user_window = $9,
                     escalated_at = NULL, escalation_baseline = NULL
                 WHERE id = $10",
                &[
                    status,
                    &f.substatus,
                    &f.resolved_at,
                    &f.ignore_until,
                    &f.ignore_count,
                    &f.resolved_in_release,
                    &f.resolved_in_mode,
                    &f.ignore_user_count,
                    &f.ignore_user_window,
                    &id,
                ],
            )?;
            if *status != old_status || f.substatus != old_sub {
                activity(
                    c,
                    id,
                    me,
                    "status_change",
                    json!({"status": status, "substatus": f.substatus, "old_status": old_status, "ignore_until": f.ignore_until,
                           "ignore_count": u.ignore_count, "ignore_user_count": u.ignore_user_count,
                           "ignore_user_window": u.ignore_user_window, "resolved_in_release": f.resolved_in_release,
                           "resolved_in_mode": f.resolved_in_mode}),
                )?;
            }
        } else if let Some(sub) = &u.substatus {
            c.execute("UPDATE issues SET substatus = $1 WHERE id = $2", &[sub, &id])?;
        }
        if let Some(p) = &u.priority
            && *p != old_prio
        {
            c.execute("UPDATE issues SET priority = $1 WHERE id = $2", &[p, &id])?;
            activity(c, id, me, "priority_change", json!({"priority": p, "old_priority": old_prio}))?;
        }
        match &assign {
            Some(AssignTo::None) => {
                c.execute("UPDATE issues SET assignee_user_id = NULL, assignee_team_id = NULL WHERE id = $1", &[&id])?;
                activity(c, id, me, "unassigned", json!({}))?;
            }
            Some(AssignTo::User(uid)) => {
                c.execute(
                    "UPDATE issues SET assignee_user_id = $1, assignee_team_id = NULL WHERE id = $2",
                    &[uid, &id],
                )?;
                activity(c, id, me, "assigned", json!({"type": "user", "id": uid.to_string()}))?;
                if old_user != Some(*uid) {
                    assigned.push(Assigned { issue_id: id, user_id: Some(*uid), team_id: None });
                }
            }
            Some(AssignTo::Team(tid)) => {
                c.execute(
                    "UPDATE issues SET assignee_user_id = NULL, assignee_team_id = $1 WHERE id = $2",
                    &[tid, &id],
                )?;
                activity(c, id, me, "assigned", json!({"type": "team", "id": tid.to_string()}))?;
                if old_team != Some(*tid) {
                    assigned.push(Assigned { issue_id: id, user_id: None, team_id: Some(*tid) });
                }
            }
            None => {}
        }
        if let (Some(b), Some(uid)) = (u.is_bookmarked, me) {
            if b {
                c.execute(
                    "INSERT INTO issue_bookmarks (issue_id, user_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
                    &[&id, &uid],
                )?;
            } else {
                c.execute("DELETE FROM issue_bookmarks WHERE issue_id = $1 AND user_id = $2", &[&id, &uid])?;
            }
        }
        if let (Some(s), Some(uid)) = (u.has_seen, me) {
            if s {
                c.execute(
                    "INSERT INTO issue_seen (issue_id, user_id) VALUES ($1, $2) ON CONFLICT (issue_id, user_id) DO UPDATE SET seen_at = now()",
                    &[&id, &uid],
                )?;
            } else {
                c.execute("DELETE FROM issue_seen WHERE issue_id = $1 AND user_id = $2", &[&id, &uid])?;
            }
        }
    }
    Ok(assigned)
}

/// An issue newly assigned to a user or a team.
pub(crate) struct Assigned {
    pub issue_id: i64,
    pub user_id: Option<i64>,
    pub team_id: Option<i64>,
}

pub(crate) fn load_issue(
    state: &ApiState,
    c: &Connection,
    p: &ProjectCtx,
    id: i64,
    user_id: Option<i64>,
) -> ApiResult<Issue> {
    let row = c
        .query_opt(&format!("SELECT {ISSUE_COLUMNS} FROM issues i WHERE i.id = $1"), &[&id])?
        .ok_or_else(ApiError::not_found)?;
    let rows = vec![row];
    let d = decorate(c, &rows, user_id)?;
    let mut issue = issue_from(state, &p.org.slug, &rows[0], &d);
    issue.stats = issue_stats(c, &[p.id], &[id], Duration::hours(24), Utc::now())?.remove(&id).unwrap_or_default();
    Ok(issue)
}

/// Bulk change of the issues given as `?id=..&id=..` (at most 100).
#[utoipa::path(put, path = "/api/v1/orgs/{org}/issues", tag = "issues",
    params(("org" = String, Path), ("id" = Vec<String>, Query, description = "issue ids (repeat or comma-separate)")),
    request_body = IssueUpdate, responses((status = 200, description = "`{\"updated\": n}`")))]
async fn bulk_update(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
    RawQuery(raw): RawQuery,
    Json(u): Json<IssueUpdate>,
) -> ApiResult<axum::Json<Value>> {
    let issue_ids = ids(&raw, "id")?;
    if issue_ids.is_empty() || issue_ids.len() > 100 {
        return Err(ApiError::invalid("id", "give 1 to 100 issue ids"));
    }
    let me = auth.user_id();
    let (n, assigned) = state
        .db(move |c| {
            let perm = if u.needs_triage() { Permission::TriageIssues } else { Permission::ReadProject };
            let o = org_sync(c, &auth, &org, perm)?;
            let txn = c.transaction()?;
            let found: Vec<i64> = txn
                .query(
                    &format!("SELECT i.id FROM issues i JOIN projects p ON p.id = i.project_id WHERE p.org_id = $1 AND i.id IN ({})", id_list(&issue_ids)),
                    &[&o.id],
                )?
                .iter()
                .map(|r| r.get(0))
                .collect();
            let assigned = apply_update(&txn, &o, &found, &u, auth.user_id())?;
            txn.commit()?;
            Ok((found.len(), assigned))
        })
        .await?;
    crate::workflow::assigned(&state, me, assigned);
    Ok(axum::Json(json!({ "updated": n })))
}

fn delete_issues(c: &Connection, auth: &Auth, org: &OrgCtx, issues: &[(i64, i64)]) -> ApiResult<()> {
    let txn = c.transaction()?;
    for (id, project) in issues {
        txn.execute("DELETE FROM issues WHERE id = $1", &[id])?;
        txn.execute("DELETE FROM event_payloads WHERE project_id = $1 AND issue_id = $2", &[project, id])?;
        txn.execute("DELETE FROM events WHERE project_id = $1 AND issue_id = $2", &[project, id])?;
        audit::record(
            &txn,
            &audit::Entry {
                target_type: Some("issue"),
                target_id: Some(id.to_string()),
                ..auth.audit(Some(org.id), action::ISSUE_DELETE)
            },
        )?;
    }
    txn.commit()?;
    Ok(())
}

/// Deletes the issues given as `?id=..` with their events.
#[utoipa::path(delete, path = "/api/v1/orgs/{org}/issues", tag = "issues",
    params(("org" = String, Path), ("id" = Vec<String>, Query)), responses((status = 204)))]
async fn bulk_delete(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
    RawQuery(raw): RawQuery,
) -> ApiResult<StatusCode> {
    let issue_ids = ids(&raw, "id")?;
    if issue_ids.is_empty() || issue_ids.len() > 100 {
        return Err(ApiError::invalid("id", "give 1 to 100 issue ids"));
    }
    state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::DeleteIssues)?;
            let found: Vec<(i64, i64)> = c
                .query(
                    &format!("SELECT i.id, i.project_id FROM issues i JOIN projects p ON p.id = i.project_id WHERE p.org_id = $1 AND i.id IN ({})", id_list(&issue_ids)),
                    &[&o.id],
                )?
                .iter()
                .map(|r| (r.get(0), r.get(1)))
                .collect();
            delete_issues(c, &auth, &o, &found)
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// One issue, with 24 hours of statistics.
#[utoipa::path(get, path = "/api/v1/issues/{id}", tag = "issues", params(("id" = String, Path)),
    responses((status = 200, body = Issue), (status = 404, body = crate::error::ErrorBody)))]
async fn get_issue(State(state): State<ApiState>, auth: Auth, Path(id): Path<i64>) -> ApiResult<axum::Json<Issue>> {
    let st = state.clone();
    let issue = state
        .db(move |c| {
            let p = issue_sync(c, &auth, id, Permission::ReadProject)?;
            load_issue(&st, c, &p, id, auth.user_id())
        })
        .await?;
    Ok(axum::Json(issue))
}

/// Splits a short id such as `WEB-APP-31` into the project slug and the
/// number (at the last `-`).
fn parse_short_id(s: &str) -> Option<(String, i64)> {
    let (slug, n) = s.trim().rsplit_once('-')?;
    let n: i64 = n.parse().ok().filter(|n| *n > 0)?;
    (!slug.is_empty()).then(|| (slug.to_ascii_lowercase(), n))
}

/// The issue with a short id (`WEB-APP-31`: the project slug in upper
/// case, `-`, the number), matched case-insensitively.
#[utoipa::path(get, path = "/api/v1/orgs/{org}/issues/by-short-id/{short_id}", tag = "issues",
    params(("org" = String, Path), ("short_id" = String, Path, description = "e.g. `WEB-APP-31`")),
    responses((status = 200, body = Issue), (status = 404, body = crate::error::ErrorBody)))]
async fn issue_by_short_id(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, short_id)): Path<(String, String)>,
) -> ApiResult<axum::Json<Issue>> {
    let (slug, number) = parse_short_id(&short_id).ok_or_else(ApiError::not_found)?;
    let st = state.clone();
    let issue = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ReadProject)?;
            let row = c.query_opt(
                "SELECT i.id, p.id, p.slug, p.name, p.platform FROM issues i JOIN projects p ON p.id = i.project_id
                 WHERE p.org_id = $1 AND lower(p.slug) = $2 AND i.short_id = $3",
                &[&o.id, &slug, &number],
            )?;
            let row = row.ok_or_else(ApiError::not_found)?;
            let p = ProjectCtx { org: o, id: row.get(1), slug: row.get(2), name: row.get(3), platform: row.get(4) };
            load_issue(&st, c, &p, row.get(0), auth.user_id())
        })
        .await?;
    Ok(axum::Json(issue))
}

/// Changes an issue.
#[utoipa::path(patch, path = "/api/v1/issues/{id}", tag = "issues", params(("id" = String, Path)), request_body = IssueUpdate,
    responses((status = 200, body = Issue)))]
async fn update_issue(
    State(state): State<ApiState>,
    auth: Auth,
    Path(id): Path<i64>,
    Json(u): Json<IssueUpdate>,
) -> ApiResult<axum::Json<Issue>> {
    let st = state.clone();
    let me = auth.user_id();
    let (issue, assigned) = state
        .db(move |c| {
            let perm = if u.needs_triage() { Permission::TriageIssues } else { Permission::ReadProject };
            let p = issue_sync(c, &auth, id, perm)?;
            let txn = c.transaction()?;
            let assigned = apply_update(&txn, &p.org, &[id], &u, auth.user_id())?;
            txn.commit()?;
            Ok((load_issue(&st, c, &p, id, auth.user_id())?, assigned))
        })
        .await?;
    crate::workflow::assigned(&state, me, assigned);
    Ok(axum::Json(issue))
}

/// Deletes an issue and its events.
#[utoipa::path(delete, path = "/api/v1/issues/{id}", tag = "issues", params(("id" = String, Path)), responses((status = 204)))]
async fn delete_issue(State(state): State<ApiState>, auth: Auth, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    state
        .db(move |c| {
            let p = issue_sync(c, &auth, id, Permission::DeleteIssues)?;
            delete_issues(c, &auth, &p.org, &[(id, p.id)])
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, ToSchema)]
pub struct UserRef {
    #[schema(value_type = String)]
    pub id: Id,
    pub name: String,
    pub email: String,
}

#[derive(Serialize, ToSchema)]
pub struct Activity {
    #[schema(value_type = String)]
    pub id: Id,
    /// `comment`, `status_change`, `assigned`, `unassigned`,
    /// `priority_change`, and kinds recorded by ingestion (e.g.
    /// `first_seen`, `regression`).
    pub kind: String,
    pub user: Option<UserRef>,
    #[schema(value_type = Object)]
    pub data: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
}

fn activity_from(r: &Row) -> Activity {
    Activity {
        id: Id(r.get(0)),
        kind: r.get(1),
        user: r.get::<Option<i64>>(2).map(|id| UserRef { id: Id(id), name: r.get(3), email: r.get(4) }),
        data: r.get(5),
        created_at: r.get(6),
        updated_at: r.get(7),
    }
}

const ACTIVITY_SELECT: &str =
    "SELECT a.id, a.kind, a.user_id, coalesce(u.name, ''), coalesce(u.email, ''), a.data, a.created_at, a.updated_at
    FROM issue_activity a LEFT JOIN users u ON u.id = a.user_id";

/// Activity and comments of an issue, newest first.
#[utoipa::path(get, path = "/api/v1/issues/{id}/activity", tag = "issues", params(("id" = String, Path), PageParams),
    responses((status = 200, body = Page<Activity>)))]
async fn issue_activity(
    State(state): State<ApiState>,
    auth: Auth,
    Path(id): Path<i64>,
    Query(page): Query<PageParams>,
) -> ApiResult<axum::Json<Page<Activity>>> {
    let before: Option<i64> = page.cursor()?;
    let limit = page.limit();
    let rows = state
        .db(move |c| {
            issue_sync(c, &auth, id, Permission::ReadProject)?;
            Ok(c.query(
                &format!("{ACTIVITY_SELECT} WHERE a.issue_id = $1 AND ($2::bigint IS NULL OR a.id < $2) ORDER BY a.id DESC LIMIT $3"),
                &[&id, &before, &(limit + 1)],
            )?)
        })
        .await?;
    Ok(axum::Json(Page::from_rows(rows.iter().map(activity_from).collect(), limit, |a| a.id.0)))
}

#[derive(Deserialize, ToSchema)]
pub struct CommentBody {
    /// Markdown text, 1 to 10000 characters.
    pub text: String,
}

fn check_comment(text: &str) -> ApiResult<String> {
    let t = text.trim();
    if t.is_empty() || t.chars().count() > 10_000 {
        return Err(ApiError::invalid("text", "1 to 10000 characters"));
    }
    Ok(t.to_string())
}

/// Adds a comment.
#[utoipa::path(post, path = "/api/v1/issues/{id}/comments", tag = "issues", params(("id" = String, Path)), request_body = CommentBody,
    responses((status = 201, body = Activity)))]
async fn add_comment(
    State(state): State<ApiState>,
    auth: Auth,
    Path(id): Path<i64>,
    Json(b): Json<CommentBody>,
) -> ApiResult<Response> {
    let uid = auth.require_user()?;
    let text = check_comment(&b.text)?;
    let a = state
        .db(move |c| {
            issue_sync(c, &auth, id, Permission::TriageIssues)?;
            let aid: i64 = c
                .query_one(
                    "INSERT INTO issue_activity (issue_id, user_id, kind, data) VALUES ($1, $2, 'comment', $3) RETURNING id",
                    &[&id, &uid, &json!({"text": text})],
                )?
                .get(0);
            Ok(activity_from(&c.query_one(&format!("{ACTIVITY_SELECT} WHERE a.id = $1"), &[&aid])?))
        })
        .await?;
    if let Some(t) = a.data.get("text").and_then(Value::as_str) {
        crate::workflow::commented(&state, uid, id, t.to_string());
    }
    Ok((StatusCode::CREATED, axum::Json(a)).into_response())
}

fn own_comment(c: &Connection, auth: &Auth, issue: i64, comment: i64, allow_admin: bool) -> ApiResult<()> {
    let p = issue_sync(c, auth, issue, Permission::TriageIssues)?;
    let row = c.query_opt(
        "SELECT user_id FROM issue_activity WHERE id = $1 AND issue_id = $2 AND kind = 'comment'",
        &[&comment, &issue],
    )?;
    let author: Option<i64> = row.ok_or_else(ApiError::not_found)?.get(0);
    let admin = allow_admin && p.org.role.at_least(rupta_core::role::Role::Admin);
    if author != auth.user_id() && !admin {
        return Err(ApiError::forbidden());
    }
    Ok(())
}

/// Edits one's own comment.
#[utoipa::path(patch, path = "/api/v1/issues/{id}/comments/{comment_id}", tag = "issues",
    params(("id" = String, Path), ("comment_id" = String, Path)), request_body = CommentBody, responses((status = 200, body = Activity)))]
async fn edit_comment(
    State(state): State<ApiState>,
    auth: Auth,
    Path((id, comment)): Path<(i64, i64)>,
    Json(b): Json<CommentBody>,
) -> ApiResult<axum::Json<Activity>> {
    let text = check_comment(&b.text)?;
    let a = state
        .db(move |c| {
            own_comment(c, &auth, id, comment, false)?;
            c.execute(
                "UPDATE issue_activity SET data = $1, updated_at = now() WHERE id = $2",
                &[&json!({"text": text}), &comment],
            )?;
            Ok(activity_from(&c.query_one(&format!("{ACTIVITY_SELECT} WHERE a.id = $1"), &[&comment])?))
        })
        .await?;
    Ok(axum::Json(a))
}

/// Deletes a comment (one's own; admins may delete any).
#[utoipa::path(delete, path = "/api/v1/issues/{id}/comments/{comment_id}", tag = "issues",
    params(("id" = String, Path), ("comment_id" = String, Path)), responses((status = 204)))]
async fn delete_comment(
    State(state): State<ApiState>,
    auth: Auth,
    Path((id, comment)): Path<(i64, i64)>,
) -> ApiResult<StatusCode> {
    state
        .db(move |c| {
            own_comment(c, &auth, id, comment, true)?;
            c.execute("DELETE FROM issue_activity WHERE id = $1", &[&comment])?;
            Ok(())
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, ToSchema)]
pub struct SavedSearch {
    #[schema(value_type = String)]
    pub id: Id,
    pub name: String,
    pub query: String,
    pub sort: String,
    /// Visible to the whole organization (otherwise only to its creator).
    pub shared: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Deserialize, ToSchema)]
pub struct SavedSearchBody {
    pub name: Option<String>,
    pub query: Option<String>,
    pub sort: Option<String>,
    /// Share with the organization (admins only).
    pub shared: Option<bool>,
}

fn saved_from(r: &Row) -> SavedSearch {
    SavedSearch {
        id: Id(r.get(0)),
        name: r.get(1),
        query: r.get(2),
        sort: r.get(3),
        shared: r.get::<Option<i64>>(4).is_none(),
        created_at: r.get(5),
    }
}

/// Saved searches: the caller's own and the organization's shared ones.
#[utoipa::path(get, path = "/api/v1/orgs/{org}/saved-searches", tag = "issues", params(("org" = String, Path)),
    responses((status = 200, body = Vec<SavedSearch>)))]
async fn list_saved_searches(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
) -> ApiResult<axum::Json<Vec<SavedSearch>>> {
    let list = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ReadProject)?;
            let rows = c.query(
                "SELECT id, name, query, sort, user_id, created_at FROM saved_searches
                 WHERE org_id = $1 AND (user_id IS NULL OR user_id = $2) ORDER BY user_id NULLS FIRST, name",
                &[&o.id, &auth.user_id()],
            )?;
            Ok(rows.iter().map(saved_from).collect())
        })
        .await?;
    Ok(axum::Json(list))
}

fn check_search(b: &SavedSearchBody) -> ApiResult<()> {
    if let Some(q) = &b.query {
        query::parse(q).map_err(|e| ApiError::invalid("query", e.message))?;
    }
    if let Some(s) = &b.sort {
        sort_key(s)?;
    }
    Ok(())
}

/// Saves a search.
#[utoipa::path(post, path = "/api/v1/orgs/{org}/saved-searches", tag = "issues", params(("org" = String, Path)),
    request_body = SavedSearchBody, responses((status = 201, body = SavedSearch)))]
async fn create_saved_search(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
    Json(b): Json<SavedSearchBody>,
) -> ApiResult<Response> {
    let uid = auth.require_user()?;
    let name = crate::routes::orgs::check_name("name", b.name.as_deref().unwrap_or(""), 100)?;
    let q = b.query.clone().ok_or_else(|| ApiError::invalid("query", "required"))?;
    check_search(&b)?;
    let s = state
        .db(move |c| {
            let shared = b.shared.unwrap_or(false);
            let o = org_sync(c, &auth, &org, if shared { Permission::ManageOrg } else { Permission::ReadProject })?;
            let owner = if shared { None } else { Some(uid) };
            let r = c.query_one(
                "INSERT INTO saved_searches (org_id, user_id, name, query, sort) VALUES ($1, $2, $3, $4, $5)
                 RETURNING id, name, query, sort, user_id, created_at",
                &[&o.id, &owner, &name, &q, &b.sort.clone().unwrap_or_else(|| "last_seen".into())],
            )?;
            Ok(saved_from(&r))
        })
        .await?;
    Ok((StatusCode::CREATED, axum::Json(s)).into_response())
}

fn saved_access(c: &Connection, auth: &Auth, org: &str, id: i64) -> ApiResult<OrgCtx> {
    let o = org_sync(c, auth, org, Permission::ReadProject)?;
    let row = c.query_opt("SELECT user_id FROM saved_searches WHERE id = $1 AND org_id = $2", &[&id, &o.id])?;
    let owner: Option<i64> = row.ok_or_else(ApiError::not_found)?.get(0);
    match owner {
        Some(u) if Some(u) == auth.user_id() => Ok(o),
        Some(_) => Err(ApiError::not_found()),
        None if o.role.at_least(rupta_core::role::Role::Admin) => Ok(o),
        None => Err(ApiError::forbidden()),
    }
}

/// Changes a saved search.
#[utoipa::path(patch, path = "/api/v1/orgs/{org}/saved-searches/{id}", tag = "issues",
    params(("org" = String, Path), ("id" = String, Path)), request_body = SavedSearchBody, responses((status = 200, body = SavedSearch)))]
async fn update_saved_search(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, id)): Path<(String, i64)>,
    Json(b): Json<SavedSearchBody>,
) -> ApiResult<axum::Json<SavedSearch>> {
    check_search(&b)?;
    let name = b.name.as_deref().map(|n| crate::routes::orgs::check_name("name", n, 100)).transpose()?;
    let s = state
        .db(move |c| {
            saved_access(c, &auth, &org, id)?;
            let r = c.query_one(
                "UPDATE saved_searches SET name = coalesce($1, name), query = coalesce($2, query), sort = coalesce($3, sort)
                 WHERE id = $4 RETURNING id, name, query, sort, user_id, created_at",
                &[&name, &b.query, &b.sort, &id],
            )?;
            Ok(saved_from(&r))
        })
        .await?;
    Ok(axum::Json(s))
}

/// Deletes a saved search.
#[utoipa::path(delete, path = "/api/v1/orgs/{org}/saved-searches/{id}", tag = "issues",
    params(("org" = String, Path), ("id" = String, Path)), responses((status = 204)))]
async fn delete_saved_search(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, id)): Path<(String, i64)>,
) -> ApiResult<StatusCode> {
    state
        .db(move |c| {
            saved_access(c, &auth, &org, id)?;
            c.execute("DELETE FROM saved_searches WHERE id = $1", &[&id])?;
            Ok(())
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
