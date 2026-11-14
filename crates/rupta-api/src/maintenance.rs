//! Hourly clean-up of API-owned data: expired sessions and invitations,
//! and alert fires older than 31 days (the longest rate limit is 30 days).

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
        .await
}
