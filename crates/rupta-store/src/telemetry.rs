//! Repository functions for traces, logs, metrics, profiles and session
//! replays: rows for the batch writer, replay upserts, and retention.

use chrono::{DateTime, NaiveDate, Utc};
use serde_json::Value as Json;
use squad::{Connection, Value};

use crate::Result;
use crate::ingest::{text, ts};

fn opt_f64(v: Option<f64>) -> Value {
    v.filter(|f| f.is_finite()).map_or(Value::Null, Value::Float8)
}

fn jsonb(v: Json) -> Value {
    if v.is_null() { Value::Null } else { Value::Jsonb(Box::new(v)) }
}

/// One row of the columnar `spans` table.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SpanRow {
    pub project_id: i64,
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub segment_id: Option<String>,
    pub is_segment: bool,
    pub transaction: Option<String>,
    pub op: Option<String>,
    pub description: Option<String>,
    pub status: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub duration_ms: Option<f64>,
    pub environment: Option<String>,
    pub release: Option<String>,
    pub platform: Option<String>,
    pub sdk: Option<String>,
    pub received: DateTime<Utc>,
    pub data: Json,
    pub tags: Json,
    pub vitals: Vitals,
}

/// Web vitals: LCP, FCP, INP and TTFB in milliseconds, CLS unitless.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Vitals {
    pub lcp: Option<f64>,
    pub fcp: Option<f64>,
    pub cls: Option<f64>,
    pub inp: Option<f64>,
    pub ttfb: Option<f64>,
}

impl SpanRow {
    /// The row as values in table column order.
    pub fn into_values(self) -> Vec<Value> {
        vec![
            Value::Int8(self.project_id),
            Value::Text(self.trace_id),
            Value::Text(self.span_id),
            text(&self.parent_span_id),
            text(&self.segment_id),
            Value::Bool(self.is_segment),
            text(&self.transaction),
            text(&self.op),
            text(&self.description),
            text(&self.status),
            ts(self.timestamp),
            opt_f64(self.duration_ms),
            text(&self.environment),
            text(&self.release),
            text(&self.platform),
            text(&self.sdk),
            ts(self.received),
            jsonb(self.data),
            jsonb(self.tags),
            opt_f64(self.vitals.lcp),
            opt_f64(self.vitals.fcp),
            opt_f64(self.vitals.cls),
            opt_f64(self.vitals.inp),
            opt_f64(self.vitals.ttfb),
        ]
    }
}

/// One row of the columnar `logs` table.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LogRow {
    pub project_id: i64,
    pub id: [u8; 16],
    pub timestamp: DateTime<Utc>,
    pub received: DateTime<Utc>,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub level: String,
    pub severity_number: Option<i32>,
    pub body: String,
    pub environment: Option<String>,
    pub release: Option<String>,
    pub sdk: Option<String>,
    pub attributes: Json,
}

impl LogRow {
    pub fn into_values(self) -> Vec<Value> {
        vec![
            Value::Int8(self.project_id),
            Value::Uuid(self.id),
            ts(self.timestamp),
            ts(self.received),
            text(&self.trace_id),
            text(&self.span_id),
            Value::Text(self.level),
            self.severity_number.map_or(Value::Null, Value::Int4),
            Value::Text(self.body),
            text(&self.environment),
            text(&self.release),
            text(&self.sdk),
            jsonb(self.attributes),
        ]
    }
}

/// One row of the columnar `metrics` table.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MetricRow {
    pub project_id: i64,
    pub timestamp: DateTime<Utc>,
    pub received: DateTime<Utc>,
    pub name: String,
    /// counter, gauge, distribution or set
    pub ty: String,
    pub unit: Option<String>,
    pub value: f64,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub environment: Option<String>,
    pub release: Option<String>,
    pub attributes: Json,
}

impl MetricRow {
    pub fn into_values(self) -> Vec<Value> {
        vec![
            Value::Int8(self.project_id),
            ts(self.timestamp),
            ts(self.received),
            Value::Text(self.name),
            Value::Text(self.ty),
            text(&self.unit),
            Value::Float8(self.value),
            text(&self.trace_id),
            text(&self.span_id),
            text(&self.environment),
            text(&self.release),
            jsonb(self.attributes),
        ]
    }
}

/// A stored profile or continuous profiling chunk.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileRow {
    pub project_id: i64,
    pub profile_id: [u8; 16],
    /// `transaction` or `chunk`.
    pub kind: String,
    pub transaction_name: Option<String>,
    pub transaction_id: Option<[u8; 16]>,
    pub trace_id: Option<String>,
    pub profiler_id: Option<String>,
    pub platform: Option<String>,
    pub release: Option<String>,
    pub environment: Option<String>,
    pub started: DateTime<Utc>,
    pub duration_ms: Option<f64>,
    pub samples: i32,
    pub size: i64,
    pub object_key: String,
    pub day: NaiveDate,
    pub received: DateTime<Utc>,
}

/// Inserts a profile; a profile id seen before is left as it is.
pub fn insert_profile(conn: &Connection, p: &ProfileRow) -> Result<u64> {
    Ok(conn.execute(
        "INSERT INTO profiles (project_id, profile_id, kind, transaction_name, transaction_id, trace_id, profiler_id,
             platform, release, environment, started, duration_ms, samples, size, object_key, day, received)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)
         ON CONFLICT (project_id, profile_id) DO NOTHING",
        &[
            &p.project_id,
            &p.profile_id,
            &p.kind,
            &p.transaction_name,
            &p.transaction_id,
            &p.trace_id,
            &p.profiler_id,
            &p.platform,
            &p.release,
            &p.environment,
            &p.started,
            &p.duration_ms,
            &p.samples,
            &p.size,
            &p.object_key,
            &p.day,
            &p.received,
        ],
    )?)
}

/// Replay attributes from a `replay_event` (the first non-empty value wins).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReplayMeta {
    pub replay_type: Option<String>,
    pub platform: Option<String>,
    pub sdk: Option<String>,
    pub environment: Option<String>,
    pub release: Option<String>,
    pub dist: Option<String>,
    pub user_id: Option<String>,
    pub user_email: Option<String>,
    pub user_name: Option<String>,
    pub user_ip: Option<String>,
    pub browser: Option<String>,
    pub os: Option<String>,
    pub device: Option<String>,
}

/// A recording segment stored in the object store.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplaySegmentRow {
    pub segment_id: i32,
    pub object_key: String,
    pub size: i64,
    /// Clicks recorded in the segment.
    pub clicks: i32,
}

/// What one envelope item contributes to a replay.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplayUpdate {
    pub project_id: i64,
    pub replay_id: [u8; 16],
    pub started: Option<DateTime<Utc>>,
    pub finished: Option<DateTime<Utc>>,
    pub received: DateTime<Utc>,
    pub day: NaiveDate,
    pub meta: Option<ReplayMeta>,
    pub urls: Vec<String>,
    /// Error event ids (32 hex digits).
    pub error_ids: Vec<String>,
    pub trace_ids: Vec<String>,
    pub segment: Option<ReplaySegmentRow>,
}

/// Most URLs, error ids and trace ids kept per replay.
pub const REPLAY_LIST_CAP: usize = 1000;

fn merge_list(current: &Json, add: &[String]) -> Json {
    let mut list: Vec<String> = current
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    for a in add {
        if list.len() >= REPLAY_LIST_CAP {
            break;
        }
        if !list.contains(a) {
            list.push(a.clone());
        }
    }
    Json::from(list)
}

/// Applies an update: stores the segment (once per segment id) and
/// creates or widens the replay row.
pub fn apply_replay_update(conn: &Connection, u: &ReplayUpdate) -> Result<()> {
    let mut new_segment = None;
    if let Some(s) = &u.segment {
        let n = conn.execute(
            "INSERT INTO replay_segments (project_id, replay_id, segment_id, object_key, size, day, received)
             VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT (project_id, replay_id, segment_id) DO NOTHING",
            &[&u.project_id, &u.replay_id, &s.segment_id, &s.object_key, &s.size, &u.day, &u.received],
        )?;
        if n > 0 {
            new_segment = Some(s);
        }
    }
    let (segments, clicks, size) = new_segment.map_or((0i32, 0i32, 0i64), |s| (1, s.clicks, s.size));
    let existing = conn.query_opt(
        "SELECT started, finished, urls, error_ids, trace_ids FROM replays WHERE project_id = $1 AND replay_id = $2",
        &[&u.project_id, &u.replay_id],
    )?;
    let m = u.meta.clone().unwrap_or_default();
    match existing {
        None => {
            let started = u.started.or(u.finished).unwrap_or(u.received);
            let finished = u.finished.unwrap_or(started).max(started);
            let error_ids = merge_list(&Json::Null, &u.error_ids);
            let count_errors = error_ids.as_array().map_or(0, Vec::len) as i32;
            conn.execute(
                "INSERT INTO replays (project_id, replay_id, started, finished, segment_count, replay_type, platform, sdk,
                     environment, release, dist, urls, error_ids, trace_ids, user_id, user_email, user_name, user_ip,
                     browser, os, device, count_errors, count_clicks, size, day)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21,
                     $22, $23, $24, $25)",
                &[
                    &u.project_id,
                    &u.replay_id,
                    &started,
                    &finished,
                    &segments,
                    &m.replay_type,
                    &m.platform,
                    &m.sdk,
                    &m.environment,
                    &m.release,
                    &m.dist,
                    &merge_list(&Json::Null, &u.urls),
                    &error_ids,
                    &merge_list(&Json::Null, &u.trace_ids),
                    &m.user_id,
                    &m.user_email,
                    &m.user_name,
                    &m.user_ip,
                    &m.browser,
                    &m.os,
                    &m.device,
                    &count_errors,
                    &clicks,
                    &size,
                    &u.day,
                ],
            )?;
        }
        Some(r) => {
            let (started, finished): (DateTime<Utc>, DateTime<Utc>) = (r.get(0), r.get(1));
            let started = u.started.map_or(started, |s| s.min(started));
            let finished = u.finished.map_or(finished, |f| f.max(finished)).max(started);
            let urls = merge_list(&r.get::<Json>(2), &u.urls);
            let error_ids = merge_list(&r.get::<Json>(3), &u.error_ids);
            let count_errors = error_ids.as_array().map_or(0, Vec::len) as i32;
            let trace_ids = merge_list(&r.get::<Json>(4), &u.trace_ids);
            conn.execute(
                "UPDATE replays SET started = $3, finished = $4, segment_count = segment_count + $5,
                     replay_type = COALESCE(replay_type, $6), platform = COALESCE(platform, $7), sdk = COALESCE(sdk, $8),
                     environment = COALESCE(environment, $9), release = COALESCE(release, $10), dist = COALESCE(dist, $11),
                     urls = $12, error_ids = $13, trace_ids = $14, user_id = COALESCE(user_id, $15),
                     user_email = COALESCE(user_email, $16), user_name = COALESCE(user_name, $17),
                     user_ip = COALESCE(user_ip, $18), browser = COALESCE(browser, $19), os = COALESCE(os, $20),
                     device = COALESCE(device, $21), count_errors = $22, count_clicks = count_clicks + $23,
                     size = size + $24
                 WHERE project_id = $1 AND replay_id = $2",
                &[
                    &u.project_id,
                    &u.replay_id,
                    &started,
                    &finished,
                    &segments,
                    &m.replay_type,
                    &m.platform,
                    &m.sdk,
                    &m.environment,
                    &m.release,
                    &m.dist,
                    &urls,
                    &error_ids,
                    &trace_ids,
                    &m.user_id,
                    &m.user_email,
                    &m.user_name,
                    &m.user_ip,
                    &m.browser,
                    &m.os,
                    &m.device,
                    &count_errors,
                    &clicks,
                    &size,
                ],
            )?;
        }
    }
    Ok(())
}

/// The day under which a replay's segments are stored, if the replay is known.
pub fn replay_day(conn: &Connection, project_id: i64, replay_id: [u8; 16]) -> Result<Option<NaiveDate>> {
    Ok(conn
        .query_opt("SELECT day FROM replays WHERE project_id = $1 AND replay_id = $2", &[&project_id, &replay_id])?
        .map(|r| r.get(0)))
}

/// An error event recorded during a replay.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueReplay {
    pub project_id: i64,
    pub event_id: [u8; 16],
    pub issue_id: i64,
    pub replay_id: [u8; 16],
    pub timestamp: DateTime<Utc>,
}

/// Links an error event to its replay and counts it as an error of the
/// replay when the replay is known already.
pub fn link_issue_replay(conn: &Connection, l: &IssueReplay) -> Result<()> {
    conn.execute(
        "INSERT INTO issue_replays (project_id, event_id, issue_id, replay_id, timestamp) VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (project_id, event_id) DO NOTHING",
        &[&l.project_id, &l.event_id, &l.issue_id, &l.replay_id, &l.timestamp],
    )?;
    let row = conn.query_opt(
        "SELECT error_ids FROM replays WHERE project_id = $1 AND replay_id = $2",
        &[&l.project_id, &l.replay_id],
    )?;
    if let Some(r) = row {
        let id = uuid::Uuid::from_bytes(l.event_id).simple().to_string();
        let ids = merge_list(&r.get::<Json>(0), &[id]);
        let n = ids.as_array().map_or(0, Vec::len) as i32;
        conn.execute(
            "UPDATE replays SET error_ids = $3, count_errors = $4 WHERE project_id = $1 AND replay_id = $2",
            &[&l.project_id, &l.replay_id, &ids, &n],
        )?;
    }
    Ok(())
}

/// What a telemetry retention run deleted.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeletedTelemetry {
    pub spans: u64,
    pub transactions: u64,
    pub logs: u64,
    pub metrics: u64,
    pub profiles: u64,
    pub replays: u64,
    pub replay_segments: u64,
}

/// The oldest `timestamp` of a project in a columnar table.
pub fn oldest(conn: &Connection, table: &str, project_id: i64) -> Result<Option<DateTime<Utc>>> {
    Ok(conn.query_one(&format!("SELECT min(timestamp) FROM {table} WHERE project_id = $1"), &[&project_id])?.get(0))
}

/// Deletes a project's rows in `[from, to)` from a columnar table.
pub fn delete_range(
    conn: &Connection,
    table: &str,
    project_id: i64,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<u64> {
    Ok(conn.execute(
        &format!("DELETE FROM {table} WHERE project_id = $1 AND timestamp >= $2 AND timestamp < $3"),
        &[&project_id, &from, &to],
    )?)
}

/// Deletes profile rows of days before `day`.
pub fn delete_profiles_before(conn: &Connection, project_id: i64, day: NaiveDate) -> Result<u64> {
    Ok(conn.execute("DELETE FROM profiles WHERE project_id = $1 AND day < $2", &[&project_id, &day])?)
}

/// Deletes replays (and their segment rows and links) of days before `day`.
pub fn delete_replays_before(conn: &Connection, project_id: i64, day: NaiveDate) -> Result<(u64, u64)> {
    let s = conn.execute("DELETE FROM replay_segments WHERE project_id = $1 AND day < $2", &[&project_id, &day])?;
    conn.execute(
        "DELETE FROM issue_replays WHERE project_id = $1 AND replay_id IN
             (SELECT replay_id FROM replays WHERE project_id = $1 AND day < $2)",
        &[&project_id, &day],
    )?;
    let r = conn.execute("DELETE FROM replays WHERE project_id = $1 AND day < $2", &[&project_id, &day])?;
    Ok((r, s))
}

/// The `YYYY-MM-DD` part of `{kind}/{project}/{day}/...` object keys.
pub fn object_key_day(key: &str) -> Option<NaiveDate> {
    let day = key.split('/').nth(2)?;
    NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use chrono::TimeZone;

    fn seed(conn: &Connection) {
        conn.batch_execute(
            "INSERT INTO organizations (slug, name) VALUES ('o', 'O');
             INSERT INTO projects (org_id, slug, name) VALUES (1, 'p', 'P');",
        )
        .unwrap();
    }

    #[test]
    fn columnar_rows_append() {
        let store = Store::open_in_memory().unwrap();
        let conn = store.connect();
        seed(&conn);
        let now = Utc::now();
        let span = SpanRow {
            project_id: 1,
            trace_id: "t".into(),
            span_id: "s".into(),
            is_segment: true,
            timestamp: now,
            received: now,
            duration_ms: Some(2.0),
            data: serde_json::json!({"k": 1}),
            ..Default::default()
        };
        conn.append("spans", &[span.into_values()]).unwrap();
        let log = LogRow {
            project_id: 1,
            id: [1; 16],
            timestamp: now,
            received: now,
            level: "info".into(),
            body: "b".into(),
            attributes: serde_json::json!({"a": "x"}),
            ..Default::default()
        };
        conn.append("logs", &[log.into_values()]).unwrap();
        let m = MetricRow {
            project_id: 1,
            timestamp: now,
            received: now,
            name: "n".into(),
            ty: "counter".into(),
            value: 3.0,
            ..Default::default()
        };
        conn.append("metrics", &[m.into_values()]).unwrap();
        for t in ["spans", "logs", "metrics"] {
            let n: i64 = conn.query_one(&format!("SELECT count(*) FROM {t}"), &[]).unwrap().get(0);
            assert_eq!(n, 1, "{t}");
        }
        let attr: Option<String> = conn.query_one("SELECT attributes ->> 'a' FROM logs", &[]).unwrap().get(0);
        assert_eq!(attr.as_deref(), Some("x"));
        assert_eq!(
            delete_range(&conn, "logs", 1, now - chrono::Duration::days(1), now + chrono::Duration::days(1)).unwrap(),
            1
        );
    }

    #[test]
    fn replay_updates_merge() {
        let store = Store::open_in_memory().unwrap();
        let conn = store.connect();
        seed(&conn);
        let t = |s| Utc.with_ymd_and_hms(2026, 11, 14, 10, 0, s).unwrap();
        let day = t(0).date_naive();
        let base = ReplayUpdate {
            project_id: 1,
            replay_id: [5; 16],
            started: Some(t(0)),
            finished: Some(t(5)),
            received: t(5),
            day,
            meta: Some(ReplayMeta { browser: Some("Chrome 140".into()), ..Default::default() }),
            urls: vec!["http://a/".into()],
            error_ids: vec!["e1".into()],
            trace_ids: vec![],
            segment: Some(ReplaySegmentRow { segment_id: 0, object_key: "k0".into(), size: 10, clicks: 2 }),
        };
        apply_replay_update(&conn, &base).unwrap();
        // The same segment again (SDK retry) changes nothing.
        apply_replay_update(&conn, &base).unwrap();
        let second = ReplayUpdate {
            started: Some(t(4)),
            finished: Some(t(20)),
            meta: None,
            urls: vec!["http://a/".into(), "http://a/b".into()],
            error_ids: vec!["e2".into()],
            segment: Some(ReplaySegmentRow { segment_id: 1, object_key: "k1".into(), size: 5, clicks: 1 }),
            ..base.clone()
        };
        apply_replay_update(&conn, &second).unwrap();
        let r = conn
            .query_one(
                "SELECT segment_count, count_clicks, size, count_errors, urls, started, finished, browser FROM replays",
                &[],
            )
            .unwrap();
        assert_eq!((r.get::<i32>(0), r.get::<i32>(1), r.get::<i64>(2), r.get::<i32>(3)), (2, 3, 15, 2));
        assert_eq!(r.get::<Json>(4), serde_json::json!(["http://a/", "http://a/b"]));
        assert_eq!((r.get::<DateTime<Utc>>(5), r.get::<DateTime<Utc>>(6)), (t(0), t(20)));
        assert_eq!(r.get::<Option<String>>(7).as_deref(), Some("Chrome 140"));
        assert_eq!(replay_day(&conn, 1, [5; 16]).unwrap(), Some(day));
        let (replays, segments) = delete_replays_before(&conn, 1, day + chrono::Duration::days(1)).unwrap();
        assert_eq!((replays, segments), (1, 2));
    }

    #[test]
    fn object_key_days() {
        assert_eq!(object_key_day("profiles/3/2026-11-14/abc.json.zst"), NaiveDate::from_ymd_opt(2026, 11, 14));
        assert_eq!(object_key_day("replays/3/x/abc"), None);
    }
}
