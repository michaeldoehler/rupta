//! Releases and release health.

use rmcp::model::Tool;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Effect, Output, args, limit, opt_list, page, pick, project_ids, seg, tool};
use crate::rest::{Rest, ToolResult};

#[derive(Deserialize, JsonSchema)]
struct ListReleases {
    /// Organization slug, as returned by list_organizations.
    org: String,
    /// Project slugs or ids (one, comma-separated, or a list); default all projects.
    #[serde(default, deserialize_with = "opt_list")]
    #[schemars(with = "Option<Vec<String>>")]
    projects: Option<Vec<String>>,
    /// Part of the version to look for, e.g. "1.4".
    query: Option<String>,
    /// Page size, 1 to 100 (default 20).
    limit: Option<u32>,
    /// The next_cursor of the previous page.
    cursor: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct GetRelease {
    /// Organization slug, as returned by list_organizations.
    org: String,
    /// The full version, e.g. "web@1.4.0".
    version: String,
}

#[derive(Deserialize, JsonSchema, Clone, Copy)]
enum HealthPeriod {
    #[serde(rename = "24h")]
    H24,
    #[serde(rename = "7d")]
    D7,
    #[serde(rename = "14d")]
    D14,
    #[serde(rename = "30d")]
    D30,
    #[serde(rename = "90d")]
    D90,
}

impl HealthPeriod {
    fn as_str(self) -> &'static str {
        match self {
            HealthPeriod::H24 => "24h",
            HealthPeriod::D7 => "7d",
            HealthPeriod::D14 => "14d",
            HealthPeriod::D30 => "30d",
            HealthPeriod::D90 => "90d",
        }
    }
}

#[derive(Deserialize, JsonSchema)]
struct ProjectHealth {
    /// Organization slug, as returned by list_organizations.
    org: String,
    /// Project slug or id.
    project: String,
    /// Period (default 24h).
    period: Option<HealthPeriod>,
    environment: Option<String>,
}

pub(super) fn definitions() -> Vec<Tool> {
    vec![
        tool::<ListReleases>(
            "list_releases",
            "List releases",
            "Releases of an organization, newest first: version, projects, first and last event, new issues, the last \
             deploy, and health from sessions (crash-free sessions and users in percent, adoption).",
            Effect::Read,
        ),
        tool::<GetRelease>(
            "get_release",
            "Get release",
            "One release: deploys, health per project (crash-free sessions and users, crashed and errored sessions) and \
             daily sessions of the last 14 days. Search its new issues with search_issues and `first_release:<version>`.",
            Effect::Read,
        ),
        tool::<ProjectHealth>(
            "get_project_health",
            "Get project health",
            "Release health of a project over a period: crash-free sessions and users, per release with adoption, and a \
             time series of sessions, crashes and users.",
            Effect::Read,
        ),
    ]
}

pub(super) async fn call(rest: &Rest, name: &str, a: &Value) -> Option<ToolResult<Output>> {
    Some(match name {
        "list_releases" => list_releases(rest, a).await,
        "get_release" => get_release(rest, a).await,
        "get_project_health" => health(rest, a).await,
        _ => return None,
    })
}

fn release(r: &Value) -> Value {
    let mut v = pick(
        r,
        &[
            "version",
            "ref",
            "url",
            "date_created",
            "date_released",
            "first_event",
            "last_event",
            "new_issues",
            "health",
            "last_deploy",
        ],
    );
    v["projects"] = json!(
        r["projects"].as_array().into_iter().flatten().map(|p| pick(p, &["slug", "new_issues"])).collect::<Vec<_>>()
    );
    v
}

async fn list_releases(rest: &Rest, a: &Value) -> ToolResult<Output> {
    let a: ListReleases = args(a)?;
    let projects = match &a.projects {
        Some(p) => Some(project_ids(rest, &a.org, p).await?.join(",")),
        None => None,
    };
    let v = rest
        .get(
            &format!("/api/v1/orgs/{}/releases", seg(&a.org)),
            &[("project", projects), ("query", a.query), ("limit", limit(a.limit, 20, 100)), ("cursor", a.cursor)],
        )
        .await?;
    Ok(Output::Json(page(&v, release)))
}

async fn get_release(rest: &Rest, a: &Value) -> ToolResult<Output> {
    let a: GetRelease = args(a)?;
    let v = rest.get(&format!("/api/v1/orgs/{}/releases/{}", seg(&a.org), seg(&a.version)), &[]).await?;
    let mut out = release(&v);
    for k in ["deploys", "project_health"] {
        out[k] = v[k].clone();
    }
    out["daily_sessions"] = json!(
        v["sessions"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| json!({"time": p[0], "sessions": p[1], "crashed": p[2], "users": p[3], "crashed_users": p[4]}))
            .collect::<Vec<_>>()
    );
    Ok(Output::Json(out))
}

async fn health(rest: &Rest, a: &Value) -> ToolResult<Output> {
    let a: ProjectHealth = args(a)?;
    let mut v = rest
        .get(
            &format!("/api/v1/projects/{}/{}/health", seg(&a.org), seg(&a.project)),
            &[("period", a.period.map(|p| p.as_str().to_string())), ("environment", a.environment)],
        )
        .await?;
    if let Some(series) = v.get_mut("series") {
        *series = json!(
            series
                .as_array()
                .into_iter()
                .flatten()
                .map(|p| json!({"time": p[0], "sessions": p[1], "crashed": p[2], "users": p[3], "crashed_users": p[4]}))
                .collect::<Vec<_>>()
        );
    }
    Ok(Output::Json(if v.is_object() { v } else { json!({ "data": v }) }))
}
