//! The alert engine: evaluates alert rules for issue signals from
//! ingestion and sends notifications.
//!
//! For each signal, the enabled rules of the issue's project are checked:
//! a rule fires when *any* of its conditions holds and *all* of its
//! filters match. Frequency conditions count the issue's events (or
//! distinct users) in the `events` table within the interval. A rule
//! fires at most once per issue within its `frequency_minutes`
//! (recorded in `alert_fires`).

use std::sync::Arc;

use chrono::{Duration, Utc};
use rupta_core::signal::IssueSignal;
use rupta_store::squad::Connection;
use serde_json::Value;
use tokio::sync::Semaphore;
use tokio::sync::broadcast::Receiver;
use tokio::sync::broadcast::error::RecvError;
use tokio::task::JoinHandle;

use crate::error::ApiResult;
use crate::notify::{self, Notification};
use crate::routes::alerts::sending_config;
use crate::state::{ApiMetrics, ApiState};

/// Notifications sent concurrently at most.
const MAX_IN_FLIGHT: usize = 16;

/// Starts the engine; it runs until the sender side is dropped.
pub fn spawn(state: ApiState, mut rx: Receiver<IssueSignal>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let limit = Arc::new(Semaphore::new(MAX_IN_FLIGHT));
        loop {
            match rx.recv().await {
                Ok(signal) => {
                    let permit = limit.clone().acquire_owned().await.expect("semaphore is never closed");
                    let state = state.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle(&state, &signal).await {
                            tracing::warn!(error = %e.message, ?signal, "alert evaluation failed");
                        }
                        drop(permit);
                    });
                }
                Err(RecvError::Lagged(n)) => {
                    tracing::warn!(missed = n, "the alert engine fell behind and skipped issue signals");
                }
                Err(RecvError::Closed) => break,
            }
        }
    })
}

/// A rule that fired, with its deliveries.
struct Firing {
    notification: Notification,
    /// (channel id, kind, decrypted configuration)
    channels: Vec<(i64, String, Value)>,
    emails: Vec<String>,
    /// `create_ticket` actions.
    tickets: Vec<Value>,
}

const LEVELS: &[&str] = &["debug", "info", "warning", "error", "fatal"];

fn level_rank(l: &str) -> usize {
    LEVELS.iter().position(|x| *x == l).unwrap_or(3)
}

fn trigger_of(signal: &IssueSignal) -> &'static str {
    match signal {
        IssueSignal::New { .. } => "first_seen",
        IssueSignal::Regressed { .. } => "regression",
        IssueSignal::Reappeared { .. } => "reappeared",
        IssueSignal::Escalating { .. } => "escalating",
        IssueSignal::Seen { .. } => "seen",
    }
}

fn count_in(c: &Connection, project: i64, issue: i64, interval: &str, users: bool) -> ApiResult<i64> {
    let d = rupta_core::query::parse_duration(interval).unwrap_or(Duration::hours(1));
    let since = Utc::now() - d;
    let expr = if users { "count(DISTINCT user_hash)" } else { "count(*)" };
    Ok(c.query_one(
        &format!("SELECT {expr} FROM events WHERE project_id = $1 AND issue_id = $2 AND timestamp >= $3"),
        &[&project, &issue, &since],
    )?
    .get(0))
}

/// Evaluates the rules for one signal and returns what to send.
fn evaluate(state: &ApiState, c: &Connection, signal: &IssueSignal) -> ApiResult<Vec<Firing>> {
    let issue_id = signal.issue_id();
    let row = c.query_opt(
        "SELECT i.project_id, i.short_id, i.title, i.culprit, i.level, i.times_seen, i.status, p.slug, p.name, o.id, o.slug
         FROM issues i JOIN projects p ON p.id = i.project_id JOIN organizations o ON o.id = p.org_id WHERE i.id = $1",
        &[&issue_id],
    )?;
    let Some(row) = row else { return Ok(Vec::new()) };
    let (project_id, org_id): (i64, i64) = (row.get(0), row.get(9));
    let issue_level: String = row.get(4);
    let status: String = row.get(6);
    if status == "ignored" {
        return Ok(Vec::new());
    }
    let rules = c.query(
        "SELECT id, name, conditions, filters, actions, frequency_minutes, environment FROM alert_rules WHERE project_id = $1 AND enabled",
        &[&project_id],
    )?;
    if rules.is_empty() {
        return Ok(Vec::new());
    }
    let (sig_env, sig_level) = match signal {
        IssueSignal::Seen { environment, level, .. } => (environment.clone(), level.clone()),
        _ => (None, None),
    };
    // The latest event, for environment and tag filters.
    let latest = c.query_opt(
        "SELECT environment, tags, level FROM events WHERE project_id = $1 AND issue_id = $2 ORDER BY timestamp DESC LIMIT 1",
        &[&project_id, &issue_id],
    )?;
    let env = sig_env.or_else(|| latest.as_ref().and_then(|r| r.get::<Option<String>>(0)));
    let tags: Value = latest.as_ref().and_then(|r| r.get::<Option<Value>>(1)).unwrap_or(Value::Null);
    let level =
        sig_level.or_else(|| latest.as_ref().and_then(|r| r.get::<Option<String>>(2))).unwrap_or(issue_level.clone());
    let tag_value = |k: &str| -> Option<String> {
        match &tags {
            Value::Object(m) => m.get(k).and_then(Value::as_str).map(str::to_string),
            Value::Array(a) => a.iter().find_map(|p| {
                let p = p.as_array()?;
                (p.first()?.as_str()? == k).then(|| p.get(1)?.as_str().map(str::to_string))?
            }),
            _ => None,
        }
    };
    let trigger = trigger_of(signal);
    let mut out = Vec::new();
    for r in rules {
        let rule_id: i64 = r.get(0);
        let conditions: Value = r.get(2);
        let filters: Value = r.get(3);
        let actions: Value = r.get(4);
        let frequency: i32 = r.get(5);
        let rule_env: Option<String> = r.get(6);

        let mut fired_by: Option<String> = None;
        for cond in conditions.as_array().into_iter().flatten() {
            let t = cond.get("type").and_then(Value::as_str).unwrap_or("");
            let hit = match t {
                "first_seen" | "regression" | "reappeared" | "escalating" => t == trigger,
                "event_frequency" | "user_frequency" => {
                    let n = cond.get("count").and_then(Value::as_i64).unwrap_or(i64::MAX);
                    let interval = cond.get("interval").and_then(Value::as_str).unwrap_or("1h");
                    count_in(c, project_id, issue_id, interval, t == "user_frequency")? >= n
                }
                _ => false,
            };
            if hit {
                fired_by = Some(t.to_string());
                break;
            }
        }
        let Some(fired_by) = fired_by else { continue };

        if rule_env.as_ref().is_some_and(|e| env.as_ref() != Some(e)) {
            continue;
        }
        let passes = filters.as_array().into_iter().flatten().all(|f| {
            let s = |k: &str| f.get(k).and_then(Value::as_str).unwrap_or("");
            match s("type") {
                "level" => {
                    if s("match") == "eq" {
                        level == s("level")
                    } else {
                        level_rank(&level) >= level_rank(s("level"))
                    }
                }
                "environment" => env.as_deref() == Some(s("value")),
                "tag" => tag_value(s("key")).as_deref() == Some(s("value")),
                _ => true,
            }
        });
        if !passes {
            continue;
        }

        // Rate limit per rule and issue; the check and the record are one transaction.
        let txn = c.transaction()?;
        let since = Utc::now() - Duration::minutes(frequency as i64);
        let recent = txn.query_opt(
            "SELECT 1 FROM alert_fires WHERE rule_id = $1 AND issue_id = $2 AND fired_at > $3 LIMIT 1",
            &[&rule_id, &issue_id, &since],
        )?;
        if recent.is_some() {
            continue;
        }
        txn.execute("INSERT INTO alert_fires (rule_id, issue_id) VALUES ($1, $2)", &[&rule_id, &issue_id])?;
        txn.commit()?;

        let mut channels = Vec::new();
        let mut tickets = Vec::new();
        let mut user_ids: Vec<i64> = Vec::new();
        let mut team_ids: Vec<i64> = Vec::new();
        let id_of = |v: &Value| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok()));
        for a in actions.as_array().into_iter().flatten() {
            match a.get("type").and_then(Value::as_str) {
                Some("channel") => {
                    if let Some(cid) = a.get("channel_id").and_then(id_of)
                        && let Some(ch) = c.query_opt(
                            "SELECT kind, config FROM notification_channels WHERE id = $1 AND org_id = $2",
                            &[&cid, &org_id],
                        )?
                    {
                        channels.push((cid, ch.get::<String>(0), sending_config(state, org_id, &ch.get::<Value>(1))));
                    }
                }
                Some("create_ticket") => tickets.push(a.clone()),
                Some("email") => {
                    user_ids
                        .extend(a.get("user_ids").and_then(Value::as_array).into_iter().flatten().filter_map(id_of));
                    team_ids
                        .extend(a.get("team_ids").and_then(Value::as_array).into_iter().flatten().filter_map(id_of));
                }
                _ => {}
            }
        }
        // Only current members of the organization who want alert emails.
        let mut recipients = user_ids;
        recipients.extend(crate::preferences::team_members(c, org_id, &team_ids)?);
        recipients.sort();
        recipients.dedup();
        let emails = crate::preferences::recipients(c, org_id, &recipients, crate::preferences::Kind::IssueAlerts)?;
        let org_slug: String = row.get(10);
        let project_slug: String = row.get(7);
        out.push(Firing {
            notification: Notification {
                trigger: fired_by,
                rule_id: Some(rule_id.to_string()),
                rule_name: r.get(1),
                url: format!("{}/organizations/{org_slug}/issues/{issue_id}", state.config.public_url),
                org_slug,
                short_id: format!("{}-{}", project_slug.to_uppercase(), row.get::<i64>(1)),
                project_slug,
                project_name: row.get(8),
                issue_id: issue_id.to_string(),
                title: row.get(2),
                culprit: row.get(3),
                level: level.clone(),
                times_seen: row.get(5),
                environment: env.clone(),
            },
            channels,
            emails,
            tickets,
        });
    }
    Ok(out)
}

/// Evaluates and delivers the notifications for one signal.
pub async fn handle(state: &ApiState, signal: &IssueSignal) -> ApiResult<()> {
    if let IssueSignal::Regressed { issue_id, .. } = signal {
        // Linked tickets hear about regressions whether or not a rule fires.
        crate::integrations::sync::status_changed(
            state,
            vec![crate::integrations::sync::StatusChange {
                issue_id: *issue_id,
                change: crate::integrations::sync::Change::Regressed,
                actor: None,
                source_integration: None,
            }],
        );
    }
    let st = state.clone();
    let sig = signal.clone();
    let firings = state.db(move |c| evaluate(&st, c, &sig)).await?;
    for f in firings {
        ApiMetrics::inc(&state.metrics.alerts_fired);
        let n = &f.notification;
        tracing::info!(rule = n.rule_name, issue = n.issue_id, trigger = n.trigger, "alert fired");
        for (channel_id, kind, config) in &f.channels {
            let r = notify::send_channel(state, kind, config, n).await;
            if r.is_ok() && notify::opens_incidents(kind) {
                notify::record_incident(state, signal.issue_id(), *channel_id).await;
            }
            record(state, kind, r);
        }
        if !f.emails.is_empty() {
            record(state, "email", notify::send_email(state, &f.emails, n).await);
        }
        for action in &f.tickets {
            if let Err(e) = crate::integrations::sync::alert_create_ticket(state, signal.issue_id(), action).await {
                tracing::warn!(error = %e, issue = n.issue_id, rule = n.rule_name, "the create_ticket action failed");
            }
        }
    }
    Ok(())
}

fn record(state: &ApiState, kind: &str, result: Result<(), String>) {
    let label = if result.is_ok() { "success" } else { "failure" };
    state.metrics.count("rupta_alert_notifications_total", &[("channel", kind), ("result", label)]);
    match result {
        Ok(()) => ApiMetrics::inc(&state.metrics.notifications_sent),
        Err(e) => {
            ApiMetrics::inc(&state.metrics.notification_failures);
            tracing::warn!(channel = kind, error = %e, "notification failed");
        }
    }
}
