//! Escalation detection, run every few minutes.
//!
//! - An unresolved issue with substatus `ongoing` (or an issue ignored
//!   "until escalating", substatus `archived_until_escalating`) becomes
//!   `unresolved` / `escalating` when its events in the last hour number
//!   at least [`MIN_EVENTS`] and more than [`FACTOR`] times its hourly
//!   average over the 7 days before that hour (events / 168, also for
//!   issues younger than 7 days). The average is stored as the issue's
//!   `escalation_baseline`; activity `set_escalating` is recorded and the
//!   signal [`IssueSignal::Escalating`] goes to the alert engine.
//! - An escalating issue returns to `ongoing` (activity `set_ongoing`) once
//!   it escalated at least 24 hours ago and every hour of the last 24 had
//!   fewer than [`CALM_FACTOR`] times the baseline events (at least: none).
//! - Issues stay `new` for 7 days after their first event; then the job
//!   moves them to `ongoing`.
//!
//! Event times are the events' own timestamps, as in the issue statistics.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use rupta_core::signal::IssueSignal;
use rupta_store::squad::Connection;
use rupta_store::{Result, Store};
use serde_json::json;

/// Escalation needs more than this multiple of the hourly average.
pub const FACTOR: f64 = 10.0;
/// ... and at least this many events in the last hour.
pub const MIN_EVENTS: i64 = 50;
/// Escalation ends when every hour of a day stays below this multiple.
pub const CALM_FACTOR: f64 = 2.0;
/// Hours of the baseline window.
const BASELINE_HOURS: i64 = 7 * 24;

/// What a run changed.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct EscalationReport {
    /// Issues that became `escalating`.
    pub escalated: Vec<i64>,
    /// Issues that went back to `ongoing`.
    pub calmed: Vec<i64>,
    /// `new` issues that became `ongoing` after 7 days.
    pub aged: u64,
    pub signals: Vec<IssueSignal>,
}

fn id_list(ids: &[i64]) -> String {
    ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",")
}

/// Event counts per issue of one project in `[from, to)`.
fn counts(
    conn: &Connection,
    project: i64,
    ids: &[i64],
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<HashMap<i64, i64>> {
    Ok(conn
        .query(
            &format!(
                "SELECT issue_id, count(*) FROM events WHERE project_id = $1 AND timestamp >= $2 AND timestamp < $3
                 AND issue_id IN ({}) GROUP BY 1",
                id_list(ids)
            ),
            &[&project, &from, &to],
        )?
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect())
}

fn by_project(rows: &[(i64, i64)]) -> HashMap<i64, Vec<i64>> {
    let mut m: HashMap<i64, Vec<i64>> = HashMap::new();
    for (id, project) in rows {
        m.entry(*project).or_default().push(*id);
    }
    m
}

/// Runs the detection once (blocking).
pub fn run_once(store: &Store, now: DateTime<Utc>) -> Result<EscalationReport> {
    let conn = store.connect();
    let aged = conn.execute(
        "UPDATE issues SET substatus = 'ongoing' WHERE status = 'unresolved' AND substatus = 'new' AND first_seen < $1",
        &[&(now - Duration::days(7))],
    )?;
    let mut report = EscalationReport { aged, ..EscalationReport::default() };

    // Escalation.
    let hour_ago = now - Duration::hours(1);
    let candidates: Vec<(i64, i64)> = conn
        .query(
            "SELECT id, project_id FROM issues
             WHERE ((status = 'unresolved' AND substatus = 'ongoing') OR (status = 'ignored' AND substatus = 'archived_until_escalating'))
               AND last_seen >= $1",
            &[&hour_ago],
        )?
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
    for (project, ids) in by_project(&candidates) {
        for chunk in ids.chunks(500) {
            let recent = counts(&conn, project, chunk, hour_ago, now + Duration::minutes(5))?;
            let busy: Vec<i64> = recent.iter().filter(|(_, n)| **n >= MIN_EVENTS).map(|(id, _)| *id).collect();
            if busy.is_empty() {
                continue;
            }
            let before = counts(&conn, project, &busy, hour_ago - Duration::hours(BASELINE_HOURS), hour_ago)?;
            for id in busy {
                let last_hour = recent[&id];
                let average = before.get(&id).copied().unwrap_or(0) as f64 / BASELINE_HOURS as f64;
                if last_hour as f64 <= FACTOR * average {
                    continue;
                }
                let txn = conn.transaction()?;
                let changed = txn.execute(
                    "UPDATE issues SET status = 'unresolved', substatus = 'escalating', escalated_at = $2, escalation_baseline = $3,
                         ignore_until = NULL, ignore_count = NULL, ignore_user_count = NULL, ignore_user_window = NULL
                     WHERE id = $1 AND ((status = 'unresolved' AND substatus = 'ongoing') OR (status = 'ignored' AND substatus = 'archived_until_escalating'))",
                    &[&id, &now, &average],
                )?;
                if changed == 1 {
                    rupta_store::ingest::add_issue_activity(
                        &txn,
                        id,
                        "set_escalating",
                        &json!({"events_last_hour": last_hour, "hourly_average": (average * 100.0).round() / 100.0}),
                    )?;
                }
                txn.commit()?;
                if changed == 1 {
                    report.escalated.push(id);
                    report.signals.push(IssueSignal::Escalating { project_id: project, issue_id: id });
                }
            }
        }
    }

    // The end of an escalation.
    let day_ago = now - Duration::hours(24);
    let escalating: Vec<(i64, i64, f64)> = conn
        .query(
            "SELECT id, project_id, coalesce(escalation_baseline, 0) FROM issues
             WHERE status = 'unresolved' AND substatus = 'escalating' AND (escalated_at IS NULL OR escalated_at <= $1)",
            &[&day_ago],
        )?
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2)))
        .collect();
    let baselines: HashMap<i64, f64> = escalating.iter().map(|(id, _, b)| (*id, *b)).collect();
    let pairs: Vec<(i64, i64)> = escalating.iter().map(|(id, p, _)| (*id, *p)).collect();
    for (project, ids) in by_project(&pairs) {
        for chunk in ids.chunks(500) {
            let mut peak: HashMap<i64, i64> = HashMap::new();
            for r in conn.query(
                &format!(
                    "SELECT issue_id, date_trunc('hour', timestamp), count(*) FROM events
                     WHERE project_id = $1 AND timestamp >= $2 AND issue_id IN ({}) GROUP BY 1, 2",
                    id_list(chunk)
                ),
                &[&project, &day_ago],
            )? {
                let e = peak.entry(r.get(0)).or_default();
                *e = (*e).max(r.get::<i64>(2));
            }
            for id in chunk {
                let limit = (CALM_FACTOR * baselines[id]).max(1.0);
                if peak.get(id).copied().unwrap_or(0) as f64 >= limit {
                    continue;
                }
                let txn = conn.transaction()?;
                let changed = txn.execute(
                    "UPDATE issues SET substatus = 'ongoing', escalated_at = NULL, escalation_baseline = NULL
                     WHERE id = $1 AND status = 'unresolved' AND substatus = 'escalating'",
                    &[id],
                )?;
                if changed == 1 {
                    rupta_store::ingest::add_issue_activity(
                        &txn,
                        *id,
                        "set_ongoing",
                        &json!({"reason": "escalation_ended"}),
                    )?;
                    report.calmed.push(*id);
                }
                txn.commit()?;
            }
        }
    }
    Ok(report)
}
