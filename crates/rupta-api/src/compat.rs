//! The subset of the `/api/0` API that the upload CLI (`sentry-cli`) uses
//! for source maps, releases and deploys. See docs/sourcemap-upload.md for
//! the commands that work.
//!
//! Errors use this API's format: `{"detail": "..."}`.

use std::collections::HashMap;
use std::io::{Cursor, Read};

use axum::Router;
use axum::extract::{DefaultBodyLimit, Multipart, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use chrono::{DateTime, Utc};
use rupta_auth::rbac::Permission;
use rupta_store::squad::Connection;
use serde::Deserialize;
use serde_json::{Value, json};
use sha1::{Digest, Sha1};

use crate::access::{OrgCtx, org_sync, project_sync};
use crate::error::ApiError;
use crate::extract::{Auth, Path};
use crate::state::{ApiMetrics, ApiState};

/// Size of one chunk the CLI uploads.
pub const CHUNK_SIZE: usize = 8 << 20;
/// Chunks per upload request.
pub const CHUNKS_PER_REQUEST: usize = 64;
/// Largest upload request.
pub const MAX_REQUEST_SIZE: usize = 32 << 20;
/// Largest assembled file.
pub const MAX_FILE_SIZE: usize = 512 << 20;

/// An error in this API's format.
pub struct CompatError(StatusCode, String);

impl From<ApiError> for CompatError {
    fn from(e: ApiError) -> CompatError {
        CompatError(e.status, e.message)
    }
}

impl IntoResponse for CompatError {
    fn into_response(self) -> Response {
        (self.0, axum::Json(json!({ "detail": self.1 }))).into_response()
    }
}

type CResult<T> = Result<T, CompatError>;

fn bad(msg: impl Into<String>) -> CompatError {
    CompatError(StatusCode::BAD_REQUEST, msg.into())
}

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/api/0/", get(index))
        .route("/api/0/organizations/{org}/", get(organization))
        .route(
            "/api/0/organizations/{org}/chunk-upload/",
            get(chunk_options).post(upload_chunks).layer(DefaultBodyLimit::max(MAX_REQUEST_SIZE + (1 << 20))),
        )
        .route("/api/0/organizations/{org}/artifactbundle/assemble/", post(assemble))
        .route("/api/0/organizations/{org}/releases/", post(create_release).get(list_org_releases))
        .route("/api/0/organizations/{org}/releases/{version}/", get(get_release).put(update_release))
        .route("/api/0/organizations/{org}/releases/{version}/deploys/", get(list_deploys).post(create_deploy))
        .route("/api/0/projects/{org}/{project}/", get(project))
        .route("/api/0/projects/{org}/{project}/releases/", get(list_project_releases))
        .route(
            "/api/0/{*rest}",
            axum::routing::any(|| async { CompatError(StatusCode::NOT_FOUND, "not supported by rupta".into()) }),
        )
}

/// `GET /api/0/`: who is calling.
async fn index(State(state): State<ApiState>, auth: Auth) -> CResult<axum::Json<Value>> {
    let user = match auth.user_id() {
        Some(uid) => state
            .db(move |c| Ok(rupta_auth::users::get(c, uid)?))
            .await?
            .map(|u| json!({"id": u.id.to_string(), "email": u.email, "name": u.name, "username": u.email})),
        None => None,
    };
    let scopes = auth.principal.token.as_ref().map(|t| t.scopes.clone()).unwrap_or_default();
    Ok(axum::Json(json!({ "version": "0", "user": user, "auth": { "scopes": scopes } })))
}

/// `GET /api/0/organizations/{org}/`.
async fn organization(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
) -> CResult<axum::Json<Value>> {
    let v = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ReadOrg)?;
            let created: DateTime<Utc> = c.query_one("SELECT created_at FROM organizations WHERE id = $1", &[&o.id])?.get(0);
            Ok(json!({"id": o.id.to_string(), "slug": o.slug, "name": o.name, "dateCreated": created, "status": {"id": "active", "name": "active"}, "features": []}))
        })
        .await?;
    Ok(axum::Json(v))
}

/// `GET /api/0/projects/{org}/{project}/`.
async fn project(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, project)): Path<(String, String)>,
) -> CResult<axum::Json<Value>> {
    let v = state
        .db(move |c| {
            let p = project_sync(c, &auth, &org, &project, Permission::ReadProject)?;
            Ok(json!({"id": p.id.to_string(), "slug": p.slug, "name": p.name, "platform": p.platform,
                      "organization": {"id": p.org.id.to_string(), "slug": p.org.slug, "name": p.org.name}}))
        })
        .await?;
    Ok(axum::Json(v))
}

/// `GET .../chunk-upload/`: how the CLI should upload.
async fn chunk_options(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
) -> CResult<axum::Json<Value>> {
    let o = state.org(&auth, &org, Permission::UploadArtifacts).await?;
    Ok(axum::Json(json!({
        // Relative: the CLI resolves it against the URL it was given, so
        // uploads work whatever RUPTA_PUBLIC_URL says.
        "url": format!("/api/0/organizations/{}/chunk-upload/", o.slug),
        "chunkSize": CHUNK_SIZE,
        "chunksPerRequest": CHUNKS_PER_REQUEST,
        "maxFileSize": MAX_FILE_SIZE,
        "maxRequestSize": MAX_REQUEST_SIZE,
        "concurrency": 4,
        "hashAlgorithm": "sha1",
        "compression": ["gzip"],
        "accept": ["artifact_bundles", "artifact_bundles_v2", "release_files", "sources"],
    })))
}

fn sha1_hex(data: &[u8]) -> String {
    hex::encode(Sha1::digest(data))
}

fn is_sha1(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// `POST .../chunk-upload/`: multipart fields `file` or `file_gzip`, each
/// named by the SHA-1 of its (uncompressed) content.
async fn upload_chunks(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
    mut form: Multipart,
) -> CResult<StatusCode> {
    state.org(&auth, &org, Permission::UploadArtifacts).await?;
    let mut chunks: Vec<(String, Vec<u8>)> = Vec::new();
    while let Some(field) = form.next_field().await.map_err(|e| bad(format!("invalid multipart body: {e}")))? {
        let name = field.name().unwrap_or("").to_string();
        let filename = field.file_name().unwrap_or("").to_lowercase();
        let raw = field.bytes().await.map_err(|e| bad(format!("invalid multipart body: {e}")))?;
        let data = match name.as_str() {
            "file" => raw.to_vec(),
            "file_gzip" => {
                let mut out = Vec::new();
                flate2::read::GzDecoder::new(&raw[..])
                    .take(CHUNK_SIZE as u64 + 1)
                    .read_to_end(&mut out)
                    .map_err(|e| bad(format!("invalid gzip data: {e}")))?;
                out
            }
            _ => continue,
        };
        if data.len() > CHUNK_SIZE {
            return Err(bad("chunk larger than chunkSize"));
        }
        let sum = sha1_hex(&data);
        if is_sha1(&filename) && filename != sum {
            return Err(bad(format!("checksum mismatch for chunk {filename}")));
        }
        chunks.push((sum, data));
        if chunks.len() > CHUNKS_PER_REQUEST {
            return Err(bad("too many chunks in one request"));
        }
    }
    let objects = state.store.objects().clone();
    state
        .db(move |c| {
            for (sum, data) in chunks {
                let key = format!("chunks/{sum}");
                objects.put(&key, &data).map_err(ApiError::internal)?;
                c.execute(
                    "INSERT INTO file_chunks (checksum, object_key, size) VALUES ($1, $2, $3) ON CONFLICT (checksum) DO NOTHING",
                    &[&sum, &key, &(data.len() as i64)],
                )?;
            }
            Ok(())
        })
        .await?;
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
struct AssembleRequest {
    checksum: String,
    chunks: Vec<String>,
    #[serde(default)]
    projects: Vec<String>,
    version: Option<String>,
    dist: Option<String>,
}

/// One file of a bundle manifest.
#[derive(Deserialize)]
struct ManifestFile {
    url: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    headers: HashMap<String, String>,
}

#[derive(Deserialize)]
struct Manifest {
    #[serde(default)]
    files: HashMap<String, ManifestFile>,
    release: Option<String>,
    dist: Option<String>,
}

fn assemble_answer(state: &str, missing: &[String], detail: Option<String>) -> axum::Json<Value> {
    axum::Json(json!({ "state": state, "missingChunks": missing, "detail": detail }))
}

/// `POST .../artifactbundle/assemble/`: builds the bundle from uploaded
/// chunks, verifies it and indexes its files. Assembly is synchronous: the
/// answer is `ok`, `not_found` with the missing chunks, or `error`.
async fn assemble(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
    body: axum::body::Bytes,
) -> CResult<axum::Json<Value>> {
    let req: AssembleRequest = serde_json::from_slice(&body).map_err(|e| bad(format!("invalid request: {e}")))?;
    let checksum = req.checksum.to_lowercase();
    if !is_sha1(&checksum)
        || req.chunks.iter().any(|c| !is_sha1(c))
        || req.chunks.len() > MAX_FILE_SIZE / CHUNK_SIZE + 1
    {
        return Err(bad("checksums must be SHA-1 hex digests"));
    }
    let objects = state.store.objects().clone();
    let st = state.clone();
    let answer = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::UploadArtifacts)?;
            let project_ids = resolve_projects(c, &o, &req.projects)?;
            // Already assembled: just link the projects.
            if let Some(r) = c.query_opt("SELECT id FROM artifact_bundles WHERE org_id = $1 AND checksum = $2", &[&o.id, &checksum])? {
                let bid: i64 = r.get(0);
                for p in &project_ids {
                    c.execute("INSERT INTO artifact_bundle_projects (bundle_id, project_id) VALUES ($1, $2) ON CONFLICT DO NOTHING", &[&bid, p])?;
                }
                return Ok(assemble_answer("ok", &[], None));
            }
            let mut missing = Vec::new();
            let mut keys = Vec::new();
            for ch in &req.chunks {
                let ch = ch.to_lowercase();
                match c.query_opt("SELECT object_key FROM file_chunks WHERE checksum = $1", &[&ch])? {
                    Some(r) => keys.push(r.get::<String>(0)),
                    None => missing.push(ch),
                }
            }
            if !missing.is_empty() {
                return Ok(assemble_answer("not_found", &missing, None));
            }
            let mut data = Vec::new();
            for k in &keys {
                let part = objects.get(k).map_err(ApiError::internal)?;
                let Some(part) = part else { return Ok(assemble_answer("not_found", &req.chunks, None)) };
                data.extend_from_slice(&part);
                if data.len() > MAX_FILE_SIZE {
                    return Ok(assemble_answer("error", &[], Some("the bundle is too large".into())));
                }
            }
            if sha1_hex(&data) != checksum {
                return Ok(assemble_answer("error", &[], Some("the assembled file does not match its checksum".into())));
            }
            let manifest = match read_manifest(&data) {
                Ok(m) => m,
                Err(e) => return Ok(assemble_answer("error", &[], Some(e))),
            };
            let release = req.version.clone().or(manifest.release.clone()).filter(|v| !v.is_empty());
            let dist = req.dist.clone().or(manifest.dist.clone()).filter(|v| !v.is_empty());
            let key = format!("artifact-bundles/{}/{checksum}.zip", o.id);
            objects.put(&key, &data).map_err(ApiError::internal)?;
            let txn = c.transaction()?;
            let bid: i64 = txn
                .query_one(
                    "INSERT INTO artifact_bundles (org_id, checksum, release, dist, object_key, file_count, size)
                     VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
                    &[&o.id, &checksum, &release, &dist, &key, &(manifest.files.len() as i32), &(data.len() as i64)],
                )?
                .get(0);
            for p in &project_ids {
                txn.execute("INSERT INTO artifact_bundle_projects (bundle_id, project_id) VALUES ($1, $2) ON CONFLICT DO NOTHING", &[&bid, p])?;
            }
            for (path, f) in &manifest.files {
                let debug_id = f
                    .headers
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case("debug-id"))
                    .and_then(|(_, v)| uuid::Uuid::parse_str(v.trim()).ok())
                    .map(|u| *u.as_bytes());
                let kind = f.kind.clone().unwrap_or_else(|| "source".into());
                txn.execute(
                    "INSERT INTO artifact_index (org_id, bundle_id, debug_id, release, dist, url, file_path, file_type)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
                    &[&o.id, &bid, &debug_id, &release, &dist, &f.url, path, &kind],
                )?;
            }
            if let Some(v) = &release {
                ensure_release(&txn, o.id, v, &project_ids)?;
            }
            txn.commit()?;
            ApiMetrics::inc(&st.metrics.artifact_bundles);
            Ok(assemble_answer("ok", &[], None))
        })
        .await?;
    Ok(answer)
}

fn read_manifest(data: &[u8]) -> Result<Manifest, String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(data)).map_err(|e| format!("not a zip file: {e}"))?;
    let mut f = zip.by_name("manifest.json").map_err(|_| "the bundle has no manifest.json".to_string())?;
    let mut s = String::new();
    f.by_ref().take(16 << 20).read_to_string(&mut s).map_err(|e| format!("cannot read manifest.json: {e}"))?;
    serde_json::from_str(&s).map_err(|e| format!("invalid manifest.json: {e}"))
}

/// Project slugs (or ids) of the organization; unknown ones are an error.
fn resolve_projects(c: &Connection, o: &OrgCtx, projects: &[String]) -> Result<Vec<i64>, ApiError> {
    let mut ids = Vec::new();
    for p in projects {
        let id = p.parse::<i64>().unwrap_or(-1);
        let row =
            c.query_opt("SELECT id FROM projects WHERE org_id = $1 AND (slug = $2 OR id = $3)", &[&o.id, p, &id])?;
        ids.push(row.ok_or_else(|| ApiError::bad_request(format!("unknown project {p:?}")))?.get(0));
    }
    Ok(ids)
}

/// Creates a release if needed and links projects. Returns (id, created).
fn ensure_release(c: &Connection, org_id: i64, version: &str, projects: &[i64]) -> Result<(i64, bool), ApiError> {
    let (id, created) = match c
        .query_opt("SELECT id FROM releases WHERE org_id = $1 AND version = $2", &[&org_id, &version])?
    {
        Some(r) => (r.get(0), false),
        None => (
            c.query_one("INSERT INTO releases (org_id, version) VALUES ($1, $2) RETURNING id", &[&org_id, &version])?
                .get(0),
            true,
        ),
    };
    for p in projects {
        c.execute(
            "INSERT INTO release_projects (release_id, project_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
            &[&id, p],
        )?;
    }
    Ok((id, created))
}

fn release_json(c: &Connection, id: i64) -> Result<Value, ApiError> {
    let r = c.query_one(
        "SELECT version, ref, url, date_released, created_at, first_event, last_event FROM releases WHERE id = $1",
        &[&id],
    )?;
    let projects: Vec<Value> = c
        .query(
            "SELECT p.slug, p.name, rp.new_issues FROM release_projects rp JOIN projects p ON p.id = rp.project_id WHERE rp.release_id = $1 ORDER BY p.slug",
            &[&id],
        )?
        .iter()
        .map(|p| json!({"slug": p.get::<String>(0), "name": p.get::<String>(1), "newGroups": p.get::<i64>(2)}))
        .collect();
    let deploys: i64 = c.query_one("SELECT count(*) FROM deploys WHERE release_id = $1", &[&id])?.get(0);
    let version: String = r.get(0);
    Ok(json!({
        "id": id,
        "version": version,
        "shortVersion": crate::routes::releases::short_version(&version),
        "ref": r.get::<Option<String>>(1),
        "url": r.get::<Option<String>>(2),
        "dateReleased": r.get::<Option<DateTime<Utc>>>(3),
        "dateCreated": r.get::<DateTime<Utc>>(4),
        "firstEvent": r.get::<Option<DateTime<Utc>>>(5),
        "lastEvent": r.get::<Option<DateTime<Utc>>>(6),
        "newGroups": projects.iter().filter_map(|p| p["newGroups"].as_i64()).sum::<i64>(),
        "projects": projects,
        "commitCount": 0,
        "deployCount": deploys,
        "lastCommit": null,
        "lastDeploy": null,
        "authors": [],
        "data": {},
    }))
}

fn parse_date(v: Option<&Value>) -> Result<Option<DateTime<Utc>>, CompatError> {
    match v.and_then(Value::as_str) {
        None => Ok(None),
        Some(s) => DateTime::parse_from_rfc3339(s)
            .map(|d| Some(d.with_timezone(&Utc)))
            .map_err(|_| bad(format!("invalid date {s:?}"))),
    }
}

fn check_version(v: &str) -> CResult<()> {
    if v.is_empty()
        || v.len() > 250
        || v.contains(['\n', '\r', '\t', '/'])
        || v == "."
        || v == ".."
        || v.eq_ignore_ascii_case("latest")
    {
        return Err(bad("invalid release version"));
    }
    Ok(())
}

/// `POST /api/0/organizations/{org}/releases/`: `{version, projects, ref?, url?, dateReleased?}`.
async fn create_release(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
    body: axum::body::Bytes,
) -> CResult<Response> {
    let b: Value = serde_json::from_slice(&body).map_err(|e| bad(format!("invalid request: {e}")))?;
    let version = b.get("version").and_then(Value::as_str).ok_or_else(|| bad("version is required"))?.to_string();
    check_version(&version)?;
    let projects: Vec<String> = b
        .get("projects")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let released = parse_date(b.get("dateReleased"))?;
    let reference = b.get("ref").and_then(Value::as_str).map(str::to_string);
    let url = b.get("url").and_then(Value::as_str).map(str::to_string);
    let (created, json) = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::WriteReleases)?;
            let ids = resolve_projects(c, &o, &projects)?;
            if ids.is_empty() {
                return Err(ApiError::bad_request("give at least one project"));
            }
            let txn = c.transaction()?;
            let (id, created) = ensure_release(&txn, o.id, &version, &ids)?;
            txn.execute(
                "UPDATE releases SET ref = coalesce($1, ref), url = coalesce($2, url), date_released = coalesce($3, date_released) WHERE id = $4",
                &[&reference, &url, &released, &id],
            )?;
            txn.commit()?;
            Ok((created, release_json(c, id)?))
        })
        .await?;
    Ok((if created { StatusCode::CREATED } else { StatusCode::OK }, axum::Json(json)).into_response())
}

/// The newest 100 releases (of one project if given).
fn list_releases(c: &Connection, org_id: i64, project: Option<i64>) -> Result<Value, ApiError> {
    let ids: Vec<i64> = c
        .query(
            "SELECT r.id FROM releases r WHERE r.org_id = $1
             AND ($2::bigint IS NULL OR EXISTS (SELECT 1 FROM release_projects rp WHERE rp.release_id = r.id AND rp.project_id = $2))
             ORDER BY r.id DESC LIMIT 100",
            &[&org_id, &project],
        )?
        .iter()
        .map(|r| r.get(0))
        .collect();
    Ok(Value::Array(ids.into_iter().map(|id| release_json(c, id)).collect::<Result<_, _>>()?))
}

/// `GET /api/0/organizations/{org}/releases/`.
async fn list_org_releases(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
) -> CResult<axum::Json<Value>> {
    let v = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ReadProject)?;
            list_releases(c, o.id, None)
        })
        .await?;
    Ok(axum::Json(v))
}

/// `GET /api/0/projects/{org}/{project}/releases/`.
async fn list_project_releases(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, project)): Path<(String, String)>,
) -> CResult<axum::Json<Value>> {
    let v = state
        .db(move |c| {
            let p = project_sync(c, &auth, &org, &project, Permission::ReadProject)?;
            list_releases(c, p.org.id, Some(p.id))
        })
        .await?;
    Ok(axum::Json(v))
}

fn release_id(c: &Connection, o: &OrgCtx, version: &str) -> Result<i64, ApiError> {
    Ok(c.query_opt("SELECT id FROM releases WHERE org_id = $1 AND version = $2", &[&o.id, &version])?
        .ok_or_else(ApiError::not_found)?
        .get(0))
}

/// `GET /api/0/organizations/{org}/releases/{version}/`.
async fn get_release(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, version)): Path<(String, String)>,
) -> CResult<axum::Json<Value>> {
    let v = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ReadProject)?;
            let id = release_id(c, &o, &version)?;
            release_json(c, id)
        })
        .await?;
    Ok(axum::Json(v))
}

/// `PUT /api/0/organizations/{org}/releases/{version}/`: `dateReleased`
/// (finalize), `ref`, `url`, `projects`. Commits and refs are ignored.
async fn update_release(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, version)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> CResult<axum::Json<Value>> {
    let b: Value = serde_json::from_slice(&body).map_err(|e| bad(format!("invalid request: {e}")))?;
    let released = parse_date(b.get("dateReleased"))?;
    let reference = b.get("ref").and_then(Value::as_str).map(str::to_string);
    let url = b.get("url").and_then(Value::as_str).map(str::to_string);
    let projects: Vec<String> = b
        .get("projects")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    if b.get("commits").is_some() || b.get("refs").is_some() {
        tracing::info!(version, "release commits and refs are not supported and were ignored");
    }
    let v = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::WriteReleases)?;
            let id = release_id(c, &o, &version)?;
            let ids = resolve_projects(c, &o, &projects)?;
            let txn = c.transaction()?;
            txn.execute(
                "UPDATE releases SET ref = coalesce($1, ref), url = coalesce($2, url), date_released = coalesce($3, date_released) WHERE id = $4",
                &[&reference, &url, &released, &id],
            )?;
            for p in &ids {
                txn.execute("INSERT INTO release_projects (release_id, project_id) VALUES ($1, $2) ON CONFLICT DO NOTHING", &[&id, p])?;
            }
            txn.commit()?;
            release_json(c, id)
        })
        .await?;
    Ok(axum::Json(v))
}

fn deploy_json(r: &rupta_store::squad::Row) -> Value {
    json!({
        "id": r.get::<i64>(0).to_string(),
        "environment": r.get::<String>(1),
        "name": r.get::<Option<String>>(2),
        "url": r.get::<Option<String>>(3),
        "dateStarted": r.get::<Option<DateTime<Utc>>>(4),
        "dateFinished": r.get::<DateTime<Utc>>(5),
    })
}

/// `GET .../releases/{version}/deploys/`.
async fn list_deploys(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, version)): Path<(String, String)>,
) -> CResult<axum::Json<Value>> {
    let v = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ReadProject)?;
            let id = release_id(c, &o, &version)?;
            let rows = c.query(
                "SELECT id, environment, name, url, date_started, date_finished FROM deploys WHERE release_id = $1 ORDER BY date_finished DESC, id DESC",
                &[&id],
            )?;
            Ok(Value::Array(rows.iter().map(deploy_json).collect()))
        })
        .await?;
    Ok(axum::Json(v))
}

/// `POST .../releases/{version}/deploys/`: `{environment, name?, url?, dateStarted?, dateFinished?}`.
async fn create_deploy(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, version)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> CResult<Response> {
    let b: Value = serde_json::from_slice(&body).map_err(|e| bad(format!("invalid request: {e}")))?;
    let env = b
        .get("environment")
        .and_then(Value::as_str)
        .filter(|e| !e.is_empty() && e.len() <= 64)
        .ok_or_else(|| bad("environment is required"))?
        .to_string();
    let name = b.get("name").and_then(Value::as_str).map(str::to_string);
    let url = b.get("url").and_then(Value::as_str).map(str::to_string);
    let started = parse_date(b.get("dateStarted"))?;
    let finished = parse_date(b.get("dateFinished"))?.unwrap_or_else(Utc::now);
    let v = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::WriteReleases)?;
            let id = release_id(c, &o, &version)?;
            let r = c.query_one(
                "INSERT INTO deploys (release_id, environment, name, url, date_started, date_finished) VALUES ($1, $2, $3, $4, $5, $6)
                 RETURNING id, environment, name, url, date_started, date_finished",
                &[&id, &env, &name, &url, &started, &finished],
            )?;
            Ok(deploy_json(&r))
        })
        .await?;
    Ok((StatusCode::CREATED, axum::Json(v)).into_response())
}
