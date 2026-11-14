//! Metrics: metric names, aggregated time series, and dashboards.

use std::collections::HashMap;

use axum::extract::State;
use axum::http::StatusCode;
use chrono::{DateTime, Duration, Utc};
use rupta_auth::rbac::Permission;
use rupta_store::squad::Row;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::access::org_sync;
use crate::error::{ApiError, ApiResult};
use crate::extract::{Auth, Json, Page, PageParams, Path, Query};
use crate::sql::Params;
use crate::state::ApiState;
use crate::telemetry::{Col, FilterSpec, Scope, attribute_expr, buckets, compile_filter, interval_for};

pub fn routes() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(metric_names))
        .routes(routes!(metric_query))
        .routes(routes!(list_dashboards, create_dashboard))
        .routes(routes!(get_dashboard, update_dashboard, delete_dashboard))
}

const METRIC_FILTER: FilterSpec<'static> = FilterSpec {
    keys: &[
        ("release", Col::Text("release")),
        ("trace", Col::Text("trace_id")),
        ("type", Col::Text("type")),
        ("value", Col::Number("value")),
    ],
    text: None,
    attributes: Some("attributes"),
};

#[derive(Serialize, ToSchema)]
pub struct MetricName {
    pub name: String,
    /// counter, gauge, distribution or set.
    #[serde(rename = "type")]
    pub ty: String,
    pub unit: Option<String>,
    pub last_seen: DateTime<Utc>,
    /// Values stored in the range.
    pub count: i64,
}

#[derive(Serialize, ToSchema)]
pub struct MetricNames {
    pub data: Vec<MetricName>,
}

/// Metric names seen in the range, by name.
#[utoipa::path(get, path = "/api/v1/orgs/{org}/metrics/names", tag = "metrics",
    params(("org" = String, Path), Scope), responses((status = 200, body = MetricNames)))]
async fn metric_names(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
    Query(scope): Query<Scope>,
) -> ApiResult<axum::Json<MetricNames>> {
    let data = state
        .db(move |c| {
            let s = scope.resolve(c, &auth, &org, Duration::days(7))?;
            if s.projects.is_empty() {
                return Ok(Vec::new());
            }
            let mut params = Params::new();
            let base = s.filter(&mut params, "", "timestamp");
            Ok(c.query(
                &format!("SELECT name, type, unit, max(timestamp), count(*) FROM metrics WHERE {base} GROUP BY name, type, unit ORDER BY name LIMIT 5000"),
                &params.refs(),
            )?
            .iter()
            .map(|r| MetricName { name: r.get(0), ty: r.get(1), unit: r.get(2), last_seen: r.get(3), count: r.get(4) })
            .collect())
        })
        .await?;
    Ok(axum::Json(MetricNames { data }))
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct MetricQueryParams {
    pub name: String,
    /// `sum`, `avg`, `min`, `max`, `count`, `count_unique` (sets), `p50`,
    /// `p75`, `p95`, `p99`. Default: `sum` for counters, `avg` for gauges,
    /// `p95` for distributions, `count_unique` for sets.
    pub op: Option<String>,
    /// An attribute name, or `environment` / `release`: one series per value
    /// (the 10 largest by total).
    pub group_by: Option<String>,
    /// `1m`, `1h` or `1d` (default: chosen from the range).
    pub interval: Option<String>,
    /// Filter: `key:value` on attributes, `release:`, `trace:`, `value:>n`.
    pub query: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct MetricSeries {
    /// `{group_by: value}`, or `null` without `group_by`.
    #[schema(value_type = Option<Object>)]
    pub group: Option<Value>,
    /// `[[unix_seconds, value_or_null], ...]`, one point per bucket.
    pub points: Vec<(i64, Option<f64>)>,
    /// The aggregate over the whole range.
    pub total: Option<f64>,
}

#[derive(Serialize, ToSchema)]
pub struct MetricQueryResult {
    pub name: String,
    pub op: String,
    #[serde(rename = "type")]
    pub ty: Option<String>,
    pub unit: Option<String>,
    /// Bucket size in seconds.
    pub interval: i64,
    pub series: Vec<MetricSeries>,
    /// True if there were more than 10 groups (only the largest are returned).
    pub truncated: bool,
}

fn aggregate(op: &str) -> Option<&'static str> {
    Some(match op {
        "sum" => "sum(value)",
        "avg" => "avg(value)",
        "min" => "min(value)",
        "max" => "max(value)",
        "count" => "count(*)",
        "count_unique" => "approx_count_distinct(value)",
        "p50" => "approx_percentile(value, 0.5)",
        "p75" => "approx_percentile(value, 0.75)",
        "p95" => "approx_percentile(value, 0.95)",
        "p99" => "approx_percentile(value, 0.99)",
        _ => return None,
    })
}

fn number(r: &Row, i: usize) -> Option<f64> {
    use rupta_store::squad::Value as V;
    match r.get::<V>(i) {
        V::Float8(f) => Some(f),
        V::Int8(n) => Some(n as f64),
        V::Int4(n) => Some(n as f64),
        V::Float4(f) => Some(f as f64),
        V::Numeric(n) => n.to_string().parse().ok(),
        _ => None,
    }
    .filter(|f| f.is_finite())
}

/// A metric aggregated over time, optionally one series per attribute value.
#[utoipa::path(get, path = "/api/v1/orgs/{org}/metrics/query", tag = "metrics",
    params(("org" = String, Path), Scope, MetricQueryParams), responses((status = 200, body = MetricQueryResult)))]
async fn metric_query(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
    Query(scope): Query<Scope>,
    Query(q): Query<MetricQueryParams>,
) -> ApiResult<axum::Json<MetricQueryResult>> {
    let out = state
        .db(move |c| {
            let s = scope.resolve(c, &auth, &org, Duration::hours(24))?;
            let (unit, step) = interval_for(s.start, s.end, q.interval.as_deref())?;
            let mut params = Params::new();
            let mut base = s.filter(&mut params, "", "timestamp");
            base.push_str(&format!(
                " AND name = {} AND ({})",
                params.bind(q.name.clone()),
                compile_filter(q.query.as_deref().unwrap_or(""), &METRIC_FILTER, &mut params)?
            ));
            let meta = if s.projects.is_empty() {
                None
            } else {
                // Grouped rather than `LIMIT 1`: aggregations run in the column engine.
                c.query(&format!("SELECT type, unit, count(*) FROM metrics WHERE {base} GROUP BY type, unit"), &params.refs())?
                    .iter()
                    .max_by_key(|r| r.get::<i64>(2))
                    .map(|r| (r.get::<String>(0), r.get::<Option<String>>(1)))
            };
            let ty = meta.as_ref().map(|m| m.0.clone());
            let op = q.op.clone().unwrap_or_else(|| match ty.as_deref() {
                Some("counter") => "sum".into(),
                Some("gauge") => "avg".into(),
                Some("set") => "count_unique".into(),
                _ => "p95".into(),
            });
            let agg = aggregate(&op).ok_or_else(|| {
                ApiError::invalid("op", "sum, avg, min, max, count, count_unique, p50, p75, p95 or p99")
            })?;
            let group_expr = match q.group_by.as_deref().filter(|g| !g.is_empty()) {
                None => None,
                Some(g @ ("environment" | "release")) => Some((g.to_string(), g.to_string())),
                Some(g) => Some((g.to_string(), attribute_expr("attributes", g)?)),
            };
            let mut result = MetricQueryResult {
                name: q.name.clone(),
                op: op.clone(),
                ty,
                unit: meta.and_then(|m| m.1),
                interval: step.num_seconds(),
                series: Vec::new(),
                truncated: false,
            };
            if s.projects.is_empty() || result.ty.is_none() {
                return Ok(result);
            }
            let times = buckets(s.start, s.end, step);
            match group_expr {
                None => {
                    let rows = c.query(
                        &format!("SELECT date_trunc('{unit}', timestamp), {agg} FROM metrics WHERE {base} GROUP BY 1"),
                        &params.refs(),
                    )?;
                    let by: HashMap<i64, Option<f64>> =
                        rows.iter().map(|r| (r.get::<DateTime<Utc>>(0).timestamp(), number(r, 1))).collect();
                    let total = c.query_one(&format!("SELECT {agg} FROM metrics WHERE {base}"), &params.refs())?;
                    result.series.push(MetricSeries {
                        group: None,
                        points: times.iter().map(|t| (*t, by.get(t).copied().flatten())).collect(),
                        total: number(&total, 0),
                    });
                }
                Some((key, expr)) => {
                    let totals = c.query(
                        &format!("SELECT {expr}, {agg}, count(*) FROM metrics WHERE {base} GROUP BY 1"),
                        &params.refs(),
                    )?;
                    let mut groups: Vec<(Option<String>, Option<f64>, i64)> =
                        totals.iter().map(|r| (r.get(0), number(r, 1), r.get(2))).collect();
                    groups.sort_by(|a, b| b.1.unwrap_or(0.0).total_cmp(&a.1.unwrap_or(0.0)).then(b.2.cmp(&a.2)));
                    result.truncated = groups.len() > 10;
                    groups.truncate(10);
                    let rows = c.query(
                        &format!("SELECT {expr}, date_trunc('{unit}', timestamp), {agg} FROM metrics WHERE {base} GROUP BY 1, 2"),
                        &params.refs(),
                    )?;
                    let mut by: HashMap<(Option<String>, i64), Option<f64>> = HashMap::new();
                    for r in &rows {
                        by.insert((r.get(0), r.get::<DateTime<Utc>>(1).timestamp()), number(r, 2));
                    }
                    for (g, total, _) in groups {
                        result.series.push(MetricSeries {
                            group: Some(json!({ key.clone(): g })),
                            points: times.iter().map(|t| (*t, by.get(&(g.clone(), *t)).copied().flatten())).collect(),
                            total,
                        });
                    }
                }
            }
            Ok(result)
        })
        .await?;
    Ok(axum::Json(out))
}

/// A dashboard of widgets.
#[derive(Serialize, ToSchema)]
pub struct Dashboard {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    /// Widgets: `{id, title, type, chart, query, layout}` where `type` is
    /// `metric`, `transactions`, `logs` or `issues`, `chart` is `line`,
    /// `area`, `bar`, `table` or `number`, `query` holds the parameters of
    /// the matching query endpoint and `layout` is `{x, y, w, h}` on a
    /// 12-column grid. Stored as given after validation.
    #[schema(value_type = Vec<Object>)]
    pub widgets: Value,
    pub created_by: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

const DASHBOARD_COLUMNS: &str = "id, title, description, widgets, created_by, created_at, updated_at";

fn dashboard(r: &Row) -> Dashboard {
    Dashboard {
        id: r.get::<i64>(0).to_string(),
        title: r.get(1),
        description: r.get(2),
        widgets: r.get(3),
        created_by: r.get::<Option<i64>>(4).map(|i| i.to_string()),
        created_at: r.get(5),
        updated_at: r.get(6),
    }
}

/// Most widgets per dashboard.
pub const MAX_WIDGETS: usize = 50;
const WIDGET_TYPES: [&str; 4] = ["metric", "transactions", "logs", "issues"];
const CHART_TYPES: [&str; 5] = ["line", "area", "bar", "table", "number"];

fn validate_widgets(w: &Value) -> ApiResult<()> {
    let Some(list) = w.as_array() else { return Err(ApiError::invalid("widgets", "must be an array")) };
    if list.len() > MAX_WIDGETS {
        return Err(ApiError::invalid("widgets", format!("at most {MAX_WIDGETS} widgets")));
    }
    if w.to_string().len() > 256 << 10 {
        return Err(ApiError::invalid("widgets", "at most 256 KiB of JSON"));
    }
    for (i, x) in list.iter().enumerate() {
        let ty = x.get("type").and_then(Value::as_str).unwrap_or("");
        if !WIDGET_TYPES.contains(&ty) {
            return Err(ApiError::invalid(&format!("widgets[{i}].type"), "metric, transactions, logs or issues"));
        }
        let chart = x.get("chart").and_then(Value::as_str).unwrap_or("line");
        if !CHART_TYPES.contains(&chart) {
            return Err(ApiError::invalid(&format!("widgets[{i}].chart"), "line, area, bar, table or number"));
        }
        if x.get("query").is_some_and(|q| !q.is_object()) {
            return Err(ApiError::invalid(&format!("widgets[{i}].query"), "must be an object"));
        }
    }
    Ok(())
}

fn validate_title(t: &str) -> ApiResult<String> {
    let t = t.trim();
    if t.is_empty() || t.chars().count() > 200 {
        return Err(ApiError::invalid("title", "1 to 200 characters"));
    }
    Ok(t.to_string())
}

/// Dashboards of an organization, newest first.
#[utoipa::path(get, path = "/api/v1/orgs/{org}/dashboards", tag = "metrics",
    params(("org" = String, Path), PageParams), responses((status = 200, body = Page<Dashboard>)))]
async fn list_dashboards(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
    Query(page): Query<PageParams>,
) -> ApiResult<axum::Json<Page<Dashboard>>> {
    let before: Option<i64> = page.cursor()?;
    let limit = page.limit();
    let rows = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ReadProject)?;
            Ok(c.query(
                &format!("SELECT {DASHBOARD_COLUMNS} FROM dashboards WHERE org_id = $1 AND ($2::bigint IS NULL OR id < $2) ORDER BY id DESC LIMIT $3"),
                &[&o.id, &before, &(limit + 1)],
            )?)
        })
        .await?;
    let list: Vec<Dashboard> = rows.iter().map(dashboard).collect();
    Ok(axum::Json(Page::from_rows(list, limit, |d| d.id.parse::<i64>().unwrap_or(0))))
}

#[derive(Deserialize, ToSchema)]
pub struct DashboardCreate {
    pub title: String,
    pub description: Option<String>,
    #[schema(value_type = Vec<Object>)]
    #[serde(default = "empty_list")]
    pub widgets: Value,
}

fn empty_list() -> Value {
    json!([])
}

/// Creates a dashboard (members and above).
#[utoipa::path(post, path = "/api/v1/orgs/{org}/dashboards", tag = "metrics",
    params(("org" = String, Path)), request_body = DashboardCreate,
    responses((status = 201, body = Dashboard), (status = 422, body = crate::error::ErrorBody)))]
async fn create_dashboard(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
    Json(req): Json<DashboardCreate>,
) -> ApiResult<(StatusCode, axum::Json<Dashboard>)> {
    let title = validate_title(&req.title)?;
    validate_widgets(&req.widgets)?;
    let d = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::TriageIssues)?;
            let r = c.query_one(
                &format!("INSERT INTO dashboards (org_id, title, description, widgets, created_by) VALUES ($1, $2, $3, $4, $5) RETURNING {DASHBOARD_COLUMNS}"),
                &[&o.id, &title, &req.description, &req.widgets, &auth.user_id()],
            )?;
            let d = dashboard(&r);
            rupta_auth::audit::record(c, &rupta_auth::audit::Entry {
                target_type: Some("dashboard"),
                target_id: Some(d.id.clone()),
                data: json!({"title": d.title}),
                ..auth.audit(Some(o.id), "dashboard.create")
            })?;
            Ok(d)
        })
        .await?;
    Ok((StatusCode::CREATED, axum::Json(d)))
}

/// A dashboard.
#[utoipa::path(get, path = "/api/v1/orgs/{org}/dashboards/{id}", tag = "metrics",
    params(("org" = String, Path), ("id" = String, Path)), responses((status = 200, body = Dashboard)))]
async fn get_dashboard(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, id)): Path<(String, i64)>,
) -> ApiResult<axum::Json<Dashboard>> {
    let d = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ReadProject)?;
            let r = c
                .query_opt(
                    &format!("SELECT {DASHBOARD_COLUMNS} FROM dashboards WHERE org_id = $1 AND id = $2"),
                    &[&o.id, &id],
                )?
                .ok_or_else(ApiError::not_found)?;
            Ok(dashboard(&r))
        })
        .await?;
    Ok(axum::Json(d))
}

#[derive(Deserialize, ToSchema)]
pub struct DashboardUpdate {
    pub title: Option<String>,
    /// `null` clears it.
    #[serde(default, deserialize_with = "crate::extract::double_option")]
    #[schema(value_type = Option<String>)]
    pub description: Option<Option<String>>,
    /// Replaces all widgets.
    #[schema(value_type = Option<Vec<Object>>)]
    pub widgets: Option<Value>,
}

/// Changes a dashboard (members and above).
#[utoipa::path(patch, path = "/api/v1/orgs/{org}/dashboards/{id}", tag = "metrics",
    params(("org" = String, Path), ("id" = String, Path)), request_body = DashboardUpdate,
    responses((status = 200, body = Dashboard)))]
async fn update_dashboard(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, id)): Path<(String, i64)>,
    Json(req): Json<DashboardUpdate>,
) -> ApiResult<axum::Json<Dashboard>> {
    let title = req.title.as_deref().map(validate_title).transpose()?;
    if let Some(w) = &req.widgets {
        validate_widgets(w)?;
    }
    let d = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::TriageIssues)?;
            let txn = c.transaction()?;
            let current = txn
                .query_opt(&format!("SELECT {DASHBOARD_COLUMNS} FROM dashboards WHERE org_id = $1 AND id = $2"), &[&o.id, &id])?
                .ok_or_else(ApiError::not_found)?;
            let cur = dashboard(&current);
            let title = title.unwrap_or(cur.title);
            let description = req.description.clone().unwrap_or(cur.description);
            let widgets = req.widgets.clone().unwrap_or(cur.widgets);
            let r = txn.query_one(
                &format!("UPDATE dashboards SET title = $3, description = $4, widgets = $5, updated_at = now() WHERE org_id = $1 AND id = $2 RETURNING {DASHBOARD_COLUMNS}"),
                &[&o.id, &id, &title, &description, &widgets],
            )?;
            rupta_auth::audit::record(&txn, &rupta_auth::audit::Entry {
                target_type: Some("dashboard"),
                target_id: Some(id.to_string()),
                data: json!({"title": title}),
                ..auth.audit(Some(o.id), "dashboard.update")
            })?;
            txn.commit()?;
            Ok(dashboard(&r))
        })
        .await?;
    Ok(axum::Json(d))
}

/// Deletes a dashboard (members and above).
#[utoipa::path(delete, path = "/api/v1/orgs/{org}/dashboards/{id}", tag = "metrics",
    params(("org" = String, Path), ("id" = String, Path)), responses((status = 204)))]
async fn delete_dashboard(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, id)): Path<(String, i64)>,
) -> ApiResult<StatusCode> {
    state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::TriageIssues)?;
            let n = c.execute("DELETE FROM dashboards WHERE org_id = $1 AND id = $2", &[&o.id, &id])?;
            if n == 0 {
                return Err(ApiError::not_found());
            }
            rupta_auth::audit::record(
                c,
                &rupta_auth::audit::Entry {
                    target_type: Some("dashboard"),
                    target_id: Some(id.to_string()),
                    ..auth.audit(Some(o.id), "dashboard.delete")
                },
            )?;
            Ok(())
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widgets_validate() {
        assert!(validate_widgets(&json!([{"type": "metric", "chart": "bar", "query": {"name": "x"}}])).is_ok());
        assert!(validate_widgets(&json!([{"type": "nope"}])).is_err());
        assert!(validate_widgets(&json!([{"type": "logs", "chart": "pie"}])).is_err());
        assert!(validate_widgets(&json!({})).is_err());
        assert!(aggregate("p95").is_some() && aggregate("median").is_none());
    }
}
