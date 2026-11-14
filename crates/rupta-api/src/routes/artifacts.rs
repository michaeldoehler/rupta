//! Source map artifact bundles of a project (uploaded with the upload CLI
//! through `/api/0`): listing and deletion. Bundles belong to the
//! organization and are linked to the projects they were uploaded for.

use axum::extract::State;
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use rupta_auth::audit::{self, action};
use rupta_auth::rbac::Permission;
use rupta_core::ids::Id;
use serde::Serialize;
use serde_json::json;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::access::project_sync;
use crate::error::{ApiError, ApiResult};
use crate::extract::{Auth, Page, PageParams, Path, Query};
use crate::state::ApiState;

pub fn routes() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new().routes(routes!(list_bundles)).routes(routes!(delete_bundle))
}

#[derive(Serialize, ToSchema)]
pub struct ArtifactBundle {
    #[schema(value_type = String)]
    pub id: Id,
    /// SHA-1 of the bundle zip.
    pub checksum: String,
    pub release: Option<String>,
    pub dist: Option<String>,
    /// Files in the bundle's manifest.
    pub file_count: i64,
    /// Bytes of the zip.
    pub size: i64,
    pub created_at: DateTime<Utc>,
    /// Distinct debug ids of the bundle's files.
    pub debug_ids_count: i64,
}

/// Artifact bundles linked to the project, newest first.
#[utoipa::path(get, path = "/api/v1/projects/{org}/{project}/artifact-bundles", tag = "projects",
    params(("org" = String, Path), ("project" = String, Path), PageParams),
    responses((status = 200, body = Page<ArtifactBundle>)))]
async fn list_bundles(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, project)): Path<(String, String)>,
    Query(page): Query<PageParams>,
) -> ApiResult<axum::Json<Page<ArtifactBundle>>> {
    let before: Option<i64> = page.cursor()?;
    let limit = page.limit();
    let bundles = state
        .db(move |c| {
            let p = project_sync(c, &auth, &org, &project, Permission::ReadProject)?;
            let rows = c.query(
                "SELECT b.id, b.checksum, b.release, b.dist, b.file_count, b.size, b.created_at
                 FROM artifact_bundles b JOIN artifact_bundle_projects bp ON bp.bundle_id = b.id
                 WHERE bp.project_id = $1 AND b.org_id = $2 AND ($3::bigint IS NULL OR b.id < $3)
                 ORDER BY b.id DESC LIMIT $4",
                &[&p.id, &p.org.id, &before, &(limit + 1)],
            )?;
            let mut out = Vec::with_capacity(rows.len());
            for r in &rows {
                let id: i64 = r.get(0);
                let debug_ids: i64 = c
                    .query_one(
                        "SELECT count(DISTINCT debug_id) FROM artifact_index WHERE bundle_id = $1 AND debug_id IS NOT NULL",
                        &[&id],
                    )?
                    .get(0);
                out.push(ArtifactBundle {
                    id: Id(id),
                    checksum: r.get(1),
                    release: r.get(2),
                    dist: r.get(3),
                    file_count: i64::from(r.get::<i32>(4)),
                    size: r.get(5),
                    created_at: r.get(6),
                    debug_ids_count: debug_ids,
                });
            }
            Ok(out)
        })
        .await?;
    Ok(axum::Json(Page::from_rows(bundles, limit, |b| b.id.0)))
}

/// Removes a bundle from the project (admins). When no other project
/// uses the bundle, its index rows, the bundle and its stored zip are
/// deleted too, so its source maps no longer symbolicate new events.
#[utoipa::path(delete, path = "/api/v1/projects/{org}/{project}/artifact-bundles/{id}", tag = "projects",
    params(("org" = String, Path), ("project" = String, Path), ("id" = String, Path)),
    responses((status = 204), (status = 404, body = crate::error::ErrorBody)))]
async fn delete_bundle(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, project, id)): Path<(String, String, i64)>,
) -> ApiResult<StatusCode> {
    let objects = state.store.objects().clone();
    let orphan = state
        .db(move |c| {
            let p = project_sync(c, &auth, &org, &project, Permission::WriteProject)?;
            let txn = c.transaction()?;
            let row = txn
                .query_opt(
                    "SELECT b.object_key, b.checksum, b.release FROM artifact_bundles b
                     JOIN artifact_bundle_projects bp ON bp.bundle_id = b.id
                     WHERE b.id = $1 AND b.org_id = $2 AND bp.project_id = $3",
                    &[&id, &p.org.id, &p.id],
                )?
                .ok_or_else(ApiError::not_found)?;
            let (key, checksum, release): (String, String, Option<String>) = (row.get(0), row.get(1), row.get(2));
            txn.execute("DELETE FROM artifact_bundle_projects WHERE bundle_id = $1 AND project_id = $2", &[&id, &p.id])?;
            let others: i64 =
                txn.query_one("SELECT count(*) FROM artifact_bundle_projects WHERE bundle_id = $1", &[&id])?.get(0);
            let orphan = others == 0;
            if orphan {
                txn.execute("DELETE FROM artifact_index WHERE bundle_id = $1", &[&id])?;
                txn.execute("DELETE FROM artifact_bundles WHERE id = $1", &[&id])?;
            }
            audit::record(
                &txn,
                &audit::Entry {
                    target_type: Some("artifact_bundle"),
                    target_id: Some(id.to_string()),
                    data: json!({"project": p.slug, "checksum": checksum, "release": release, "bundle_deleted": orphan}),
                    ..auth.audit(Some(p.org.id), action::ARTIFACT_BUNDLE_DELETE)
                },
            )?;
            txn.commit()?;
            Ok(orphan.then_some(key))
        })
        .await?;
    if let Some(key) = orphan {
        // After the commit: a failed delete leaves an unreferenced object, not a dangling row.
        let result = tokio::task::spawn_blocking(move || objects.delete(&key)).await.map_err(ApiError::internal)?;
        if let Err(e) = result {
            tracing::warn!(error = %e, "deleting an artifact bundle object failed");
        }
    }
    Ok(StatusCode::NO_CONTENT)
}
