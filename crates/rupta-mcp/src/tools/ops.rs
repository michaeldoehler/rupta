//! Cron monitors and alerts.

use rmcp::model::Tool;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use super::{Effect, Output, args, limit, list, page, pick, seg, tool};
use crate::rest::{Rest, ToolResult};

#[derive(Deserialize, JsonSchema)]
struct ProjectArgs {
    /// Organization slug, as returned by list_organizations.
    org: String,
    /// Project slug or id.
    project: String,
}

#[derive(Deserialize, JsonSchema)]
struct CheckIns {
    /// Organization slug, as returned by list_organizations.
    org: String,
    /// Project slug or id.
    project: String,
    /// Monitor slug (from list_monitors).
    monitor: String,
    /// Page size, 1 to 100 (default 25).
    limit: Option<u32>,
    /// The next_cursor of the previous page.
    cursor: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct Fires {
    /// Organization slug, as returned by list_organizations.
    org: String,
    /// Project slug or id.
    project: String,
    /// Only firings of this alert rule (id from list_alert_rules).
    #[serde(default, deserialize_with = "super::opt_string_or_number")]
    #[schemars(with = "Option<String>")]
    rule: Option<String>,
    /// Page size, 1 to 100 (default 25).
    limit: Option<u32>,
    /// The next_cursor of the previous page.
    cursor: Option<String>,
}

pub(super) fn definitions() -> Vec<Tool> {
    vec![
        tool::<ProjectArgs>(
            "list_monitors",
            "List cron monitors",
            "The cron monitors of a project: schedule, time zone, margins, status, health (ok, missed, error, timeout), \
             last and next expected check-in.",
            Effect::Read,
        ),
        tool::<CheckIns>(
            "list_monitor_checkins",
            "List check-ins",
            "Check-ins of a cron monitor, newest first: status (in_progress, ok, error, missed, timeout), environment, \
             duration and time.",
            Effect::Read,
        ),
        tool::<ProjectArgs>(
            "list_alert_rules",
            "List alert rules",
            "The alert rules of a project: conditions (new issue, regression, reappeared, escalating, event or user \
             frequency), filters (level, environment, tag), actions (channel, email, create ticket), rate limit and \
             whether each is enabled.",
            Effect::Read,
        ),
        tool::<Fires>(
            "list_alert_firings",
            "List alert firings",
            "When the project's alert rules fired and for which issue, newest first (kept for 31 days). Shows which \
             problems people were alerted about recently.",
            Effect::Read,
        ),
    ]
}

pub(super) async fn call(rest: &Rest, name: &str, a: &Value) -> Option<ToolResult<Output>> {
    Some(match name {
        "list_monitors" => monitors(rest, a).await,
        "list_monitor_checkins" => checkins(rest, a).await,
        "list_alert_rules" => rules(rest, a).await,
        "list_alert_firings" => fires(rest, a).await,
        _ => return None,
    })
}

async fn monitors(rest: &Rest, a: &Value) -> ToolResult<Output> {
    let a: ProjectArgs = args(a)?;
    let v = rest.get(&format!("/api/v1/projects/{}/{}/monitors", seg(&a.org), seg(&a.project)), &[]).await?;
    Ok(Output::Json(list(&v, |m| {
        pick(
            m,
            &[
                "slug",
                "name",
                "schedule",
                "timezone",
                "checkin_margin",
                "max_runtime",
                "status",
                "health",
                "last_checkin",
                "next_checkin",
            ],
        )
    })))
}

async fn checkins(rest: &Rest, a: &Value) -> ToolResult<Output> {
    let a: CheckIns = args(a)?;
    let v = rest
        .get(
            &format!("/api/v1/projects/{}/{}/monitors/{}/checkins", seg(&a.org), seg(&a.project), seg(&a.monitor)),
            &[("limit", limit(a.limit, 25, 100)), ("cursor", a.cursor)],
        )
        .await?;
    Ok(Output::Json(page(&v, |c| pick(c, &["id", "status", "environment", "duration_ms", "created_at", "updated_at"]))))
}

async fn rules(rest: &Rest, a: &Value) -> ToolResult<Output> {
    let a: ProjectArgs = args(a)?;
    let v = rest.get(&format!("/api/v1/projects/{}/{}/alert-rules", seg(&a.org), seg(&a.project)), &[]).await?;
    Ok(Output::Json(list(&v, |r| {
        pick(
            r,
            &[
                "id",
                "name",
                "enabled",
                "conditions",
                "filters",
                "actions",
                "frequency_minutes",
                "environment",
                "updated_at",
            ],
        )
    })))
}

async fn fires(rest: &Rest, a: &Value) -> ToolResult<Output> {
    let a: Fires = args(a)?;
    let v = rest
        .get(
            &format!("/api/v1/projects/{}/{}/alert-fires", seg(&a.org), seg(&a.project)),
            &[("rule", a.rule), ("limit", limit(a.limit, 25, 100)), ("cursor", a.cursor)],
        )
        .await?;
    Ok(Output::Json(v))
}
