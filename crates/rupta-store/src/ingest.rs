//! Repository functions used by ingestion: reading keys and projects,
//! writing events, payloads, issues, releases, sessions, outcomes,
//! feedback, attachments, check-ins and transactions, and deleting
//! expired data.
//!
//! The functions take a [`Connection`] (or a transaction, which derefs to
//! one) so that callers decide the transaction boundaries; the batch
//! writer runs a whole batch in one transaction.

use std::collections::HashSet;

use chrono::{DateTime, NaiveDate, Utc};
use squad::{Connection, Value};

use crate::Result;

/// Microseconds between the Unix epoch and 2000-01-01, squad's epoch.
const PG_EPOCH_MICROS: i64 = 946_684_800_000_000;
/// Days between 1970-01-01 and 2000-01-01.
const PG_EPOCH_DAYS: i32 = 10_957;

/// A `timestamptz` value.
pub fn ts(dt: DateTime<Utc>) -> Value {
    Value::TimestampTz(dt.timestamp_micros() - PG_EPOCH_MICROS)
}

/// A `date` value.
pub fn date(d: NaiveDate) -> Value {
    Value::Date((d - NaiveDate::from_ymd_opt(1970, 1, 1).unwrap()).num_days() as i32 - PG_EPOCH_DAYS)
}

pub(crate) fn text(s: &Option<String>) -> Value {
    s.as_ref().map_or(Value::Null, |s| Value::Text(s.clone()))
}

fn opt_i64(v: Option<i64>) -> Value {
    v.map_or(Value::Null, Value::Int8)
}

/// A DSN key as ingestion needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyRow {
    pub id: i64,
    pub project_id: i64,
    pub public_key: String,
    pub is_active: bool,
    pub rate_limit_per_min: Option<i64>,
}

pub fn load_keys(conn: &Connection) -> Result<Vec<KeyRow>> {
    Ok(conn
        .query("SELECT id, project_id, public_key, is_active, rate_limit_per_min FROM project_keys", &[])?
        .iter()
        .map(|r| KeyRow {
            id: r.get(0),
            project_id: r.get(1),
            public_key: r.get(2),
            is_active: r.get(3),
            rate_limit_per_min: r.get::<Option<i32>>(4).map(i64::from),
        })
        .collect())
}

/// A project as ingestion needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectRow {
    pub id: i64,
    pub org_id: i64,
    pub settings: serde_json::Value,
}

pub fn load_projects(conn: &Connection) -> Result<Vec<ProjectRow>> {
    Ok(conn
        .query("SELECT id, org_id, settings FROM projects", &[])?
        .iter()
        .map(|r| ProjectRow { id: r.get(0), org_id: r.get(1), settings: r.get(2) })
        .collect())
}

/// One row of the columnar `events` table.
#[derive(Debug, Clone, Default)]
pub struct EventRow {
    pub project_id: i64,
    pub issue_id: i64,
    pub event_id: [u8; 16],
    pub timestamp: DateTime<Utc>,
    pub received: DateTime<Utc>,
    pub level: Option<String>,
    pub platform: Option<String>,
    pub environment: Option<String>,
    pub release: Option<String>,
    pub dist: Option<String>,
    pub transaction: Option<String>,
    pub message: Option<String>,
    pub exception_type: Option<String>,
    pub exception_value: Option<String>,
    pub user_id: Option<String>,
    pub user_email: Option<String>,
    pub user_ip: Option<String>,
    pub user_hash: Option<i64>,
    pub os: Option<String>,
    pub browser: Option<String>,
    pub device: Option<String>,
    pub runtime: Option<String>,
    pub sdk: Option<String>,
    pub server_name: Option<String>,
    pub url: Option<String>,
    /// JSON object `{key: value}`.
    pub tags: serde_json::Value,
    /// JSON object.
    pub contexts: serde_json::Value,
    /// `contexts.trace.trace_id` and `span_id`.
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
}

impl EventRow {
    /// The row as values in table column order, for [`append_values`].
    pub fn into_values(mut self) -> Vec<Value> {
        let tags = Value::Jsonb(Box::new(std::mem::take(&mut self.tags)));
        let contexts = Value::Jsonb(Box::new(std::mem::take(&mut self.contexts)));
        let mut v = self.values_without_json();
        v.push(tags);
        v.push(contexts);
        v.push(text(&self.trace_id));
        v.push(text(&self.span_id));
        v
    }

    fn values(&self) -> Vec<Value> {
        let mut v = self.values_without_json();
        v.push(Value::Jsonb(Box::new(self.tags.clone())));
        v.push(Value::Jsonb(Box::new(self.contexts.clone())));
        v.push(text(&self.trace_id));
        v.push(text(&self.span_id));
        v
    }

    fn values_without_json(&self) -> Vec<Value> {
        vec![
            Value::Int8(self.project_id),
            Value::Int8(self.issue_id),
            Value::Uuid(self.event_id),
            ts(self.timestamp),
            ts(self.received),
            text(&self.level),
            text(&self.platform),
            text(&self.environment),
            text(&self.release),
            text(&self.dist),
            text(&self.transaction),
            text(&self.message),
            text(&self.exception_type),
            text(&self.exception_value),
            text(&self.user_id),
            text(&self.user_email),
            text(&self.user_ip),
            opt_i64(self.user_hash),
            text(&self.os),
            text(&self.browser),
            text(&self.device),
            text(&self.runtime),
            text(&self.sdk),
            text(&self.server_name),
            text(&self.url),
        ]
    }
}

pub fn append_events(conn: &Connection, rows: &[EventRow]) -> Result<u64> {
    if rows.is_empty() {
        return Ok(0);
    }
    let values: Vec<Vec<Value>> = rows.iter().map(EventRow::values).collect();
    Ok(conn.append("events", &values)?)
}

/// Appends rows already converted to values (see [`EventRow::into_values`]
/// and [`PayloadRow::into_values`]).
pub fn append_values(conn: &Connection, table: &str, rows: &[Vec<Value>]) -> Result<u64> {
    if rows.is_empty() {
        return Ok(0);
    }
    Ok(conn.append(table, rows)?)
}

/// A compressed normalized event.
#[derive(Debug, Clone)]
pub struct PayloadRow {
    pub project_id: i64,
    pub event_id: [u8; 16],
    pub issue_id: Option<i64>,
    pub day: NaiveDate,
    /// zstd-compressed JSON.
    pub data: Vec<u8>,
}

impl PayloadRow {
    /// The row as values in table column order, for [`append_values`].
    pub fn into_values(self) -> Vec<Value> {
        vec![
            Value::Int8(self.project_id),
            Value::Uuid(self.event_id),
            opt_i64(self.issue_id),
            date(self.day),
            Value::Bytea(self.data),
        ]
    }
}

/// Inserts payloads; fails with a unique violation if one exists already.
pub fn append_payloads(conn: &Connection, rows: &[PayloadRow]) -> Result<u64> {
    let values: Vec<Vec<Value>> = rows.iter().cloned().map(PayloadRow::into_values).collect();
    append_values(conn, "event_payloads", &values)
}

/// The ids among `ids` that already have a payload.
pub fn existing_event_ids(conn: &Connection, project_id: i64, ids: &[[u8; 16]]) -> Result<HashSet<[u8; 16]>> {
    let mut out = HashSet::new();
    for chunk in ids.chunks(500) {
        let list: Vec<[u8; 16]> = chunk.to_vec();
        for r in conn.query(
            "SELECT event_id FROM event_payloads WHERE project_id = $1 AND event_id = ANY($2)",
            &[&project_id, &list],
        )? {
            out.insert(r.get::<[u8; 16]>(0));
        }
    }
    Ok(out)
}

/// The issue of a stored event, if the event is known.
pub fn event_issue_id(conn: &Connection, project_id: i64, event_id: [u8; 16]) -> Result<Option<i64>> {
    Ok(conn
        .query_opt(
            "SELECT issue_id FROM event_payloads WHERE project_id = $1 AND event_id = $2",
            &[&project_id, &event_id],
        )?
        .and_then(|r| r.get::<Option<i64>>(0)))
}

/// What ingestion needs to know about an existing issue.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueState {
    pub id: i64,
    pub project_id: i64,
    pub status: String,
    pub substatus: String,
    pub resolved_at: Option<DateTime<Utc>>,
    pub resolved_in_release: Option<String>,
    pub ignore_until: Option<DateTime<Utc>>,
    /// Absolute threshold: the ignore ends when `times_seen` reaches it.
    pub ignore_count: Option<i64>,
    pub times_seen: i64,
}

const ISSUE_STATE_COLUMNS: &str =
    "id, project_id, status, substatus, resolved_at, resolved_in_release, ignore_until, ignore_count, times_seen";

fn issue_state(r: &squad::Row) -> IssueState {
    IssueState {
        id: r.get(0),
        project_id: r.get(1),
        status: r.get(2),
        substatus: r.get(3),
        resolved_at: r.get(4),
        resolved_in_release: r.get(5),
        ignore_until: r.get(6),
        ignore_count: r.get(7),
        times_seen: r.get(8),
    }
}

pub fn find_issue_id(conn: &Connection, project_id: i64, fingerprint_hash: &str) -> Result<Option<i64>> {
    Ok(conn
        .query_opt(
            "SELECT id FROM issues WHERE project_id = $1 AND fingerprint_hash = $2",
            &[&project_id, &fingerprint_hash],
        )?
        .map(|r| r.get(0)))
}

/// Current states of the given issues (missing ones were deleted).
pub fn issue_states(conn: &Connection, ids: &[i64]) -> Result<Vec<IssueState>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let list = ids.to_vec();
    Ok(conn
        .query(&format!("SELECT {ISSUE_STATE_COLUMNS} FROM issues WHERE id = ANY($1)"), &[&list])?
        .iter()
        .map(issue_state)
        .collect())
}

#[derive(Debug, Clone)]
pub struct NewIssue {
    pub project_id: i64,
    pub fingerprint_hash: String,
    pub grouping_version: String,
    pub title: String,
    pub culprit: Option<String>,
    pub ty: String,
    pub level: String,
    pub platform: Option<String>,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub release: Option<String>,
    pub metadata: serde_json::Value,
}

/// Creates an issue with the project's next short id (counters start at
/// zero; the batch writer adds the events). Returns `(id, short_id)`.
pub fn create_issue(conn: &Connection, n: &NewIssue) -> Result<(i64, i64)> {
    let short_id: i64 = conn
        .query_one(
            "UPDATE projects SET next_short_id = next_short_id + 1 WHERE id = $1 RETURNING next_short_id - 1",
            &[&n.project_id],
        )?
        .get(0);
    let id: i64 = conn
        .query_one(
            "INSERT INTO issues (project_id, short_id, fingerprint_hash, grouping_version, title, culprit, type, level,
                platform, status, substatus, first_seen, last_seen, times_seen, users_seen, first_release, last_release, metadata)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'unresolved', 'new', $10, $11, 0, 0, $12, $12, $13)
             RETURNING id",
            &[
                &n.project_id,
                &short_id,
                &n.fingerprint_hash,
                &n.grouping_version,
                &n.title,
                &n.culprit,
                &n.ty,
                &n.level,
                &n.platform,
                &n.first_seen,
                &n.last_seen,
                &n.release,
                &n.metadata,
            ],
        )?
        .get(0);
    Ok((id, short_id))
}

/// Counters of one issue, aggregated over a batch.
#[derive(Debug, Clone, Default)]
pub struct IssueCounts {
    pub id: i64,
    pub count: i64,
    pub users_added: i64,
    pub first_seen: Option<DateTime<Utc>>,
    pub last_seen: Option<DateTime<Utc>>,
    pub last_release: Option<String>,
}

pub fn add_issue_counts(conn: &Connection, c: &IssueCounts) -> Result<()> {
    conn.execute(
        "UPDATE issues SET times_seen = times_seen + $2, users_seen = users_seen + $3,
             first_seen = LEAST(first_seen, COALESCE($4, first_seen)),
             last_seen = GREATEST(last_seen, COALESCE($5, last_seen)),
             last_release = COALESCE($6, last_release),
             first_release = COALESCE(first_release, $6)
         WHERE id = $1",
        &[&c.id, &c.count, &c.users_added, &c.first_seen, &c.last_seen, &c.last_release],
    )?;
    Ok(())
}

/// Sets status and substatus, clearing resolution and ignore conditions.
pub fn reopen_issue(conn: &Connection, id: i64, substatus: &str) -> Result<()> {
    conn.execute(
        "UPDATE issues SET status = 'unresolved', substatus = $2, resolved_at = NULL, resolved_in_release = NULL,
             ignore_until = NULL, ignore_count = NULL
         WHERE id = $1",
        &[&id, &substatus],
    )?;
    Ok(())
}

pub fn add_issue_activity(conn: &Connection, issue_id: i64, kind: &str, data: &serde_json::Value) -> Result<()> {
    conn.execute("INSERT INTO issue_activity (issue_id, kind, data) VALUES ($1, $2, $3)", &[&issue_id, &kind, data])?;
    Ok(())
}

/// Records events of a release: creates the release and its link to the
/// project if needed and widens its first/last event times.
pub fn upsert_release(
    conn: &Connection,
    org_id: i64,
    project_id: i64,
    version: &str,
    first: DateTime<Utc>,
    last: DateTime<Utc>,
    new_issues: i64,
) -> Result<()> {
    let id: i64 = conn
        .query_one(
            "INSERT INTO releases (org_id, version, first_event, last_event) VALUES ($1, $2, $3, $4)
             ON CONFLICT (org_id, version) DO UPDATE SET
                 first_event = LEAST(COALESCE(releases.first_event, EXCLUDED.first_event), EXCLUDED.first_event),
                 last_event = GREATEST(COALESCE(releases.last_event, EXCLUDED.last_event), EXCLUDED.last_event)
             RETURNING id",
            &[&org_id, &version, &first, &last],
        )?
        .get(0);
    conn.execute(
        "INSERT INTO release_projects (release_id, project_id, new_issues) VALUES ($1, $2, $3)
         ON CONFLICT (release_id, project_id) DO UPDATE SET new_issues = release_projects.new_issues + EXCLUDED.new_issues",
        &[&id, &project_id, &new_issues],
    )?;
    Ok(())
}

/// When a release was first seen (created), if it exists.
pub fn release_created_at(conn: &Connection, org_id: i64, version: &str) -> Result<Option<DateTime<Utc>>> {
    Ok(conn
        .query_opt("SELECT created_at FROM releases WHERE org_id = $1 AND version = $2", &[&org_id, &version])?
        .map(|r| r.get(0)))
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SessionCount {
    pub project_id: i64,
    pub release: String,
    pub environment: String,
    pub hour: DateTime<Utc>,
    pub total: i64,
    pub errored: i64,
    pub crashed: i64,
    pub abnormal: i64,
}

pub fn add_session_counts(conn: &Connection, rows: &[SessionCount]) -> Result<()> {
    for r in rows {
        conn.execute(
            "INSERT INTO session_counts (project_id, release, environment, hour, total, errored, crashed, abnormal)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (project_id, release, environment, hour) DO UPDATE SET
                 total = session_counts.total + EXCLUDED.total,
                 errored = session_counts.errored + EXCLUDED.errored,
                 crashed = session_counts.crashed + EXCLUDED.crashed,
                 abnormal = session_counts.abnormal + EXCLUDED.abnormal",
            &[&r.project_id, &r.release, &r.environment, &r.hour, &r.total, &r.errored, &r.crashed, &r.abnormal],
        )?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OutcomeKey {
    pub project_id: i64,
    pub hour: DateTime<Utc>,
    pub category: String,
    pub outcome: String,
    pub reason: String,
}

pub fn add_outcomes(conn: &Connection, rows: &[(OutcomeKey, i64)]) -> Result<()> {
    for (k, n) in rows {
        conn.execute(
            "INSERT INTO outcomes (project_id, hour, category, outcome, reason, quantity) VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (project_id, hour, category, outcome, reason) DO UPDATE SET quantity = outcomes.quantity + EXCLUDED.quantity",
            &[&k.project_id, &k.hour, &k.category, &k.outcome, &k.reason, n],
        )?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub struct FeedbackRow {
    pub project_id: i64,
    pub event_id: Option<[u8; 16]>,
    pub issue_id: Option<i64>,
    pub name: Option<String>,
    pub email: Option<String>,
    pub message: String,
    pub url: Option<String>,
    pub created_at: DateTime<Utc>,
}

pub fn insert_feedback(conn: &Connection, f: &FeedbackRow) -> Result<i64> {
    Ok(conn
        .query_one(
            "INSERT INTO user_feedback (project_id, event_id, issue_id, name, email, message, url, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id",
            &[&f.project_id, &f.event_id, &f.issue_id, &f.name, &f.email, &f.message, &f.url, &f.created_at],
        )?
        .get(0))
}

/// Links feedback that arrived before its event to the event's issue.
pub fn link_feedback(conn: &Connection, project_id: i64, event_id: [u8; 16], issue_id: i64) -> Result<u64> {
    Ok(conn.execute(
        "UPDATE user_feedback SET issue_id = $3 WHERE project_id = $1 AND event_id = $2 AND issue_id IS NULL",
        &[&project_id, &event_id, &issue_id],
    )?)
}

#[derive(Debug, Clone, PartialEq)]
pub struct AttachmentRow {
    pub project_id: i64,
    pub event_id: [u8; 16],
    pub name: String,
    pub content_type: Option<String>,
    pub size: i64,
    pub object_key: String,
    pub day: NaiveDate,
}

pub fn insert_attachment(conn: &Connection, a: &AttachmentRow) -> Result<i64> {
    Ok(conn
        .query_one(
            "INSERT INTO attachments (project_id, event_id, name, content_type, size, object_key, day)
             VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
            &[&a.project_id, &a.event_id, &a.name, &a.content_type, &a.size, &a.object_key, &a.day],
        )?
        .get(0))
}

/// A cron check-in.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckIn {
    pub project_id: i64,
    pub monitor_slug: String,
    pub check_in_id: [u8; 16],
    /// in_progress, ok, error
    pub status: String,
    pub environment: Option<String>,
    pub duration_ms: Option<i64>,
    pub received: DateTime<Utc>,
    /// `monitor_config` from the SDK, already validated: creates or
    /// updates the monitor's schedule.
    pub config: Option<MonitorConfig>,
    pub next_checkin: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MonitorConfig {
    /// `{type: crontab|interval, value, unit}`
    pub schedule: serde_json::Value,
    pub timezone: String,
    pub checkin_margin: Option<i32>,
    pub max_runtime: Option<i32>,
}

/// Creates the monitor if needed (updating its schedule when the SDK sent
/// one) and records the check-in. A check-in id seen before updates that
/// check-in (in_progress followed by ok/error). Returns the monitor id.
pub fn record_checkin(conn: &Connection, c: &CheckIn) -> Result<i64> {
    let monitor_id: i64 = match &c.config {
        Some(cfg) => conn
            .query_one(
                "INSERT INTO monitors (project_id, slug, name, schedule, timezone, checkin_margin, max_runtime)
                 VALUES ($1, $2, $2, $3, $4, $5, $6)
                 ON CONFLICT (project_id, slug) DO UPDATE SET schedule = EXCLUDED.schedule, timezone = EXCLUDED.timezone,
                     checkin_margin = EXCLUDED.checkin_margin, max_runtime = EXCLUDED.max_runtime
                 RETURNING id",
                &[&c.project_id, &c.monitor_slug, &cfg.schedule, &cfg.timezone, &cfg.checkin_margin, &cfg.max_runtime],
            )?
            .get(0),
        None => {
            conn.execute(
                "INSERT INTO monitors (project_id, slug, name) VALUES ($1, $2, $2) ON CONFLICT (project_id, slug) DO NOTHING",
                &[&c.project_id, &c.monitor_slug],
            )?;
            conn.query_one("SELECT id FROM monitors WHERE project_id = $1 AND slug = $2", &[&c.project_id, &c.monitor_slug])?
                .get(0)
        }
    };
    let previous = conn.query_opt("SELECT created_at, monitor_id FROM checkins WHERE id = $1", &[&c.check_in_id])?;
    match previous {
        Some(row) if row.get::<i64>(1) == monitor_id => {
            let created: DateTime<Utc> = row.get(0);
            let duration = c
                .duration_ms
                .or_else(|| (c.status != "in_progress").then(|| (c.received - created).num_milliseconds()));
            conn.execute(
                "UPDATE checkins SET status = $2, duration_ms = COALESCE($3, duration_ms), updated_at = $4,
                     environment = COALESCE($5, environment)
                 WHERE id = $1",
                &[&c.check_in_id, &c.status, &duration, &c.received, &c.environment],
            )?;
        }
        Some(_) => {} // a check-in id of another monitor: ignored
        None => {
            conn.execute(
                "INSERT INTO checkins (id, monitor_id, status, environment, duration_ms, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $6)",
                &[&c.check_in_id, &monitor_id, &c.status, &c.environment, &c.duration_ms, &c.received],
            )?;
        }
    }
    let health = match c.status.as_str() {
        "ok" => Some("ok"),
        "error" => Some("error"),
        _ => None,
    };
    conn.execute(
        "UPDATE monitors SET last_checkin = GREATEST(COALESCE(last_checkin, $2), $2), health = COALESCE($3, health),
             next_checkin = COALESCE($4, next_checkin)
         WHERE id = $1",
        &[&monitor_id, &c.received, &health, &c.next_checkin],
    )?;
    Ok(monitor_id)
}

/// One row of the columnar `transactions` table.
#[derive(Debug, Clone, Default)]
pub struct TransactionRow {
    pub project_id: i64,
    pub event_id: [u8; 16],
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub parent_span_id: Option<String>,
    pub name: Option<String>,
    pub op: Option<String>,
    pub status: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub start_timestamp: Option<DateTime<Utc>>,
    pub duration_ms: Option<f64>,
    pub received: DateTime<Utc>,
    pub environment: Option<String>,
    pub release: Option<String>,
    pub platform: Option<String>,
    pub sdk: Option<String>,
    pub user_hash: Option<i64>,
    pub span_count: Option<i32>,
    pub tags: String,
    pub measurements: String,
    /// Web vitals: LCP, FCP, INP and TTFB in milliseconds, CLS unitless.
    pub lcp: Option<f64>,
    pub fcp: Option<f64>,
    pub cls: Option<f64>,
    pub inp: Option<f64>,
    pub ttfb: Option<f64>,
    /// `contexts.profile.profile_id` (transaction profiling).
    pub profile_id: Option<String>,
    /// `contexts.profile.profiler_id` (continuous profiling).
    pub profiler_id: Option<String>,
}

pub fn append_transactions(conn: &Connection, rows: &[TransactionRow]) -> Result<u64> {
    if rows.is_empty() {
        return Ok(0);
    }
    let values: Vec<Vec<Value>> = rows
        .iter()
        .map(|r| {
            vec![
                Value::Int8(r.project_id),
                Value::Uuid(r.event_id),
                text(&r.trace_id),
                text(&r.span_id),
                text(&r.parent_span_id),
                text(&r.name),
                text(&r.op),
                text(&r.status),
                ts(r.timestamp),
                r.start_timestamp.map_or(Value::Null, ts),
                r.duration_ms.map_or(Value::Null, Value::Float8),
                ts(r.received),
                text(&r.environment),
                text(&r.release),
                text(&r.platform),
                text(&r.sdk),
                opt_i64(r.user_hash),
                r.span_count.map_or(Value::Null, Value::Int4),
                Value::Text(r.tags.clone()),
                Value::Text(r.measurements.clone()),
                r.lcp.map_or(Value::Null, Value::Float8),
                r.fcp.map_or(Value::Null, Value::Float8),
                r.cls.map_or(Value::Null, Value::Float8),
                r.inp.map_or(Value::Null, Value::Float8),
                r.ttfb.map_or(Value::Null, Value::Float8),
                text(&r.profile_id),
                text(&r.profiler_id),
            ]
        })
        .collect();
    Ok(conn.append("transactions", &values)?)
}

/// Distinct user hashes of an issue's stored events (at most `limit`),
/// to continue counting distinct users after a restart.
pub fn issue_user_hashes(conn: &Connection, project_id: i64, issue_id: i64, limit: i64) -> Result<Vec<i64>> {
    Ok(conn
        .query(
            "SELECT DISTINCT user_hash FROM events WHERE project_id = $1 AND issue_id = $2 AND user_hash IS NOT NULL LIMIT $3",
            &[&project_id, &issue_id, &limit],
        )?
        .iter()
        .map(|r| r.get(0))
        .collect())
}

/// Number of artifact bundles of an organization (symbolication skips
/// organizations without any).
pub fn artifact_bundle_count(conn: &Connection, org_id: i64) -> Result<i64> {
    Ok(conn.query_one("SELECT count(*) FROM artifact_bundles WHERE org_id = $1", &[&org_id])?.get(0))
}

/// A file in an artifact bundle.
#[derive(Debug, Clone, PartialEq)]
pub struct ArtifactFile {
    pub bundle_id: i64,
    pub object_key: String,
    pub file_path: String,
    pub file_type: String,
    pub url: Option<String>,
}

const ARTIFACT_SELECT: &str = "SELECT i.bundle_id, b.object_key, i.file_path, i.file_type, i.url
     FROM artifact_index i JOIN artifact_bundles b ON b.id = i.bundle_id";

fn artifact_file(r: &squad::Row) -> ArtifactFile {
    ArtifactFile { bundle_id: r.get(0), object_key: r.get(1), file_path: r.get(2), file_type: r.get(3), url: r.get(4) }
}

/// Files with this debug id, newest bundle first.
pub fn artifacts_by_debug_id(conn: &Connection, org_id: i64, debug_id: [u8; 16]) -> Result<Vec<ArtifactFile>> {
    Ok(conn
        .query(
            &format!("{ARTIFACT_SELECT} WHERE i.org_id = $1 AND i.debug_id = $2 ORDER BY i.bundle_id DESC"),
            &[&org_id, &debug_id],
        )?
        .iter()
        .map(artifact_file)
        .collect())
}

/// Files of a release (and dist) at one of the given URLs, newest first.
pub fn artifacts_by_url(
    conn: &Connection,
    org_id: i64,
    release: &str,
    dist: Option<&str>,
    urls: &[String],
) -> Result<Vec<ArtifactFile>> {
    let list = urls.to_vec();
    Ok(conn
        .query(
            &format!(
                "{ARTIFACT_SELECT} WHERE i.org_id = $1 AND i.release = $2 AND i.url = ANY($3)
                   AND (i.dist IS NOT DISTINCT FROM $4) ORDER BY i.bundle_id DESC"
            ),
            &[&org_id, &release, &list, &dist],
        )?
        .iter()
        .map(artifact_file)
        .collect())
}

/// What a retention run deleted.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Deleted {
    pub events: u64,
    pub payloads: u64,
    pub attachments: u64,
    pub transactions: u64,
    pub session_counts: u64,
    pub outcomes: u64,
    pub checkins: u64,
    pub issues: u64,
}

/// The oldest event time of a project, if it has events.
pub fn oldest_event(conn: &Connection, project_id: i64) -> Result<Option<DateTime<Utc>>> {
    Ok(conn.query_one("SELECT min(timestamp) FROM events WHERE project_id = $1", &[&project_id])?.get(0))
}

/// Deletes events of a project in `[from, to)`. Transactions follow the
/// trace retention (see [`crate::telemetry::delete_range`]).
pub fn delete_events_range(conn: &Connection, project_id: i64, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<u64> {
    Ok(conn.execute(
        "DELETE FROM events WHERE project_id = $1 AND timestamp >= $2 AND timestamp < $3",
        &[&project_id, &from, &to],
    )?)
}

/// Object keys of a project's attachments stored before `day`.
pub fn expired_attachments(conn: &Connection, project_id: i64, day: NaiveDate) -> Result<Vec<(i64, String)>> {
    Ok(conn
        .query("SELECT id, object_key FROM attachments WHERE project_id = $1 AND day < $2", &[&project_id, &day])?
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect())
}

pub fn delete_attachment_rows(conn: &Connection, ids: &[i64]) -> Result<u64> {
    if ids.is_empty() {
        return Ok(0);
    }
    let list = ids.to_vec();
    Ok(conn.execute("DELETE FROM attachments WHERE id = ANY($1)", &[&list])?)
}

/// Deletes a project's row-store data older than `cutoff`: payloads,
/// session counts, outcomes, check-ins and issues not seen since.
pub fn delete_rows_before(conn: &Connection, project_id: i64, cutoff: DateTime<Utc>, d: &mut Deleted) -> Result<()> {
    let day = cutoff.date_naive();
    d.payloads +=
        conn.execute("DELETE FROM event_payloads WHERE project_id = $1 AND day < $2", &[&project_id, &day])?;
    d.session_counts +=
        conn.execute("DELETE FROM session_counts WHERE project_id = $1 AND hour < $2", &[&project_id, &cutoff])?;
    d.outcomes += conn.execute("DELETE FROM outcomes WHERE project_id = $1 AND hour < $2", &[&project_id, &cutoff])?;
    d.checkins += conn.execute(
        "DELETE FROM checkins WHERE created_at < $2 AND monitor_id IN (SELECT id FROM monitors WHERE project_id = $1)",
        &[&project_id, &cutoff],
    )?;
    d.issues += conn.execute("DELETE FROM issues WHERE project_id = $1 AND last_seen < $2", &[&project_id, &cutoff])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use chrono::TimeZone;

    fn seed(conn: &Connection) -> i64 {
        conn.batch_execute(
            "INSERT INTO organizations (slug, name) VALUES ('o', 'O');
             INSERT INTO projects (org_id, slug, name) VALUES (1, 'p', 'P');
             INSERT INTO project_keys (project_id, public_key, secret_key) VALUES (1, 'pk', 'sk');",
        )
        .unwrap();
        1
    }

    #[test]
    fn epoch_conversions() {
        assert_eq!(ts(Utc.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).unwrap()), Value::TimestampTz(0));
        assert_eq!(date(NaiveDate::from_ymd_opt(2000, 1, 2).unwrap()), Value::Date(1));
    }

    #[test]
    fn write_path_round_trip() {
        let store = Store::open_in_memory().unwrap();
        let conn = store.connect();
        let p = seed(&conn);
        assert_eq!(load_keys(&conn).unwrap()[0].public_key, "pk");
        assert_eq!(load_projects(&conn).unwrap()[0].org_id, 1);
        let now = Utc::now();
        let n = NewIssue {
            project_id: p,
            fingerprint_hash: "h".into(),
            grouping_version: "v1".into(),
            title: "T".into(),
            culprit: None,
            ty: "error".into(),
            level: "error".into(),
            platform: None,
            first_seen: now,
            last_seen: now,
            release: Some("1.0".into()),
            metadata: serde_json::json!({}),
        };
        let (id, short) = create_issue(&conn, &n).unwrap();
        let (_, short2) = create_issue(&conn, &NewIssue { fingerprint_hash: "h2".into(), ..n.clone() }).unwrap();
        assert_eq!((short, short2), (1, 2));
        assert_eq!(find_issue_id(&conn, p, "h").unwrap(), Some(id));
        add_issue_counts(
            &conn,
            &IssueCounts { id, count: 3, users_added: 1, last_seen: Some(now), ..Default::default() },
        )
        .unwrap();
        let st = &issue_states(&conn, &[id]).unwrap()[0];
        assert_eq!(st.status, "unresolved");
        let eid = [7u8; 16];
        append_events(
            &conn,
            &[EventRow {
                project_id: p,
                issue_id: id,
                event_id: eid,
                timestamp: now,
                received: now,
                user_hash: Some(5),
                tags: serde_json::json!({}),
                contexts: serde_json::json!({}),
                ..Default::default()
            }],
        )
        .unwrap();
        append_payloads(
            &conn,
            &[PayloadRow { project_id: p, event_id: eid, issue_id: Some(id), day: now.date_naive(), data: vec![1] }],
        )
        .unwrap();
        let dup = append_payloads(
            &conn,
            &[PayloadRow { project_id: p, event_id: eid, issue_id: Some(id), day: now.date_naive(), data: vec![1] }],
        );
        assert!(dup.unwrap_err().is_unique_violation());
        assert!(existing_event_ids(&conn, p, &[eid, [8; 16]]).unwrap().contains(&eid));
        assert_eq!(event_issue_id(&conn, p, eid).unwrap(), Some(id));
        assert_eq!(issue_user_hashes(&conn, p, id, 10).unwrap(), vec![5]);

        upsert_release(&conn, 1, p, "1.0", now, now, 1).unwrap();
        upsert_release(&conn, 1, p, "1.0", now, now, 1).unwrap();
        let nr: i64 = conn.query_one("SELECT new_issues FROM release_projects", &[]).unwrap().get(0);
        assert_eq!(nr, 2);

        let hour = Utc.with_ymd_and_hms(2026, 11, 14, 10, 0, 0).unwrap();
        let sc = SessionCount {
            project_id: p,
            release: "1.0".into(),
            environment: "prod".into(),
            hour,
            total: 2,
            errored: 1,
            ..Default::default()
        };
        add_session_counts(&conn, &[sc.clone(), sc]).unwrap();
        let (t, e): (i64, i64) = {
            let r = conn.query_one("SELECT total, errored FROM session_counts", &[]).unwrap();
            (r.get(0), r.get(1))
        };
        assert_eq!((t, e), (4, 2));

        let k = OutcomeKey {
            project_id: p,
            hour,
            category: "error".into(),
            outcome: "accepted".into(),
            reason: String::new(),
        };
        add_outcomes(&conn, &[(k.clone(), 2), (k, 3)]).unwrap();
        let q: i64 = conn.query_one("SELECT quantity FROM outcomes", &[]).unwrap().get(0);
        assert_eq!(q, 5);

        let cid = [9u8; 16];
        let mut c = CheckIn {
            project_id: p,
            monitor_slug: "nightly".into(),
            check_in_id: cid,
            status: "in_progress".into(),
            environment: None,
            duration_ms: None,
            received: now,
            config: Some(MonitorConfig {
                schedule: serde_json::json!({"type": "crontab", "value": "0 * * * *"}),
                timezone: "UTC".into(),
                checkin_margin: Some(5),
                max_runtime: None,
            }),
            next_checkin: None,
        };
        let m = record_checkin(&conn, &c).unwrap();
        c.status = "ok".into();
        c.received = now + chrono::Duration::seconds(2);
        c.config = None;
        assert_eq!(record_checkin(&conn, &c).unwrap(), m);
        let r = conn.query_one("SELECT status, duration_ms FROM checkins", &[]).unwrap();
        assert_eq!(r.get::<String>(0), "ok");
        assert_eq!(r.get::<Option<i64>>(1), Some(2000));
        let health: String = conn.query_one("SELECT health FROM monitors", &[]).unwrap().get(0);
        assert_eq!(health, "ok");

        append_transactions(
            &conn,
            &[TransactionRow {
                project_id: p,
                event_id: eid,
                timestamp: now,
                received: now,
                duration_ms: Some(1.5),
                tags: "{}".into(),
                measurements: "{}".into(),
                ..Default::default()
            }],
        )
        .unwrap();

        let fid = insert_feedback(
            &conn,
            &FeedbackRow {
                project_id: p,
                event_id: Some([3; 16]),
                issue_id: None,
                name: None,
                email: None,
                message: "m".into(),
                url: None,
                created_at: now,
            },
        )
        .unwrap();
        assert!(fid > 0);
        assert_eq!(link_feedback(&conn, p, [3; 16], id).unwrap(), 1);

        let mut d = Deleted::default();
        let range = (now - chrono::Duration::days(1), now + chrono::Duration::days(1));
        assert_eq!(delete_events_range(&conn, p, range.0, range.1).unwrap(), 1);
        assert_eq!(crate::telemetry::delete_range(&conn, "transactions", p, range.0, range.1).unwrap(), 1);
        delete_rows_before(&conn, p, now + chrono::Duration::days(1), &mut d).unwrap();
        assert_eq!((d.payloads, d.issues, d.checkins), (1, 2, 1));
    }
}
