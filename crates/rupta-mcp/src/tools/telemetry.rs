//! Performance (transactions, traces, spans), logs and metrics.

use rmcp::model::Tool;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Effect, Output, args, limit, opt_list, seg, tool};
use crate::format::trace_text;
use crate::rest::{Rest, ToolError, ToolResult};

/// Projects, environment and time range, shared by every telemetry tool.
#[derive(Deserialize, JsonSchema, Default)]
struct Scope {
    /// Project slugs or ids (one, comma-separated, or a list); default all projects of the organization.
    #[serde(default, deserialize_with = "opt_list")]
    #[schemars(with = "Option<Vec<String>>")]
    projects: Option<Vec<String>>,
    environment: Option<String>,
    /// Time range back from now, e.g. "15m", "1h", "24h", "7d" (default 24h, at most 366d). Ignored with start.
    period: Option<String>,
    /// Start of the range, RFC 3339 (e.g. "2026-11-14T08:00:00Z").
    start: Option<String>,
    /// End of the range, RFC 3339 (default now).
    end: Option<String>,
}

impl Scope {
    fn params(&self) -> Vec<(&'static str, Option<String>)> {
        vec![
            ("project", self.projects.as_ref().map(|p| p.join(","))),
            ("environment", self.environment.clone()),
            ("period", self.period.clone()),
            ("start", self.start.clone()),
            ("end", self.end.clone()),
        ]
    }
}

#[derive(Deserialize, JsonSchema, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum TxSort {
    /// Transactions per minute (default).
    Tpm,
    Count,
    P50,
    P95,
    P99,
    FailureRate,
    Apdex,
    Name,
}

impl TxSort {
    fn as_str(self) -> &'static str {
        match self {
            TxSort::Tpm => "tpm",
            TxSort::Count => "count",
            TxSort::P50 => "p50",
            TxSort::P95 => "p95",
            TxSort::P99 => "p99",
            TxSort::FailureRate => "failure_rate",
            TxSort::Apdex => "apdex",
            TxSort::Name => "name",
        }
    }
}

#[derive(Deserialize, JsonSchema)]
struct SearchTransactions {
    /// Organization slug, as returned by list_organizations.
    org: String,
    #[serde(flatten)]
    scope: Scope,
    /// Filter: free text matches the transaction name; keys `op:`, `status:`, `release:`, `platform:`, `sdk:`,
    /// `trace:`, `duration:>500` (ms); any other key matches a tag. E.g. `op:http.server duration:>1000`.
    query: Option<String>,
    /// Sort order, largest first (default tpm).
    sort: Option<TxSort>,
    /// Sort ascending instead.
    ascending: Option<bool>,
    /// Page size, 1 to 100 (default 25).
    limit: Option<u32>,
    /// The next_cursor of the previous page.
    cursor: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct TransactionSummary {
    /// Organization slug, as returned by list_organizations.
    org: String,
    /// The transaction name, e.g. "GET /api/orders" (from search_transactions).
    name: String,
    #[serde(flatten)]
    scope: Scope,
}

#[derive(Deserialize, JsonSchema)]
struct GetTrace {
    /// Organization slug, as returned by list_organizations.
    org: String,
    /// The trace id, 32 hex digits (from an event's trace, a transaction, a log or a span).
    trace_id: String,
    /// Spans to show in the tree (default 200, at most 2000).
    max_spans: Option<usize>,
}

#[derive(Deserialize, JsonSchema, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum SpanGroupBy {
    /// Group by operation (default), e.g. db, http.client.
    Op,
    /// Group by description, e.g. the SQL statement.
    Description,
    Transaction,
    /// No grouping: individual spans, newest first.
    None,
}

#[derive(Deserialize, JsonSchema, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum SpanSort {
    /// Total time (default).
    Sum,
    Count,
    P50,
    P95,
    Avg,
}

#[derive(Deserialize, JsonSchema)]
struct SearchSpans {
    /// Organization slug, as returned by list_organizations.
    org: String,
    #[serde(flatten)]
    scope: Scope,
    /// Filter: free text matches the description; keys `op:`, `description:` (substring), `transaction:`,
    /// `status:`, `release:`, `trace:`, `platform:`, `duration:>100` (ms); other keys match span data.
    query: Option<String>,
    group_by: Option<SpanGroupBy>,
    /// Sort of the groups (default sum); spans without grouping are newest first.
    sort: Option<SpanSort>,
    /// Groups or spans, 1 to 100 (default 25).
    limit: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
struct SearchLogs {
    /// Organization slug, as returned by list_organizations.
    org: String,
    #[serde(flatten)]
    scope: Scope,
    /// Filter: free text matches the body; `level:error`, `level:[warn,error]`, `trace:<id>`, `span:<id>`,
    /// `release:`, `sdk:`, `severity:>=17`, `body:` (substring); any other `key:value` matches an attribute;
    /// `!` negates a term.
    query: Option<String>,
    /// Oldest first instead of newest first.
    oldest_first: Option<bool>,
    /// Page size, 1 to 200 (default 50).
    limit: Option<u32>,
    /// The next_cursor of the previous page.
    cursor: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct ListMetrics {
    /// Organization slug, as returned by list_organizations.
    org: String,
    #[serde(flatten)]
    scope: Scope,
}

#[derive(Deserialize, JsonSchema, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum MetricOp {
    Sum,
    Avg,
    Min,
    Max,
    Count,
    CountUnique,
    P50,
    P75,
    P95,
    P99,
}

#[derive(Deserialize, JsonSchema, Clone, Copy)]
enum Interval {
    #[serde(rename = "1m")]
    Minute,
    #[serde(rename = "1h")]
    Hour,
    #[serde(rename = "1d")]
    Day,
}

#[derive(Deserialize, JsonSchema)]
struct QueryMetric {
    /// Organization slug, as returned by list_organizations.
    org: String,
    /// Metric name (from list_metrics).
    name: String,
    /// Aggregation. Default: sum for counters, avg for gauges, p95 for distributions, count_unique for sets.
    op: Option<MetricOp>,
    /// One series per value of this attribute (or `environment`, `release`); the 10 largest.
    group_by: Option<String>,
    /// Bucket size (default: chosen from the range).
    interval: Option<Interval>,
    /// Filter: `key:value` on attributes, `release:`, `trace:`, `value:>n`.
    query: Option<String>,
    #[serde(flatten)]
    scope: Scope,
}

pub(super) fn definitions() -> Vec<Tool> {
    vec![
        tool::<SearchTransactions>(
            "search_transactions",
            "Search transactions",
            "Performance of transactions (requests, page loads, jobs) grouped by name: count, throughput (tpm), p50/p75/\
             p95/p99 and average duration in ms, failure rate, Apdex and users, over a time range (default 24h).",
            Effect::Read,
        ),
        tool::<TransactionSummary>(
            "get_transaction_summary",
            "Get transaction summary",
            "One transaction name in detail: statistics, a time series of count and latency, the ten slowest samples \
             (with trace ids), the issues with errors in it, and web vitals for browser transactions.",
            Effect::Read,
        ),
        tool::<GetTrace>(
            "get_trace",
            "Get trace",
            "A distributed trace as a span tree (offset, duration, op, description, status), with its transactions, the \
             errors that happened in it and the number of logs. Get trace ids from events (trace_id), \
             search_transactions/get_transaction_summary samples, spans or logs.",
            Effect::Read,
        ),
        tool::<SearchSpans>(
            "search_spans",
            "Search spans",
            "Spans across traces, grouped by op, description or transaction with count, p50, p95, average and total \
             duration; or individual spans (group_by none). Finds slow database queries or HTTP calls.",
            Effect::Read,
        ),
        tool::<SearchLogs>(
            "search_logs",
            "Search logs",
            "Structured logs sent by the SDKs, newest first: time, level, body, trace and span ids, release, \
             environment and attributes, over a time range (default 24h). Filter by level, text, trace or attribute.",
            Effect::Read,
        ),
        tool::<ListMetrics>(
            "list_metrics",
            "List metrics",
            "The metric names seen in a time range (default 7 days) with type (counter, gauge, distribution, set), unit, \
             last seen and number of data points. Query one with query_metric.",
            Effect::Read,
        ),
        tool::<QueryMetric>(
            "query_metric",
            "Query metric",
            "A metric aggregated over time (sum, avg, min, max, count, count_unique, percentiles), optionally one series \
             per attribute value, over a time range (default 24h).",
            Effect::Read,
        ),
    ]
}

pub(super) async fn call(rest: &Rest, name: &str, a: &Value) -> Option<ToolResult<Output>> {
    Some(match name {
        "search_transactions" => transactions(rest, a).await,
        "get_transaction_summary" => summary(rest, a).await,
        "get_trace" => trace(rest, a).await,
        "search_spans" => spans(rest, a).await,
        "search_logs" => logs(rest, a).await,
        "list_metrics" => metric_names(rest, a).await,
        "query_metric" => metric(rest, a).await,
        _ => return None,
    })
}

async fn transactions(rest: &Rest, a: &Value) -> ToolResult<Output> {
    let a: SearchTransactions = args(a)?;
    let sort = a.sort.map(|s| format!("{}{}", if a.ascending == Some(true) { "-" } else { "" }, s.as_str()));
    let mut p = a.scope.params();
    p.extend([("query", a.query), ("sort", sort), ("limit", limit(a.limit, 25, 100)), ("cursor", a.cursor)]);
    let v = rest.get(&format!("/api/v1/orgs/{}/performance/transactions", seg(&a.org)), &p).await?;
    Ok(Output::Json(v))
}

async fn summary(rest: &Rest, a: &Value) -> ToolResult<Output> {
    let a: TransactionSummary = args(a)?;
    let mut p = a.scope.params();
    p.push(("name", Some(a.name)));
    let mut v = rest.get(&format!("/api/v1/orgs/{}/performance/transactions/summary", seg(&a.org)), &p).await?;
    if let Some(o) = v.as_object_mut() {
        o.remove("histogram");
    }
    Ok(Output::Json(v))
}

async fn trace(rest: &Rest, a: &Value) -> ToolResult<Output> {
    let a: GetTrace = args(a)?;
    let id = a.trace_id.trim().to_ascii_lowercase().replace('-', "");
    if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ToolError::new("trace_id: 32 hex digits"));
    }
    let v = rest.get(&format!("/api/v1/orgs/{}/traces/{id}", seg(&a.org)), &[]).await?;
    let text = trace_text(&v, a.max_spans.unwrap_or(200).clamp(1, 2000));
    let structured = json!({
        "trace_id": v["trace_id"], "start": v["start"], "end": v["end"], "duration_ms": v["duration_ms"],
        "span_count": v["span_count"], "logs_count": v["logs_count"], "errors": v["errors"],
        "transactions": v["transactions"],
    });
    Ok(Output::Text(text, Some(structured)))
}

async fn spans(rest: &Rest, a: &Value) -> ToolResult<Output> {
    let a: SearchSpans = args(a)?;
    let group_by = a.group_by.map(|g| {
        match g {
            SpanGroupBy::Op => "op",
            SpanGroupBy::Description => "description",
            SpanGroupBy::Transaction => "transaction",
            SpanGroupBy::None => "none",
        }
        .to_string()
    });
    let sort = a.sort.map(|s| {
        match s {
            SpanSort::Sum => "sum",
            SpanSort::Count => "count",
            SpanSort::P50 => "p50",
            SpanSort::P95 => "p95",
            SpanSort::Avg => "avg",
        }
        .to_string()
    });
    let mut p = a.scope.params();
    p.extend([("query", a.query), ("group_by", group_by), ("sort", sort), ("limit", limit(a.limit, 25, 100))]);
    let v = rest.get(&format!("/api/v1/orgs/{}/spans", seg(&a.org)), &p).await?;
    Ok(Output::Json(v))
}

async fn logs(rest: &Rest, a: &Value) -> ToolResult<Output> {
    let a: SearchLogs = args(a)?;
    let sort = (a.oldest_first == Some(true)).then(|| "timestamp".to_string());
    let mut p = a.scope.params();
    p.extend([("query", a.query), ("sort", sort), ("limit", limit(a.limit, 50, 200)), ("cursor", a.cursor)]);
    let v = rest.get(&format!("/api/v1/orgs/{}/logs", seg(&a.org)), &p).await?;
    Ok(Output::Json(super::page(&v, |l| {
        let mut l = l.clone();
        if let Some(o) = l.as_object_mut() {
            o.remove("received");
            if o.get("attributes").and_then(Value::as_object).is_some_and(|a| a.is_empty()) {
                o.remove("attributes");
            }
        }
        l
    })))
}

async fn metric_names(rest: &Rest, a: &Value) -> ToolResult<Output> {
    let a: ListMetrics = args(a)?;
    let v = rest.get(&format!("/api/v1/orgs/{}/metrics/names", seg(&a.org)), &a.scope.params()).await?;
    Ok(Output::Json(v))
}

async fn metric(rest: &Rest, a: &Value) -> ToolResult<Output> {
    let a: QueryMetric = args(a)?;
    let op = a.op.map(|o| {
        match o {
            MetricOp::Sum => "sum",
            MetricOp::Avg => "avg",
            MetricOp::Min => "min",
            MetricOp::Max => "max",
            MetricOp::Count => "count",
            MetricOp::CountUnique => "count_unique",
            MetricOp::P50 => "p50",
            MetricOp::P75 => "p75",
            MetricOp::P95 => "p95",
            MetricOp::P99 => "p99",
        }
        .to_string()
    });
    let interval = a.interval.map(|i| {
        match i {
            Interval::Minute => "1m",
            Interval::Hour => "1h",
            Interval::Day => "1d",
        }
        .to_string()
    });
    let mut p = a.scope.params();
    p.extend([
        ("name", Some(a.name)),
        ("op", op),
        ("group_by", a.group_by),
        ("interval", interval),
        ("query", a.query),
    ]);
    let v = rest.get(&format!("/api/v1/orgs/{}/metrics/query", seg(&a.org)), &p).await?;
    Ok(Output::Json(v))
}
