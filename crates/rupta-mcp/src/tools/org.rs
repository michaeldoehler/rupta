//! Organizations, projects, members and teams.

use rmcp::model::Tool;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Effect, NoArgs, Output, args, limit, list, page, pick, seg, tool};
use crate::rest::{Rest, ToolResult};

#[derive(Deserialize, JsonSchema)]
struct OrgArgs {
    /// Organization slug, as returned by list_organizations (e.g. "acme").
    org: String,
}

#[derive(Deserialize, JsonSchema)]
struct MembersArgs {
    /// Organization slug, as returned by list_organizations.
    org: String,
    /// Page size, 1 to 100 (default 50).
    limit: Option<u32>,
    /// The next_cursor of the previous page.
    cursor: Option<String>,
}

pub(super) fn definitions() -> Vec<Tool> {
    vec![
        tool::<NoArgs>(
            "list_organizations",
            "List organizations",
            "List the organizations this token can access, with the caller's role in each. \
             Organization slugs are the `org` argument of almost every other tool; call this first.",
            Effect::Read,
        ),
        tool::<OrgArgs>(
            "list_projects",
            "List projects",
            "List the projects of an organization (id, slug, name, platform, team). Project slugs or ids \
             narrow searches in the other tools.",
            Effect::Read,
        ),
        tool::<MembersArgs>(
            "list_members",
            "List members",
            "List the members of an organization (user id, name, email, role, teams). Use the user id to \
             assign issues (update_issue with assignee \"user:<id>\"). Needs the member:read or org:read scope.",
            Effect::Read,
        ),
        tool::<OrgArgs>(
            "list_teams",
            "List teams",
            "List the teams of an organization with their members. Use the team id to assign issues \
             (update_issue with assignee \"team:<id>\").",
            Effect::Read,
        ),
    ]
}

pub(super) async fn call(rest: &Rest, name: &str, a: &Value) -> Option<ToolResult<Output>> {
    Some(match name {
        "list_organizations" => list_orgs(rest).await,
        "list_projects" => list_projects(rest, a).await,
        "list_members" => list_members(rest, a).await,
        "list_teams" => list_teams(rest, a).await,
        _ => return None,
    })
}

async fn list_orgs(rest: &Rest) -> ToolResult<Output> {
    let v = rest.get("/api/v1/orgs", &[]).await?;
    Ok(Output::Json(list(&v, |o| pick(o, &["id", "slug", "name", "role"]))))
}

async fn list_projects(rest: &Rest, a: &Value) -> ToolResult<Output> {
    let a: OrgArgs = args(a)?;
    let v = rest.get(&format!("/api/v1/orgs/{}/projects", seg(&a.org)), &[]).await?;
    Ok(Output::Json(list(&v, |p| {
        json!({"id": p["id"], "slug": p["slug"], "name": p["name"], "platform": p["platform"],
               "team": p["team"].get("slug"), "created_at": p["created_at"]})
    })))
}

async fn list_members(rest: &Rest, a: &Value) -> ToolResult<Output> {
    let a: MembersArgs = args(a)?;
    let v = rest
        .get(
            &format!("/api/v1/orgs/{}/members", seg(&a.org)),
            &[("limit", limit(a.limit, 50, 100)), ("cursor", a.cursor)],
        )
        .await?;
    Ok(Output::Json(page(&v, |m| pick(m, &["user_id", "name", "email", "role", "teams"]))))
}

async fn list_teams(rest: &Rest, a: &Value) -> ToolResult<Output> {
    let a: OrgArgs = args(a)?;
    let v = rest.get(&format!("/api/v1/orgs/{}/teams", seg(&a.org)), &[]).await?;
    Ok(Output::Json(list(&v, |t| {
        let members: Vec<Value> = t["members"]
            .as_array()
            .map(|m| m.iter().map(|m| pick(m, &["user_id", "name", "email"])).collect())
            .unwrap_or_default();
        json!({"id": t["id"], "slug": t["slug"], "name": t["name"], "members": members})
    })))
}
