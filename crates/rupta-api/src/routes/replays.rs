//! Session replays: the replay list, replay details, the recording
//! segments (decompressed rrweb events, streamed) and the replays of an
//! issue.

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Duration, Utc};
use rupta_auth::rbac::Permission;
use rupta_store::squad::Row;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::access::{issue_sync, project_sync};
use crate::error::{ApiError, ApiResult};
use crate::extract::{Auth, Page, PageParams, Path, Query, decode_cursor, encode_cursor};
use crate::sql::Params;
use crate::state::ApiState;
use crate::telemetry::{Col, FilterSpec, Scope, compile_filter, hex_id, parse_hex_id};

pub fn routes() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(list_replays))
        .routes(routes!(get_replay))
        .routes(routes!(recording_segments))
        .routes(routes!(issue_replays))
}

#[derive(Serialize, ToSchema)]
pub struct ReplayUser {
    pub id: Option<String>,
    pub email: Option<String>,
    pub name: Option<String>,
    pub ip_address: Option<String>,
}

/// A replay in lists.
#[derive(Serialize, ToSchema)]
pub struct ReplaySummary {
    pub replay_id: String,
    pub project_id: String,
    pub started: DateTime<Utc>,
    pub finished: DateTime<Utc>,
    pub duration_ms: i64,
    pub segment_count: i32,
    pub count_errors: i32,
    pub count_clicks: i32,
    /// Pages visited, in order.
    pub urls: Vec<String>,
    pub user: ReplayUser,
    pub browser: Option<String>,
    pub os: Option<String>,
    pub device: Option<String>,
    pub environment: Option<String>,
    pub release: Option<String>,
    pub platform: Option<String>,
    pub sdk: Option<String>,
    /// `session` or `buffer`.
    pub replay_type: Option<String>,
    /// Error event ids (32 hex digits) recorded during the replay.
    pub error_ids: Vec<String>,
    pub trace_ids: Vec<String>,
}

const REPLAY_COLUMNS: &str = "r.replay_id, r.project_id, r.started, r.finished, r.segment_count, r.count_errors, r.count_clicks,
    r.urls, r.user_id, r.user_email, r.user_name, r.user_ip, r.browser, r.os, r.device, r.environment, r.release, r.platform,
    r.sdk, r.replay_type, r.error_ids, r.trace_ids";

fn strings(v: Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn summary(r: &Row) -> ReplaySummary {
    let (started, finished): (DateTime<Utc>, DateTime<Utc>) = (r.get(2), r.get(3));
    ReplaySummary {
        replay_id: hex_id(r.get(0)),
        project_id: r.get::<i64>(1).to_string(),
        started,
        finished,
        duration_ms: (finished - started).num_milliseconds().max(0),
        segment_count: r.get(4),
        count_errors: r.get(5),
        count_clicks: r.get(6),
        urls: strings(r.get(7)),
        user: ReplayUser { id: r.get(8), email: r.get(9), name: r.get(10), ip_address: r.get(11) },
        browser: r.get(12),
        os: r.get(13),
        device: r.get(14),
        environment: r.get(15),
        release: r.get(16),
        platform: r.get(17),
        sdk: r.get(18),
        replay_type: r.get(19),
        error_ids: strings(r.get(20)),
        trace_ids: strings(r.get(21)),
    }
}

const REPLAY_FILTER: FilterSpec<'static> = FilterSpec {
    keys: &[
        ("url", Col::Contains("r.urls::text")),
        ("user.email", Col::Text("r.user_email")),
        ("user.id", Col::Text("r.user_id")),
        ("user.name", Col::Text("r.user_name")),
        ("user.ip", Col::Text("r.user_ip")),
        ("browser", Col::Contains("r.browser")),
        ("os", Col::Contains("r.os")),
        ("device", Col::Contains("r.device")),
        ("release", Col::Text("r.release")),
        ("platform", Col::Text("r.platform")),
        ("replay_type", Col::Text("r.replay_type")),
        ("count_errors", Col::Number("r.count_errors")),
        ("count_clicks", Col::Number("r.count_clicks")),
        ("count_segments", Col::Number("r.segment_count")),
        ("duration", Col::Number("EXTRACT(EPOCH FROM (r.finished - r.started)) * 1000")),
        ("error_id", Col::Contains("r.error_ids::text")),
        ("trace", Col::Contains("r.trace_ids::text")),
    ],
    text: Some("(r.urls::text || ' ' || coalesce(r.user_email, '') || ' ' || coalesce(r.user_id, ''))"),
    attributes: None,
};

#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ReplayListParams {
    /// Filter: `url:` (substring), `user.email:`, `user.id:`, `browser:`,
    /// `os:`, `device:` (substrings), `release:`, `count_errors:>0`,
    /// `count_clicks:`, `duration:>60000` (ms), `error_id:`, `trace:`;
    /// free text matches URLs and the user.
    pub query: Option<String>,
    /// `-started` (default), `started`, `-duration`, `duration`,
    /// `-count_errors`, `-count_clicks`.
    pub sort: Option<String>,
    pub cursor: Option<String>,
    /// 1 to 100, default 50.
    pub limit: Option<u32>,
}

/// Replays, newest first.
#[utoipa::path(get, path = "/api/v1/orgs/{org}/replays", tag = "replays",
    params(("org" = String, Path), Scope, ReplayListParams), responses((status = 200, body = Page<ReplaySummary>)))]
async fn list_replays(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
    Query(scope): Query<Scope>,
    Query(q): Query<ReplayListParams>,
) -> ApiResult<axum::Json<Page<ReplaySummary>>> {
    let limit = q.limit.unwrap_or(50).clamp(1, 100) as i64;
    let offset: i64 = match &q.cursor {
        Some(c) if !c.is_empty() => decode_cursor(c).ok_or_else(|| ApiError::bad_request("invalid cursor"))?,
        _ => 0,
    };
    let order = match q.sort.as_deref().unwrap_or("-started") {
        "-started" => "r.started DESC",
        "started" => "r.started ASC",
        "-duration" => "(r.finished - r.started) DESC",
        "duration" => "(r.finished - r.started) ASC",
        "-count_errors" => "r.count_errors DESC",
        "-count_clicks" => "r.count_clicks DESC",
        _ => {
            return Err(ApiError::invalid(
                "sort",
                "-started, started, -duration, duration, -count_errors or -count_clicks",
            ));
        }
    };
    let query = q.query.clone().unwrap_or_default();
    let rows = state
        .db(move |c| {
            let s = scope.resolve(c, &auth, &org, Duration::days(7))?;
            if s.projects.is_empty() {
                return Ok(Vec::new());
            }
            let mut params = Params::new();
            let filter = format!("{} AND ({})", s.filter(&mut params, "r", "started"), compile_filter(&query, &REPLAY_FILTER, &mut params)?);
            Ok(c.query(
                &format!(
                    "SELECT {REPLAY_COLUMNS} FROM replays r WHERE {filter} ORDER BY {order}, r.replay_id DESC LIMIT {} OFFSET {offset}",
                    limit + 1
                ),
                &params.refs(),
            )?
            .iter()
            .map(summary)
            .collect::<Vec<_>>())
        })
        .await?;
    let next = offset + limit;
    Ok(axum::Json(Page::from_rows(rows, limit, |_| next)))
}

#[derive(Serialize, ToSchema)]
pub struct ReplayIssue {
    pub id: String,
    pub short_id: String,
    pub title: String,
    pub level: String,
    pub status: String,
    /// Error events of the issue during the replay.
    pub event_ids: Vec<String>,
}

#[derive(Serialize, ToSchema)]
pub struct ReplayDetail {
    #[serde(flatten)]
    pub replay: ReplaySummary,
    /// Issues of errors recorded during the replay.
    pub issues: Vec<ReplayIssue>,
    /// API path of the recording segments.
    pub recording_url: String,
}

/// A replay with the issues of its errors.
#[utoipa::path(get, path = "/api/v1/orgs/{org}/replays/{project}/{replay_id}", tag = "replays",
    params(("org" = String, Path), ("project" = String, Path, description = "slug or id"), ("replay_id" = String, Path)),
    responses((status = 200, body = ReplayDetail), (status = 404, body = crate::error::ErrorBody)))]
async fn get_replay(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, project, replay_id)): Path<(String, String, String)>,
) -> ApiResult<axum::Json<ReplayDetail>> {
    let id = parse_hex_id(&replay_id)?;
    let d = state
        .db(move |c| {
            let p = project_sync(c, &auth, &org, &project, Permission::ReadProject)?;
            let r = c
                .query_opt(
                    &format!("SELECT {REPLAY_COLUMNS} FROM replays r WHERE r.project_id = $1 AND r.replay_id = $2"),
                    &[&p.id, &id],
                )?
                .ok_or_else(ApiError::not_found)?;
            let links = c.query(
                "SELECT l.issue_id, l.event_id, i.short_id, i.title, i.level, i.status FROM issue_replays l
                 JOIN issues i ON i.id = l.issue_id WHERE l.project_id = $1 AND l.replay_id = $2 ORDER BY l.timestamp",
                &[&p.id, &id],
            )?;
            let mut issues: Vec<ReplayIssue> = Vec::new();
            for l in &links {
                let issue_id = l.get::<i64>(0).to_string();
                let event = hex_id(l.get(1));
                match issues.iter_mut().find(|i| i.id == issue_id) {
                    Some(i) => i.event_ids.push(event),
                    None => issues.push(ReplayIssue {
                        id: issue_id,
                        short_id: format!("{}-{}", p.slug.to_uppercase(), l.get::<i64>(2)),
                        title: l.get(3),
                        level: l.get(4),
                        status: l.get(5),
                        event_ids: vec![event],
                    }),
                }
            }
            let replay = summary(&r);
            let recording_url =
                format!("/api/v1/orgs/{}/replays/{}/{}/recording-segments", p.org.slug, p.slug, replay.replay_id);
            Ok(ReplayDetail { replay, issues, recording_url })
        })
        .await?;
    Ok(axum::Json(d))
}

/// Recording segments in order: `{"data": [[rrweb events of segment 0],
/// [segment 1], ...], "next_cursor": ...}`. Each inner array is the
/// decompressed segment exactly as recorded. `limit` counts segments (1 to
/// 100, default 100). The body is streamed.
#[utoipa::path(get, path = "/api/v1/orgs/{org}/replays/{project}/{replay_id}/recording-segments", tag = "replays",
    params(("org" = String, Path), ("project" = String, Path, description = "slug or id"), ("replay_id" = String, Path), PageParams),
    responses((status = 200, description = "`{\"data\": [[event, ...], ...], \"next_cursor\": string|null}`", body = Object)))]
async fn recording_segments(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, project, replay_id)): Path<(String, String, String)>,
    Query(page): Query<PageParams>,
) -> ApiResult<Response> {
    let id = parse_hex_id(&replay_id)?;
    let after: Option<i32> = page.cursor()?;
    let limit = page.limit.unwrap_or(100).clamp(1, 100) as i64;
    let segments: Vec<(i32, String)> = state
        .db(move |c| {
            let p = project_sync(c, &auth, &org, &project, Permission::ReadProject)?;
            if c.query_opt("SELECT 1 FROM replays WHERE project_id = $1 AND replay_id = $2", &[&p.id, &id])?.is_none() {
                return Err(ApiError::not_found());
            }
            Ok(c.query(
                "SELECT segment_id, object_key FROM replay_segments WHERE project_id = $1 AND replay_id = $2
                 AND ($3::int IS NULL OR segment_id > $3) ORDER BY segment_id LIMIT $4",
                &[&p.id, &id, &after, &(limit + 1)],
            )?
            .iter()
            .map(|r| (r.get(0), r.get(1)))
            .collect())
        })
        .await?;
    let more = segments.len() as i64 > limit;
    let segments: Vec<(i32, String)> = segments.into_iter().take(limit as usize).collect();
    let next_cursor = if more { segments.last().map(|(s, _)| encode_cursor(s)) } else { None };
    let objects = state.store.objects().clone();
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<bytes::Bytes, std::io::Error>>(4);
    tokio::task::spawn_blocking(move || {
        let send = |b: Vec<u8>| tx.blocking_send(Ok(bytes::Bytes::from(b))).is_ok();
        if !send(b"{\"data\":[".to_vec()) {
            return;
        }
        for (i, (_, key)) in segments.iter().enumerate() {
            let events = match objects.get(key) {
                Ok(Some(data)) => zstd::decode_all(data.as_slice()).unwrap_or_else(|_| b"[]".to_vec()),
                // A segment whose object is gone (retention running): empty.
                _ => b"[]".to_vec(),
            };
            let mut chunk = if i > 0 { b",".to_vec() } else { Vec::new() };
            chunk.extend(events);
            if !send(chunk) {
                return;
            }
        }
        let tail =
            format!("],\"next_cursor\":{}}}", serde_json::to_string(&next_cursor).unwrap_or_else(|_| "null".into()));
        let _ = send(tail.into_bytes());
    });
    let stream = futures_util::stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|item| (item, rx)) });
    let mut resp = Body::from_stream(stream).into_response();
    resp.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
    Ok(resp)
}

/// Replays in which errors of this issue were recorded, newest first.
#[utoipa::path(get, path = "/api/v1/issues/{id}/replays", tag = "replays",
    params(("id" = String, Path), PageParams), responses((status = 200, body = Page<ReplaySummary>)))]
async fn issue_replays(
    State(state): State<ApiState>,
    auth: Auth,
    Path(id): Path<i64>,
    Query(page): Query<PageParams>,
) -> ApiResult<axum::Json<Page<ReplaySummary>>> {
    let offset: i64 = page.cursor()?.unwrap_or(0);
    let limit = page.limit();
    let rows = state
        .db(move |c| {
            let p = issue_sync(c, &auth, id, Permission::ReadProject)?;
            let ids: Vec<[u8; 16]> = c
                .query(
                    "SELECT DISTINCT replay_id FROM issue_replays WHERE project_id = $1 AND issue_id = $2",
                    &[&p.id, &id],
                )?
                .iter()
                .map(|r| r.get(0))
                .collect();
            if ids.is_empty() {
                return Ok(Vec::new());
            }
            Ok(c.query(
                &format!(
                    "SELECT {REPLAY_COLUMNS} FROM replays r WHERE r.project_id = $1 AND r.replay_id = ANY($2)
                     ORDER BY r.started DESC, r.replay_id DESC LIMIT $3 OFFSET $4"
                ),
                &[&p.id, &ids, &(limit + 1), &offset],
            )?
            .iter()
            .map(summary)
            .collect::<Vec<_>>())
        })
        .await?;
    let next = offset + limit;
    Ok(axum::Json(Page::from_rows(rows, limit, |_| next)))
}
