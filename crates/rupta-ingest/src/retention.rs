//! Retention: deletes data older than a project's `retention_days` (or the
//! instance default): events (columnar, one day per `DELETE` so each
//! statement rewrites a bounded part of the segments), event payloads,
//! attachments (objects and rows), session counts, outcomes, check-ins,
//! and issues last seen before the cutoff.
//!
//! Telemetry follows the per-kind `retention` settings: transactions and
//! spans (`spans`), logs and metrics (columnar, a day at a time), and
//! profiles and replays (rows, and objects deleted by day prefix).

use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use rupta_core::settings::ProjectSettings;
use rupta_store::ingest::{self, Deleted};
use rupta_store::telemetry::{self, DeletedTelemetry};
use rupta_store::{Result, Store};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RetentionReport {
    pub projects: usize,
    pub deleted: Deleted,
    pub telemetry: DeletedTelemetry,
    pub objects_deleted: u64,
    pub elapsed: Duration,
}

fn midnight(t: DateTime<Utc>) -> DateTime<Utc> {
    t.date_naive().and_hms_opt(0, 0, 0).expect("midnight").and_utc()
}

/// Deletes a project's rows older than `cutoff` from a columnar table
/// with a `timestamp` column, one day per statement.
fn delete_columnar(
    conn: &rupta_store::squad::Connection,
    table: &str,
    project: i64,
    cutoff: DateTime<Utc>,
) -> Result<u64> {
    let mut n = 0;
    if let Some(oldest) = telemetry::oldest(conn, table, project)? {
        let mut from = midnight(oldest);
        while from < cutoff {
            let to = (from + chrono::Duration::days(1)).min(cutoff);
            n += telemetry::delete_range(conn, table, project, from, to)?;
            from = to;
        }
    }
    Ok(n)
}

/// Deletes the objects under `{kind}/{project}/{day}/` for every day from
/// `first` up to (not including) `cutoff`. Returns the number deleted.
fn delete_object_days(
    store: &Store,
    kind: &str,
    project: i64,
    first: chrono::NaiveDate,
    cutoff: chrono::NaiveDate,
) -> u64 {
    let mut n = 0;
    let mut day = first;
    while day < cutoff {
        let prefix = format!("{kind}/{project}/{}/", day.format("%Y-%m-%d"));
        match store.objects().list(&prefix) {
            Ok(list) => {
                for o in list {
                    match store.objects().delete(&o.key) {
                        Ok(()) => n += 1,
                        Err(e) => tracing::warn!(error = %e, key = o.key, "deleting an expired object failed"),
                    }
                }
            }
            Err(e) => tracing::warn!(error = %e, prefix, "listing expired objects failed"),
        }
        day += chrono::Duration::days(1);
    }
    n
}

fn oldest_day(conn: &rupta_store::squad::Connection, table: &str, project: i64) -> Result<Option<chrono::NaiveDate>> {
    Ok(conn.query_one(&format!("SELECT min(day) FROM {table} WHERE project_id = $1"), &[&project])?.get(0))
}

/// Telemetry retention of one project.
fn telemetry_once(
    store: &Store,
    project: i64,
    settings: &ProjectSettings,
    now: DateTime<Utc>,
    report: &mut RetentionReport,
) -> Result<()> {
    let conn = store.connect();
    let r = &settings.retention;
    let cutoff = |days: u32| now - chrono::Duration::days(days.max(1) as i64);
    let d = &mut report.telemetry;
    d.transactions += delete_columnar(&conn, "transactions", project, cutoff(r.spans))?;
    d.spans += delete_columnar(&conn, "spans", project, cutoff(r.spans))?;
    d.logs += delete_columnar(&conn, "logs", project, cutoff(r.logs))?;
    d.metrics += delete_columnar(&conn, "metrics", project, cutoff(r.metrics))?;
    let profiles_cutoff = cutoff(r.profiles).date_naive();
    if let Some(first) = oldest_day(&conn, "profiles", project)?.filter(|d| *d < profiles_cutoff) {
        report.objects_deleted += delete_object_days(store, "profiles", project, first, profiles_cutoff);
        d.profiles += telemetry::delete_profiles_before(&conn, project, profiles_cutoff)?;
    }
    let replays_cutoff = cutoff(r.replays).date_naive();
    let first = [oldest_day(&conn, "replays", project)?, oldest_day(&conn, "replay_segments", project)?]
        .into_iter()
        .flatten()
        .min();
    if let Some(first) = first.filter(|d| *d < replays_cutoff) {
        report.objects_deleted += delete_object_days(store, "replays", project, first, replays_cutoff);
        let (replays, segments) = telemetry::delete_replays_before(&conn, project, replays_cutoff)?;
        d.replays += replays;
        d.replay_segments += segments;
    }
    Ok(())
}

/// Runs retention once for every project (blocking; call from a blocking
/// thread).
pub fn run_once(store: &Store, default_days: u32, now: DateTime<Utc>) -> Result<RetentionReport> {
    let start = Instant::now();
    let conn = store.connect();
    let projects = ingest::load_projects(&conn)?;
    let mut report = RetentionReport { projects: projects.len(), ..RetentionReport::default() };
    for p in projects {
        let settings = ProjectSettings::from_json(&p.settings);
        let days = settings.retention_days.unwrap_or(default_days).max(1);
        let cutoff = now - chrono::Duration::days(days as i64);
        // Events, a day at a time from the oldest event.
        if let Some(oldest) = ingest::oldest_event(&conn, p.id)? {
            let mut from = midnight(oldest);
            while from < cutoff {
                let to = (from + chrono::Duration::days(1)).min(cutoff);
                report.deleted.events += ingest::delete_events_range(&conn, p.id, from, to)?;
                from = to;
            }
        }
        telemetry_once(store, p.id, &settings, now, &mut report)?;
        let expired = ingest::expired_attachments(&conn, p.id, cutoff.date_naive())?;
        let mut ids = Vec::with_capacity(expired.len());
        for (id, key) in expired {
            match store.objects().delete(&key) {
                Ok(()) => {
                    report.objects_deleted += 1;
                    ids.push(id);
                }
                Err(e) => tracing::warn!(error = %e, key, "deleting an expired attachment failed; retried next run"),
            }
        }
        report.deleted.attachments += ingest::delete_attachment_rows(&conn, &ids)?;
        ingest::delete_rows_before(&conn, p.id, cutoff, &mut report.deleted)?;
    }
    report.deleted.transactions = report.telemetry.transactions;
    report.elapsed = start.elapsed();
    Ok(report)
}

/// Runs retention a few minutes after start and then every 24 hours.
pub fn spawn(store: Store, default_days: u32) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(300)).await;
        loop {
            let s = store.clone();
            match tokio::task::spawn_blocking(move || run_once(&s, default_days, Utc::now())).await {
                Ok(Ok(r)) => tracing::info!(
                    projects = r.projects,
                    events = r.deleted.events,
                    payloads = r.deleted.payloads,
                    attachments = r.deleted.attachments,
                    issues = r.deleted.issues,
                    spans = r.telemetry.spans,
                    transactions = r.telemetry.transactions,
                    logs = r.telemetry.logs,
                    metrics = r.telemetry.metrics,
                    profiles = r.telemetry.profiles,
                    replays = r.telemetry.replays,
                    objects = r.objects_deleted,
                    elapsed_ms = r.elapsed.as_millis() as u64,
                    "retention run finished"
                ),
                Ok(Err(e)) => tracing::error!(error = %e, "retention run failed"),
                Err(e) => tracing::error!(error = %e, "retention task panicked"),
            }
            tokio::time::sleep(Duration::from_secs(24 * 3600)).await;
        }
    })
}
