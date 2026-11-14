//! Performance: transaction statistics, transaction summaries, traces and
//! the span explorer. Aggregations run on the columnar `transactions` and
//! `spans` tables; percentiles are `approx_percentile` estimates.

use std::collections::HashMap;

use axum::extract::State;
use chrono::{DateTime, Duration, Utc};
use rupta_auth::rbac::Permission;
use rupta_store::squad::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::access::org_sync;
use crate::error::{ApiError, ApiResult};
use crate::extract::{Auth, Path, Query, decode_cursor, encode_cursor};
use crate::sql::{Params, id_list};
use crate::state::ApiState;
use crate::telemetry::{Col, FilterSpec, Resolved, Scope, buckets, compile_filter, hex_id, interval_for};

pub fn routes() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(transactions))
        .routes(routes!(transaction_summary))
        .routes(routes!(trace))
        .routes(routes!(spans))
}

/// Apdex threshold T in milliseconds: satisfied ≤ T, tolerating ≤ 4T.
pub const APDEX_THRESHOLD_MS: f64 = 300.0;
/// Span statuses that do not count as failures.
const OK_STATUSES: &str = "'ok', 'cancelled', 'unknown'";

const TRANSACTION_FILTER: FilterSpec<'static> = FilterSpec {
    keys: &[
        ("transaction", Col::Text("name")),
        ("name", Col::Text("name")),
        ("op", Col::Text("op")),
        ("status", Col::Text("status")),
        ("release", Col::Text("release")),
        ("platform", Col::Text("platform")),
        ("sdk", Col::Text("sdk")),
        ("trace", Col::Text("trace_id")),
        ("duration", Col::Number("duration_ms")),
    ],
    text: Some("name"),
    attributes: Some("tags"),
};

#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct TransactionListParams {
    /// Filter: free text matches the name; keys `op`, `status`, `release`,
    /// `platform`, `sdk`, `trace`, `duration` (ms, comparisons), any other
    /// key matches a tag.
    pub query: Option<String>,
    /// `tpm` (default), `count`, `p50`, `p95`, `p99`, `failure_rate`,
    /// `apdex`, `name`; prefix `-` for ascending (`-p95`).
    pub sort: Option<String>,
    pub cursor: Option<String>,
    /// 1 to 100, default 50.
    pub limit: Option<u32>,
}

/// One transaction name with its statistics over the range.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TransactionStats {
    pub name: String,
    pub op: Option<String>,
    pub count: i64,
    /// Transactions per minute over the range.
    pub tpm: f64,
    /// Durations in milliseconds.
    pub p50: Option<f64>,
    pub p75: Option<f64>,
    pub p95: Option<f64>,
    pub p99: Option<f64>,
    pub avg: Option<f64>,
    /// Share (0 to 1) of transactions whose status is not `ok`,
    /// `cancelled` or `unknown`.
    pub failure_rate: f64,
    /// Apdex with T = 300 ms.
    pub apdex: f64,
    /// Distinct users (estimate).
    pub users: i64,
}

#[derive(Serialize, ToSchema)]
pub struct TransactionStatsPage {
    pub data: Vec<TransactionStats>,
    pub next_cursor: Option<String>,
}

fn opt_f64(r: &rupta_store::squad::Row, i: usize) -> Option<f64> {
    r.get::<Option<f64>>(i).filter(|f| f.is_finite())
}

/// Counts per (name, op) with an extra condition.
fn counts_by_name(
    c: &Connection,
    base: &str,
    params: &Params,
    extra: &str,
) -> ApiResult<HashMap<(String, Option<String>), i64>> {
    Ok(c.query(
        &format!("SELECT name, op, count(*) FROM transactions WHERE {base} AND {extra} GROUP BY name, op"),
        &params.refs(),
    )?
    .iter()
    .filter_map(|r| Some(((r.get::<Option<String>>(0)?, r.get::<Option<String>>(1)), r.get::<i64>(2))))
    .collect())
}

/// Statistics per transaction name (and op) over the scope.
fn stats(c: &Connection, s: &Resolved, query: &str, name: Option<&str>) -> ApiResult<Vec<TransactionStats>> {
    if s.projects.is_empty() {
        return Ok(Vec::new());
    }
    let mut params = Params::new();
    let mut base = s.filter(&mut params, "", "timestamp");
    base.push_str(&format!(" AND name IS NOT NULL AND ({})", compile_filter(query, &TRANSACTION_FILTER, &mut params)?));
    if let Some(n) = name {
        base.push_str(&format!(" AND name = {}", params.bind(n.to_string())));
    }
    let rows = c.query(
        &format!(
            "SELECT name, op, count(*), approx_percentile(duration_ms, 0.5), approx_percentile(duration_ms, 0.75),
                 approx_percentile(duration_ms, 0.95), approx_percentile(duration_ms, 0.99), avg(duration_ms),
                 approx_count_distinct(user_hash)
             FROM transactions WHERE {base} GROUP BY name, op"
        ),
        &params.refs(),
    )?;
    let failed = counts_by_name(c, &base, &params, &format!("status IS NOT NULL AND status NOT IN ({OK_STATUSES})"))?;
    let satisfied = counts_by_name(c, &base, &params, &format!("duration_ms <= {APDEX_THRESHOLD_MS}"))?;
    let tolerating = counts_by_name(
        c,
        &base,
        &params,
        &format!("duration_ms > {APDEX_THRESHOLD_MS} AND duration_ms <= {}", APDEX_THRESHOLD_MS * 4.0),
    )?;
    let minutes = s.minutes();
    Ok(rows
        .iter()
        .filter_map(|r| {
            let key = (r.get::<Option<String>>(0)?, r.get::<Option<String>>(1));
            let count: i64 = r.get(2);
            let n = count.max(1) as f64;
            let get = |m: &HashMap<(String, Option<String>), i64>| m.get(&key).copied().unwrap_or(0) as f64;
            Some(TransactionStats {
                failure_rate: get(&failed) / n,
                apdex: (get(&satisfied) + get(&tolerating) / 2.0) / n,
                name: key.0.clone(),
                op: key.1.clone(),
                count,
                tpm: count as f64 / minutes,
                p50: opt_f64(r, 3),
                p75: opt_f64(r, 4),
                p95: opt_f64(r, 5),
                p99: opt_f64(r, 6),
                avg: opt_f64(r, 7),
                users: r.get(8),
            })
        })
        .collect())
}

fn sort_stats(rows: &mut [TransactionStats], sort: &str) -> ApiResult<()> {
    let (asc, key) = match sort.strip_prefix('-') {
        Some(k) => (true, k),
        None => (false, sort),
    };
    let num = |t: &TransactionStats| -> f64 {
        match key {
            "tpm" => t.tpm,
            "count" => t.count as f64,
            "p50" => t.p50.unwrap_or(0.0),
            "p75" => t.p75.unwrap_or(0.0),
            "p95" => t.p95.unwrap_or(0.0),
            "p99" => t.p99.unwrap_or(0.0),
            "avg" => t.avg.unwrap_or(0.0),
            "failure_rate" => t.failure_rate,
            "apdex" => t.apdex,
            "users" => t.users as f64,
            _ => 0.0,
        }
    };
    if !matches!(
        key,
        "tpm" | "count" | "p50" | "p75" | "p95" | "p99" | "avg" | "failure_rate" | "apdex" | "users" | "name"
    ) {
        return Err(ApiError::invalid(
            "sort",
            "tpm, count, p50, p75, p95, p99, avg, failure_rate, apdex, users or name",
        ));
    }
    rows.sort_by(|a, b| {
        let o = if key == "name" { a.name.cmp(&b.name) } else { num(b).total_cmp(&num(a)) };
        let o = if asc ^ (key == "name") { o.reverse() } else { o };
        o.then_with(|| a.name.cmp(&b.name))
    });
    Ok(())
}

/// Transactions grouped by name: throughput, latency percentiles,
/// failure rate and Apdex.
#[utoipa::path(get, path = "/api/v1/orgs/{org}/performance/transactions", tag = "performance",
    params(("org" = String, Path), Scope, TransactionListParams),
    responses((status = 200, body = TransactionStatsPage)))]
async fn transactions(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
    Query(scope): Query<Scope>,
    Query(q): Query<TransactionListParams>,
) -> ApiResult<axum::Json<TransactionStatsPage>> {
    let limit = q.limit.unwrap_or(50).clamp(1, 100) as usize;
    let offset: usize = match &q.cursor {
        Some(c) if !c.is_empty() => decode_cursor(c).ok_or_else(|| ApiError::bad_request("invalid cursor"))?,
        _ => 0,
    };
    let sort = q.sort.clone().unwrap_or_else(|| "tpm".into());
    let query = q.query.clone().unwrap_or_default();
    let mut rows = state
        .db(move |c| {
            let s = scope.resolve(c, &auth, &org, Duration::hours(24))?;
            stats(c, &s, &query, None)
        })
        .await?;
    sort_stats(&mut rows, &sort)?;
    let total = rows.len();
    let data: Vec<TransactionStats> = rows.into_iter().skip(offset).take(limit).collect();
    let next_cursor = (offset + limit < total).then(|| encode_cursor(&(offset + limit)));
    Ok(axum::Json(TransactionStatsPage { data, next_cursor }))
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SummaryParams {
    /// The transaction name.
    pub name: String,
    /// `1m`, `1h` or `1d` (default: chosen from the range).
    pub interval: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct SeriesPoint {
    /// Bucket start, Unix seconds.
    pub ts: i64,
    pub count: i64,
    pub p50: Option<f64>,
    pub p95: Option<f64>,
}

#[derive(Serialize, ToSchema)]
pub struct HistogramBucket {
    /// Lower bound in milliseconds (inclusive).
    pub start: f64,
    /// Upper bound in milliseconds (exclusive; the last bucket also holds
    /// everything slower).
    pub end: f64,
    pub count: i64,
}

#[derive(Serialize, ToSchema)]
pub struct SampleTransaction {
    pub event_id: String,
    pub project_id: String,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub duration_ms: Option<f64>,
    pub status: Option<String>,
    pub release: Option<String>,
    pub environment: Option<String>,
    pub profile_id: Option<String>,
    pub profiler_id: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct RelatedIssue {
    pub id: String,
    pub short_id: String,
    pub title: String,
    pub level: String,
    pub status: String,
    /// Events of the issue in this transaction within the range.
    pub count: i64,
    pub last_seen: DateTime<Utc>,
}

#[derive(Serialize, ToSchema, Default)]
pub struct WebVital {
    /// 75th percentile (milliseconds; unitless for CLS).
    pub p75: Option<f64>,
    /// Measurements.
    pub count: i64,
}

#[derive(Serialize, ToSchema, Default)]
pub struct WebVitals {
    pub lcp: WebVital,
    pub fcp: WebVital,
    pub cls: WebVital,
    pub inp: WebVital,
    pub ttfb: WebVital,
}

#[derive(Serialize, ToSchema)]
pub struct TransactionSummary {
    pub name: String,
    /// Overall statistics; `null` if there is no transaction in the range.
    pub stats: Option<TransactionStats>,
    /// Bucket size in seconds.
    pub interval: i64,
    pub series: Vec<SeriesPoint>,
    /// 20 buckets from 0 to the 99th percentile.
    pub histogram: Vec<HistogramBucket>,
    /// The ten slowest transactions.
    pub slowest: Vec<SampleTransaction>,
    /// Issues with events in this transaction, most events first.
    pub related_issues: Vec<RelatedIssue>,
    /// Web vitals (browser transactions).
    pub web_vitals: WebVitals,
}

/// One transaction name: statistics, time series, duration histogram,
/// slowest samples, related issues and web vitals.
#[utoipa::path(get, path = "/api/v1/orgs/{org}/performance/transactions/summary", tag = "performance",
    params(("org" = String, Path), Scope, SummaryParams),
    responses((status = 200, body = TransactionSummary)))]
async fn transaction_summary(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
    Query(scope): Query<Scope>,
    Query(q): Query<SummaryParams>,
) -> ApiResult<axum::Json<TransactionSummary>> {
    let summary = state
        .db(move |c| {
            let s = scope.resolve(c, &auth, &org, Duration::hours(24))?;
            summarize(c, &s, &q)
        })
        .await?;
    Ok(axum::Json(summary))
}

fn summarize(c: &Connection, s: &Resolved, q: &SummaryParams) -> ApiResult<TransactionSummary> {
    let (unit, step) = interval_for(s.start, s.end, q.interval.as_deref())?;
    let mut out = TransactionSummary {
        name: q.name.clone(),
        stats: None,
        interval: step.num_seconds(),
        series: Vec::new(),
        histogram: Vec::new(),
        slowest: Vec::new(),
        related_issues: Vec::new(),
        web_vitals: WebVitals::default(),
    };
    if s.projects.is_empty() {
        return Ok(out);
    }
    let all = stats(c, s, "", Some(&q.name))?;
    // Several ops under one name: merge into one row (counts summed,
    // percentiles of the most frequent op).
    out.stats = all.iter().max_by_key(|t| t.count).cloned().map(|mut top| {
        let total: i64 = all.iter().map(|t| t.count).sum();
        if total > 0 {
            top.failure_rate = all.iter().map(|t| t.failure_rate * t.count as f64).sum::<f64>() / total as f64;
            top.apdex = all.iter().map(|t| t.apdex * t.count as f64).sum::<f64>() / total as f64;
            top.tpm = all.iter().map(|t| t.tpm).sum();
            top.count = total;
        }
        top
    });
    let mut params = Params::new();
    let base = format!("{} AND name = {}", s.filter(&mut params, "", "timestamp"), params.bind(q.name.clone()));
    let rows = c.query(
        &format!(
            "SELECT date_trunc('{unit}', timestamp), count(*), approx_percentile(duration_ms, 0.5), approx_percentile(duration_ms, 0.95)
             FROM transactions WHERE {base} GROUP BY 1"
        ),
        &params.refs(),
    )?;
    let by_ts: HashMap<i64, (i64, Option<f64>, Option<f64>)> = rows
        .iter()
        .map(|r| (r.get::<DateTime<Utc>>(0).timestamp(), (r.get(1), opt_f64(r, 2), opt_f64(r, 3))))
        .collect();
    out.series = buckets(s.start, s.end, step)
        .into_iter()
        .map(|ts| {
            let (count, p50, p95) = by_ts.get(&ts).copied().unwrap_or((0, None, None));
            SeriesPoint { ts, count, p50, p95 }
        })
        .collect();
    if let Some(p99) = out.stats.as_ref().and_then(|t| t.p99).filter(|p| *p > 0.0) {
        let width = p99 / 20.0;
        let rows = c.query(
            &format!(
                "SELECT least(floor(duration_ms / {width}), 19), count(*) FROM transactions WHERE {base} AND duration_ms IS NOT NULL GROUP BY 1"
            ),
            &params.refs(),
        )?;
        let counts: HashMap<i64, i64> = rows.iter().map(|r| (r.get::<f64>(0) as i64, r.get::<i64>(1))).collect();
        out.histogram = (0..20)
            .map(|i| HistogramBucket {
                start: i as f64 * width,
                end: (i + 1) as f64 * width,
                count: counts.get(&i).copied().unwrap_or(0),
            })
            .collect();
    }
    out.slowest = c
        .query(
            &format!(
                "SELECT event_id, project_id, trace_id, span_id, timestamp, duration_ms, status, release, environment, profile_id, profiler_id
                 FROM transactions WHERE {base} AND duration_ms IS NOT NULL ORDER BY duration_ms DESC LIMIT 10"
            ),
            &params.refs(),
        )?
        .iter()
        .map(|r| SampleTransaction {
            event_id: hex_id(r.get(0)),
            project_id: r.get::<i64>(1).to_string(),
            trace_id: r.get(2),
            span_id: r.get(3),
            timestamp: r.get(4),
            duration_ms: opt_f64(r, 5),
            status: r.get(6),
            release: r.get(7),
            environment: r.get(8),
            profile_id: r.get(9),
            profiler_id: r.get(10),
        })
        .collect();
    // Issues of error events in this transaction.
    let mut ep = Params::new();
    let ebase = format!("{} AND transaction = {}", s.filter(&mut ep, "", "timestamp"), ep.bind(q.name.clone()));
    let counts: Vec<(i64, i64)> = c
        .query(
            &format!("SELECT issue_id, count(*) FROM events WHERE {ebase} GROUP BY issue_id ORDER BY 2 DESC LIMIT 10"),
            &ep.refs(),
        )?
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
    if !counts.is_empty() {
        let ids: Vec<i64> = counts.iter().map(|(i, _)| *i).collect();
        let issues = c.query(
            &format!(
                "SELECT i.id, p.slug, i.short_id, i.title, i.level, i.status, i.last_seen FROM issues i JOIN projects p ON p.id = i.project_id WHERE i.id IN ({})",
                id_list(&ids)
            ),
            &[],
        )?;
        for (id, n) in counts {
            if let Some(r) = issues.iter().find(|r| r.get::<i64>(0) == id) {
                out.related_issues.push(RelatedIssue {
                    id: id.to_string(),
                    short_id: format!("{}-{}", r.get::<String>(1).to_uppercase(), r.get::<i64>(2)),
                    title: r.get(3),
                    level: r.get(4),
                    status: r.get(5),
                    count: n,
                    last_seen: r.get(6),
                });
            }
        }
    }
    // Web vitals from spans of this transaction (segment spans carry the
    // measurements of classic transactions; streamed vitals are child spans).
    let mut vp = Params::new();
    let vbase = format!("{} AND transaction = {}", s.filter(&mut vp, "", "timestamp"), vp.bind(q.name.clone()));
    let r = c.query_one(
        &format!(
            "SELECT approx_percentile(lcp, 0.75), count(lcp), approx_percentile(fcp, 0.75), count(fcp),
                 approx_percentile(cls, 0.75), count(cls), approx_percentile(inp, 0.75), count(inp),
                 approx_percentile(ttfb, 0.75), count(ttfb)
             FROM spans WHERE {vbase}"
        ),
        &vp.refs(),
    )?;
    let v = |i: usize| WebVital { p75: opt_f64(&r, i), count: r.get(i + 1) };
    out.web_vitals = WebVitals { lcp: v(0), fcp: v(2), cls: v(4), inp: v(6), ttfb: v(8) };
    Ok(out)
}

#[derive(Serialize, ToSchema, Clone)]
pub struct TraceTransaction {
    pub event_id: String,
    pub project_id: String,
    pub project_slug: String,
    pub name: Option<String>,
    pub op: Option<String>,
    pub status: Option<String>,
    pub span_id: Option<String>,
    pub parent_span_id: Option<String>,
    pub start_timestamp: Option<DateTime<Utc>>,
    pub timestamp: DateTime<Utc>,
    pub duration_ms: Option<f64>,
    pub environment: Option<String>,
    pub release: Option<String>,
    pub platform: Option<String>,
    pub profile_id: Option<String>,
    pub profiler_id: Option<String>,
}

/// A span with its children (the trace waterfall).
#[derive(Serialize, ToSchema, Clone)]
pub struct SpanNode {
    pub span_id: String,
    pub parent_span_id: Option<String>,
    /// Span id of the transaction (segment) this span belongs to.
    pub segment_id: Option<String>,
    pub is_segment: bool,
    pub project_id: String,
    pub transaction: Option<String>,
    pub op: Option<String>,
    pub description: Option<String>,
    pub status: Option<String>,
    pub start_timestamp: DateTime<Utc>,
    pub end_timestamp: DateTime<Utc>,
    pub duration_ms: Option<f64>,
    #[schema(value_type = Object)]
    pub data: Value,
    #[schema(value_type = Object)]
    pub tags: Value,
    /// Child spans, by start time.
    #[schema(no_recursion)]
    pub children: Vec<SpanNode>,
}

#[derive(Serialize, ToSchema)]
pub struct TraceError {
    pub event_id: String,
    pub issue_id: String,
    pub project_id: String,
    pub title: String,
    pub level: Option<String>,
    /// The span that was active when the error happened.
    pub span_id: Option<String>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Serialize, ToSchema)]
pub struct Trace {
    pub trace_id: String,
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
    pub duration_ms: Option<f64>,
    pub transactions: Vec<TraceTransaction>,
    /// Root spans of the trace (spans whose parent is not in the trace),
    /// each with its descendants.
    pub spans: Vec<SpanNode>,
    pub span_count: usize,
    /// True if the trace had more than 10,000 spans and only those were loaded.
    pub truncated: bool,
    pub errors: Vec<TraceError>,
    pub logs_count: i64,
}

const MAX_TRACE_SPANS: i64 = 10_000;
const MAX_TREE_DEPTH: usize = 256;

fn build_tree(mut flat: Vec<SpanNode>) -> Vec<SpanNode> {
    flat.sort_by(|a, b| a.start_timestamp.cmp(&b.start_timestamp).then_with(|| a.span_id.cmp(&b.span_id)));
    // Duplicate span ids (a retried envelope): keep the first.
    let mut seen = std::collections::HashSet::new();
    flat.retain(|s| seen.insert(s.span_id.clone()));
    let index: HashMap<String, usize> = flat.iter().enumerate().map(|(i, s)| (s.span_id.clone(), i)).collect();
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); flat.len()];
    let mut roots = Vec::new();
    for (i, s) in flat.iter().enumerate() {
        match s.parent_span_id.as_ref().and_then(|p| index.get(p)) {
            Some(&p) if p != i => children[p].push(i),
            _ => roots.push(i),
        }
    }
    fn take(i: usize, depth: usize, flat: &[SpanNode], children: &[Vec<usize>], used: &mut [bool]) -> Option<SpanNode> {
        if used[i] {
            return None;
        }
        used[i] = true;
        let mut node = flat[i].clone();
        if depth < MAX_TREE_DEPTH {
            node.children = children[i].iter().filter_map(|c| take(*c, depth + 1, flat, children, used)).collect();
        }
        Some(node)
    }
    let mut used = vec![false; flat.len()];
    let mut out: Vec<SpanNode> = roots.iter().filter_map(|r| take(*r, 0, &flat, &children, &mut used)).collect();
    // Cycles or spans below the depth limit: attach as roots.
    for i in 0..flat.len() {
        if !used[i] {
            used[i] = true;
            out.push(flat[i].clone());
        }
    }
    out
}

/// A trace: its transactions, the span tree, errors and the number of logs.
#[utoipa::path(get, path = "/api/v1/orgs/{org}/traces/{trace_id}", tag = "performance",
    params(("org" = String, Path), ("trace_id" = String, Path, description = "32 hex digits")),
    responses((status = 200, body = Trace), (status = 404, body = crate::error::ErrorBody)))]
async fn trace(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, trace_id)): Path<(String, String)>,
) -> ApiResult<axum::Json<Trace>> {
    let trace_id = trace_id.trim().to_ascii_lowercase().replace('-', "");
    if trace_id.len() != 32 || !trace_id.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ApiError::not_found());
    }
    let t = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ReadProject)?;
            let projects: Vec<(i64, String)> = c
                .query("SELECT id, slug FROM projects WHERE org_id = $1", &[&o.id])?
                .iter()
                .map(|r| (r.get(0), r.get(1)))
                .collect();
            if projects.is_empty() {
                return Err(ApiError::not_found());
            }
            let ids: Vec<i64> = projects.iter().map(|(i, _)| *i).collect();
            let plist = id_list(&ids);
            let slug = |id: i64| projects.iter().find(|(i, _)| *i == id).map(|(_, s)| s.clone()).unwrap_or_default();
            let transactions: Vec<TraceTransaction> = c
                .query(
                    &format!(
                        "SELECT event_id, project_id, name, op, status, span_id, parent_span_id, start_timestamp, timestamp,
                             duration_ms, environment, release, platform, profile_id, profiler_id
                         FROM transactions WHERE project_id IN ({plist}) AND trace_id = $1 ORDER BY start_timestamp LIMIT 1000"
                    ),
                    &[&trace_id],
                )?
                .iter()
                .map(|r| TraceTransaction {
                    event_id: hex_id(r.get(0)),
                    project_id: r.get::<i64>(1).to_string(),
                    project_slug: slug(r.get(1)),
                    name: r.get(2),
                    op: r.get(3),
                    status: r.get(4),
                    span_id: r.get(5),
                    parent_span_id: r.get(6),
                    start_timestamp: r.get(7),
                    timestamp: r.get(8),
                    duration_ms: opt_f64(r, 9),
                    environment: r.get(10),
                    release: r.get(11),
                    platform: r.get(12),
                    profile_id: r.get(13),
                    profiler_id: r.get(14),
                })
                .collect();
            let rows = c.query(
                &format!(
                    "SELECT span_id, parent_span_id, segment_id, is_segment, project_id, transaction, op, description, status,
                         timestamp, duration_ms, data, tags
                     FROM spans WHERE project_id IN ({plist}) AND trace_id = $1 ORDER BY timestamp LIMIT {}",
                    MAX_TRACE_SPANS + 1
                ),
                &[&trace_id],
            )?;
            let truncated = rows.len() as i64 > MAX_TRACE_SPANS;
            let flat: Vec<SpanNode> = rows
                .iter()
                .take(MAX_TRACE_SPANS as usize)
                .map(|r| {
                    let start: DateTime<Utc> = r.get(9);
                    let d = opt_f64(r, 10);
                    SpanNode {
                        span_id: r.get(0),
                        parent_span_id: r.get(1),
                        segment_id: r.get(2),
                        is_segment: r.get(3),
                        project_id: r.get::<i64>(4).to_string(),
                        transaction: r.get(5),
                        op: r.get(6),
                        description: r.get(7),
                        status: r.get(8),
                        start_timestamp: start,
                        end_timestamp: start + Duration::microseconds((d.unwrap_or(0.0) * 1000.0) as i64),
                        duration_ms: d,
                        data: r.get::<Option<Value>>(11).unwrap_or(Value::Null),
                        tags: r.get::<Option<Value>>(12).unwrap_or(Value::Null),
                        children: Vec::new(),
                    }
                })
                .collect();
            let errors: Vec<TraceError> = c
                .query(
                    &format!(
                        "SELECT event_id, issue_id, project_id, exception_type, exception_value, message, level, span_id, timestamp
                         FROM events WHERE project_id IN ({plist}) AND trace_id = $1 ORDER BY timestamp LIMIT 1000"
                    ),
                    &[&trace_id],
                )?
                .iter()
                .map(|r| {
                    let (ty, val, msg): (Option<String>, Option<String>, Option<String>) = (r.get(3), r.get(4), r.get(5));
                    let title = match (ty, val) {
                        (Some(t), Some(v)) => format!("{t}: {v}"),
                        (Some(t), None) => t,
                        (None, Some(v)) => v,
                        _ => msg.unwrap_or_else(|| "<unlabeled event>".into()),
                    };
                    TraceError {
                        event_id: hex_id(r.get(0)),
                        issue_id: r.get::<i64>(1).to_string(),
                        project_id: r.get::<i64>(2).to_string(),
                        title,
                        level: r.get(6),
                        span_id: r.get(7),
                        timestamp: r.get(8),
                    }
                })
                .collect();
            let logs_count: i64 = c
                .query_one(&format!("SELECT count(*) FROM logs WHERE project_id IN ({plist}) AND trace_id = $1"), &[&trace_id])?
                .get(0);
            if transactions.is_empty() && flat.is_empty() && errors.is_empty() && logs_count == 0 {
                return Err(ApiError::not_found());
            }
            let start = flat.iter().map(|s| s.start_timestamp).chain(transactions.iter().filter_map(|t| t.start_timestamp)).min();
            let end = flat.iter().map(|s| s.end_timestamp).chain(transactions.iter().map(|t| t.timestamp)).max();
            let span_count = flat.len();
            Ok(Trace {
                duration_ms: start.zip(end).map(|(s, e)| (e - s).num_microseconds().unwrap_or(0) as f64 / 1000.0),
                trace_id,
                start,
                end,
                transactions,
                spans: build_tree(flat),
                span_count,
                truncated,
                errors,
                logs_count,
            })
        })
        .await?;
    Ok(axum::Json(t))
}

const SPAN_FILTER: FilterSpec<'static> = FilterSpec {
    keys: &[
        ("op", Col::Text("op")),
        ("description", Col::Contains("description")),
        ("transaction", Col::Text("transaction")),
        ("status", Col::Text("status")),
        ("release", Col::Text("release")),
        ("trace", Col::Text("trace_id")),
        ("platform", Col::Text("platform")),
        ("duration", Col::Number("duration_ms")),
    ],
    text: Some("description"),
    attributes: Some("data"),
};

#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SpanQueryParams {
    /// Filter: free text matches the description; keys `op`,
    /// `description` (substring), `transaction`, `status`, `release`,
    /// `trace`, `platform`, `duration` (ms); other keys match span data.
    pub query: Option<String>,
    /// `op` (default), `description`, `transaction` or `none` (span rows).
    pub group_by: Option<String>,
    /// `sum` (default), `count`, `p50`, `p95`, `avg` (groups);
    /// rows are sorted newest first.
    pub sort: Option<String>,
    /// 1 to 100, default 50.
    pub limit: Option<u32>,
}

#[derive(Serialize, ToSchema)]
pub struct SpanGroup {
    /// The value of the grouping column (`null` when unset).
    pub group: Option<String>,
    pub count: i64,
    pub p50: Option<f64>,
    pub p95: Option<f64>,
    pub avg: Option<f64>,
    /// Total time spent in these spans, milliseconds.
    pub sum_duration_ms: f64,
}

#[derive(Serialize, ToSchema)]
pub struct SpanRowOut {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub project_id: String,
    pub transaction: Option<String>,
    pub op: Option<String>,
    pub description: Option<String>,
    pub status: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub duration_ms: Option<f64>,
}

#[derive(Serialize, ToSchema)]
pub struct SpanQueryResult {
    /// `op`, `description`, `transaction` or `none`.
    pub group_by: String,
    /// Groups (when grouped).
    pub groups: Vec<SpanGroup>,
    /// Span rows, newest first (`group_by=none`).
    pub spans: Vec<SpanRowOut>,
}

/// The span explorer: spans grouped by operation, description or
/// transaction with counts and latency, or the matching spans themselves.
#[utoipa::path(get, path = "/api/v1/orgs/{org}/spans", tag = "performance",
    params(("org" = String, Path), Scope, SpanQueryParams),
    responses((status = 200, body = SpanQueryResult)))]
async fn spans(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
    Query(scope): Query<Scope>,
    Query(q): Query<SpanQueryParams>,
) -> ApiResult<axum::Json<SpanQueryResult>> {
    let group_by = q.group_by.clone().unwrap_or_else(|| "op".into());
    if !matches!(group_by.as_str(), "op" | "description" | "transaction" | "none") {
        return Err(ApiError::invalid("group_by", "op, description, transaction or none"));
    }
    let sort = q.sort.clone().unwrap_or_else(|| "sum".into());
    if !matches!(sort.as_str(), "sum" | "count" | "p50" | "p95" | "avg") {
        return Err(ApiError::invalid("sort", "sum, count, p50, p95 or avg"));
    }
    let limit = q.limit.unwrap_or(50).clamp(1, 100) as i64;
    let query = q.query.clone().unwrap_or_default();
    let out = state
        .db(move |c| {
            let s = scope.resolve(c, &auth, &org, Duration::hours(24))?;
            let mut out = SpanQueryResult { group_by: group_by.clone(), groups: Vec::new(), spans: Vec::new() };
            if s.projects.is_empty() {
                return Ok(out);
            }
            let mut params = Params::new();
            let base = format!("{} AND ({})", s.filter(&mut params, "", "timestamp"), compile_filter(&query, &SPAN_FILTER, &mut params)?);
            if group_by == "none" {
                out.spans = c
                    .query(
                        &format!(
                            "SELECT trace_id, span_id, parent_span_id, project_id, transaction, op, description, status, timestamp, duration_ms
                             FROM spans WHERE {base} ORDER BY timestamp DESC LIMIT {limit}"
                        ),
                        &params.refs(),
                    )?
                    .iter()
                    .map(|r| SpanRowOut {
                        trace_id: r.get(0),
                        span_id: r.get(1),
                        parent_span_id: r.get(2),
                        project_id: r.get::<i64>(3).to_string(),
                        transaction: r.get(4),
                        op: r.get(5),
                        description: r.get(6),
                        status: r.get(7),
                        timestamp: r.get(8),
                        duration_ms: opt_f64(r, 9),
                    })
                    .collect();
                return Ok(out);
            }
            let rows = c.query(
                &format!(
                    "SELECT {group_by}, count(*), approx_percentile(duration_ms, 0.5), approx_percentile(duration_ms, 0.95),
                         avg(duration_ms), sum(duration_ms)
                     FROM spans WHERE {base} GROUP BY {group_by}"
                ),
                &params.refs(),
            )?;
            let mut groups: Vec<SpanGroup> = rows
                .iter()
                .map(|r| SpanGroup {
                    group: r.get(0),
                    count: r.get(1),
                    p50: opt_f64(r, 2),
                    p95: opt_f64(r, 3),
                    avg: opt_f64(r, 4),
                    sum_duration_ms: opt_f64(r, 5).unwrap_or(0.0),
                })
                .collect();
            let key = |g: &SpanGroup| match sort.as_str() {
                "count" => g.count as f64,
                "p50" => g.p50.unwrap_or(0.0),
                "p95" => g.p95.unwrap_or(0.0),
                "avg" => g.avg.unwrap_or(0.0),
                _ => g.sum_duration_ms,
            };
            groups.sort_by(|a, b| key(b).total_cmp(&key(a)).then_with(|| a.group.cmp(&b.group)));
            groups.truncate(limit as usize);
            out.groups = groups;
            Ok(out)
        })
        .await?;
    Ok(axum::Json(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, parent: Option<&str>, start_ms: i64) -> SpanNode {
        let t = DateTime::from_timestamp_millis(1_794_650_000_000 + start_ms).unwrap();
        SpanNode {
            span_id: id.into(),
            parent_span_id: parent.map(Into::into),
            segment_id: None,
            is_segment: parent.is_none(),
            project_id: "1".into(),
            transaction: None,
            op: None,
            description: None,
            status: None,
            start_timestamp: t,
            end_timestamp: t,
            duration_ms: None,
            data: Value::Null,
            tags: Value::Null,
            children: Vec::new(),
        }
    }

    #[test]
    fn span_trees() {
        let tree = build_tree(vec![
            node("c2", Some("b"), 30),
            node("a", None, 0),
            node("b", Some("a"), 10),
            node("c1", Some("b"), 20),
            node("orphan", Some("missing"), 5),
            node("a", None, 0),
        ]);
        let ids: Vec<&str> = tree.iter().map(|n| n.span_id.as_str()).collect();
        assert_eq!(ids, vec!["a", "orphan"]);
        let b = &tree[0].children[0];
        assert_eq!(b.span_id, "b");
        assert_eq!(b.children.iter().map(|c| c.span_id.as_str()).collect::<Vec<_>>(), vec!["c1", "c2"]);
        // A cycle does not loop forever.
        let cyc = build_tree(vec![node("x", Some("y"), 0), node("y", Some("x"), 1)]);
        assert_eq!(cyc.len(), 2);
    }
}
