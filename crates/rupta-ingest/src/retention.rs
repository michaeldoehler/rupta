//! Retention: deletes data older than a project's `retention_days` (or the
//! instance default): events and transactions (columnar, one day per
//! `DELETE` so each statement rewrites a bounded part of the segments),
//! event payloads, attachments (objects and rows), session counts,
//! outcomes, check-ins, and issues last seen before the cutoff.

use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use rupta_core::settings::ProjectSettings;
use rupta_store::ingest::{self, Deleted};
use rupta_store::{Result, Store};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RetentionReport {
    pub projects: usize,
    pub deleted: Deleted,
    pub objects_deleted: u64,
    pub elapsed: Duration,
}

/// Runs retention once for every project (blocking; call from a blocking
/// thread).
pub fn run_once(store: &Store, default_days: u32, now: DateTime<Utc>) -> Result<RetentionReport> {
    let start = Instant::now();
    let conn = store.connect();
    let projects = ingest::load_projects(&conn)?;
    let mut report = RetentionReport { projects: projects.len(), ..RetentionReport::default() };
    for p in projects {
        let days = ProjectSettings::from_json(&p.settings).retention_days.unwrap_or(default_days).max(1);
        let cutoff = now - chrono::Duration::days(days as i64);
        // Columnar tables, a day at a time from the oldest event.
        if let Some(oldest) = ingest::oldest_event(&conn, p.id)? {
            let mut from = oldest.date_naive().and_hms_opt(0, 0, 0).expect("midnight").and_utc();
            while from < cutoff {
                let to = (from + chrono::Duration::days(1)).min(cutoff);
                let (e, t) = ingest::delete_columnar_range(&conn, p.id, from, to)?;
                report.deleted.events += e;
                report.deleted.transactions += t;
                from = to;
            }
        }
        // Transactions older than any event.
        let (_, t) = ingest::delete_columnar_range(&conn, p.id, DateTime::<Utc>::UNIX_EPOCH, cutoff)?;
        report.deleted.transactions += t;
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
