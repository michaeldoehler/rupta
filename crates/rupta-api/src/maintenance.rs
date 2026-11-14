//! Hourly clean-up of API-owned data: expired sessions and invitations,
//! alert fires older than 31 days (the longest rate limit is 30 days), and
//! source map upload chunks older than 24 hours.
//!
//! Chunks are only needed until the upload CLI asks to assemble them into
//! a bundle; an assembled bundle is stored as one object of its own and
//! references no chunks. Chunks of uploads that were never assembled (or
//! were assembled long ago) are deleted with their objects.

use std::time::Duration;

use tokio::task::JoinHandle;

use crate::error::ApiResult;
use crate::state::ApiState;

/// Runs [`run_once`] every hour.
pub fn spawn(state: ApiState) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(3600));
        loop {
            tick.tick().await;
            if let Err(e) = run_once(&state).await {
                tracing::warn!(error = %e.message, "API maintenance failed");
            }
        }
    })
}

/// Deletes expired data; returns the number of rows removed.
pub async fn run_once(state: &ApiState) -> ApiResult<u64> {
    state
        .db(|c| {
            let mut n = rupta_auth::session::purge_expired(c)?;
            n += c.execute("DELETE FROM invitations WHERE expires_at <= now()", &[])?;
            n += c.execute("DELETE FROM alert_fires WHERE fired_at < now() - interval '31 days'", &[])?;
            Ok(n)
        })
        .await?
        .checked_add(purge_chunks(state, chrono::Duration::hours(24)).await?)
        .ok_or_else(|| crate::error::ApiError::internal("row count overflow"))
}

/// Deletes upload chunks older than `age` (objects first, then rows).
/// Returns the number of chunks removed.
pub async fn purge_chunks(state: &ApiState, age: chrono::Duration) -> ApiResult<u64> {
    let objects = state.store.objects().clone();
    state
        .db(move |c| {
            let cutoff = chrono::Utc::now() - age;
            let rows =
                c.query("SELECT checksum, object_key FROM file_chunks WHERE created_at < $1 LIMIT 10000", &[&cutoff])?;
            let mut removed = Vec::with_capacity(rows.len());
            for r in &rows {
                let key: String = r.get(1);
                match objects.delete(&key) {
                    Ok(()) => removed.push(r.get::<String>(0)),
                    Err(e) => tracing::warn!(error = %e, key, "deleting an upload chunk failed; retried next hour"),
                }
            }
            let mut n = 0;
            for sum in removed {
                n += c.execute("DELETE FROM file_chunks WHERE checksum = $1", &[&sum])?;
            }
            if n > 0 {
                tracing::info!(chunks = n, "deleted old source map upload chunks");
            }
            Ok(n)
        })
        .await
}
