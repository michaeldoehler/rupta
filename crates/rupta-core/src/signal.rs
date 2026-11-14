//! Signals from ingestion to the alert engine.

use serde::{Deserialize, Serialize};

/// What happened to an issue when an event was stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IssueSignal {
    /// The first event of a new issue.
    New { project_id: i64, issue_id: i64 },
    /// An event for an issue that was resolved.
    Regressed { project_id: i64, issue_id: i64 },
    /// An event for an issue that was ignored until a condition that is
    /// now met (time passed or count exceeded).
    Reappeared { project_id: i64, issue_id: i64 },
    /// An issue whose event rate rose far above its usual rate (see
    /// docs/ingestion.md, "Escalating issues").
    Escalating { project_id: i64, issue_id: i64 },
    /// Events for an existing issue, aggregated per batch.
    Seen { project_id: i64, issue_id: i64, count: u64, environment: Option<String>, level: Option<String> },
}

impl IssueSignal {
    pub fn issue_id(&self) -> i64 {
        match self {
            IssueSignal::New { issue_id, .. }
            | IssueSignal::Regressed { issue_id, .. }
            | IssueSignal::Reappeared { issue_id, .. }
            | IssueSignal::Escalating { issue_id, .. }
            | IssueSignal::Seen { issue_id, .. } => *issue_id,
        }
    }
}
