//! Organizations, members, invitations and teams.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Duration, Utc};
use rupta_auth::audit::{self, action};
use rupta_auth::rbac::{Permission, may_manage_role};
use rupta_auth::{crypto, password, session, users};
use rupta_core::ids::Id;
use rupta_core::role::Role;
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::access::org_sync;
use crate::error::{ApiError, ApiResult};
use crate::extract::{Auth, ClientIp, Json, MaybeAuth, Page, PageParams, Path, Query};
use crate::mail::html_escape;
use crate::routes::auth::{SessionInfo, session_info};
use crate::state::ApiState;

pub fn routes() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(list_orgs, create_org))
        .routes(routes!(get_org, update_org, delete_org))
        .routes(routes!(list_members, invite_member))
        .routes(routes!(update_member, remove_member))
        .routes(routes!(list_invites))
        .routes(routes!(cancel_invite))
        .routes(routes!(get_invite))
        .routes(routes!(accept_invite))
        .routes(routes!(list_teams, create_team))
        .routes(routes!(update_team, delete_team))
        .routes(routes!(add_team_member, remove_team_member))
}

#[derive(Serialize, ToSchema)]
pub struct Org {
    #[schema(value_type = String)]
    pub id: Id,
    pub slug: String,
    pub name: String,
    /// The caller's role.
    pub role: String,
    pub created_at: DateTime<Utc>,
}

pub(crate) fn check_name(field: &str, name: &str, max: usize) -> ApiResult<String> {
    let n = name.trim();
    if n.is_empty() || n.chars().count() > max || n.chars().any(char::is_control) {
        return Err(ApiError::invalid(field, format!("must have 1 to {max} characters")));
    }
    Ok(n.to_string())
}

pub(crate) fn check_slug(field: &str, slug: &str) -> ApiResult<()> {
    if !users::valid_slug(slug) {
        return Err(ApiError::invalid(
            field,
            "use 1 to 50 lowercase letters, digits and dashes, not starting with a dash",
        ));
    }
    Ok(())
}

/// Organizations of the caller (all of them for superusers).
#[utoipa::path(get, path = "/api/v1/orgs", tag = "orgs", responses((status = 200, body = Vec<Org>)))]
async fn list_orgs(State(state): State<ApiState>, auth: Auth) -> ApiResult<axum::Json<Vec<Org>>> {
    let rows = state
        .db(move |c| {
            let rows = match (&auth.principal.token, auth.principal.user_id) {
                (Some(t), _) if t.org_id.is_some() => c.query(
                    "SELECT id, slug, name, 'admin', created_at FROM organizations WHERE id = $1",
                    &[&t.org_id],
                )?,
                (_, Some(uid)) if auth.principal.is_superuser => c.query(
                    "SELECT o.id, o.slug, o.name, coalesce(m.role, 'owner'), o.created_at FROM organizations o
                     LEFT JOIN members m ON m.org_id = o.id AND m.user_id = $1 ORDER BY o.name, o.id",
                    &[&uid],
                )?,
                (_, Some(uid)) => c.query(
                    "SELECT o.id, o.slug, o.name, m.role, o.created_at FROM members m JOIN organizations o ON o.id = m.org_id
                     WHERE m.user_id = $1 ORDER BY o.name, o.id",
                    &[&uid],
                )?,
                _ => Vec::new(),
            };
            if !auth.principal.has_scope(Permission::ReadOrg.scopes()) {
                return Err(ApiError::forbidden());
            }
            Ok(rows)
        })
        .await?;
    Ok(axum::Json(
        rows.iter()
            .map(|r| Org { id: Id(r.get(0)), slug: r.get(1), name: r.get(2), role: r.get(3), created_at: r.get(4) })
            .collect(),
    ))
}

#[derive(Deserialize, ToSchema)]
pub struct CreateOrg {
    pub name: String,
    /// Derived from the name if omitted.
    pub slug: Option<String>,
}

/// Creates an organization; the caller becomes its owner. Needs a session
/// or a user token with `org:admin`.
#[utoipa::path(post, path = "/api/v1/orgs", tag = "orgs", request_body = CreateOrg,
    responses((status = 201, body = Org), (status = 409, body = crate::error::ErrorBody)))]
async fn create_org(State(state): State<ApiState>, auth: Auth, Json(req): Json<CreateOrg>) -> ApiResult<Response> {
    let uid = auth.require_user()?;
    if !auth.principal.has_scope(&["org:admin"]) {
        return Err(ApiError::forbidden());
    }
    let name = check_name("name", &req.name, 100)?;
    let slug = req.slug.unwrap_or_else(|| users::slugify(&name));
    check_slug("slug", &slug)?;
    let org = state
        .db(move |c| {
            let txn = c.transaction()?;
            let id = users::create_org(&txn, &slug, &name).map_err(|e| {
                if e.is_unique_violation() {
                    ApiError::conflict("an organization with this slug exists")
                } else {
                    e.into()
                }
            })?;
            users::set_member(&txn, id, uid, Role::Owner)?;
            audit::record(
                &txn,
                &audit::Entry {
                    target_type: Some("org"),
                    target_id: Some(id.to_string()),
                    data: json!({"slug": slug}),
                    ..auth.audit(Some(id), action::ORG_CREATE)
                },
            )?;
            let created_at: DateTime<Utc> =
                txn.query_one("SELECT created_at FROM organizations WHERE id = $1", &[&id])?.get(0);
            txn.commit()?;
            Ok(Org { id: Id(id), slug, name, role: "owner".into(), created_at })
        })
        .await?;
    Ok((StatusCode::CREATED, axum::Json(org)).into_response())
}

/// An organization.
#[utoipa::path(get, path = "/api/v1/orgs/{org}", tag = "orgs", params(("org" = String, Path)),
    responses((status = 200, body = Org), (status = 404, body = crate::error::ErrorBody)))]
async fn get_org(State(state): State<ApiState>, auth: Auth, Path(org): Path<String>) -> ApiResult<axum::Json<Org>> {
    let o = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ReadOrg)?;
            let created_at: DateTime<Utc> =
                c.query_one("SELECT created_at FROM organizations WHERE id = $1", &[&o.id])?.get(0);
            Ok(Org { id: Id(o.id), slug: o.slug, name: o.name, role: o.role.as_str().into(), created_at })
        })
        .await?;
    Ok(axum::Json(o))
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateOrg {
    pub name: Option<String>,
    pub slug: Option<String>,
}

/// Renames an organization or changes its slug.
#[utoipa::path(patch, path = "/api/v1/orgs/{org}", tag = "orgs", params(("org" = String, Path)), request_body = UpdateOrg,
    responses((status = 200, body = Org)))]
async fn update_org(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
    Json(req): Json<UpdateOrg>,
) -> ApiResult<axum::Json<Org>> {
    let name = req.name.as_deref().map(|n| check_name("name", n, 100)).transpose()?;
    if let Some(s) = &req.slug {
        check_slug("slug", s)?;
    }
    let o = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ManageOrg)?;
            let row = c.query_one(
                "UPDATE organizations SET name = coalesce($1, name), slug = coalesce($2, slug) WHERE id = $3 RETURNING slug, name, created_at",
                &[&name, &req.slug, &o.id],
            )?;
            audit::record(
                c,
                &audit::Entry { target_type: Some("org"), target_id: Some(o.id.to_string()), data: json!({"name": name, "slug": req.slug}), ..auth.audit(Some(o.id), action::ORG_UPDATE) },
            )?;
            Ok(Org { id: Id(o.id), slug: row.get(0), name: row.get(1), role: o.role.as_str().into(), created_at: row.get(2) })
        })
        .await?;
    Ok(axum::Json(o))
}

/// Deletes an organization with all its projects and data. Owners only.
#[utoipa::path(delete, path = "/api/v1/orgs/{org}", tag = "orgs", params(("org" = String, Path)), responses((status = 204)))]
async fn delete_org(State(state): State<ApiState>, auth: Auth, Path(org): Path<String>) -> ApiResult<StatusCode> {
    let projects = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::DeleteOrg)?;
            let projects: Vec<i64> =
                c.query("SELECT id FROM projects WHERE org_id = $1", &[&o.id])?.iter().map(|r| r.get(0)).collect();
            let txn = c.transaction()?;
            txn.execute("DELETE FROM organizations WHERE id = $1", &[&o.id])?;
            audit::record(
                &txn,
                &audit::Entry {
                    target_type: Some("org"),
                    target_id: Some(o.id.to_string()),
                    data: json!({"slug": o.slug}),
                    ..auth.audit(None, action::ORG_DELETE)
                },
            )?;
            txn.commit()?;
            Ok(projects)
        })
        .await?;
    for p in projects {
        crate::routes::projects::purge_project_data(&state, p).await;
    }
    state.store.bump_config();
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, ToSchema)]
pub struct Member {
    #[schema(value_type = String)]
    pub user_id: Id,
    pub email: String,
    pub name: String,
    pub avatar_url: Option<String>,
    pub role: String,
    pub created_at: DateTime<Utc>,
    /// Slugs of the member's teams.
    pub teams: Vec<String>,
    /// Signed in with single sign-on at least once.
    pub sso_linked: bool,
}

/// Members of an organization, ordered by email.
#[utoipa::path(get, path = "/api/v1/orgs/{org}/members", tag = "orgs", params(("org" = String, Path), PageParams),
    responses((status = 200, body = Page<Member>)))]
async fn list_members(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
    Query(page): Query<PageParams>,
) -> ApiResult<axum::Json<Page<Member>>> {
    let after: Option<String> = page.cursor()?;
    let limit = page.limit();
    let rows = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ReadMembers)?;
            let rows = c.query(
                "SELECT u.id, u.email, u.name, u.avatar_url, m.role, m.created_at,
                        (SELECT count(*) FROM identities i WHERE i.user_id = u.id) > 0
                 FROM members m JOIN users u ON u.id = m.user_id
                 WHERE m.org_id = $1 AND ($2::text IS NULL OR u.email > $2)
                 ORDER BY u.email LIMIT $3",
                &[&o.id, &after, &(limit + 1)],
            )?;
            let mut out = Vec::new();
            for r in rows {
                let uid: i64 = r.get(0);
                let teams = c
                    .query(
                        "SELECT t.slug FROM team_members tm JOIN teams t ON t.id = tm.team_id WHERE t.org_id = $1 AND tm.user_id = $2 ORDER BY t.slug",
                        &[&o.id, &uid],
                    )?
                    .iter()
                    .map(|t| t.get(0))
                    .collect();
                out.push(Member {
                    user_id: Id(uid),
                    email: r.get(1),
                    name: r.get(2),
                    avatar_url: r.get(3),
                    role: r.get(4),
                    created_at: r.get(5),
                    teams,
                    sso_linked: r.get(6),
                });
            }
            Ok(out)
        })
        .await?;
    Ok(axum::Json(Page::from_rows(rows, limit, |m| m.email.clone())))
}

#[derive(Deserialize, ToSchema)]
pub struct InviteRequest {
    pub email: String,
    /// `viewer`, `member`, `admin` or `owner`.
    pub role: String,
}

#[derive(Serialize, ToSchema)]
pub struct Invite {
    #[schema(value_type = String)]
    pub id: Id,
    pub email: String,
    pub role: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    /// Whether the invitation email was sent (needs SMTP).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email_sent: Option<bool>,
    /// The acceptance link; returned only when it was just created, so an
    /// admin can pass it on when email is not configured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accept_url: Option<String>,
}

const INVITE_DAYS: i64 = 7;

/// Invites someone by email. The invitation link is valid for 7 days.
#[utoipa::path(post, path = "/api/v1/orgs/{org}/members", tag = "orgs", params(("org" = String, Path)), request_body = InviteRequest,
    responses((status = 201, body = Invite), (status = 409, body = crate::error::ErrorBody)))]
async fn invite_member(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
    Json(req): Json<InviteRequest>,
) -> ApiResult<Response> {
    let email =
        users::normalize_email(&req.email).ok_or_else(|| ApiError::invalid("email", "not a valid email address"))?;
    let role =
        Role::parse(&req.role).ok_or_else(|| ApiError::invalid("role", "must be viewer, member, admin or owner"))?;
    let token = crypto::random_token();
    let token_hash = crypto::sha256_hex(token.as_bytes());
    let expires_at = Utc::now() + Duration::days(INVITE_DAYS);
    let (invite, org_name) = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ManageMembers)?;
            if !may_manage_role(o.role, role) {
                return Err(ApiError::forbidden().with_details(json!({"reason": "only owners can invite owners"})));
            }
            let already = c.query_opt(
                "SELECT 1 FROM members m JOIN users u ON u.id = m.user_id WHERE m.org_id = $1 AND u.email = $2",
                &[&o.id, &email],
            )?;
            if already.is_some() {
                return Err(ApiError::conflict("this person is already a member"));
            }
            let txn = c.transaction()?;
            // A new invitation replaces an older one for the same email.
            txn.execute("DELETE FROM invitations WHERE org_id = $1 AND email = $2", &[&o.id, &email])?;
            let row = txn.query_one(
                "INSERT INTO invitations (org_id, email, role, token_hash, invited_by, expires_at)
                 VALUES ($1, $2, $3, $4, $5, $6) RETURNING id, created_at",
                &[&o.id, &email, &role.as_str(), &token_hash, &auth.user_id(), &expires_at],
            )?;
            let id: i64 = row.get(0);
            audit::record(
                &txn,
                &audit::Entry {
                    target_type: Some("invitation"),
                    target_id: Some(id.to_string()),
                    data: json!({"email": email, "role": role.as_str()}),
                    ..auth.audit(Some(o.id), action::MEMBER_INVITE)
                },
            )?;
            txn.commit()?;
            Ok((
                Invite {
                    id: Id(id),
                    email,
                    role: role.as_str().into(),
                    created_at: row.get(1),
                    expires_at,
                    email_sent: None,
                    accept_url: None,
                },
                o.name,
            ))
        })
        .await?;
    let url = format!("{}/accept-invite/{token}", state.config.public_url);
    let sent = match &state.mailer {
        Some(m) => {
            let text = format!(
                "You have been invited to join {org_name} on rupta as {role}.\n\nAccept the invitation: {url}\n\nThe link is valid for {INVITE_DAYS} days.\n",
                role = invite.role
            );
            let html = format!(
                "<p>You have been invited to join <strong>{}</strong> on rupta as {}.</p><p><a href=\"{}\">Accept the invitation</a></p><p>The link is valid for {INVITE_DAYS} days.</p>",
                html_escape(&org_name),
                html_escape(&invite.role),
                html_escape(&url)
            );
            match m
                .send(
                    std::slice::from_ref(&invite.email),
                    &format!("Invitation to {org_name} on rupta"),
                    &text,
                    Some(&html),
                )
                .await
            {
                Ok(()) => true,
                Err(e) => {
                    tracing::warn!(error = %e, "sending the invitation email failed");
                    false
                }
            }
        }
        None => false,
    };
    let invite = Invite { email_sent: Some(sent), accept_url: Some(url), ..invite };
    Ok((StatusCode::CREATED, axum::Json(invite)).into_response())
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateMember {
    pub role: String,
}

/// Changes a member's role. Admins cannot touch owners; the last owner
/// cannot be demoted.
#[utoipa::path(patch, path = "/api/v1/orgs/{org}/members/{user_id}", tag = "orgs",
    params(("org" = String, Path), ("user_id" = String, Path)), request_body = UpdateMember,
    responses((status = 200, body = Member)))]
async fn update_member(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, user_id)): Path<(String, i64)>,
    Json(req): Json<UpdateMember>,
) -> ApiResult<axum::Json<Member>> {
    let role =
        Role::parse(&req.role).ok_or_else(|| ApiError::invalid("role", "must be viewer, member, admin or owner"))?;
    let m = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ManageMembers)?;
            let txn = c.transaction()?;
            let current = users::membership(&txn, o.id, user_id)?.ok_or_else(ApiError::not_found)?;
            if !may_manage_role(o.role, current) || !may_manage_role(o.role, role) {
                return Err(ApiError::forbidden());
            }
            if current == Role::Owner && role != Role::Owner && users::owner_count(&txn, o.id)? <= 1 {
                return Err(ApiError::conflict("an organization needs at least one owner"));
            }
            users::set_member(&txn, o.id, user_id, role)?;
            audit::record(
                &txn,
                &audit::Entry { target_type: Some("user"), target_id: Some(user_id.to_string()), data: json!({"old_role": current.as_str(), "role": role.as_str()}), ..auth.audit(Some(o.id), action::MEMBER_ROLE_CHANGE) },
            )?;
            let r = txn.query_one(
                "SELECT u.id, u.email, u.name, u.avatar_url, m.role, m.created_at, (SELECT count(*) FROM identities i WHERE i.user_id = u.id) > 0
                 FROM members m JOIN users u ON u.id = m.user_id WHERE m.org_id = $1 AND m.user_id = $2",
                &[&o.id, &user_id],
            )?;
            txn.commit()?;
            Ok(Member { user_id: Id(r.get(0)), email: r.get(1), name: r.get(2), avatar_url: r.get(3), role: r.get(4), created_at: r.get(5), teams: vec![], sso_linked: r.get(6) })
        })
        .await?;
    Ok(axum::Json(m))
}

/// Removes a member (or leaves, when it is the caller). The last owner
/// cannot be removed.
#[utoipa::path(delete, path = "/api/v1/orgs/{org}/members/{user_id}", tag = "orgs",
    params(("org" = String, Path), ("user_id" = String, Path)), responses((status = 204)))]
async fn remove_member(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, user_id)): Path<(String, i64)>,
) -> ApiResult<StatusCode> {
    state
        .db(move |c| {
            let leaving = auth.user_id() == Some(user_id);
            let o = org_sync(c, &auth, &org, if leaving { Permission::ReadOrg } else { Permission::ManageMembers })?;
            let txn = c.transaction()?;
            let current = users::membership(&txn, o.id, user_id)?.ok_or_else(ApiError::not_found)?;
            if !leaving && !may_manage_role(o.role, current) {
                return Err(ApiError::forbidden());
            }
            if current == Role::Owner && users::owner_count(&txn, o.id)? <= 1 {
                return Err(ApiError::conflict("an organization needs at least one owner"));
            }
            txn.execute("DELETE FROM members WHERE org_id = $1 AND user_id = $2", &[&o.id, &user_id])?;
            txn.execute(
                "DELETE FROM team_members WHERE user_id = $1 AND team_id IN (SELECT id FROM teams WHERE org_id = $2)",
                &[&user_id, &o.id],
            )?;
            txn.execute(
                "UPDATE issues SET assignee_user_id = NULL WHERE assignee_user_id = $1 AND project_id IN (SELECT id FROM projects WHERE org_id = $2)",
                &[&user_id, &o.id],
            )?;
            audit::record(
                &txn,
                &audit::Entry { target_type: Some("user"), target_id: Some(user_id.to_string()), data: json!({"role": current.as_str(), "left": leaving}), ..auth.audit(Some(o.id), action::MEMBER_REMOVE) },
            )?;
            txn.commit()?;
            Ok(())
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Pending invitations.
#[utoipa::path(get, path = "/api/v1/orgs/{org}/invites", tag = "orgs", params(("org" = String, Path)),
    responses((status = 200, body = Vec<Invite>)))]
async fn list_invites(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
) -> ApiResult<axum::Json<Vec<Invite>>> {
    let rows = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ManageMembers)?;
            Ok(c.query(
                "SELECT id, email, role, created_at, expires_at FROM invitations WHERE org_id = $1 AND expires_at > now() ORDER BY email",
                &[&o.id],
            )?)
        })
        .await?;
    Ok(axum::Json(
        rows.iter()
            .map(|r| Invite {
                id: Id(r.get(0)),
                email: r.get(1),
                role: r.get(2),
                created_at: r.get(3),
                expires_at: r.get(4),
                email_sent: None,
                accept_url: None,
            })
            .collect(),
    ))
}

/// Withdraws an invitation.
#[utoipa::path(delete, path = "/api/v1/orgs/{org}/invites/{id}", tag = "orgs",
    params(("org" = String, Path), ("id" = String, Path)), responses((status = 204)))]
async fn cancel_invite(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, id)): Path<(String, i64)>,
) -> ApiResult<StatusCode> {
    state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ManageMembers)?;
            let n = c.execute("DELETE FROM invitations WHERE id = $1 AND org_id = $2", &[&id, &o.id])?;
            if n == 0 {
                return Err(ApiError::not_found());
            }
            audit::record(
                c,
                &audit::Entry {
                    target_type: Some("invitation"),
                    target_id: Some(id.to_string()),
                    ..auth.audit(Some(o.id), action::MEMBER_INVITE_CANCEL)
                },
            )?;
            Ok(())
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, ToSchema)]
pub struct InviteInfo {
    pub org_slug: String,
    pub org_name: String,
    pub email: String,
    pub role: String,
    pub expires_at: DateTime<Utc>,
    /// An account with this email exists (log in to accept), otherwise
    /// one can be created when accepting (if password login is enabled).
    pub user_exists: bool,
}

fn find_invite(c: &rupta_store::squad::Connection, token: &str) -> ApiResult<rupta_store::squad::Row> {
    let hash = crypto::sha256_hex(token.as_bytes());
    let row = c.query_opt(
        "SELECT i.id, i.org_id, o.slug, o.name, i.email, i.role, i.expires_at
         FROM invitations i JOIN organizations o ON o.id = i.org_id WHERE i.token_hash = $1",
        &[&hash],
    )?;
    let row = row.ok_or_else(ApiError::not_found)?;
    if row.get::<DateTime<Utc>>(6) <= Utc::now() {
        return Err(ApiError::not_found());
    }
    Ok(row)
}

/// What an invitation link is for. No authentication needed.
#[utoipa::path(get, path = "/api/v1/invites/{token}", tag = "orgs", params(("token" = String, Path)),
    responses((status = 200, body = InviteInfo), (status = 404, body = crate::error::ErrorBody)))]
async fn get_invite(State(state): State<ApiState>, Path(token): Path<String>) -> ApiResult<axum::Json<InviteInfo>> {
    let info = state
        .db(move |c| {
            let r = find_invite(c, &token)?;
            let email: String = r.get(4);
            let user_exists = users::find_by_email(c, &email)?.is_some();
            Ok(InviteInfo {
                org_slug: r.get(2),
                org_name: r.get(3),
                email,
                role: r.get(5),
                expires_at: r.get(6),
                user_exists,
            })
        })
        .await?;
    Ok(axum::Json(info))
}

#[derive(Deserialize, ToSchema, Default)]
pub struct AcceptInvite {
    /// For a new account: name and password.
    pub name: Option<String>,
    pub password: Option<String>,
}

/// Accepts an invitation: as the signed-in user, or by creating an account
/// for the invited email (password login must be enabled).
#[utoipa::path(post, path = "/api/v1/invites/{token}/accept", tag = "orgs", params(("token" = String, Path)), request_body = AcceptInvite,
    responses((status = 200, body = SessionInfo)))]
async fn accept_invite(
    State(state): State<ApiState>,
    MaybeAuth(auth): MaybeAuth,
    ip: ClientIp,
    Path(token): Path<String>,
    body: axum::body::Bytes,
) -> ApiResult<Response> {
    let req: AcceptInvite = if body.iter().all(u8::is_ascii_whitespace) {
        AcceptInvite::default()
    } else {
        serde_json::from_slice(&body).map_err(|e| ApiError::bad_request(e.to_string()))?
    };
    let new_account = match (&auth, &req.password) {
        (Some(a), _) => {
            a.require_user()?;
            None
        }
        (None, Some(pw)) => {
            if !state.config.local_login {
                return Err(ApiError::new(
                    StatusCode::FORBIDDEN,
                    "local_login_disabled",
                    "sign in with single sign-on to accept",
                ));
            }
            password::check_policy(pw).map_err(|e| ApiError::invalid("password", e))?;
            let pw = pw.clone();
            Some(tokio::task::spawn_blocking(move || password::hash(&pw)).await.map_err(ApiError::internal)?)
        }
        (None, None) => return Err(ApiError::unauthorized()),
    };
    let name = req.name.unwrap_or_default().trim().chars().take(100).collect::<String>();
    let ipv = ip.string();
    let (session_token, info) = state
        .db(move |c| {
            let txn = c.transaction()?;
            let r = find_invite(&txn, &token)?;
            let (invite_id, org_id, email, role): (i64, i64, String, String) = (r.get(0), r.get(1), r.get(4), r.get(5));
            let role = Role::parse(&role).unwrap_or(Role::Member);
            let (user_id, new_session) = match (&auth, new_account) {
                (Some(a), _) => (a.user_id().expect("checked"), None),
                (None, Some(hash)) => {
                    if users::find_by_email(&txn, &email)?.is_some() {
                        return Err(ApiError::conflict("an account with this email exists; log in to accept"));
                    }
                    let uid = users::create(&txn, &email, &name, Some(&hash), false)?;
                    (uid, Some(session::create(&txn, uid, ipv.as_deref(), None)?))
                }
                (None, None) => unreachable!(),
            };
            if users::membership(&txn, org_id, user_id)?.is_none() {
                users::set_member(&txn, org_id, user_id, role)?;
            }
            txn.execute("DELETE FROM invitations WHERE id = $1", &[&invite_id])?;
            audit::record(
                &txn,
                &audit::Entry {
                    org_id: Some(org_id),
                    actor_user_id: Some(user_id),
                    action: action::MEMBER_JOIN,
                    target_type: Some("user"),
                    target_id: Some(user_id.to_string()),
                    data: json!({"role": role.as_str(), "invitation": invite_id}),
                    ip: ipv.clone(),
                    ..Default::default()
                },
            )?;
            txn.commit()?;
            let csrf = match &new_session {
                Some(s) => Some(s.csrf_token.clone()),
                None => auth.as_ref().and_then(|a| a.csrf_token.clone()),
            };
            Ok((new_session.map(|s| s.token), session_info(c, user_id, csrf)?))
        })
        .await?;
    let mut resp = axum::Json(info).into_response();
    if let Some(t) = session_token {
        resp.headers_mut().append(
            axum::http::header::SET_COOKIE,
            axum::http::HeaderValue::from_str(&session::cookie(&t, state.secure_cookies())).unwrap(),
        );
    }
    Ok(resp)
}

#[derive(Serialize, ToSchema)]
pub struct TeamMember {
    #[schema(value_type = String)]
    pub user_id: Id,
    pub email: String,
    pub name: String,
}

#[derive(Serialize, ToSchema)]
pub struct Team {
    #[schema(value_type = String)]
    pub id: Id,
    pub slug: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub members: Vec<TeamMember>,
}

fn load_team(c: &rupta_store::squad::Connection, org_id: i64, slug: &str) -> ApiResult<Team> {
    let r = c
        .query_opt("SELECT id, slug, name, created_at FROM teams WHERE org_id = $1 AND slug = $2", &[&org_id, &slug])?
        .ok_or_else(ApiError::not_found)?;
    let id: i64 = r.get(0);
    let members = c
        .query(
            "SELECT u.id, u.email, u.name FROM team_members tm JOIN users u ON u.id = tm.user_id WHERE tm.team_id = $1 ORDER BY u.email",
            &[&id],
        )?
        .iter()
        .map(|m| TeamMember { user_id: Id(m.get(0)), email: m.get(1), name: m.get(2) })
        .collect();
    Ok(Team { id: Id(id), slug: r.get(1), name: r.get(2), created_at: r.get(3), members })
}

/// Teams of an organization with their members.
#[utoipa::path(get, path = "/api/v1/orgs/{org}/teams", tag = "orgs", params(("org" = String, Path)),
    responses((status = 200, body = Vec<Team>)))]
async fn list_teams(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
) -> ApiResult<axum::Json<Vec<Team>>> {
    let teams = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ReadMembers)?;
            let slugs: Vec<String> = c
                .query("SELECT slug FROM teams WHERE org_id = $1 ORDER BY slug", &[&o.id])?
                .iter()
                .map(|r| r.get(0))
                .collect();
            slugs.iter().map(|s| load_team(c, o.id, s)).collect::<ApiResult<Vec<_>>>()
        })
        .await?;
    Ok(axum::Json(teams))
}

#[derive(Deserialize, ToSchema)]
pub struct CreateTeam {
    pub name: String,
    pub slug: Option<String>,
}

/// Creates a team.
#[utoipa::path(post, path = "/api/v1/orgs/{org}/teams", tag = "orgs", params(("org" = String, Path)), request_body = CreateTeam,
    responses((status = 201, body = Team)))]
async fn create_team(
    State(state): State<ApiState>,
    auth: Auth,
    Path(org): Path<String>,
    Json(req): Json<CreateTeam>,
) -> ApiResult<Response> {
    let name = check_name("name", &req.name, 100)?;
    let slug = req.slug.unwrap_or_else(|| users::slugify(&name));
    check_slug("slug", &slug)?;
    let team = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ManageTeams)?;
            let id: i64 = c
                .query_one(
                    "INSERT INTO teams (org_id, slug, name) VALUES ($1, $2, $3) RETURNING id",
                    &[&o.id, &slug, &name],
                )
                .map_err(|e| {
                    if e.code() == "23505" { ApiError::conflict("a team with this slug exists") } else { e.into() }
                })?
                .get(0);
            audit::record(
                c,
                &audit::Entry {
                    target_type: Some("team"),
                    target_id: Some(id.to_string()),
                    data: json!({"slug": slug}),
                    ..auth.audit(Some(o.id), action::TEAM_CREATE)
                },
            )?;
            load_team(c, o.id, &slug)
        })
        .await?;
    Ok((StatusCode::CREATED, axum::Json(team)).into_response())
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateTeam {
    pub name: Option<String>,
    pub slug: Option<String>,
}

/// Renames a team.
#[utoipa::path(patch, path = "/api/v1/orgs/{org}/teams/{team}", tag = "orgs",
    params(("org" = String, Path), ("team" = String, Path)), request_body = UpdateTeam, responses((status = 200, body = Team)))]
async fn update_team(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, team)): Path<(String, String)>,
    Json(req): Json<UpdateTeam>,
) -> ApiResult<axum::Json<Team>> {
    let name = req.name.as_deref().map(|n| check_name("name", n, 100)).transpose()?;
    if let Some(s) = &req.slug {
        check_slug("slug", s)?;
    }
    let t = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ManageTeams)?;
            let row = c.query_opt(
                "UPDATE teams SET name = coalesce($1, name), slug = coalesce($2, slug) WHERE org_id = $3 AND slug = $4 RETURNING id, slug",
                &[&name, &req.slug, &o.id, &team],
            )?;
            let row = row.ok_or_else(ApiError::not_found)?;
            let id: i64 = row.get(0);
            audit::record(c, &audit::Entry { target_type: Some("team"), target_id: Some(id.to_string()), data: json!({"name": name, "slug": req.slug}), ..auth.audit(Some(o.id), action::TEAM_UPDATE) })?;
            load_team(c, o.id, &row.get::<String>(1))
        })
        .await?;
    Ok(axum::Json(t))
}

/// Deletes a team. Issues assigned to it become unassigned.
#[utoipa::path(delete, path = "/api/v1/orgs/{org}/teams/{team}", tag = "orgs",
    params(("org" = String, Path), ("team" = String, Path)), responses((status = 204)))]
async fn delete_team(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, team)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ManageTeams)?;
            let row = c.query_opt("DELETE FROM teams WHERE org_id = $1 AND slug = $2 RETURNING id", &[&o.id, &team])?;
            let id: i64 = row.ok_or_else(ApiError::not_found)?.get(0);
            audit::record(
                c,
                &audit::Entry {
                    target_type: Some("team"),
                    target_id: Some(id.to_string()),
                    data: json!({"slug": team}),
                    ..auth.audit(Some(o.id), action::TEAM_DELETE)
                },
            )?;
            Ok(())
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Adds a member of the organization to a team.
#[utoipa::path(put, path = "/api/v1/orgs/{org}/teams/{team}/members/{user_id}", tag = "orgs",
    params(("org" = String, Path), ("team" = String, Path), ("user_id" = String, Path)), responses((status = 200, body = Team)))]
async fn add_team_member(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, team, user_id)): Path<(String, String, i64)>,
) -> ApiResult<axum::Json<Team>> {
    let t = state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ManageTeams)?;
            let t = load_team(c, o.id, &team)?;
            if users::membership(c, o.id, user_id)?.is_none() {
                return Err(ApiError::invalid("user_id", "not a member of the organization"));
            }
            c.execute(
                "INSERT INTO team_members (team_id, user_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
                &[&t.id.0, &user_id],
            )?;
            audit::record(
                c,
                &audit::Entry {
                    target_type: Some("team"),
                    target_id: Some(t.id.0.to_string()),
                    data: json!({"added_user": user_id}),
                    ..auth.audit(Some(o.id), action::TEAM_UPDATE)
                },
            )?;
            load_team(c, o.id, &team)
        })
        .await?;
    Ok(axum::Json(t))
}

/// Removes someone from a team.
#[utoipa::path(delete, path = "/api/v1/orgs/{org}/teams/{team}/members/{user_id}", tag = "orgs",
    params(("org" = String, Path), ("team" = String, Path), ("user_id" = String, Path)), responses((status = 204)))]
async fn remove_team_member(
    State(state): State<ApiState>,
    auth: Auth,
    Path((org, team, user_id)): Path<(String, String, i64)>,
) -> ApiResult<StatusCode> {
    state
        .db(move |c| {
            let o = org_sync(c, &auth, &org, Permission::ManageTeams)?;
            let t = load_team(c, o.id, &team)?;
            let n = c.execute("DELETE FROM team_members WHERE team_id = $1 AND user_id = $2", &[&t.id.0, &user_id])?;
            if n == 0 {
                return Err(ApiError::not_found());
            }
            audit::record(
                c,
                &audit::Entry {
                    target_type: Some("team"),
                    target_id: Some(t.id.0.to_string()),
                    data: json!({"removed_user": user_id}),
                    ..auth.audit(Some(o.id), action::TEAM_UPDATE)
                },
            )?;
            Ok(())
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
