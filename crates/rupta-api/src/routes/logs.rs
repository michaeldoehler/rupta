//! Logs: search, a level histogram and a live tail as Server-Sent Events.

use std::collections::{HashMap, HashSet, VecDeque};
use std::convert::Infallible;
use std::time::Duration as StdDuration;

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use chrono::{DateTime, Duration, Utc};
use rupta_store::squad::{Connection, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::error::{ApiError, ApiResult};
use crate::extract::{Auth, Path, Query, decode_cursor, encode_cursor};
use crate::sql::Params;
use crate::state::ApiState;
use crate::telemetry::{Col, FilterSpec, Resolved, Scope, buckets, compile_filter, hex_id, interval_for};

pub fn routes() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new().routes(routes!(list_logs)).routes(routes!(histogram)).routes(routes!(stream))
}

/// Level names, most severe last.
pub const LEVELS: [&str; 6] = ["trace", "debug", "info", "warn", "error", "fatal"];

const LOG_FILTER: FilterSpec<'static> = FilterSpec {
    keys: &[
        ("level", Col::Text("level")),
        ("trace", Col::Text("trace_id")),
        ("trace_id", Col::Text("trace_id")),
        ("span", Col::Text("span_id")),
        ("span_id", Col::Text("span_id")),
        ("release", Col::Text("release")),
        ("sdk", Col::Text("sdk")),
        ("severity", Col::Number("severity_number")),
        ("message", Col::Contains("body")),
        ("body", Col::Contains("body")),
    ],
    text: Some("body"),
    attributes: Some("attributes"),
};

/// One log record.
#[derive(Debug, Serialize, ToSchema)]
pub struct LogRecord {
    pub id: String,
    pub project_id: String,
    pub timestamp: DateTime<Utc>,
    /// When rupta received it.
    pub received: DateTime<Utc>,
    /// trace, debug, info, warn, error or fatal.
    pub level: String,
    pub severity_number: Option<i32>,
    pub body: String,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub environment: Option<String>,
    pub release: Option<String>,
    pub sdk: Option<String>,
    /// Attributes as plain JSON values (`{"key": value}`).
    #[schema(value_type = Object)]
    pub attributes: Value,
}

const LOG_COLUMNS: &str = "id, project_id, timestamp, received, level, severity_number, body, trace_id, span_id, environment, release, sdk, attributes";

fn record(r: &Row) -> LogRecord {
    LogRecord {
        id: hex_id(r.get(0)),
        project_id: r.get::<i64>(1).to_string(),
        timestamp: r.get(2),
        received: r.get(3),
        level: r.get(4),
        severity_number: r.get(5),
        body: r.get(6),
        trace_id: r.get(7),
        span_id: r.get(8),
        environment: r.get(9),
        release: r.get(10),
        sdk: r.get(11),
        attributes: r.get::<Option<Value>>(12).unwrap_or_else(|| Value::Object(Default::default())),
    }
}

#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct LogQueryParams {
    /// Filter: free text matches the body (substring); `level:error`,
    /// `level:[warn,error]`, `trace:<id>`, `span:<id>`, `release:`,
    /// `sdk:`, `severity:>=17`, `body:`/`message:` (substring); any other
    /// `key:value` matches an attribute. `!` negates a term.
    pub query: Option<String>,
    /// `-timestamp` (newest first, default) or `timestamp`.
    pub sort: Option<String>,
    pub cursor: Option<String>,
    /// 1 to 1000, default 100.
    pub limit: Option<u32>,
}

#[derive(Serialize, ToSchema)]
pub struct LogPage {
    pub data: Vec<LogRecord>,
    pub next_cursor: Option<String>,
}

/// Windows (minutes) a page of logs is looked for in before the whole range.
const PAGE_WINDOWS: [i64; 5] = [5, 60, 6 * 60, 24 * 60, 7 * 24 * 60];

fn base_filter(s: &Resolved, query: &str, params: &mut Params) -> ApiResult<String> {
    Ok(format!("{} AND ({})", s.filter(params, "", "timestamp"), compile_filter(query, &LOG_FILTER, params)?))
}

/// Log records, newest first.
#[utoipa::path(get, path = "/api/v1/orgs/{org}/logs", tag = "logs",
    params(("org" = String, Path), Scope, LogQueryParams),
    responses((status = 200, body = LogPage)))]
async fn list_logs(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
    Query(scope): Query<Scope>,
    Query(q): Query<LogQueryParams>,
) -> ApiResult<axum::Json<LogPage>> {
    let limit = q.limit.unwrap_or(100).clamp(1, 1000) as i64;
    let asc = match q.sort.as_deref() {
        None | Some("-timestamp") => false,
        Some("timestamp") => true,
        Some(_) => return Err(ApiError::invalid("sort", "-timestamp or timestamp")),
    };
    let cursor: Option<(i64, String)> = match &q.cursor {
        Some(c) if !c.is_empty() => Some(decode_cursor(c).ok_or_else(|| ApiError::bad_request("invalid cursor"))?),
        _ => None,
    };
    let query = q.query.clone().unwrap_or_default();
    let rows = state
        .db(move |c| {
            let s = scope.resolve(c, &auth, &org, Duration::hours(24))?;
            if s.projects.is_empty() {
                return Ok(Vec::new());
            }
            let cursor = match &cursor {
                Some((micros, id)) => Some((
                    DateTime::from_timestamp_micros(*micros).ok_or_else(|| ApiError::bad_request("invalid cursor"))?,
                    *uuid::Uuid::try_parse(id).map_err(|_| ApiError::bad_request("invalid cursor"))?.as_bytes(),
                )),
                None => None,
            };
            // Sorting the whole range is slow on large tables; look at a
            // growing window next to the cursor (or the range end) until it
            // holds a full page.
            let (lo, hi) = match (asc, cursor) {
                (false, Some((t, _))) => (s.start, t.min(s.end) + Duration::microseconds(1)),
                (true, Some((t, _))) => (t.max(s.start), s.end),
                _ => (s.start, s.end),
            };
            let mut rows = Vec::new();
            for width in PAGE_WINDOWS.iter().map(|m| Duration::minutes(*m)).chain(std::iter::once(hi - lo)) {
                let mut w = s.clone();
                (w.start, w.end) = if asc { (lo, (lo + width).min(hi)) } else { ((hi - width).max(lo), hi) };
                let mut params = Params::new();
                let mut sql = format!("SELECT {LOG_COLUMNS} FROM logs WHERE {}", base_filter(&w, &query, &mut params)?);
                if let Some((ts, id)) = cursor {
                    let (t, i) = (params.bind(ts), params.bind(id));
                    let cmp = if asc { ">" } else { "<" };
                    sql.push_str(&format!(" AND (timestamp {cmp} {t} OR (timestamp = {t} AND id {cmp} {i}))"));
                }
                let dir = if asc { "ASC" } else { "DESC" };
                sql.push_str(&format!(" ORDER BY timestamp {dir}, id {dir} LIMIT {}", limit + 1));
                rows = c.query(&sql, &params.refs())?;
                if rows.len() as i64 > limit || width >= hi - lo {
                    break;
                }
            }
            Ok(rows.iter().map(record).collect::<Vec<_>>())
        })
        .await?;
    let mut data = rows;
    let next_cursor = if data.len() as i64 > limit {
        data.truncate(limit as usize);
        data.last().map(|l| encode_cursor(&(l.timestamp.timestamp_micros(), l.id.clone())))
    } else {
        None
    };
    Ok(axum::Json(LogPage { data, next_cursor }))
}

#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct HistogramParams {
    /// Same filter as the log list.
    pub query: Option<String>,
    /// `1m`, `1h` or `1d` (default: chosen from the range).
    pub interval: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct LogBucket {
    /// Bucket start, Unix seconds.
    pub ts: i64,
    pub total: i64,
    /// Records per level (only levels that occur).
    pub counts: HashMap<String, i64>,
}

#[derive(Serialize, ToSchema)]
pub struct LogHistogram {
    /// Bucket size in seconds.
    pub interval: i64,
    pub buckets: Vec<LogBucket>,
    /// Records per level over the whole range.
    pub totals: HashMap<String, i64>,
}

/// Log counts per interval and level.
#[utoipa::path(get, path = "/api/v1/orgs/{org}/logs/histogram", tag = "logs",
    params(("org" = String, Path), Scope, HistogramParams),
    responses((status = 200, body = LogHistogram)))]
async fn histogram(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
    Query(scope): Query<Scope>,
    Query(q): Query<HistogramParams>,
) -> ApiResult<axum::Json<LogHistogram>> {
    let query = q.query.clone().unwrap_or_default();
    let out = state
        .db(move |c| {
            let s = scope.resolve(c, &auth, &org, Duration::hours(24))?;
            let (unit, step) = interval_for(s.start, s.end, q.interval.as_deref())?;
            let mut by_ts: HashMap<i64, HashMap<String, i64>> = HashMap::new();
            if !s.projects.is_empty() {
                let mut params = Params::new();
                let base = base_filter(&s, &query, &mut params)?;
                for r in c.query(
                    &format!(
                        "SELECT date_trunc('{unit}', timestamp), level, count(*) FROM logs WHERE {base} GROUP BY 1, 2"
                    ),
                    &params.refs(),
                )? {
                    by_ts.entry(r.get::<DateTime<Utc>>(0).timestamp()).or_default().insert(r.get(1), r.get(2));
                }
            }
            let mut totals: HashMap<String, i64> = HashMap::new();
            let buckets = buckets(s.start, s.end, step)
                .into_iter()
                .map(|ts| {
                    let counts = by_ts.remove(&ts).unwrap_or_default();
                    for (l, n) in &counts {
                        *totals.entry(l.clone()).or_default() += n;
                    }
                    LogBucket { ts, total: counts.values().sum(), counts }
                })
                .collect();
            Ok(LogHistogram { interval: step.num_seconds(), buckets, totals })
        })
        .await?;
    Ok(axum::Json(out))
}

/// How often the live tail polls for new records.
pub const TAIL_POLL: StdDuration = StdDuration::from_secs(1);
/// Records received up to this long before the newest one seen are
/// looked at again, so records committed out of order are not missed.
pub const TAIL_OVERLAP_SECS: i64 = 10;
/// Most records sent per poll; more are skipped (a `lagging` event says how many).
pub const TAIL_MAX_PER_POLL: i64 = 500;
/// The live tail shows records whose timestamp is at most this much
/// older than the time they were received.
pub const TAIL_MAX_AGE: Duration = Duration::hours(1);
/// A live tail ends after this long; clients reconnect.
pub const TAIL_MAX_DURATION: StdDuration = StdDuration::from_secs(30 * 60);

#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct StreamParams {
    /// Same filter as the log list.
    pub query: Option<String>,
}

struct Tail {
    state: ApiState,
    auth: Auth,
    org: String,
    scope: Scope,
    query: String,
    /// Received time of the newest record sent.
    since: DateTime<Utc>,
    seen: HashSet<[u8; 16]>,
    order: VecDeque<([u8; 16], DateTime<Utc>)>,
    started: std::time::Instant,
    first: bool,
}

fn poll_tail(c: &Connection, t: &Tail) -> ApiResult<(Vec<Row>, i64)> {
    let mut s: Resolved = t.scope.resolve(c, &t.auth, &t.org, Duration::days(366))?;
    if s.projects.is_empty() {
        return Ok((Vec::new(), 0));
    }
    // Select by received time; records whose own timestamp is more than
    // TAIL_MAX_AGE older than that are not shown (this keeps the scan small).
    s.start = t.since - Duration::seconds(TAIL_OVERLAP_SECS) - TAIL_MAX_AGE;
    s.end = Utc::now() + Duration::hours(1);
    let mut params = Params::new();
    let base = base_filter(&s, &t.query, &mut params)?;
    let from = params.bind(t.since - Duration::seconds(TAIL_OVERLAP_SECS));
    let rows = c.query(
        &format!(
            "SELECT {LOG_COLUMNS} FROM logs WHERE {base} AND received >= {from} ORDER BY received, timestamp LIMIT {}",
            TAIL_MAX_PER_POLL * 4
        ),
        &params.refs(),
    )?;
    let fresh: Vec<Row> = rows.into_iter().filter(|r| !t.seen.contains(&r.get::<[u8; 16]>(0))).collect();
    let skipped = (fresh.len() as i64 - TAIL_MAX_PER_POLL).max(0);
    Ok((fresh.into_iter().skip(skipped as usize).collect(), skipped))
}

/// Live tail: new log records as Server-Sent Events. Each `logs` event
/// carries a JSON array of records (oldest first); a `lagging` event
/// `{"skipped": n}` reports records skipped because more than 500 arrived
/// within a second. The stream polls once per second and ends after 30
/// minutes; reconnect to continue. Only records received after the
/// stream started are sent.
#[utoipa::path(get, path = "/api/v1/orgs/{org}/logs/stream", tag = "logs",
    params(("org" = String, Path), Scope, StreamParams),
    responses((status = 200, description = "`text/event-stream` with `logs` events (data: JSON array of log records)", content_type = "text/event-stream")))]
async fn stream(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
    Query(scope): Query<Scope>,
    Query(q): Query<StreamParams>,
) -> ApiResult<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>> {
    // Validate access and the filter before the stream starts.
    let (a, o, sc, query) = (auth.clone(), org.clone(), scope.clone(), q.query.clone().unwrap_or_default());
    state
        .db(move |c| {
            let s = sc.resolve(c, &a, &o, Duration::days(366))?;
            compile_filter(&query, &LOG_FILTER, &mut Params::new())?;
            Ok(s.projects.len())
        })
        .await?;
    let tail = Tail {
        state,
        auth,
        org,
        scope,
        query: q.query.unwrap_or_default(),
        since: Utc::now(),
        seen: HashSet::new(),
        order: VecDeque::new(),
        started: std::time::Instant::now(),
        first: true,
    };
    // Fused like every streamed body: layers may poll once more after the end.
    let s = futures_util::stream::unfold(tail, |mut t| async move {
        if t.first {
            t.first = false;
            return Some((Ok(Event::default().event("ready").data("{}")), t));
        }
        loop {
            if t.started.elapsed() > TAIL_MAX_DURATION {
                return None;
            }
            tokio::time::sleep(TAIL_POLL).await;
            let polled = {
                let (auth, org, scope, query) = (t.auth.clone(), t.org.clone(), t.scope.clone(), t.query.clone());
                let (since, seen) = (t.since, t.seen.clone());
                let probe = Tail {
                    state: t.state.clone(),
                    auth,
                    org,
                    scope,
                    query,
                    since,
                    seen,
                    order: VecDeque::new(),
                    started: t.started,
                    first: false,
                };
                t.state.db(move |c| poll_tail(c, &probe)).await
            };
            let (rows, skipped) = match polled {
                Ok(v) => v,
                Err(e) => {
                    let msg = serde_json::json!({"code": e.code, "message": e.message}).to_string();
                    return Some((Ok(Event::default().event("error").data(msg)), t));
                }
            };
            let horizon = t.since - Duration::seconds(TAIL_OVERLAP_SECS * 2);
            while t.order.front().is_some_and(|(_, r)| *r < horizon) {
                if let Some((id, _)) = t.order.pop_front() {
                    t.seen.remove(&id);
                }
            }
            if skipped > 0 {
                return Some((Ok(Event::default().event("lagging").data(format!("{{\"skipped\":{skipped}}}"))), t));
            }
            if rows.is_empty() {
                continue;
            }
            let mut out = Vec::with_capacity(rows.len());
            for r in &rows {
                let id: [u8; 16] = r.get(0);
                let received: DateTime<Utc> = r.get(3);
                t.since = t.since.max(received);
                t.seen.insert(id);
                t.order.push_back((id, received));
                out.push(record(r));
            }
            let data = serde_json::to_string(&out).unwrap_or_else(|_| "[]".into());
            return Some((Ok(Event::default().event("logs").data(data)), t));
        }
    });
    Ok(Sse::new(futures_util::StreamExt::fuse(s)).keep_alive(KeepAlive::new().interval(StdDuration::from_secs(15))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_are_ordered() {
        assert_eq!(LEVELS.iter().position(|l| *l == "error"), Some(4));
    }
}
