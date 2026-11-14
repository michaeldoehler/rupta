//! Numbered schema migrations.
//!
//! Each migration is a SQL file in `migrations/`, applied once, in version
//! order, inside one transaction. Applied versions are recorded in
//! `schema_migrations`. Migrations are never edited after release; a change
//! is a new file. Version ranges keep parallel work apart: 0001–0999 core,
//! 1000–1999 ingest, 2000–2999 API and auth.

use squad::Connection;

use crate::Result;

/// `(version, name, sql)`, in ascending version order.
pub(crate) const MIGRATIONS: &[(i64, &str, &str)] = &[
    (1, "core", include_str!("../migrations/0001_core.sql")),
    (2, "issues_events", include_str!("../migrations/0002_issues_events.sql")),
];

pub(crate) fn apply(conn: &Connection) -> Result<()> {
    conn.batch_execute(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version     bigint PRIMARY KEY,
            name        text NOT NULL,
            applied_at  timestamptz NOT NULL DEFAULT now()
        )",
    )?;
    let applied: Vec<i64> =
        conn.query("SELECT version FROM schema_migrations ORDER BY version", &[])?.iter().map(|r| r.get(0)).collect();
    for (version, name, sql) in MIGRATIONS {
        if applied.contains(version) {
            continue;
        }
        tracing::info!(version, name, "applying migration");
        let txn = conn.transaction()?;
        txn.batch_execute(sql)?;
        txn.execute("INSERT INTO schema_migrations (version, name) VALUES ($1, $2)", &[version, name])?;
        txn.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::MIGRATIONS;

    #[test]
    fn versions_ascend() {
        assert!(MIGRATIONS.windows(2).all(|w| w[0].0 < w[1].0));
    }
}
