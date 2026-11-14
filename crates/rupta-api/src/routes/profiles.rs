//! Profiles: the list of stored profiles and chunks, flamegraphs of one
//! profile, raw downloads, and flamegraphs aggregated over the profiles of
//! one transaction.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Duration, Utc};
use rupta_auth::rbac::Permission;
use rupta_core::profile::{Flamegraph, FlamegraphBuilder, Frame, Node, SampledProfile};
use rupta_store::squad::{Connection, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::access::project_sync;
use crate::error::{ApiError, ApiResult};
use crate::extract::{Auth, Path, Query, decode_cursor, encode_cursor};
use crate::sql::{Params, id_list};
use crate::state::ApiState;
use crate::telemetry::{Scope, hex_id, parse_hex_id};

pub fn routes() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(list_profiles))
        .routes(routes!(aggregate_flamegraph))
        .routes(routes!(flamegraph))
        .routes(routes!(raw_profile))
}

/// Most profiles merged into an aggregated flamegraph.
pub const AGGREGATE_LIMIT: i64 = 50;

#[derive(Serialize, ToSchema, Clone)]
pub struct ProfileSummary {
    pub profile_id: String,
    pub project_id: String,
    /// `transaction` (transaction profiling) or `chunk` (continuous profiling).
    pub kind: String,
    pub transaction_name: Option<String>,
    pub transaction_id: Option<String>,
    pub trace_id: Option<String>,
    pub profiler_id: Option<String>,
    pub platform: Option<String>,
    pub release: Option<String>,
    pub environment: Option<String>,
    pub started: DateTime<Utc>,
    pub duration_ms: Option<f64>,
    pub samples: i32,
    /// Size of the document in bytes.
    pub size: i64,
}

const PROFILE_COLUMNS: &str =
    "profile_id, project_id, kind, transaction_name, transaction_id, trace_id, profiler_id, platform,
    release, environment, started, duration_ms, samples, size";

fn summary(r: &Row) -> ProfileSummary {
    ProfileSummary {
        profile_id: hex_id(r.get(0)),
        project_id: r.get::<i64>(1).to_string(),
        kind: r.get(2),
        transaction_name: r.get(3),
        transaction_id: r.get::<Option<[u8; 16]>>(4).map(hex_id),
        trace_id: r.get(5),
        profiler_id: r.get(6),
        platform: r.get(7),
        release: r.get(8),
        environment: r.get(9),
        started: r.get(10),
        duration_ms: r.get(11),
        samples: r.get(12),
        size: r.get(13),
    }
}

#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ProfileListParams {
    /// Only profiles of this transaction name.
    pub transaction: Option<String>,
    /// `transaction` or `chunk`.
    pub kind: Option<String>,
    pub release: Option<String>,
    /// Only chunks of this continuous profiler session.
    pub profiler_id: Option<String>,
    pub cursor: Option<String>,
    /// 1 to 100, default 50.
    pub limit: Option<u32>,
}

#[derive(Serialize, ToSchema)]
pub struct ProfilePage {
    pub data: Vec<ProfileSummary>,
    pub next_cursor: Option<String>,
}

/// Profiles and continuous profiling chunks, newest first.
#[utoipa::path(get, path = "/api/v1/orgs/{org}/profiles", tag = "profiling",
    params(("org" = String, Path), Scope, ProfileListParams), responses((status = 200, body = ProfilePage)))]
async fn list_profiles(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
    Query(scope): Query<Scope>,
    Query(q): Query<ProfileListParams>,
) -> ApiResult<axum::Json<ProfilePage>> {
    let limit = q.limit.unwrap_or(50).clamp(1, 100) as i64;
    let cursor: Option<(i64, String)> = match &q.cursor {
        Some(c) if !c.is_empty() => Some(decode_cursor(c).ok_or_else(|| ApiError::bad_request("invalid cursor"))?),
        _ => None,
    };
    let rows = state
        .db(move |c| {
            let s = scope.resolve(c, &auth, &org, Duration::hours(24))?;
            if s.projects.is_empty() {
                return Ok(Vec::new());
            }
            let mut params = Params::new();
            let mut sql =
                format!("SELECT {PROFILE_COLUMNS} FROM profiles WHERE {}", s.filter(&mut params, "", "started"));
            for (col, v) in [
                ("transaction_name", &q.transaction),
                ("kind", &q.kind),
                ("release", &q.release),
                ("profiler_id", &q.profiler_id),
            ] {
                if let Some(v) = v.as_ref().filter(|v| !v.is_empty()) {
                    sql.push_str(&format!(" AND {col} = {}", params.bind(v.clone())));
                }
            }
            if let Some((micros, id)) = &cursor {
                let ts =
                    DateTime::from_timestamp_micros(*micros).ok_or_else(|| ApiError::bad_request("invalid cursor"))?;
                let id = uuid::Uuid::try_parse(id).map_err(|_| ApiError::bad_request("invalid cursor"))?;
                let (t, i) = (params.bind(ts), params.bind(*id.as_bytes()));
                sql.push_str(&format!(" AND (started < {t} OR (started = {t} AND profile_id < {i}))"));
            }
            sql.push_str(&format!(" ORDER BY started DESC, profile_id DESC LIMIT {}", limit + 1));
            Ok(c.query(&sql, &params.refs())?.iter().map(summary).collect::<Vec<_>>())
        })
        .await?;
    let mut data = rows;
    let next_cursor = if data.len() as i64 > limit {
        data.truncate(limit as usize);
        data.last().map(|p| encode_cursor(&(p.started.timestamp_micros(), p.profile_id.clone())))
    } else {
        None
    };
    Ok(axum::Json(ProfilePage { data, next_cursor }))
}

#[derive(Serialize, ToSchema)]
pub struct ThreadInfo {
    pub id: String,
    pub name: Option<String>,
    pub samples: usize,
}

/// A frame of a flamegraph.
#[derive(Serialize, ToSchema)]
pub struct FrameOut {
    pub name: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub package: Option<String>,
    pub is_application: bool,
}

/// A call tree node: `frame_index` into `frames` (`null` for the root),
/// weights in samples.
#[derive(Serialize, ToSchema)]
pub struct NodeOut {
    pub frame_index: Option<usize>,
    pub self_weight: u64,
    pub total_weight: u64,
    #[schema(no_recursion)]
    pub children: Vec<NodeOut>,
}

/// A flamegraph. Children are in the order first sampled (time order);
/// sort by `total_weight` for a left-heavy view.
#[derive(Serialize, ToSchema)]
pub struct FlamegraphOut {
    pub frames: Vec<FrameOut>,
    pub root: NodeOut,
    /// Samples in the tree.
    pub samples: u64,
    /// Profiles merged.
    pub profiles: u64,
    /// The thread shown (single profiles).
    pub thread_id: Option<String>,
    /// Threads of the profile with their sample counts (single profiles).
    pub threads: Vec<ThreadInfo>,
    pub platform: Option<String>,
    /// Profiles (and chunks) merged (aggregated flamegraphs).
    pub profile_ids: Vec<String>,
}

/// The flamegraph types serialize like the `*Out` schema types.
#[derive(Serialize)]
struct FlamegraphJson<'a> {
    frames: &'a [Frame],
    root: &'a Node,
    samples: u64,
    profiles: u64,
    thread_id: Option<String>,
    threads: Vec<ThreadInfo>,
    platform: Option<String>,
    profile_ids: Vec<String>,
}

fn flamegraph_json(
    g: &Flamegraph,
    thread_id: Option<String>,
    threads: Vec<ThreadInfo>,
    platform: Option<String>,
    profile_ids: Vec<String>,
) -> Value {
    serde_json::to_value(FlamegraphJson {
        frames: &g.frames,
        root: &g.root,
        samples: g.samples,
        profiles: g.profiles,
        thread_id,
        threads,
        platform,
        profile_ids,
    })
    .unwrap_or(Value::Null)
}

fn load_document(state: &ApiState, key: &str) -> ApiResult<Vec<u8>> {
    let data = state.store.objects().get(key).map_err(ApiError::internal)?.ok_or_else(ApiError::not_found)?;
    zstd::decode_all(data.as_slice()).map_err(|e| ApiError::internal(format!("corrupt profile: {e}")))
}

fn parse_profile(raw: &[u8]) -> ApiResult<(Value, SampledProfile)> {
    let doc: Value = serde_json::from_slice(raw).map_err(|e| ApiError::internal(format!("corrupt profile: {e}")))?;
    let p = SampledProfile::parse(&doc).map_err(ApiError::internal)?;
    Ok((doc, p))
}

/// Recently built single-profile flamegraphs.
static CACHE: LazyLock<Mutex<HashMap<String, Arc<Value>>>> = LazyLock::new(Default::default);
const CACHE_ENTRIES: usize = 64;

#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct FlamegraphParams {
    /// Thread id (default: the transaction's thread, else the main thread,
    /// else the busiest thread).
    pub thread: Option<String>,
    /// Only samples at or after this time (Unix seconds), e.g. the start
    /// of a transaction within a continuous profiling chunk.
    pub start: Option<f64>,
    /// Only samples at or before this time (Unix seconds).
    pub end: Option<f64>,
}

fn profile_row(c: &Connection, auth: &Auth, org: &str, project: &str, id: &str) -> ApiResult<(String, Option<String>)> {
    let p = project_sync(c, auth, org, project, Permission::ReadProject)?;
    let id = parse_hex_id(id)?;
    let r = c
        .query_opt(
            "SELECT object_key, platform FROM profiles WHERE project_id = $1 AND profile_id = $2",
            &[&p.id, &id],
        )?
        .ok_or_else(ApiError::not_found)?;
    Ok((r.get(0), r.get(1)))
}

/// The flamegraph of one profile or chunk.
#[utoipa::path(get, path = "/api/v1/orgs/{org}/profiles/{project}/{profile_id}/flamegraph", tag = "profiling",
    params(("org" = String, Path), ("project" = String, Path, description = "slug or id"), ("profile_id" = String, Path), FlamegraphParams),
    responses((status = 200, body = FlamegraphOut), (status = 404, body = crate::error::ErrorBody)))]
async fn flamegraph(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, project, id)): Path<(String, String, String)>,
    Query(q): Query<FlamegraphParams>,
) -> ApiResult<Response> {
    let st = state.clone();
    let out = state
        .db(move |c| {
            let (key, platform) = profile_row(c, &auth, &org, &project, &id)?;
            let cache_key = format!("{key}|{:?}|{:?}|{:?}", q.thread, q.start, q.end);
            if let Some(v) = CACHE.lock().unwrap_or_else(|e| e.into_inner()).get(&cache_key) {
                return Ok(v.clone());
            }
            let (_, p) = parse_profile(&load_document(&st, &key)?)?;
            let thread = q.thread.clone().or_else(|| p.default_thread());
            let window = match (q.start, q.end) {
                (None, None) => None,
                (s, e) => Some((s.unwrap_or(f64::MIN), e.unwrap_or(f64::MAX))),
            };
            let mut b = FlamegraphBuilder::new();
            b.add(&p, thread.as_deref(), window);
            let counts = p.thread_sample_counts();
            let threads = p
                .threads
                .iter()
                .map(|(tid, name)| ThreadInfo {
                    id: tid.clone(),
                    name: name.clone(),
                    samples: counts.iter().find(|(t, _)| t == tid).map_or(0, |(_, n)| *n),
                })
                .collect();
            let v = Arc::new(flamegraph_json(&b.build(), thread, threads, platform, vec![hex_id(parse_hex_id(&id)?)]));
            let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
            if cache.len() >= CACHE_ENTRIES {
                cache.clear();
            }
            cache.insert(cache_key, v.clone());
            Ok(v)
        })
        .await?;
    Ok(axum::Json(out.as_ref()).into_response())
}

/// The profile document as sent by the SDK (JSON, as a download).
#[utoipa::path(get, path = "/api/v1/orgs/{org}/profiles/{project}/{profile_id}/raw", tag = "profiling",
    params(("org" = String, Path), ("project" = String, Path, description = "slug or id"), ("profile_id" = String, Path)),
    responses((status = 200, description = "the profile document", content_type = "application/json")))]
async fn raw_profile(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, project, id)): Path<(String, String, String)>,
) -> ApiResult<Response> {
    let st = state.clone();
    let name = format!("profile-{}.json", id.chars().filter(char::is_ascii_hexdigit).collect::<String>());
    let data = state
        .db(move |c| {
            let (key, _) = profile_row(c, &auth, &org, &project, &id)?;
            load_document(&st, &key)
        })
        .await?;
    let mut resp = Body::from(data).into_response();
    let h = resp.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
    if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename=\"{name}\"")) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    Ok(resp)
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AggregateParams {
    /// The transaction name.
    pub transaction: String,
}

/// A flamegraph merged from the latest (at most 50) profiles of a
/// transaction: transaction profiles of that name, and for transactions
/// recorded with continuous profiling the samples of their chunks within
/// the transaction's time span.
#[utoipa::path(get, path = "/api/v1/orgs/{org}/profiles/aggregate-flamegraph", tag = "profiling",
    params(("org" = String, Path), Scope, AggregateParams), responses((status = 200, body = FlamegraphOut)))]
async fn aggregate_flamegraph(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
    Query(scope): Query<Scope>,
    Query(q): Query<AggregateParams>,
) -> ApiResult<Response> {
    let st = state.clone();
    let out = state
        .db(move |c| {
            let s = scope.resolve(c, &auth, &org, Duration::days(7))?;
            let mut b = FlamegraphBuilder::new();
            let mut ids = Vec::new();
            let mut platform = None;
            if !s.projects.is_empty() {
                let mut params = Params::new();
                let filter = s.filter(&mut params, "", "started");
                let name = params.bind(q.transaction.clone());
                let rows = c.query(
                    &format!(
                        "SELECT profile_id, object_key, platform FROM profiles WHERE {filter} AND kind = 'transaction'
                         AND transaction_name = {name} ORDER BY started DESC LIMIT {AGGREGATE_LIMIT}"
                    ),
                    &params.refs(),
                )?;
                for r in &rows {
                    let Ok(raw) = load_document(&st, &r.get::<String>(1)) else { continue };
                    let Ok((_, p)) = parse_profile(&raw) else { continue };
                    b.add(&p, None, None);
                    ids.push(hex_id(r.get(0)));
                    platform = platform.or(r.get::<Option<String>>(2));
                }
                // Continuous profiling: transactions of that name with a profiler session.
                let remaining = AGGREGATE_LIMIT - rows.len() as i64;
                if remaining > 0 {
                    let mut tp = Params::new();
                    let tfilter = s.filter(&mut tp, "", "timestamp");
                    let tname = tp.bind(q.transaction.clone());
                    let txs = c.query(
                        &format!(
                            "SELECT project_id, profiler_id, start_timestamp, timestamp FROM transactions
                             WHERE {tfilter} AND name = {tname} AND profiler_id IS NOT NULL AND start_timestamp IS NOT NULL
                             ORDER BY timestamp DESC LIMIT {remaining}"
                        ),
                        &tp.refs(),
                    )?;
                    let mut docs: HashMap<String, Option<SampledProfile>> = HashMap::new();
                    for t in &txs {
                        let (project, profiler, start, end): (i64, String, DateTime<Utc>, DateTime<Utc>) =
                            (t.get(0), t.get(1), t.get(2), t.get(3));
                        let chunks = c.query(
                            &format!(
                                "SELECT profile_id, object_key, platform FROM profiles WHERE project_id IN ({}) AND project_id = $1
                                 AND profiler_id = $2 AND kind = 'chunk' AND started <= $3 AND started >= $4 ORDER BY started"
                            , id_list(&s.projects)),
                            &[&project, &profiler, &end, &(start - Duration::seconds(120))],
                        )?;
                        let window = (start.timestamp_micros() as f64 / 1e6, end.timestamp_micros() as f64 / 1e6);
                        for ch in &chunks {
                            let key: String = ch.get(1);
                            let p = docs.entry(key.clone()).or_insert_with(|| {
                                load_document(&st, &key).ok().and_then(|raw| parse_profile(&raw).ok()).map(|(_, p)| p)
                            });
                            if let Some(p) = p
                                && b.add(p, None, Some(window)) > 0
                            {
                                let id = hex_id(ch.get(0));
                                if !ids.contains(&id) {
                                    ids.push(id);
                                }
                                platform = platform.or(ch.get::<Option<String>>(2));
                            }
                        }
                    }
                }
            }
            Ok(flamegraph_json(&b.build(), None, Vec::new(), platform, ids))
        })
        .await?;
    Ok(axum::Json(out).into_response())
}
