//! The authorization guard: every handler that touches an organization's
//! data resolves it through these functions, which check the caller's role
//! and token scopes with [`rupta_auth::authorize`]. Resources of
//! organizations the caller is not a member of answer `404`, exactly like
//! resources that do not exist.

use rupta_auth::rbac::{Denied, Permission};
use rupta_auth::{authorize, users};
use rupta_core::role::Role;
use rupta_store::squad::Connection;

use crate::error::{ApiError, ApiResult};
use crate::extract::Auth;
use crate::state::ApiState;

/// An organization the caller may act in.
#[derive(Debug, Clone)]
pub struct OrgCtx {
    pub id: i64,
    pub slug: String,
    pub name: String,
    /// The caller's effective role.
    pub role: Role,
}

/// A project the caller may act in.
#[derive(Debug, Clone)]
pub struct ProjectCtx {
    pub org: OrgCtx,
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub platform: Option<String>,
}

fn deny(d: Denied) -> ApiError {
    match d {
        Denied::NotFound => ApiError::not_found(),
        Denied::Forbidden => ApiError::forbidden(),
    }
}

/// Checks `perm` for an organization already loaded.
pub fn check_org(
    conn: &Connection,
    auth: &Auth,
    id: i64,
    slug: String,
    name: String,
    perm: Permission,
) -> ApiResult<OrgCtx> {
    let membership = match auth.principal.user_id {
        Some(uid) => users::membership(conn, id, uid)?,
        None => None,
    };
    let role = authorize(&auth.principal, id, membership, perm).map_err(deny)?;
    Ok(OrgCtx { id, slug, name, role })
}

/// Resolves an organization by slug (blocking).
pub fn org_sync(conn: &Connection, auth: &Auth, slug: &str, perm: Permission) -> ApiResult<OrgCtx> {
    let row = conn.query_opt("SELECT id, slug, name FROM organizations WHERE slug = $1", &[&slug])?;
    let row = row.ok_or_else(ApiError::not_found)?;
    check_org(conn, auth, row.get(0), row.get(1), row.get(2), perm)
}

/// Resolves an organization by id (blocking).
pub fn org_by_id_sync(conn: &Connection, auth: &Auth, id: i64, perm: Permission) -> ApiResult<OrgCtx> {
    let row = conn.query_opt("SELECT id, slug, name FROM organizations WHERE id = $1", &[&id])?;
    let row = row.ok_or_else(ApiError::not_found)?;
    check_org(conn, auth, row.get(0), row.get(1), row.get(2), perm)
}

/// Resolves a project by organization slug and project slug (or numeric id).
pub fn project_sync(
    conn: &Connection,
    auth: &Auth,
    org: &str,
    project: &str,
    perm: Permission,
) -> ApiResult<ProjectCtx> {
    let org = org_sync(conn, auth, org, perm)?;
    let row = match project.parse::<i64>() {
        Ok(id) => conn.query_opt(
            "SELECT id, slug, name, platform FROM projects WHERE org_id = $1 AND (slug = $2 OR id = $3)",
            &[&org.id, &project, &id],
        )?,
        Err(_) => conn.query_opt(
            "SELECT id, slug, name, platform FROM projects WHERE org_id = $1 AND slug = $2",
            &[&org.id, &project],
        )?,
    };
    let row = row.ok_or_else(ApiError::not_found)?;
    Ok(ProjectCtx { org, id: row.get(0), slug: row.get(1), name: row.get(2), platform: row.get(3) })
}

/// Resolves a project by id (blocking).
pub fn project_by_id_sync(conn: &Connection, auth: &Auth, project_id: i64, perm: Permission) -> ApiResult<ProjectCtx> {
    let row = conn.query_opt(
        "SELECT p.id, p.slug, p.name, p.platform, o.id, o.slug, o.name
         FROM projects p JOIN organizations o ON o.id = p.org_id WHERE p.id = $1",
        &[&project_id],
    )?;
    let row = row.ok_or_else(ApiError::not_found)?;
    let org = check_org(conn, auth, row.get(4), row.get(5), row.get(6), perm)?;
    Ok(ProjectCtx { org, id: row.get(0), slug: row.get(1), name: row.get(2), platform: row.get(3) })
}

/// Resolves the project of an issue (blocking).
pub fn issue_sync(conn: &Connection, auth: &Auth, issue_id: i64, perm: Permission) -> ApiResult<ProjectCtx> {
    let pid: i64 = conn
        .query_opt("SELECT project_id FROM issues WHERE id = $1", &[&issue_id])?
        .ok_or_else(ApiError::not_found)?
        .get(0);
    project_by_id_sync(conn, auth, pid, perm)
}

impl ApiState {
    pub async fn org(&self, auth: &Auth, slug: &str, perm: Permission) -> ApiResult<OrgCtx> {
        let (auth, slug) = (auth.clone(), slug.to_string());
        self.db(move |c| org_sync(c, &auth, &slug, perm)).await
    }

    pub async fn project(&self, auth: &Auth, org: &str, project: &str, perm: Permission) -> ApiResult<ProjectCtx> {
        let (auth, org, project) = (auth.clone(), org.to_string(), project.to_string());
        self.db(move |c| project_sync(c, &auth, &org, &project, perm)).await
    }

    pub async fn issue_project(&self, auth: &Auth, issue_id: i64, perm: Permission) -> ApiResult<ProjectCtx> {
        let auth = auth.clone();
        self.db(move |c| issue_sync(c, &auth, issue_id, perm)).await
    }
}
