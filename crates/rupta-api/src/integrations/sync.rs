//! Linking issues to tickets and keeping their statuses in step.
//!
//! **Tracker → rupta** (webhooks): a verified webhook updates the stored
//! title, status and assignee of every link to the ticket. When the
//! ticket's status category changes and the project's defaults for the
//! integration have `incoming_sync`, the rupta issue is resolved when the
//! ticket is done and unresolved when it is reopened.
//!
//! **rupta → tracker**: when a person resolves or unresolves an issue, or
//! an issue regresses, every linked ticket whose project defaults have
//! `outgoing_comment` gets a comment and with `outgoing_transition` is
//! moved to a done or open state.
//!
//! **Loop protection**: status changes that came from a tracker never
//! cause outgoing calls to that tracker, and applying a status the issue
//! already has does nothing, so a tracker's webhook about a change rupta
//! made ends there. In addition, for two minutes after rupta moves a
//! ticket (the quiet period), webhooks about that ticket update the link
//! but do not resolve or unresolve the rupta issue: trackers may deliver
//! the webhook of an earlier change after a later one, and such a stale
//! echo must not undo what a person just did in rupta.

use std::collections::HashSet;

use chrono::{Duration, Utc};
use rupta_store::squad::Connection;
use serde_json::{Value, json};

use super::content::{Block, Doc, Inline, ticket_body, ticket_title};
use super::store::{self, IntegrationRow, LinkRow, activity, ticket_json};
use super::{
    AnyTracker, Category, IssueTracker, Settings, Ticket, TicketDraft, TicketTarget, TrackerError, WebhookError,
    WebhookRequest,
};
use crate::error::{ApiError, ApiResult};
use crate::state::ApiState;

/// After rupta moves a ticket, webhooks about it do not change the issue
/// for this long.
const QUIET_PERIOD_SECONDS: i64 = 120;

/// An error answer for a failed tracker call.
pub fn tracker_error(e: &TrackerError) -> ApiError {
    let status = match e {
        TrackerError::Invalid(_) | TrackerError::Blocked(_) => axum::http::StatusCode::UNPROCESSABLE_ENTITY,
        _ => axum::http::StatusCode::BAD_GATEWAY,
    };
    ApiError::new(status, "integration_error", e.to_string()).with_details(json!({"reason": e.reason()}))
}

/// Counts a tracker call on `/metrics`.
pub fn count_request<T>(state: &ApiState, i: &IntegrationRow, operation: &'static str, r: &Result<T, TrackerError>) {
    let result = match r {
        Ok(_) => "success",
        Err(e) => e.reason(),
    };
    state.metrics.count(
        "rupta_integration_requests_total",
        &[("kind", i.kind.as_str()), ("operation", operation), ("result", result)],
    );
}

async fn record_error(state: &ApiState, id: i64, e: Option<String>) {
    let _ = state.db(move |c| store::set_error(c, id, e.as_deref())).await;
}

/// What to link.
#[derive(Debug, Clone)]
pub enum NewLink {
    /// Create a ticket; `title` and `description` override the generated ones.
    Create { target: TicketTarget, title: Option<String>, description: Option<String> },
    /// Link an existing ticket by key or URL.
    Existing { reference: String },
}

fn upsert_link(
    c: &Connection,
    issue_id: i64,
    integration_id: i64,
    t: &Ticket,
    user: Option<i64>,
) -> ApiResult<LinkRow> {
    let r = c.query_opt(
        &format!(
            "INSERT INTO issue_links (issue_id, integration_id, external_id, external_key, url, title, status, status_category, assignee, created_by, synced_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, now())
             ON CONFLICT (issue_id, integration_id, external_id) DO NOTHING RETURNING {}",
            store::LINK_COLUMNS
        ),
        &[&issue_id, &integration_id, &t.external_id, &t.key, &t.url, &t.title, &t.status, &t.category.as_str(), &t.assignee, &user],
    )?;
    r.map(|r| store::link_from(&r)).ok_or_else(|| ApiError::conflict("this ticket is already linked to the issue"))
}

/// Creates or links a ticket for an issue and records the link and an
/// activity entry.
pub async fn link(
    state: &ApiState,
    user: Option<i64>,
    issue_id: i64,
    integration: &IntegrationRow,
    what: NewLink,
) -> ApiResult<LinkRow> {
    let tracker = integration.tracker(state).map_err(|e| tracker_error(&e))?;
    let (ticket, created) = match what {
        NewLink::Create { target, title, description } => {
            let st = state.clone();
            let summary = state.db(move |c| store::summary(&st, c, issue_id)).await?;
            let draft = TicketDraft {
                title: title.filter(|t| !t.trim().is_empty()).unwrap_or_else(|| ticket_title(&summary)),
                body: match description.filter(|d| !d.trim().is_empty()) {
                    Some(d) => {
                        let mut doc = Doc::from_text(&d);
                        doc.0.push(Block::Paragraph(vec![Inline::Link {
                            text: format!("{} in rupta", summary.short_id),
                            url: summary.url.clone(),
                        }]));
                        doc
                    }
                    None => ticket_body(&summary),
                },
            };
            let r = tracker.create_ticket(&target, &draft).await;
            count_request(state, integration, "create", &r);
            (r, true)
        }
        NewLink::Existing { reference } => {
            let iid = integration.id;
            let default_target = state
                .db(move |c| {
                    let pid: i64 = c.query_one("SELECT project_id FROM issues WHERE id = $1", &[&issue_id])?.get(0);
                    Ok(store::defaults(c, pid, iid)?.map(|d| d.target))
                })
                .await?;
            let Some(r) = tracker.parse_reference(&reference, default_target.as_deref()) else {
                return Err(ApiError::invalid("reference", "not a ticket key or URL of this tracker"));
            };
            let res = tracker.get_ticket(&r).await;
            count_request(state, integration, "get", &res);
            if let Err(TrackerError::NotFound(_)) = &res {
                return Err(ApiError::invalid(
                    "reference",
                    "the tracker has no such ticket (or the token cannot see it)",
                ));
            }
            (res, false)
        }
    };
    let ticket = match ticket {
        Ok(t) => t,
        Err(e) => {
            record_error(state, integration.id, Some(e.to_string())).await;
            return Err(tracker_error(&e));
        }
    };
    let i = integration.clone();
    state
        .db(move |c| {
            let txn = c.transaction()?;
            let link = upsert_link(&txn, issue_id, i.id, &ticket, user)?;
            let mut data = ticket_json(&i, &ticket.key, &ticket.url);
            data["created"] = json!(created);
            data["title"] = json!(ticket.title);
            activity(&txn, issue_id, user, "ticket_linked", data)?;
            store::set_error(&txn, i.id, None)?;
            txn.commit()?;
            Ok(link)
        })
        .await
}

/// Fetches a linked ticket again and stores what the tracker says.
pub async fn refresh(state: &ApiState, integration: &IntegrationRow, link: &LinkRow) -> ApiResult<LinkRow> {
    let tracker = integration.tracker(state).map_err(|e| tracker_error(&e))?;
    let r = tracker.get_ticket(&link.external_id).await;
    count_request(state, integration, "get", &r);
    let t = match r {
        Ok(t) => t,
        Err(e) => {
            record_error(state, integration.id, Some(e.to_string())).await;
            return Err(tracker_error(&e));
        }
    };
    let id = link.id;
    state
        .db(move |c| {
            let r = c.query_one(
                &format!(
                    "UPDATE issue_links SET external_key = $1, url = $2, title = $3, status = $4, status_category = $5, assignee = $6, synced_at = now()
                     WHERE id = $7 RETURNING {}",
                    store::LINK_COLUMNS
                ),
                &[&t.key, &t.url, &t.title, &t.status, &t.category.as_str(), &t.assignee, &id],
            )?;
            Ok(store::link_from(&r))
        })
        .await
}

/// Why an issue's status changed, for outgoing sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Resolved,
    Unresolved,
    Regressed,
}

/// A status change of a rupta issue.
#[derive(Debug, Clone)]
pub struct StatusChange {
    pub issue_id: i64,
    pub change: Change,
    /// The person who made it.
    pub actor: Option<i64>,
    /// The integration whose webhook caused it (never called back).
    pub source_integration: Option<i64>,
}

impl StatusChange {
    /// From an issue update: old and new status.
    pub fn from_statuses(issue_id: i64, old: &str, new: &str, actor: Option<i64>) -> Option<StatusChange> {
        let change = match (old, new) {
            (o, "resolved") if o != "resolved" => Change::Resolved,
            ("resolved", "unresolved") => Change::Unresolved,
            _ => return None,
        };
        Some(StatusChange { issue_id, change, actor, source_integration: None })
    }
}

/// One linked ticket to update.
struct Outgoing {
    integration: IntegrationRow,
    link: LinkRow,
    transition: bool,
    comment: bool,
}

fn comment_for(change: Change, actor: Option<&str>, url: &str, short_id: &str) -> Doc {
    let by = actor.map(|a| format!(" by {a}")).unwrap_or_default();
    let text = match change {
        Change::Resolved => format!("{short_id} was resolved in rupta{by}."),
        Change::Unresolved => format!("{short_id} was reopened in rupta{by}."),
        Change::Regressed => format!("{short_id} regressed in rupta: a new event arrived after it was resolved."),
    };
    Doc(vec![Block::Paragraph(vec![
        Inline::Text(format!("{text} ")),
        Inline::Link { text: "Open in rupta".into(), url: url.into() },
    ])])
}

/// Pushes status changes to linked tickets, in the background, and
/// resolves paging incidents of resolved issues.
pub fn status_changed(state: &ApiState, changes: Vec<StatusChange>) {
    if changes.is_empty() {
        return;
    }
    let state = state.clone();
    tokio::spawn(async move {
        for ch in changes {
            if ch.change == Change::Resolved {
                crate::notify::resolve_incidents(&state, ch.issue_id).await;
            }
            if let Err(e) = push(&state, &ch).await {
                tracing::warn!(error = %e.message, issue = ch.issue_id, "syncing linked tickets failed");
            }
        }
    });
}

async fn push(state: &ApiState, ch: &StatusChange) -> ApiResult<()> {
    let (issue_id, source, actor) = (ch.issue_id, ch.source_integration, ch.actor);
    let st = state.clone();
    let (work, actor_name, url, short_id) = state
        .db(move |c| {
            let rows = c.query(
                &format!(
                    "SELECT {}, d.outgoing_transition, d.outgoing_comment FROM issue_links l
                     JOIN issues i ON i.id = l.issue_id
                     JOIN project_integration_defaults d ON d.project_id = i.project_id AND d.integration_id = l.integration_id
                     WHERE l.issue_id = $1 AND (d.outgoing_transition OR d.outgoing_comment) ORDER BY l.id",
                    store::LINK_COLUMNS.split(", ").map(|c| format!("l.{c}")).collect::<Vec<_>>().join(", ")
                ),
                &[&issue_id],
            )?;
            let mut work = Vec::new();
            for r in rows {
                let link = store::link_from(&r);
                if Some(link.integration_id) == source {
                    continue;
                }
                let Some(i) = store::load_any(c, link.integration_id)? else { continue };
                if !i.enabled {
                    continue;
                }
                work.push(Outgoing { integration: i, link, transition: r.get(15), comment: r.get(16) });
            }
            let name: Option<String> = match actor {
                Some(u) => c
                    .query_opt("SELECT CASE WHEN name = '' THEN email ELSE name END FROM users WHERE id = $1", &[&u])?
                    .map(|r| r.get(0)),
                None => None,
            };
            let s = c.query_one(
                "SELECT o.slug, p.slug, i.short_id FROM issues i JOIN projects p ON p.id = i.project_id JOIN organizations o ON o.id = p.org_id WHERE i.id = $1",
                &[&issue_id],
            )?;
            let url = format!("{}/organizations/{}/issues/{issue_id}", st.config.public_url, s.get::<String>(0));
            let short_id = format!("{}-{}", s.get::<String>(1).to_uppercase(), s.get::<i64>(2));
            Ok((work, name, url, short_id))
        })
        .await?;
    for w in work {
        sync_one(state, ch.change, &w, actor_name.as_deref(), &url, &short_id).await;
    }
    Ok(())
}

async fn sync_one(state: &ApiState, change: Change, w: &Outgoing, actor: Option<&str>, url: &str, short_id: &str) {
    let i = &w.integration;
    let tracker = match i.tracker(state) {
        Ok(t) => t,
        Err(e) => return finish(state, i, &w.link, "sync", Err(e), None).await,
    };
    if w.comment {
        let r = tracker.add_comment(&w.link.external_id, &comment_for(change, actor, url, short_id)).await;
        count_request(state, i, "comment", &r);
        finish(state, i, &w.link, "comment", r.map(|_| None), None).await;
    }
    if w.transition {
        let to = if change == Change::Resolved { Category::Done } else { Category::Open };
        if w.link.status_category == to {
            return;
        }
        // Record what is pushed before the call: the tracker's webhook may
        // arrive before the answer.
        let id = w.link.id;
        let until = Utc::now() + Duration::seconds(QUIET_PERIOD_SECONDS);
        let _ = state
            .db(move |c| {
                c.execute(
                    "UPDATE issue_links SET pushed_category = $1, pushed_until = $2 WHERE id = $3",
                    &[&to.as_str(), &until, &id],
                )?;
                Ok(())
            })
            .await;
        let r = tracker.transition(&w.link.external_id, to).await;
        count_request(state, i, "transition", &r);
        let r = match r {
            Ok(None) => Err(TrackerError::Invalid(format!(
                "the ticket has no transition to a {} status; configure the transition name",
                to.as_str()
            ))),
            other => other,
        };
        finish(state, i, &w.link, "transition", r, Some(to)).await;
    }
}

/// Records the outcome of an outgoing call: link, activity, metrics.
async fn finish(
    state: &ApiState,
    i: &IntegrationRow,
    link: &LinkRow,
    action: &'static str,
    r: Result<Option<Ticket>, TrackerError>,
    to: Option<Category>,
) {
    let result = if r.is_ok() { "success" } else { "failure" };
    state.metrics.count(
        "rupta_integration_sync_total",
        &[("kind", i.kind.as_str()), ("direction", "outgoing"), ("result", result)],
    );
    let (i, link) = (i.clone(), link.clone());
    let res = state
        .db(move |c| {
            let mut data = ticket_json(&i, &link.external_key, &link.url);
            data["direction"] = json!("outgoing");
            data["action"] = json!(action);
            if let Some(to) = to {
                data["to"] = json!(to.as_str());
            }
            match &r {
                Ok(t) => {
                    if let Some(t) = t {
                        c.execute(
                            "UPDATE issue_links SET status = $1, status_category = $2, assignee = $3, title = $4, synced_at = now() WHERE id = $5",
                            &[&t.status, &t.category.as_str(), &t.assignee, &t.title, &link.id],
                        )?;
                        data["status"] = json!(t.status);
                    }
                    data["ok"] = json!(true);
                    store::set_error(c, i.id, None)?;
                }
                Err(e) => {
                    data["ok"] = json!(false);
                    data["error"] = json!(e.to_string());
                    if to.is_some() {
                        // Nothing was pushed: no echo to expect.
                        c.execute("UPDATE issue_links SET pushed_category = NULL, pushed_until = NULL WHERE id = $1", &[&link.id])?;
                    }
                    store::set_error(c, i.id, Some(&e.to_string()))?;
                }
            }
            activity(c, link.issue_id, None, "ticket_sync", data)
        })
        .await;
    if let Err(e) = res {
        tracing::warn!(error = %e.message, "recording a ticket sync failed");
    }
}

/// The answer to a webhook.
#[derive(Debug)]
pub enum WebhookOutcome {
    Processed { updated_links: usize, changed_issues: usize, echoes: usize },
    Duplicate,
}

/// Verifies and applies an incoming webhook.
pub async fn webhook(
    state: &ApiState,
    kind: &str,
    integration_id: i64,
    req: WebhookRequest<'_>,
) -> ApiResult<WebhookOutcome> {
    let count =
        |result: &str| state.metrics.count("rupta_integration_webhooks_total", &[("kind", kind), ("result", result)]);
    let row = state.db(move |c| store::load_any(c, integration_id)).await?;
    let Some(row) = row.filter(|r| r.kind.as_str() == kind && r.enabled) else {
        count("unauthorized");
        return Err(ApiError::unauthorized());
    };
    // Verification needs only the webhook secret.
    let settings = Settings { kind: row.kind, config: row.config.clone(), token: row.token(state).unwrap_or_default() };
    let tracker = AnyTracker::new(&settings, state.outbound.clone()).map_err(|e| tracker_error(&e))?;
    let secret = row.webhook_secret(state);
    let parsed = match tracker.parse_webhook(&req, secret.as_deref()) {
        Ok(p) => p,
        Err(WebhookError::Unauthorized) => {
            count("unauthorized");
            return Err(ApiError::new(
                axum::http::StatusCode::UNAUTHORIZED,
                "unauthorized",
                "invalid webhook signature or token",
            ));
        }
        Err(WebhookError::Invalid(m)) => {
            count("invalid");
            return Err(ApiError::bad_request(m));
        }
    };
    let r = row.clone();
    let outcome = state
        .db(move |c| {
            let txn = c.transaction()?;
            if let Some(d) = &parsed.delivery_id {
                let fresh = txn.execute(
                    "INSERT INTO integration_deliveries (integration_id, delivery_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
                    &[&r.id, &super::truncate(d, 200)],
                )?;
                if fresh == 0 {
                    return Ok((WebhookOutcome::Duplicate, Vec::new()));
                }
                let old = Utc::now() - Duration::days(7);
                txn.execute("DELETE FROM integration_deliveries WHERE integration_id = $1 AND received_at < $2", &[&r.id, &old])?;
            }
            let mut updated = 0;
            let mut echoes = 0;
            let mut changes = Vec::new();
            for u in &parsed.updates {
                let links: Vec<LinkRow> = txn
                    .query(&format!("SELECT {} FROM issue_links WHERE integration_id = $1 AND external_id = $2", store::LINK_COLUMNS), &[&r.id, &u.external_id])?
                    .iter()
                    .map(store::link_from)
                    .collect();
                for link in links {
                    updated += 1;
                    match apply_update(&txn, &r, &link, u)? {
                        Incoming::Changed(ch) => changes.push(ch),
                        Incoming::Echo => echoes += 1,
                        Incoming::Updated => {}
                    }
                }
            }
            txn.commit()?;
            let n = changes.len();
            Ok((WebhookOutcome::Processed { updated_links: updated, changed_issues: n, echoes }, changes))
        })
        .await?;
    let (outcome, changes) = outcome;
    match &outcome {
        WebhookOutcome::Duplicate => count("duplicate"),
        WebhookOutcome::Processed { updated_links, changed_issues, echoes } => {
            count("accepted");
            let sync = |result: &str, n: usize| {
                state.metrics.add(
                    "rupta_integration_sync_total",
                    &[("kind", kind), ("direction", "incoming"), ("result", result)],
                    n as u64,
                );
            };
            sync("applied", *changed_issues);
            sync("echo", *echoes);
            sync("updated", updated_links.saturating_sub(changed_issues + echoes));
        }
    }
    status_changed(state, changes);
    Ok(outcome)
}

/// What an incoming update did.
enum Incoming {
    /// Only the stored ticket changed.
    Updated,
    /// A status change within the quiet period after rupta moved the ticket.
    Echo,
    /// The rupta issue was resolved or unresolved.
    Changed(StatusChange),
}

/// Applies one ticket update to one link.
fn apply_update(c: &Connection, i: &IntegrationRow, link: &LinkRow, u: &super::TicketUpdate) -> ApiResult<Incoming> {
    let now = Utc::now();
    if u.deleted {
        c.execute("UPDATE issue_links SET status = 'deleted', synced_at = now() WHERE id = $1", &[&link.id])?;
        let mut data = ticket_json(i, &link.external_key, &link.url);
        data["direction"] = json!("incoming");
        data["status"] = json!("deleted");
        activity(c, link.issue_id, None, "ticket_status_change", data)?;
        return Ok(Incoming::Updated);
    }
    let category = u.category.unwrap_or(link.status_category);
    let assignee = match &u.assignee {
        Some(a) => a.clone(),
        None => link.assignee.clone(),
    };
    c.execute(
        "UPDATE issue_links SET external_key = coalesce($1, external_key), url = coalesce($2, url), title = coalesce($3, title),
             status = coalesce($4, status), status_category = $5, assignee = $6, synced_at = now() WHERE id = $7",
        &[&u.key, &u.url, &u.title, &u.status, &category.as_str(), &assignee, &link.id],
    )?;
    if category == link.status_category {
        return Ok(Incoming::Updated);
    }
    let mut data =
        ticket_json(i, u.key.as_deref().unwrap_or(&link.external_key), u.url.as_deref().unwrap_or(&link.url));
    data["direction"] = json!("incoming");
    data["status"] = json!(u.status);
    data["category"] = json!(category.as_str());
    data["old_category"] = json!(link.status_category.as_str());
    activity(c, link.issue_id, None, "ticket_status_change", data.clone())?;
    if link.pushed_until.is_some_and(|t| t > now) {
        return Ok(Incoming::Echo);
    }
    let issue = c.query_opt("SELECT project_id, status, substatus FROM issues WHERE id = $1", &[&link.issue_id])?;
    let Some(issue) = issue else { return Ok(Incoming::Updated) };
    let (project_id, status, substatus): (i64, String, String) = (issue.get(0), issue.get(1), issue.get(2));
    let Some(d) = store::defaults(c, project_id, i.id)? else { return Ok(Incoming::Updated) };
    if !d.incoming_sync {
        return Ok(Incoming::Updated);
    }
    let change = match (category, status.as_str()) {
        (Category::Done, "unresolved") => {
            c.execute(
                "UPDATE issues SET status = 'resolved', resolved_at = now(), resolved_in_release = NULL, resolved_in_mode = NULL,
                     escalated_at = NULL, escalation_baseline = NULL WHERE id = $1",
                &[&link.issue_id],
            )?;
            Change::Resolved
        }
        (Category::Open, "resolved") => {
            rupta_store::ingest::reopen_issue(c, link.issue_id, "ongoing")?;
            Change::Unresolved
        }
        _ => return Ok(Incoming::Updated),
    };
    let new_status = if change == Change::Resolved { "resolved" } else { "unresolved" };
    let new_sub = if change == Change::Resolved { substatus.clone() } else { "ongoing".into() };
    activity(
        c,
        link.issue_id,
        None,
        "status_change",
        json!({"status": new_status, "substatus": new_sub, "old_status": status, "source": "integration",
               "integration_id": i.id.to_string(), "integration": i.name, "key": data["key"]}),
    )?;
    Ok(Incoming::Changed(StatusChange { issue_id: link.issue_id, change, actor: None, source_integration: Some(i.id) }))
}

/// Issues with a ticket being created by an alert rule right now.
#[derive(Default)]
pub struct InFlight(std::sync::Mutex<HashSet<(i64, i64)>>);

impl InFlight {
    fn start(&self, key: (i64, i64)) -> bool {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).insert(key)
    }
    fn end(&self, key: (i64, i64)) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).remove(&key);
    }
}

/// The `create_ticket` alert action: creates a ticket unless the issue is
/// already linked to a ticket of the integration.
pub async fn alert_create_ticket(state: &ApiState, issue_id: i64, action: &Value) -> Result<(), String> {
    let id_of = |v: &Value| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok()));
    let integration_id = action.get("integration_id").and_then(id_of).ok_or("no integration_id")?;
    let key = (issue_id, integration_id);
    if !state.ticket_in_flight.start(key) {
        return Ok(());
    }
    let result = alert_create_inner(state, issue_id, integration_id, action).await;
    state.ticket_in_flight.end(key);
    result
}

async fn alert_create_inner(
    state: &ApiState,
    issue_id: i64,
    integration_id: i64,
    action: &Value,
) -> Result<(), String> {
    let s = |k: &str| action.get(k).and_then(Value::as_str).filter(|v| !v.is_empty()).map(str::to_string);
    let found = state
        .db(move |c| {
            if c.query_opt(
                "SELECT 1 FROM issue_links WHERE issue_id = $1 AND integration_id = $2",
                &[&issue_id, &integration_id],
            )?
            .is_some()
            {
                return Ok(None);
            }
            let r = c.query_opt(
                "SELECT i.project_id, p.org_id FROM issues i JOIN projects p ON p.id = i.project_id WHERE i.id = $1",
                &[&issue_id],
            )?;
            let Some(r) = r else { return Ok(None) };
            let (project_id, org_id): (i64, i64) = (r.get(0), r.get(1));
            let Some(row) = store::load(c, org_id, integration_id)? else { return Ok(None) };
            Ok(Some((row, store::defaults(c, project_id, integration_id)?)))
        })
        .await
        .map_err(|e| e.message)?;
    let Some((row, defaults)) = found else { return Ok(()) };
    if !row.enabled {
        return Err(format!("integration {} is disabled", row.name));
    }
    let target = s("target").or_else(|| defaults.as_ref().map(|d| d.target.clone()));
    let Some(target) = target else {
        return Err(format!(
            "no target for integration {}: set one on the action or in the project defaults",
            row.name
        ));
    };
    let labels: Vec<String> = match action.get("labels").and_then(Value::as_array) {
        Some(l) => l.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
        None => defaults.as_ref().map(|d| d.labels.clone()).unwrap_or_default(),
    };
    let target = TicketTarget {
        target,
        issue_type: s("issue_type").or_else(|| defaults.as_ref().and_then(|d| d.issue_type.clone())),
        project: s("project").or_else(|| defaults.as_ref().and_then(|d| d.project.clone())),
        labels,
    };
    link(state, None, issue_id, &row, NewLink::Create { target, title: None, description: None })
        .await
        .map(|_| ())
        .map_err(|e| e.message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_changes() {
        assert_eq!(StatusChange::from_statuses(1, "unresolved", "resolved", None).unwrap().change, Change::Resolved);
        assert_eq!(StatusChange::from_statuses(1, "ignored", "resolved", None).unwrap().change, Change::Resolved);
        assert_eq!(StatusChange::from_statuses(1, "resolved", "unresolved", None).unwrap().change, Change::Unresolved);
        assert!(StatusChange::from_statuses(1, "unresolved", "ignored", None).is_none());
        assert!(StatusChange::from_statuses(1, "resolved", "resolved", None).is_none());
        assert!(StatusChange::from_statuses(1, "ignored", "unresolved", None).is_none());
    }

    #[test]
    fn comments() {
        let d = comment_for(Change::Resolved, Some("Ann"), "https://r/x", "WEB-1");
        assert!(
            d.markdown().starts_with("WEB\\-1 was resolved in rupta by Ann.")
                || d.markdown().starts_with("WEB-1 was resolved in rupta by Ann.")
        );
    }
}
