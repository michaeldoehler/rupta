//! Events of issues: lists, full event documents with navigation, tag
//! distributions, statistics, user feedback, attachments, and a minimal
//! event query endpoint.

use std::collections::{BTreeMap, HashMap};

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Duration, Utc};
use rupta_auth::rbac::Permission;
use rupta_core::ids::Id;
use rupta_core::query::{self, CompileContext};
use rupta_store::squad::{Connection, Row};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::access::{ProjectCtx, issue_sync, org_sync, project_by_id_sync, project_sync};
use crate::error::{ApiError, ApiResult};
use crate::extract::{Auth, Page, PageParams, Path, Query};
use crate::routes::issues::{bucket_for, parse_period};
use crate::sql::{Params, id_list};
use crate::state::ApiState;

pub fn routes() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(issue_events))
        .routes(routes!(issue_event))
        .routes(routes!(issue_tags))
        .routes(routes!(tag_values))
        .routes(routes!(issue_stats))
        .routes(routes!(issue_feedback))
        .routes(routes!(project_feedback))
        .routes(routes!(issue_attachments))
        .routes(routes!(download_attachment))
        .routes(routes!(org_event))
        .routes(routes!(query_events))
}

/// Event ids on the wire: 32 lowercase hex digits.
fn event_id_str(bytes: [u8; 16]) -> String {
    Uuid::from_bytes(bytes).simple().to_string()
}

fn parse_event_id(s: &str) -> ApiResult<[u8; 16]> {
    Uuid::parse_str(s).map(|u| *u.as_bytes()).map_err(|_| ApiError::not_found())
}

#[derive(Serialize, ToSchema)]
pub struct EventUser {
    pub id: Option<String>,
    pub email: Option<String>,
    pub ip_address: Option<String>,
}

/// One event in a list.
#[derive(Serialize, ToSchema)]
pub struct EventSummary {
    pub event_id: String,
    #[schema(value_type = String)]
    pub issue_id: Id,
    #[schema(value_type = String)]
    pub project_id: Id,
    pub timestamp: DateTime<Utc>,
    pub received: DateTime<Utc>,
    pub level: Option<String>,
    pub title: String,
    pub message: Option<String>,
    pub environment: Option<String>,
    pub release: Option<String>,
    pub transaction: Option<String>,
    pub user: EventUser,
    pub browser: Option<String>,
    pub os: Option<String>,
    pub device: Option<String>,
    pub url: Option<String>,
    #[schema(value_type = Object)]
    pub tags: Value,
}

const EVENT_COLUMNS: &str = "e.event_id, e.issue_id, e.project_id, e.timestamp, e.received, e.level, e.message, e.exception_type,
    e.exception_value, e.environment, e.release, e.transaction, e.user_id, e.user_email, e.user_ip, e.browser, e.os, e.device, e.url, e.tags";

fn summary_from(r: &Row) -> EventSummary {
    let message: Option<String> = r.get(6);
    let (ty, val): (Option<String>, Option<String>) = (r.get(7), r.get(8));
    let title = match (&ty, &val) {
        (Some(t), Some(v)) => format!("{t}: {v}"),
        (Some(t), None) => t.clone(),
        (None, Some(v)) => v.clone(),
        _ => message.clone().unwrap_or_else(|| "<unlabeled event>".into()),
    };
    EventSummary {
        event_id: event_id_str(r.get(0)),
        issue_id: Id(r.get(1)),
        project_id: Id(r.get(2)),
        timestamp: r.get(3),
        received: r.get(4),
        level: r.get(5),
        title,
        message,
        environment: r.get(9),
        release: r.get(10),
        transaction: r.get(11),
        user: EventUser { id: r.get(12), email: r.get(13), ip_address: r.get(14) },
        browser: r.get(15),
        os: r.get(16),
        device: r.get(17),
        url: r.get(18),
        tags: r.get::<Option<Value>>(19).unwrap_or(json!({})),
    }
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct EventListParams {
    /// Event filter (docs/search.md, event keys only).
    pub query: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

/// Events of an issue, newest first.
#[utoipa::path(get, path = "/api/v1/issues/{id}/events", tag = "issues", params(("id" = String, Path), EventListParams),
    responses((status = 200, body = Page<EventSummary>)))]
async fn issue_events(
    State(state): State<ApiState>,
    auth: Auth,
    Path(id): Path<i64>,
    Query(q): Query<EventListParams>,
) -> ApiResult<axum::Json<Page<EventSummary>>> {
    let page = PageParams { cursor: q.cursor.clone(), limit: q.limit };
    let limit = page.limit();
    let cursor: Option<(i64, String)> = page.cursor()?;
    let parsed = query::parse(q.query.as_deref().unwrap_or("")).map_err(|e| ApiError::invalid("query", e.message))?;
    let rows = state
        .db(move |c| {
            let p = issue_sync(c, &auth, id, Permission::ReadProject)?;
            let mut params = Params::new();
            let mut sql = format!(
                "SELECT {EVENT_COLUMNS} FROM events e WHERE e.project_id = {} AND e.issue_id = {}",
                p.id,
                params.bind(id)
            );
            if let Some((micros, eid)) = &cursor {
                let ts =
                    DateTime::from_timestamp_micros(*micros).ok_or_else(|| ApiError::bad_request("invalid cursor"))?;
                let eid = parse_event_id(eid).map_err(|_| ApiError::bad_request("invalid cursor"))?;
                let (t, e) = (params.bind(ts), params.bind(eid));
                sql.push_str(&format!(" AND (e.timestamp < {t} OR (e.timestamp = {t} AND e.event_id < {e}))"));
            }
            let ctx = CompileContext {
                user_id: auth.user_id(),
                org_id: p.org.id,
                project_ids: vec![p.id],
                now: Utc::now(),
                first_param: params.next_index(),
            };
            let compiled = query::compile_events(&parsed, &ctx).map_err(|e| ApiError::invalid("query", e.message))?;
            params.extend_query(&compiled.params);
            sql.push_str(&format!(
                " AND ({}) ORDER BY e.timestamp DESC, e.event_id DESC LIMIT {}",
                compiled.sql,
                limit + 1
            ));
            Ok(c.query(&sql, &params.refs())?)
        })
        .await?;
    let events: Vec<EventSummary> = rows.iter().map(summary_from).collect();
    Ok(axum::Json(Page::from_rows(events, limit, |e| (e.timestamp.timestamp_micros(), e.event_id.clone()))))
}

/// Reads and decompresses a stored event document.
pub(crate) fn load_payload(c: &Connection, project_id: i64, event_id: [u8; 16]) -> ApiResult<Option<Value>> {
    let row = c.query_opt(
        "SELECT data FROM event_payloads WHERE project_id = $1 AND event_id = $2",
        &[&project_id, &event_id],
    )?;
    let Some(row) = row else { return Ok(None) };
    let data: Vec<u8> = row.get(0);
    let raw =
        zstd::decode_all(data.as_slice()).map_err(|e| ApiError::internal(format!("corrupt event payload: {e}")))?;
    let v: Value =
        serde_json::from_slice(&raw).map_err(|e| ApiError::internal(format!("corrupt event payload: {e}")))?;
    Ok(Some(v))
}

/// The full event, with ids filled in and the neighbours within the issue.
fn full_event(c: &Connection, p: &ProjectCtx, issue_id: i64, event_id: [u8; 16]) -> ApiResult<Value> {
    let row = c.query_opt(
        &format!("SELECT {EVENT_COLUMNS} FROM events e WHERE e.project_id = $1 AND e.event_id = $2"),
        &[&p.id, &event_id],
    )?;
    let payload = load_payload(c, p.id, event_id)?;
    let (mut doc, ts, issue) = match (&payload, &row) {
        (None, None) => return Err(ApiError::not_found()),
        (Some(v), r) => (
            v.clone(),
            r.as_ref().map(|r| r.get::<DateTime<Utc>>(3)),
            r.as_ref().map(|r| r.get::<i64>(1)).unwrap_or(issue_id),
        ),
        (None, Some(r)) => {
            // Payload removed (retention) but the row is there: a summary.
            let s = summary_from(r);
            (serde_json::to_value(&s).unwrap_or_default(), Some(s.timestamp), s.issue_id.0)
        }
    };
    if issue != issue_id && issue_id != 0 {
        return Err(ApiError::not_found());
    }
    let (prev, next) = match ts {
        Some(ts) => {
            let prev = c.query_opt(
                "SELECT event_id FROM events WHERE project_id = $1 AND issue_id = $2 AND (timestamp < $3 OR (timestamp = $3 AND event_id < $4))
                 ORDER BY timestamp DESC, event_id DESC LIMIT 1",
                &[&p.id, &issue, &ts, &event_id],
            )?;
            let next = c.query_opt(
                "SELECT event_id FROM events WHERE project_id = $1 AND issue_id = $2 AND (timestamp > $3 OR (timestamp = $3 AND event_id > $4))
                 ORDER BY timestamp, event_id LIMIT 1",
                &[&p.id, &issue, &ts, &event_id],
            )?;
            (prev.map(|r| event_id_str(r.get(0))), next.map(|r| event_id_str(r.get(0))))
        }
        None => (None, None),
    };
    if let Some(obj) = doc.as_object_mut() {
        obj.insert("event_id".into(), json!(event_id_str(event_id)));
        obj.insert("project_id".into(), json!(p.id.to_string()));
        obj.insert("issue_id".into(), json!(issue.to_string()));
        obj.insert("previous_event_id".into(), json!(prev));
        obj.insert("next_event_id".into(), json!(next));
        // Links to the replay and trace the error was recorded in.
        let replay_id = obj
            .get("contexts")
            .and_then(|c| c.get("replay"))
            .and_then(|r| r.get("replay_id"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                obj.get("tags")?
                    .as_array()?
                    .iter()
                    .find(|t| t[0] == "replayId")
                    .and_then(|t| t[1].as_str().map(str::to_string))
            })
            .and_then(|s| Uuid::try_parse(&s).ok())
            .map(|u| u.simple().to_string());
        let trace_id = obj
            .get("contexts")
            .and_then(|c| c.get("trace"))
            .and_then(|t| t.get("trace_id"))
            .and_then(Value::as_str)
            .map(str::to_string);
        obj.insert("replay_id".into(), json!(replay_id));
        obj.insert("trace_id".into(), json!(trace_id));
    }
    Ok(doc)
}

/// The full normalized event (see the event shape in docs/api.md) plus
/// `previous_event_id`, `next_event_id`, and `replay_id` and `trace_id`
/// (from the replay and trace contexts; `null` when absent).
#[utoipa::path(get, path = "/api/v1/issues/{id}/events/{event}", tag = "issues",
    params(("id" = String, Path), ("event" = String, Path, description = "`latest`, `oldest`, `recommended` or an event id")),
    responses((status = 200, description = "the event document", body = Object), (status = 404, body = crate::error::ErrorBody)))]
async fn issue_event(
    State(state): State<ApiState>,
    auth: Auth,
    Path((id, which)): Path<(i64, String)>,
) -> ApiResult<axum::Json<Value>> {
    let doc = state
        .db(move |c| {
            let p = issue_sync(c, &auth, id, Permission::ReadProject)?;
            let order = match which.as_str() {
                "latest" => Some("DESC"),
                "oldest" => Some("ASC"),
                // The newest event with an exception, else the newest.
                "recommended" => None,
                _ => {
                    return full_event(c, &p, id, parse_event_id(&which)?);
                }
            };
            let row = match order {
                Some(o) => c.query_opt(
                    &format!("SELECT event_id FROM events WHERE project_id = $1 AND issue_id = $2 ORDER BY timestamp {o}, event_id {o} LIMIT 1"),
                    &[&p.id, &id],
                )?,
                None => match c.query_opt(
                    "SELECT event_id FROM events WHERE project_id = $1 AND issue_id = $2 AND exception_type IS NOT NULL
                     ORDER BY timestamp DESC, event_id DESC LIMIT 1",
                    &[&p.id, &id],
                )? {
                    Some(r) => Some(r),
                    None => c.query_opt(
                        "SELECT event_id FROM events WHERE project_id = $1 AND issue_id = $2 ORDER BY timestamp DESC, event_id DESC LIMIT 1",
                        &[&p.id, &id],
                    )?,
                },
            };
            let eid: [u8; 16] = match row {
                Some(r) => r.get(0),
                // Rows not written yet (or removed): fall back to payloads.
                None => c
                    .query_opt("SELECT event_id FROM event_payloads WHERE project_id = $1 AND issue_id = $2 ORDER BY day DESC LIMIT 1", &[&p.id, &id])?
                    .ok_or_else(ApiError::not_found)?
                    .get(0),
            };
            full_event(c, &p, id, eid)
        })
        .await?;
    Ok(axum::Json(doc))
}

/// An event by project and id.
#[utoipa::path(get, path = "/api/v1/orgs/{org}/events/{project}/{event_id}", tag = "issues",
    params(("org" = String, Path), ("project" = String, Path, description = "slug or id"), ("event_id" = String, Path)),
    responses((status = 200, body = Object)))]
async fn org_event(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, project, event_id)): Path<(String, String, String)>,
) -> ApiResult<axum::Json<Value>> {
    let doc = state
        .db(move |c| {
            let p = project_sync(c, &auth, &org, &project, Permission::ReadProject)?;
            full_event(c, &p, 0, parse_event_id(&event_id)?)
        })
        .await?;
    Ok(axum::Json(doc))
}

/// Tag keys answered from first-class event columns.
const TAG_COLUMNS: &[(&str, &str)] = &[
    ("level", "level"),
    ("environment", "environment"),
    ("release", "release"),
    ("dist", "dist"),
    ("browser", "browser"),
    ("os", "os"),
    ("device", "device"),
    ("runtime", "runtime"),
    ("transaction", "transaction"),
    ("server_name", "server_name"),
    ("url", "url"),
    ("user", "coalesce(user_email, user_id, user_ip)"),
];

#[derive(Serialize, ToSchema)]
pub struct TagValue {
    pub value: String,
    pub count: i64,
}

#[derive(Serialize, ToSchema)]
pub struct TagSummary {
    pub key: String,
    /// Events with this tag.
    pub total_values: i64,
    pub unique_values: i64,
    /// The ten most frequent values.
    pub top_values: Vec<TagValue>,
}

/// (key, value, count) for every tag of an issue's events.
pub(crate) fn tag_counts(
    c: &Connection,
    project: i64,
    issue: i64,
    only_key: Option<&str>,
) -> ApiResult<Vec<(String, String, i64)>> {
    let mut parts = Vec::new();
    for (key, col) in TAG_COLUMNS {
        if only_key.is_some_and(|k| k != *key) {
            continue;
        }
        parts.push(format!(
            "SELECT '{key}' AS k, {col} AS v, count(*) AS n FROM events WHERE project_id = $1 AND issue_id = $2 AND {col} IS NOT NULL AND {col} <> '' GROUP BY {col}"
        ));
    }
    let mut out: Vec<(String, String, i64)> = Vec::new();
    if !parts.is_empty() {
        for r in c.query(&parts.join(" UNION ALL "), &[&project, &issue])? {
            out.push((r.get(0), r.get(1), r.get(2)));
        }
    }
    let first_class: Vec<&str> = TAG_COLUMNS.iter().map(|(k, _)| *k).collect();
    if only_key.is_none_or(|k| !first_class.contains(&k)) {
        // Tags are a JSON object; arrays of pairs work as well.
        let rows = c.query(
            "SELECT t.key, t.value, count(*) FROM events, jsonb_each_text(CASE WHEN jsonb_typeof(tags) = 'object' THEN tags ELSE '{}'::jsonb END) AS t(key, value)
             WHERE project_id = $1 AND issue_id = $2 GROUP BY 1, 2
             UNION ALL
             SELECT a.elem ->> 0, a.elem ->> 1, count(*) FROM events, jsonb_array_elements(CASE WHEN jsonb_typeof(tags) = 'array' THEN tags ELSE '[]'::jsonb END) AS a(elem)
             WHERE project_id = $1 AND issue_id = $2 AND jsonb_typeof(a.elem) = 'array' GROUP BY 1, 2",
            &[&project, &issue],
        )?;
        for r in rows {
            let (k, v): (Option<String>, Option<String>) = (r.get(0), r.get(1));
            let (Some(k), Some(v)) = (k, v) else { continue };
            if first_class.contains(&k.as_str()) || only_key.is_some_and(|o| o != k) {
                continue;
            }
            out.push((k, v, r.get(2)));
        }
    }
    Ok(out)
}

/// Tag distributions of an issue's events (first-class attributes and tags).
#[utoipa::path(get, path = "/api/v1/issues/{id}/tags", tag = "issues", params(("id" = String, Path)),
    responses((status = 200, body = Vec<TagSummary>)))]
async fn issue_tags(
    State(state): State<ApiState>,
    auth: Auth,
    Path(id): Path<i64>,
) -> ApiResult<axum::Json<Vec<TagSummary>>> {
    let counts = state
        .db(move |c| {
            let p = issue_sync(c, &auth, id, Permission::ReadProject)?;
            tag_counts(c, p.id, id, None)
        })
        .await?;
    let mut by_key: BTreeMap<String, Vec<(String, i64)>> = BTreeMap::new();
    for (k, v, n) in counts {
        let e = by_key.entry(k).or_default();
        match e.iter_mut().find(|(x, _)| *x == v) {
            Some(slot) => slot.1 += n,
            None => e.push((v, n)),
        }
    }
    let mut out: Vec<TagSummary> = by_key
        .into_iter()
        .map(|(key, mut values)| {
            values.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            TagSummary {
                total_values: values.iter().map(|(_, n)| n).sum(),
                unique_values: values.len() as i64,
                top_values: values.into_iter().take(10).map(|(value, count)| TagValue { value, count }).collect(),
                key,
            }
        })
        .collect();
    // First-class keys first, in their fixed order, then tags by name.
    out.sort_by_key(|t| (TAG_COLUMNS.iter().position(|(k, _)| *k == t.key).unwrap_or(usize::MAX), t.key.clone()));
    Ok(axum::Json(out))
}

/// All values of one tag, most frequent first.
#[utoipa::path(get, path = "/api/v1/issues/{id}/tags/{key}/values", tag = "issues",
    params(("id" = String, Path), ("key" = String, Path), PageParams), responses((status = 200, body = Page<TagValue>)))]
async fn tag_values(
    State(state): State<ApiState>,
    auth: Auth,
    Path((id, key)): Path<(i64, String)>,
    Query(page): Query<PageParams>,
) -> ApiResult<axum::Json<Page<TagValue>>> {
    let offset: i64 = page.cursor()?.unwrap_or(0);
    let limit = page.limit();
    let counts = state
        .db(move |c| {
            let p = issue_sync(c, &auth, id, Permission::ReadProject)?;
            tag_counts(c, p.id, id, Some(&key))
        })
        .await?;
    let mut merged: HashMap<String, i64> = HashMap::new();
    for (_, v, n) in counts {
        *merged.entry(v).or_default() += n;
    }
    let mut values: Vec<TagValue> = merged.into_iter().map(|(value, count)| TagValue { value, count }).collect();
    values.sort_by(|a, b| b.count.cmp(&a.count).then(a.value.cmp(&b.value)));
    let rest: Vec<TagValue> = values.into_iter().skip(offset.max(0) as usize).take(limit as usize + 1).collect();
    let next = offset + limit;
    Ok(axum::Json(Page::from_rows(rest, limit, |_| next)))
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct StatsPeriod {
    /// `24h` (hourly, default), `14d`, `30d`, `90d` (daily).
    pub period: Option<String>,
    pub environment: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct IssueStats {
    pub period: String,
    /// Bucket size in seconds.
    pub resolution: i64,
    /// `[[unix_seconds, events], ...]`.
    pub events: Vec<(i64, i64)>,
    /// `[[unix_seconds, distinct users], ...]`.
    pub users: Vec<(i64, i64)>,
}

/// Events and users of an issue over time.
#[utoipa::path(get, path = "/api/v1/issues/{id}/stats", tag = "issues", params(("id" = String, Path), StatsPeriod),
    responses((status = 200, body = IssueStats)))]
async fn issue_stats(
    State(state): State<ApiState>,
    auth: Auth,
    Path(id): Path<i64>,
    Query(q): Query<StatsPeriod>,
) -> ApiResult<axum::Json<IssueStats>> {
    let period = parse_period("period", q.period.as_deref(), Duration::hours(24))?;
    let (unit, step) = bucket_for(period);
    let env = q.environment.clone();
    let now = Utc::now();
    let since = now - period;
    let rows = state
        .db(move |c| {
            let p = issue_sync(c, &auth, id, Permission::ReadProject)?;
            Ok(c.query(
                &format!(
                    "SELECT date_trunc('{unit}', timestamp), count(*), count(DISTINCT user_hash) FROM events
                     WHERE project_id = $1 AND issue_id = $2 AND timestamp >= $3 AND ($4::text IS NULL OR environment = $4)
                     GROUP BY 1"
                ),
                &[&p.id, &id, &since, &env],
            )?)
        })
        .await?;
    let ev: Vec<(DateTime<Utc>, i64)> = rows.iter().map(|r| (r.get(0), r.get(1))).collect();
    let us: Vec<(DateTime<Utc>, i64)> = rows.iter().map(|r| (r.get(0), r.get(2))).collect();
    let first = since + step;
    Ok(axum::Json(IssueStats {
        period: q.period.unwrap_or_else(|| "24h".into()),
        resolution: step.num_seconds(),
        events: crate::routes::projects::fill_series(first, now, step, &ev),
        users: crate::routes::projects::fill_series(first, now, step, &us),
    }))
}

#[derive(Serialize, ToSchema)]
pub struct Feedback {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub project_id: Id,
    pub event_id: Option<String>,
    #[schema(value_type = Option<String>)]
    pub issue_id: Option<Id>,
    pub name: Option<String>,
    pub email: Option<String>,
    pub message: String,
    pub url: Option<String>,
    pub created_at: DateTime<Utc>,
}

fn feedback_rows(rows: Vec<Row>) -> Vec<Feedback> {
    rows.iter()
        .map(|r| Feedback {
            id: Id(r.get(0)),
            project_id: Id(r.get(1)),
            event_id: r.get::<Option<[u8; 16]>>(2).map(event_id_str),
            issue_id: r.get::<Option<i64>>(3).map(Id),
            name: r.get(4),
            email: r.get(5),
            message: r.get(6),
            url: r.get(7),
            created_at: r.get(8),
        })
        .collect()
}

const FEEDBACK_SELECT: &str =
    "SELECT id, project_id, event_id, issue_id, name, email, message, url, created_at FROM user_feedback";

/// User feedback attached to an issue.
#[utoipa::path(get, path = "/api/v1/issues/{id}/feedback", tag = "issues", params(("id" = String, Path), PageParams),
    responses((status = 200, body = Page<Feedback>)))]
async fn issue_feedback(
    State(state): State<ApiState>,
    auth: Auth,
    Path(id): Path<i64>,
    Query(page): Query<PageParams>,
) -> ApiResult<axum::Json<Page<Feedback>>> {
    let before: Option<i64> = page.cursor()?;
    let limit = page.limit();
    let rows = state
        .db(move |c| {
            let p = issue_sync(c, &auth, id, Permission::ReadProject)?;
            Ok(c.query(
                &format!("{FEEDBACK_SELECT} WHERE project_id = $1 AND issue_id = $2 AND ($3::bigint IS NULL OR id < $3) ORDER BY id DESC LIMIT $4"),
                &[&p.id, &id, &before, &(limit + 1)],
            )?)
        })
        .await?;
    Ok(axum::Json(Page::from_rows(feedback_rows(rows), limit, |f| f.id.0)))
}

/// User feedback of a project.
#[utoipa::path(get, path = "/api/v1/projects/{org}/{project}/feedback", tag = "projects",
    params(("org" = String, Path), ("project" = String, Path), PageParams), responses((status = 200, body = Page<Feedback>)))]
async fn project_feedback(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, project)): Path<(String, String)>,
    Query(page): Query<PageParams>,
) -> ApiResult<axum::Json<Page<Feedback>>> {
    let before: Option<i64> = page.cursor()?;
    let limit = page.limit();
    let rows = state
        .db(move |c| {
            let p = project_sync(c, &auth, &org, &project, Permission::ReadProject)?;
            Ok(c.query(
                &format!("{FEEDBACK_SELECT} WHERE project_id = $1 AND ($2::bigint IS NULL OR id < $2) ORDER BY id DESC LIMIT $3"),
                &[&p.id, &before, &(limit + 1)],
            )?)
        })
        .await?;
    Ok(axum::Json(Page::from_rows(feedback_rows(rows), limit, |f| f.id.0)))
}

#[derive(Serialize, ToSchema)]
pub struct Attachment {
    #[schema(value_type = String)]
    pub id: Id,
    pub event_id: String,
    pub name: String,
    pub content_type: Option<String>,
    pub size: i64,
    pub created_at: DateTime<Utc>,
    /// API path to download the file.
    pub download_url: String,
}

/// Attachments of an issue's events.
#[utoipa::path(get, path = "/api/v1/issues/{id}/attachments", tag = "issues", params(("id" = String, Path), PageParams),
    responses((status = 200, body = Page<Attachment>)))]
async fn issue_attachments(
    State(state): State<ApiState>,
    auth: Auth,
    Path(id): Path<i64>,
    Query(page): Query<PageParams>,
) -> ApiResult<axum::Json<Page<Attachment>>> {
    let before: Option<i64> = page.cursor()?;
    let limit = page.limit();
    let rows = state
        .db(move |c| {
            let p = issue_sync(c, &auth, id, Permission::ReadProject)?;
            Ok(c.query(
                "SELECT a.id, a.event_id, a.name, a.content_type, a.size, a.created_at FROM attachments a
                 JOIN event_payloads e ON e.project_id = a.project_id AND e.event_id = a.event_id
                 WHERE a.project_id = $1 AND e.issue_id = $2 AND ($3::bigint IS NULL OR a.id < $3) ORDER BY a.id DESC LIMIT $4",
                &[&p.id, &id, &before, &(limit + 1)],
            )?)
        })
        .await?;
    let list = rows
        .iter()
        .map(|r| {
            let aid: i64 = r.get(0);
            Attachment {
                id: Id(aid),
                event_id: event_id_str(r.get(1)),
                name: r.get(2),
                content_type: r.get(3),
                size: r.get(4),
                created_at: r.get(5),
                download_url: format!("/api/v1/attachments/{aid}/download"),
            }
        })
        .collect();
    Ok(axum::Json(Page::from_rows(list, limit, |a: &Attachment| a.id.0)))
}

/// Content types served as such; everything else is sent as
/// `application/octet-stream` so a browser never renders uploaded HTML.
const SAFE_TYPES: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp", "text/plain", "application/json"];

/// Downloads an attachment (always as a file download).
#[utoipa::path(get, path = "/api/v1/attachments/{id}/download", tag = "issues", params(("id" = String, Path)),
    responses((status = 200, description = "the file", content_type = "application/octet-stream")))]
async fn download_attachment(State(state): State<ApiState>, auth: Auth, Path(id): Path<i64>) -> ApiResult<Response> {
    let objects = state.store.objects().clone();
    let (name, ctype, data) = state
        .db(move |c| {
            let row = c
                .query_opt("SELECT project_id, name, content_type, object_key FROM attachments WHERE id = $1", &[&id])?
                .ok_or_else(ApiError::not_found)?;
            project_by_id_sync(c, &auth, row.get(0), Permission::ReadProject)?;
            let key: String = row.get(3);
            let data = objects.get(&key).map_err(ApiError::internal)?.ok_or_else(ApiError::not_found)?;
            Ok((row.get::<String>(1), row.get::<Option<String>>(2), data))
        })
        .await?;
    let ctype = ctype.filter(|t| SAFE_TYPES.contains(&t.as_str())).unwrap_or_else(|| "application/octet-stream".into());
    let safe_name: String =
        name.chars().map(|c| if c.is_ascii_alphanumeric() || ".-_".contains(c) { c } else { '_' }).collect();
    let mut resp = Body::from(data).into_response();
    let h = resp.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&ctype).unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    h.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{safe_name}\""))
            .unwrap_or(HeaderValue::from_static("attachment")),
    );
    Ok(resp)
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct EventQueryParams {
    /// Event filter (docs/search.md, event keys).
    pub query: Option<String>,
    /// Comma-separated columns to return; ignored with `group_by`.
    pub fields: Option<String>,
    /// Column to group by; the result has `count` per value.
    pub group_by: Option<String>,
    /// Time range back from now (default 24h, at most 90d).
    pub period: Option<String>,
    /// Project ids, comma-separated (default all).
    pub project: Option<String>,
    pub limit: Option<u32>,
}

/// Columns usable in `fields` and `group_by`.
const QUERY_COLUMNS: &[&str] = &[
    "event_id",
    "issue_id",
    "project_id",
    "timestamp",
    "level",
    "platform",
    "environment",
    "release",
    "dist",
    "transaction",
    "message",
    "exception_type",
    "exception_value",
    "user_id",
    "user_email",
    "os",
    "browser",
    "device",
    "runtime",
    "sdk",
    "server_name",
    "url",
];

/// A minimal event query: filtered rows or counts grouped by one column.
#[utoipa::path(get, path = "/api/v1/orgs/{org}/events", tag = "issues", params(("org" = String, Path), EventQueryParams),
    responses((status = 200, description = "`{\"data\": [{column: value, ...}]}`", body = Object)))]
async fn query_events(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
    Query(q): Query<EventQueryParams>,
) -> ApiResult<axum::Json<Value>> {
    let period = parse_period("period", q.period.as_deref(), Duration::hours(24))?;
    if period > Duration::days(90) {
        return Err(ApiError::invalid("period", "at most 90d"));
    }
    let limit = q.limit.unwrap_or(50).clamp(1, 1000) as i64;
    let group_by = q.group_by.clone().filter(|g| !g.is_empty());
    if let Some(g) = &group_by
        && !QUERY_COLUMNS.contains(&g.as_str())
    {
        return Err(ApiError::invalid("group_by", format!("one of {}", QUERY_COLUMNS.join(", "))));
    }
    let fields: Vec<String> = match &q.fields {
        Some(f) => f.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
        None => ["event_id", "timestamp", "level", "message", "environment", "release"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
    };
    if let Some(bad) = fields.iter().find(|f| !QUERY_COLUMNS.contains(&f.as_str())) {
        return Err(ApiError::invalid("fields", format!("unknown column {bad:?}")));
    }
    let wanted: Vec<i64> = match &q.project {
        Some(p) => p
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| s.trim().parse().map_err(|_| ApiError::invalid("project", "numeric ids")))
            .collect::<ApiResult<_>>()?,
        None => Vec::new(),
    };
    let parsed = query::parse(q.query.as_deref().unwrap_or("")).map_err(|e| ApiError::invalid("query", e.message))?;
    let data = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ReadProject)?;
            let all: Vec<i64> =
                c.query("SELECT id FROM projects WHERE org_id = $1", &[&o.id])?.iter().map(|r| r.get(0)).collect();
            let projects: Vec<i64> =
                if wanted.is_empty() { all } else { wanted.into_iter().filter(|p| all.contains(p)).collect() };
            if projects.is_empty() {
                return Ok(Vec::new());
            }
            let mut params = Params::new();
            let since = params.bind(Utc::now() - period);
            let ctx = CompileContext {
                user_id: auth.user_id(),
                org_id: o.id,
                project_ids: projects.clone(),
                now: Utc::now(),
                first_param: params.next_index(),
            };
            let compiled = query::compile_events(&parsed, &ctx).map_err(|e| ApiError::invalid("query", e.message))?;
            params.extend_query(&compiled.params);
            let filter =
                format!("e.project_id IN ({}) AND e.timestamp >= {since} AND ({})", id_list(&projects), compiled.sql);
            let (sql, cols): (String, Vec<String>) = match &group_by {
                Some(g) => (
                    format!(
                        "SELECT e.{g}, count(*) FROM events e WHERE {filter} GROUP BY 1 ORDER BY 2 DESC LIMIT {limit}"
                    ),
                    vec![g.clone(), "count".into()],
                ),
                None => (
                    format!(
                        "SELECT {} FROM events e WHERE {filter} ORDER BY e.timestamp DESC LIMIT {limit}",
                        fields.iter().map(|f| format!("e.{f}")).collect::<Vec<_>>().join(", ")
                    ),
                    fields.clone(),
                ),
            };
            let rows = c.query(&sql, &params.refs())?;
            Ok(rows
                .iter()
                .map(|r| {
                    let mut obj = serde_json::Map::new();
                    for (i, col) in cols.iter().enumerate() {
                        let v: rupta_store::squad::Value = r.get(i);
                        obj.insert(col.clone(), sql_value_json(col, &v));
                    }
                    Value::Object(obj)
                })
                .collect::<Vec<_>>())
        })
        .await?;
    Ok(axum::Json(json!({ "data": data })))
}

fn sql_value_json(col: &str, v: &rupta_store::squad::Value) -> Value {
    use rupta_store::squad::Value as V;
    match v {
        V::Null => Value::Null,
        V::Int8(n) if col == "count" => json!(n),
        V::Int8(n) => json!(n.to_string()),
        V::Int4(n) => json!(n),
        V::Text(s) => json!(s),
        V::Bool(b) => json!(b),
        V::Uuid(b) => json!(event_id_str(*b)),
        other => {
            if let Ok(t) = <DateTime<Utc> as rupta_store::squad::FromSql>::from_value(other) {
                json!(t)
            } else {
                json!(format!("{other:?}"))
            }
        }
    }
}
