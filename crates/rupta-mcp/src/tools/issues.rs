//! Issues: search, details, events, activity, triage and comments.

use rmcp::model::Tool;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{Effect, Output, args, limit, opt_list, page, pick, project_ids, seg, string_or_number, tool};
use crate::format::{EventOptions, event_text, issue_detail, issue_summary};
use crate::rest::{Rest, ToolError, ToolResult};

#[derive(Deserialize, JsonSchema, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum IssueSort {
    /// Most recently seen first (default).
    LastSeen,
    /// Newest issues first.
    FirstSeen,
    /// Most events first.
    Freq,
    /// Most affected users first.
    Users,
    /// Highest priority first.
    Priority,
}

impl IssueSort {
    fn as_str(self) -> &'static str {
        match self {
            IssueSort::LastSeen => "last_seen",
            IssueSort::FirstSeen => "first_seen",
            IssueSort::Freq => "freq",
            IssueSort::Users => "users",
            IssueSort::Priority => "priority",
        }
    }
}

#[derive(Deserialize, JsonSchema)]
struct SearchIssues {
    /// Organization slug, as returned by list_organizations.
    org: String,
    /// Search in rupta's query language (resource rupta://docs/search). Terms are ANDed, e.g.
    /// `is:unresolved level:error release:web@1.4.0 assigned:me "connection reset" !environment:staging
    /// first_seen:-24h times_seen:>10 browser:Chrome user.email:ada@example.com tags[region]:eu`.
    /// Useful `is:` values: unresolved, resolved, ignored, assigned, unassigned, bookmarked, new, regressed, escalating.
    /// Empty: every issue.
    query: Option<String>,
    /// Project slugs or ids (one, comma-separated, or a list); default all projects.
    #[serde(default, deserialize_with = "opt_list")]
    #[schemars(with = "Option<Vec<String>>")]
    projects: Option<Vec<String>>,
    /// Only issues with events in this environment.
    environment: Option<String>,
    /// Only issues seen within this period, e.g. "1h", "24h", "14d" (at most 366d). Also the range of
    /// events_in_period. Default: no time limit (events_in_period covers 24h).
    period: Option<String>,
    sort: Option<IssueSort>,
    /// Page size, 1 to 100 (default 25).
    limit: Option<u32>,
    /// The next_cursor of the previous page.
    cursor: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct GetIssue {
    /// Issue id (e.g. "1203"), short id (e.g. "WEB-31", needs org) or the issue's rupta URL.
    #[serde(deserialize_with = "string_or_number")]
    #[schemars(with = "String")]
    issue: String,
    /// Organization slug; needed with a short id.
    org: Option<String>,
    /// How many of the most frequent values to show per tag (default 5, at most 10).
    top_tag_values: Option<usize>,
    /// Include the latest activity and comments (default true).
    include_activity: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
struct TagValues {
    /// Issue id (e.g. "1203"), short id (e.g. "WEB-31", needs org) or the issue's rupta URL.
    #[serde(deserialize_with = "string_or_number")]
    #[schemars(with = "String")]
    issue: String,
    /// Organization slug; needed with a short id.
    org: Option<String>,
    /// Tag key, e.g. "browser", "release", "environment", "user", "url" or any custom tag.
    key: String,
    /// Page size, 1 to 100 (default 25).
    limit: Option<u32>,
    /// The next_cursor of the previous page.
    cursor: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct ListEvents {
    /// Issue id (e.g. "1203"), short id (e.g. "WEB-31", needs org) or the issue's rupta URL.
    #[serde(deserialize_with = "string_or_number")]
    #[schemars(with = "String")]
    issue: String,
    /// Organization slug; needed with a short id.
    org: Option<String>,
    /// Event filter in the query language with event keys only, e.g. `environment:production release:web@1.4.0
    /// browser:Chrome user.email:ada@example.com timestamp:-1h`, plus free text.
    query: Option<String>,
    /// Page size, 1 to 100 (default 20).
    limit: Option<u32>,
    /// The next_cursor of the previous page.
    cursor: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct GetEvent {
    /// The issue the event belongs to (id, short id with org, or URL). Give either issue, or org + project + event.
    #[serde(default, deserialize_with = "super::opt_string_or_number")]
    #[schemars(with = "Option<String>")]
    issue: Option<String>,
    /// `latest` (default), `oldest`, `recommended` (the newest with an exception) or an event id (32 hex digits).
    event: Option<String>,
    /// Organization slug; needed with a short id or without issue.
    org: Option<String>,
    /// Project slug or id; needed without issue.
    project: Option<String>,
    /// Show library (non in-app) frames too (default false: they are collapsed).
    all_frames: Option<bool>,
    /// How many of the last breadcrumbs to show (default 20, at most 100).
    max_breadcrumbs: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
struct Activity {
    /// Issue id (e.g. "1203"), short id (e.g. "WEB-31", needs org) or the issue's rupta URL.
    #[serde(deserialize_with = "string_or_number")]
    #[schemars(with = "String")]
    issue: String,
    /// Organization slug; needed with a short id.
    org: Option<String>,
    /// Page size, 1 to 100 (default 25).
    limit: Option<u32>,
    /// The next_cursor of the previous page.
    cursor: Option<String>,
}

#[derive(Deserialize, JsonSchema, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum Status {
    Resolved,
    Unresolved,
    Ignored,
}

#[derive(Deserialize, JsonSchema, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum Priority {
    High,
    Medium,
    Low,
}

#[derive(Deserialize, JsonSchema)]
struct UpdateIssue {
    /// Issue id (e.g. "1203"), short id (e.g. "WEB-31", needs org) or the issue's rupta URL.
    #[serde(deserialize_with = "string_or_number")]
    #[schemars(with = "String")]
    issue: String,
    /// Organization slug; needed with a short id.
    org: Option<String>,
    /// New status. `resolved`: fixed (comes back as a regression if it happens again); `unresolved`: reopen;
    /// `ignored`: hide it, forever or until one of the ignore_* conditions is met.
    status: Option<Status>,
    /// With status resolved: the release (version) that fixes it.
    resolved_in_release: Option<String>,
    /// With status resolved: resolved in the next release (the release after the latest one seen so far).
    resolve_in_next_release: Option<bool>,
    /// With status ignored: minutes until it shows again (1 to 525600).
    ignore_minutes: Option<i64>,
    /// With status ignored: show again after this many more events.
    ignore_count: Option<i64>,
    /// With status ignored: show again after this many more distinct users.
    ignore_user_count: Option<i64>,
    /// With ignore_user_count: count the users within this many minutes instead of in total.
    ignore_user_window_minutes: Option<i64>,
    /// With status ignored: keep it ignored until it escalates (a spike of events).
    ignore_until_escalating: Option<bool>,
    /// Assign: `me`, `user:<id>` (list_members), `team:<id>` (list_teams), or `none` to unassign.
    assignee: Option<String>,
    priority: Option<Priority>,
    /// Bookmark (true) or remove the bookmark (false), for the token's user.
    bookmarked: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
struct AddComment {
    /// Issue id (e.g. "1203"), short id (e.g. "WEB-31", needs org) or the issue's rupta URL.
    #[serde(deserialize_with = "string_or_number")]
    #[schemars(with = "String")]
    issue: String,
    /// Organization slug; needed with a short id.
    org: Option<String>,
    /// The comment, Markdown, 1 to 10000 characters. Say that it was written by an agent if the user wants that.
    text: String,
}

pub(super) fn definitions() -> Vec<Tool> {
    vec![
        tool::<SearchIssues>(
            "search_issues",
            "Search issues",
            "Search the issues (groups of similar errors) of an organization with rupta's query language: status, level, \
             release, environment, assignee, tags, times, free text. Returns id, short id, title, culprit, level, status, \
             counts, first/last seen and assignee per issue, newest activity first by default. Paginate with next_cursor.",
            Effect::Read,
        ),
        tool::<GetIssue>(
            "get_issue",
            "Get issue details",
            "Details of one issue: title, culprit, level, status and how it was resolved or ignored, event and user \
             counts, first and last seen, releases, assignee, the distribution of tag values (browser, release, \
             environment, user, url, custom tags), linked tickets in issue trackers and recent activity and comments. \
             Use get_event for the stack trace.",
            Effect::Read,
        ),
        tool::<TagValues>(
            "get_issue_tag_values",
            "Get tag values of an issue",
            "All values of one tag of an issue's events with their counts, most frequent first (get_issue shows only the top values).",
            Effect::Read,
        ),
        tool::<ListEvents>(
            "list_issue_events",
            "List events of an issue",
            "The events (occurrences) of an issue, newest first, with time, release, environment, user, browser, OS, url \
             and tags; optionally filtered. Use get_event with an event id for the full event.",
            Effect::Read,
        ),
        tool::<GetEvent>(
            "get_event",
            "Get event",
            "One event as readable text: exception and chained causes, the stack trace (most recent call last) with source \
             context of in-app frames, breadcrumbs, the HTTP request, user, contexts (browser, OS, runtime, device), tags, \
             extra data and SDK. By default the latest event of an issue; or `oldest`, `recommended`, or a specific event id.",
            Effect::Read,
        ),
        tool::<Activity>(
            "list_issue_activity",
            "List issue activity",
            "The history of an issue, newest first: comments, status changes, assignments, priority changes, regressions, \
             linked tickets.",
            Effect::Read,
        ),
        tool::<UpdateIssue>(
            "update_issue",
            "Update issue",
            "Triage an issue: resolve (optionally in a release or the next release), unresolve, ignore (forever, for a time, \
             until more events or users, or until it escalates), assign or unassign, set the priority, bookmark. Only the \
             given fields change. Status, assignment and priority need the issue:write (or project:write) scope and at \
             least the member role; bookmarks work for every role. Returns the updated issue. Confirm with the user before \
             changing issues they did not ask about.",
            Effect::Change { idempotent: true },
        ),
        tool::<AddComment>(
            "add_comment",
            "Add comment",
            "Add a Markdown comment to an issue, as the token's user (organization tokens cannot comment). Needs the \
             issue:write scope and at least the member role. The assignee is notified by email when enabled.",
            Effect::Change { idempotent: false },
        ),
    ]
}

pub(super) async fn call(rest: &Rest, name: &str, a: &Value) -> Option<ToolResult<Output>> {
    Some(match name {
        "search_issues" => search(rest, a).await,
        "get_issue" => get(rest, a).await,
        "get_issue_tag_values" => tag_values(rest, a).await,
        "list_issue_events" => events(rest, a).await,
        "get_event" => event(rest, a).await,
        "list_issue_activity" => activity(rest, a).await,
        "update_issue" => update(rest, a).await,
        "add_comment" => comment(rest, a).await,
        _ => return None,
    })
}

/// The numeric id of an issue given by id, short id or URL.
pub(crate) async fn issue_id(rest: &Rest, issue: &str, org: Option<&str>) -> ToolResult<String> {
    let s = issue.trim();
    if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) {
        return Ok(s.to_string());
    }
    // A link to the issue in the web app: .../issues/{id}
    if let Some((_, tail)) = s.rsplit_once("/issues/") {
        let id: String = tail.chars().take_while(char::is_ascii_digit).collect();
        if !id.is_empty() {
            return Ok(id);
        }
    }
    if let Some(id) = s.strip_prefix("rupta://issue/") {
        let id: String = id.chars().take_while(char::is_ascii_digit).collect();
        if !id.is_empty() {
            return Ok(id);
        }
    }
    let Some(org) = org.filter(|o| !o.is_empty()) else {
        return Err(ToolError::new(format!(
            "issue {s:?} is not a numeric id; pass org together with a short id such as WEB-31"
        )));
    };
    let v = rest.get(&format!("/api/v1/orgs/{}/issues/by-short-id/{}", seg(org), seg(s)), &[]).await?;
    v["id"].as_str().map(str::to_string).ok_or_else(|| ToolError::new("issue not found"))
}

async fn search(rest: &Rest, a: &Value) -> ToolResult<Output> {
    let a: SearchIssues = args(a)?;
    let projects = match &a.projects {
        Some(p) => Some(project_ids(rest, &a.org, p).await?.join(",")),
        None => None,
    };
    let v = rest
        .get(
            &format!("/api/v1/orgs/{}/issues", seg(&a.org)),
            &[
                ("query", a.query),
                ("project", projects),
                ("environment", a.environment),
                ("period", a.period),
                ("sort", a.sort.map(|s| s.as_str().to_string())),
                ("limit", limit(a.limit, 25, 100)),
                ("cursor", a.cursor),
            ],
        )
        .await?;
    Ok(Output::Json(page(&v, issue_summary)))
}

async fn get(rest: &Rest, a: &Value) -> ToolResult<Output> {
    let a: GetIssue = args(a)?;
    let id = issue_id(rest, &a.issue, a.org.as_deref()).await?;
    let issue = rest.get(&format!("/api/v1/issues/{id}"), &[]).await?;
    let tags = rest.get(&format!("/api/v1/issues/{id}/tags"), &[]).await?;
    let links = rest.get(&format!("/api/v1/issues/{id}/links"), &[]).await?;
    let top = a.top_tag_values.unwrap_or(5).clamp(1, 10);
    let tags: Vec<Value> = tags
        .as_array()
        .into_iter()
        .flatten()
        .map(|t| {
            let total = t["total_values"].as_i64().unwrap_or(0).max(1) as f64;
            let values: Vec<Value> = t["top_values"]
                .as_array()
                .into_iter()
                .flatten()
                .take(top)
                .map(|v| {
                    let n = v["count"].as_i64().unwrap_or(0);
                    json!({"value": v["value"], "count": n, "percent": ((n as f64 / total) * 1000.0).round() / 10.0})
                })
                .collect();
            json!({"key": t["key"], "events": t["total_values"], "unique_values": t["unique_values"], "top_values": values})
        })
        .collect();
    let tickets: Vec<Value> = links
        .as_array()
        .into_iter()
        .flatten()
        .map(|l| {
            json!({"id": l["id"], "tracker": l["integration"]["kind"], "integration": l["integration"]["name"],
                   "key": l["external_key"], "url": l["url"], "title": l["title"], "status": l["status"],
                   "status_category": l["status_category"], "assignee": l["assignee"], "synced_at": l["synced_at"]})
        })
        .collect();
    let mut out = json!({"issue": issue_detail(&issue), "tags": tags, "linked_tickets": tickets});
    if a.include_activity.unwrap_or(true) {
        let act = rest.get(&format!("/api/v1/issues/{id}/activity"), &[("limit", Some("10".into()))]).await?;
        out["recent_activity"] = page(&act, activity_item)["data"].take();
    }
    Ok(Output::Json(out))
}

async fn tag_values(rest: &Rest, a: &Value) -> ToolResult<Output> {
    let a: TagValues = args(a)?;
    let id = issue_id(rest, &a.issue, a.org.as_deref()).await?;
    let v = rest
        .get(
            &format!("/api/v1/issues/{id}/tags/{}/values", seg(&a.key)),
            &[("limit", limit(a.limit, 25, 100)), ("cursor", a.cursor)],
        )
        .await?;
    Ok(Output::Json(page(&v, |t| pick(t, &["value", "count"]))))
}

async fn events(rest: &Rest, a: &Value) -> ToolResult<Output> {
    let a: ListEvents = args(a)?;
    let id = issue_id(rest, &a.issue, a.org.as_deref()).await?;
    let v = rest
        .get(
            &format!("/api/v1/issues/{id}/events"),
            &[("query", a.query), ("limit", limit(a.limit, 20, 100)), ("cursor", a.cursor)],
        )
        .await?;
    Ok(Output::Json(page(&v, |e| {
        pick(
            e,
            &[
                "event_id",
                "timestamp",
                "level",
                "title",
                "message",
                "environment",
                "release",
                "transaction",
                "user",
                "browser",
                "os",
                "device",
                "url",
                "tags",
            ],
        )
    })))
}

async fn event(rest: &Rest, a: &Value) -> ToolResult<Output> {
    let a: GetEvent = args(a)?;
    let which = a.event.as_deref().map(str::trim).filter(|e| !e.is_empty()).unwrap_or("latest").to_ascii_lowercase();
    let doc = match &a.issue {
        Some(issue) => {
            let id = issue_id(rest, issue, a.org.as_deref()).await?;
            rest.get(&format!("/api/v1/issues/{id}/events/{}", seg(&which)), &[]).await?
        }
        None => {
            let (Some(org), Some(project)) = (&a.org, &a.project) else {
                return Err(ToolError::new("give issue, or org, project and an event id"));
            };
            if matches!(which.as_str(), "latest" | "oldest" | "recommended") {
                return Err(ToolError::new("without issue, event must be an event id (32 hex digits)"));
            }
            rest.get(&format!("/api/v1/orgs/{}/events/{}/{}", seg(org), seg(project), seg(&which)), &[]).await?
        }
    };
    let text = event_text(
        &doc,
        &EventOptions {
            all_frames: a.all_frames.unwrap_or(false),
            max_breadcrumbs: a.max_breadcrumbs.unwrap_or(20).min(100),
        },
    );
    let structured = pick(
        &doc,
        &[
            "event_id",
            "issue_id",
            "project_id",
            "title",
            "level",
            "timestamp",
            "environment",
            "release",
            "trace_id",
            "replay_id",
            "previous_event_id",
            "next_event_id",
        ],
    );
    Ok(Output::Text(text, Some(structured)))
}

fn activity_item(x: &Value) -> Value {
    let mut o = Map::new();
    o.insert("id".into(), x["id"].clone());
    o.insert("kind".into(), x["kind"].clone());
    o.insert("created_at".into(), x["created_at"].clone());
    if let Some(u) = x.get("user").filter(|u| u.is_object()) {
        o.insert("user".into(), pick(u, &["id", "name", "email"]));
    }
    if x["kind"] == "comment" {
        o.insert("text".into(), x["data"]["text"].clone());
    } else if x["data"].as_object().is_some_and(|d| !d.is_empty()) {
        o.insert("data".into(), x["data"].clone());
    }
    Value::Object(o)
}

async fn activity(rest: &Rest, a: &Value) -> ToolResult<Output> {
    let a: Activity = args(a)?;
    let id = issue_id(rest, &a.issue, a.org.as_deref()).await?;
    let v = rest
        .get(&format!("/api/v1/issues/{id}/activity"), &[("limit", limit(a.limit, 25, 100)), ("cursor", a.cursor)])
        .await?;
    Ok(Output::Json(page(&v, activity_item)))
}

/// The REST body of an update.
fn update_body(a: &UpdateIssue) -> ToolResult<Value> {
    let mut b = Map::new();
    if let Some(s) = a.status {
        b.insert(
            "status".into(),
            json!(match s {
                Status::Resolved => "resolved",
                Status::Unresolved => "unresolved",
                Status::Ignored => "ignored",
            }),
        );
    }
    let resolving = matches!(a.status, Some(Status::Resolved));
    let ignoring = matches!(a.status, Some(Status::Ignored));
    if let Some(r) = &a.resolved_in_release {
        if !resolving {
            return Err(ToolError::new("resolved_in_release needs status resolved"));
        }
        b.insert("resolved_in_release".into(), json!(r));
    }
    if a.resolve_in_next_release == Some(true) {
        if !resolving {
            return Err(ToolError::new("resolve_in_next_release needs status resolved"));
        }
        b.insert("resolved_in".into(), json!("next_release"));
    }
    for (field, key, v) in [
        ("ignore_minutes", "ignore_duration", a.ignore_minutes),
        ("ignore_count", "ignore_count", a.ignore_count),
        ("ignore_user_count", "ignore_user_count", a.ignore_user_count),
        ("ignore_user_window_minutes", "ignore_user_window", a.ignore_user_window_minutes),
    ] {
        if let Some(v) = v {
            if !ignoring {
                return Err(ToolError::new(format!("{field} needs status ignored")));
            }
            b.insert(key.into(), json!(v));
        }
    }
    if a.ignore_until_escalating == Some(true) {
        if !ignoring {
            return Err(ToolError::new("ignore_until_escalating needs status ignored"));
        }
        b.insert("substatus".into(), json!("archived_until_escalating"));
    }
    if let Some(who) = a.assignee.as_deref().map(str::trim) {
        let v = match who {
            "" | "none" | "null" | "nobody" => Value::Null,
            other => json!(other),
        };
        b.insert("assignee".into(), v);
    }
    if let Some(p) = a.priority {
        b.insert(
            "priority".into(),
            json!(match p {
                Priority::High => "high",
                Priority::Medium => "medium",
                Priority::Low => "low",
            }),
        );
    }
    if let Some(x) = a.bookmarked {
        b.insert("is_bookmarked".into(), json!(x));
    }
    if b.is_empty() {
        return Err(ToolError::new("nothing to change: give status, assignee, priority or bookmarked"));
    }
    Ok(Value::Object(b))
}

async fn update(rest: &Rest, a: &Value) -> ToolResult<Output> {
    let a: UpdateIssue = args(a)?;
    let body = update_body(&a)?;
    let id = issue_id(rest, &a.issue, a.org.as_deref()).await?;
    let v = rest.patch(&format!("/api/v1/issues/{id}"), body).await?;
    Ok(Output::Json(json!({"updated": true, "issue": issue_detail(&v)})))
}

async fn comment(rest: &Rest, a: &Value) -> ToolResult<Output> {
    let a: AddComment = args(a)?;
    let id = issue_id(rest, &a.issue, a.org.as_deref()).await?;
    let v = rest.post(&format!("/api/v1/issues/{id}/comments"), json!({"text": a.text})).await?;
    Ok(Output::Json(json!({"created": true, "comment": activity_item(&v)})))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn upd(v: Value) -> ToolResult<Value> {
        let mut v = v;
        v["issue"] = json!("1");
        update_body(&serde_json::from_value(v).unwrap())
    }

    #[test]
    fn updates_map_to_the_api_body() {
        assert_eq!(
            upd(json!({"status": "resolved", "resolve_in_next_release": true})).unwrap(),
            json!({"status": "resolved", "resolved_in": "next_release"})
        );
        assert_eq!(
            upd(json!({"status": "ignored", "ignore_user_count": 10, "ignore_user_window_minutes": 60})).unwrap(),
            json!({"status": "ignored", "ignore_user_count": 10, "ignore_user_window": 60})
        );
        assert_eq!(
            upd(json!({"status": "ignored", "ignore_until_escalating": true})).unwrap(),
            json!({"status": "ignored", "substatus": "archived_until_escalating"})
        );
        assert_eq!(upd(json!({"assignee": "none"})).unwrap(), json!({"assignee": null}));
        assert_eq!(
            upd(json!({"assignee": "user:4", "priority": "high"})).unwrap(),
            json!({"assignee": "user:4", "priority": "high"})
        );
        assert!(upd(json!({"ignore_count": 5})).unwrap_err().0.contains("needs status ignored"));
        assert!(upd(json!({"resolved_in_release": "1.0"})).is_err());
        assert!(upd(json!({})).unwrap_err().0.contains("nothing to change"));
    }
}
