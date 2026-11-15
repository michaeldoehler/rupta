//! Issue tracker integrations and tickets.

use rmcp::model::Tool;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::issues::issue_id;
use super::{Effect, Output, args, list, pick, seg, string_or_number, tool};
use crate::rest::{Rest, ToolResult};

#[derive(Deserialize, JsonSchema)]
struct ListIntegrations {
    /// Organization slug, as returned by list_organizations.
    org: String,
    /// Project slug or id: also show the project's default target per integration (where new tickets go).
    project: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct CreateTicket {
    /// Issue id (e.g. "1203"), short id (e.g. "WEB-31", needs org) or the issue's rupta URL.
    #[serde(deserialize_with = "string_or_number")]
    #[schemars(with = "String")]
    issue: String,
    /// Organization slug; needed with a short id.
    org: Option<String>,
    /// The integration to use (id from list_integrations).
    #[serde(deserialize_with = "string_or_number")]
    #[schemars(with = "String")]
    integration_id: String,
    /// Where to create it: Jira project key, Linear team id, GitHub `owner/repo` or GitLab project path. Default:
    /// the project's default target (list_integrations with project).
    target: Option<String>,
    /// Jira issue type (id or name); default from the project's settings.
    issue_type: Option<String>,
    /// Linear project id; default from the project's settings.
    linear_project: Option<String>,
    /// Labels; default from the project's settings.
    labels: Option<Vec<String>>,
    /// Replaces the generated title (by default the issue's title).
    title: Option<String>,
    /// Replaces the generated description (Markdown; a link back to the issue is added). By default rupta writes
    /// the error, culprit, counts, first and last seen, release and the in-app stack trace.
    description: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct LinkTicket {
    /// Issue id (e.g. "1203"), short id (e.g. "WEB-31", needs org) or the issue's rupta URL.
    #[serde(deserialize_with = "string_or_number")]
    #[schemars(with = "String")]
    issue: String,
    /// Organization slug; needed with a short id.
    org: Option<String>,
    /// The integration of the tracker the ticket is in (id from list_integrations).
    #[serde(deserialize_with = "string_or_number")]
    #[schemars(with = "String")]
    integration_id: String,
    /// The ticket: a key (`PROJ-123`, `ENG-42`, `owner/repo#17`, `#17` with a default target) or its URL.
    reference: String,
}

pub(super) fn definitions() -> Vec<Tool> {
    vec![
        tool::<ListIntegrations>(
            "list_integrations",
            "List integrations",
            "The issue tracker integrations of an organization (Jira, Linear, GitHub, GitLab): id, kind, name, enabled, \
             the last error. With project, also the project's default target, issue type and labels for new tickets.",
            Effect::Read,
        ),
        tool::<CreateTicket>(
            "create_ticket",
            "Create ticket",
            "Create a ticket for an issue in an issue tracker through an existing integration, and link it to the issue \
             (status sync follows the integration's settings). Creates something outside rupta: confirm with the user \
             first. Needs the issue:write scope and at least the member role; fails if the issue already has a ticket \
             of that integration.",
            Effect::External,
        ),
        tool::<LinkTicket>(
            "link_ticket",
            "Link existing ticket",
            "Link an existing ticket in an issue tracker to an issue (by key or URL), so its status is shown and \
             synchronised. Needs the issue:write scope and at least the member role.",
            Effect::Change { idempotent: false },
        ),
    ]
}

pub(super) async fn call(rest: &Rest, name: &str, a: &Value) -> Option<ToolResult<Output>> {
    Some(match name {
        "list_integrations" => integrations(rest, a).await,
        "create_ticket" => create(rest, a).await,
        "link_ticket" => link(rest, a).await,
        _ => return None,
    })
}

async fn integrations(rest: &Rest, a: &Value) -> ToolResult<Output> {
    let a: ListIntegrations = args(a)?;
    let v = rest.get(&format!("/api/v1/orgs/{}/integrations", seg(&a.org)), &[]).await?;
    let mut out = list(&v, |i| pick(i, &["id", "kind", "name", "enabled", "account", "last_error", "last_error_at"]));
    if let Some(p) = &a.project {
        let d = rest.get(&format!("/api/v1/projects/{}/{}/integrations", seg(&a.org), seg(p)), &[]).await?;
        out["project_defaults"] = list(&d, |x| {
            json!({"integration_id": x["integration"]["id"], "target": x["target"], "target_name": x["target_name"],
                   "issue_type": x["issue_type"], "linear_project": x["project"], "labels": x["labels"],
                   "incoming_sync": x["incoming_sync"], "outgoing_transition": x["outgoing_transition"]})
        })["data"]
            .take();
    }
    Ok(Output::Json(out))
}

fn link_view(l: &Value) -> Value {
    json!({"id": l["id"], "issue_id": l["issue_id"], "tracker": l["integration"]["kind"], "integration": l["integration"]["name"],
           "key": l["external_key"], "url": l["url"], "title": l["title"], "status": l["status"],
           "status_category": l["status_category"]})
}

async fn create(rest: &Rest, a: &Value) -> ToolResult<Output> {
    let a: CreateTicket = args(a)?;
    let id = issue_id(rest, &a.issue, a.org.as_deref()).await?;
    let mut body = Map::new();
    body.insert("integration_id".into(), json!(a.integration_id));
    body.insert("mode".into(), json!("create"));
    for (k, v) in [
        ("target", a.target),
        ("issue_type", a.issue_type),
        ("project", a.linear_project),
        ("title", a.title),
        ("description", a.description),
    ] {
        if let Some(v) = v {
            body.insert(k.into(), json!(v));
        }
    }
    if let Some(l) = a.labels {
        body.insert("labels".into(), json!(l));
    }
    let v = rest.post(&format!("/api/v1/issues/{id}/links"), Value::Object(body)).await?;
    Ok(Output::Json(json!({"created": true, "ticket": link_view(&v)})))
}

async fn link(rest: &Rest, a: &Value) -> ToolResult<Output> {
    let a: LinkTicket = args(a)?;
    let id = issue_id(rest, &a.issue, a.org.as_deref()).await?;
    let v = rest
        .post(
            &format!("/api/v1/issues/{id}/links"),
            json!({"integration_id": a.integration_id, "mode": "link", "reference": a.reference}),
        )
        .await?;
    Ok(Output::Json(json!({"linked": true, "ticket": link_view(&v)})))
}
