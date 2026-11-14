//! Integration rows, project defaults, issue links and the issue summary
//! that generated tickets are made of.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use rupta_store::squad::{Connection, Row};
use serde_json::{Value, json};

use super::content::{Frame, IssueSummary};
use super::http::Outbound;
use super::{AnyTracker, Category, Kind, Settings, TrackerError};
use crate::error::{ApiError, ApiResult};
use crate::state::ApiState;

/// An integration as stored.
#[derive(Debug, Clone)]
pub struct IntegrationRow {
    pub id: i64,
    pub org_id: i64,
    pub kind: Kind,
    pub name: String,
    pub config: Value,
    pub secret_enc: Option<String>,
    pub webhook_secret_enc: Option<String>,
    pub enabled: bool,
    pub created_by: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_error: Option<String>,
    pub last_error_at: Option<DateTime<Utc>>,
}

pub const INTEGRATION_COLUMNS: &str = "id, org_id, kind, name, config, secret_enc, webhook_secret_enc, enabled, created_by, created_at, updated_at, last_error, last_error_at";

pub fn integration_from(r: &Row) -> ApiResult<IntegrationRow> {
    let kind: String = r.get(2);
    Ok(IntegrationRow {
        id: r.get(0),
        org_id: r.get(1),
        kind: Kind::parse(&kind).ok_or_else(|| ApiError::internal(format!("unknown integration kind {kind}")))?,
        name: r.get(3),
        config: r.get(4),
        secret_enc: r.get(5),
        webhook_secret_enc: r.get(6),
        enabled: r.get(7),
        created_by: r.get(8),
        created_at: r.get(9),
        updated_at: r.get(10),
        last_error: r.get(11),
        last_error_at: r.get(12),
    })
}

/// An integration of an organization.
pub fn load(c: &Connection, org_id: i64, id: i64) -> ApiResult<Option<IntegrationRow>> {
    c.query_opt(
        &format!("SELECT {INTEGRATION_COLUMNS} FROM integrations WHERE id = $1 AND org_id = $2"),
        &[&id, &org_id],
    )?
    .map(|r| integration_from(&r))
    .transpose()
}

/// An integration by id alone (webhooks, background work).
pub fn load_any(c: &Connection, id: i64) -> ApiResult<Option<IntegrationRow>> {
    c.query_opt(&format!("SELECT {INTEGRATION_COLUMNS} FROM integrations WHERE id = $1"), &[&id])?
        .map(|r| integration_from(&r))
        .transpose()
}

/// Encryption context of an integration's secrets: bound to the row.
pub fn secret_ctx(id: i64, which: &str) -> String {
    format!("integration:{id}:{which}")
}

impl IntegrationRow {
    pub fn token(&self, state: &ApiState) -> Option<String> {
        let enc = self.secret_enc.as_deref()?;
        state.keys.decrypt(&secret_ctx(self.id, "token"), enc).and_then(|b| String::from_utf8(b).ok())
    }

    pub fn webhook_secret(&self, state: &ApiState) -> Option<String> {
        let enc = self.webhook_secret_enc.as_deref()?;
        state.keys.decrypt(&secret_ctx(self.id, "webhook"), enc).and_then(|b| String::from_utf8(b).ok())
    }

    pub fn tracker(&self, state: &ApiState) -> Result<AnyTracker, TrackerError> {
        let token = self.token(state).ok_or_else(|| {
            TrackerError::Invalid(
                "the stored token cannot be read; was RUPTA_SECRET_KEY changed? Enter the token again".into(),
            )
        })?;
        AnyTracker::new(&Settings { kind: self.kind, config: self.config.clone(), token }, state.outbound.clone())
    }
}

/// Builds a tracker from unsaved settings (validation before saving).
pub fn tracker_from(kind: Kind, config: &Value, token: &str, http: Outbound) -> Result<AnyTracker, TrackerError> {
    AnyTracker::new(&Settings { kind, config: config.clone(), token: token.to_string() }, http)
}

/// Records the last error of an integration (or clears it).
pub fn set_error(c: &Connection, id: i64, error: Option<&str>) -> ApiResult<()> {
    match error {
        Some(e) => c.execute(
            "UPDATE integrations SET last_error = $1, last_error_at = now() WHERE id = $2",
            &[&super::truncate(e, 500), &id],
        )?,
        None => c.execute(
            "UPDATE integrations SET last_error = NULL, last_error_at = NULL WHERE id = $1 AND last_error IS NOT NULL",
            &[&id],
        )?,
    };
    Ok(())
}

/// Per-project settings of an integration.
#[derive(Debug, Clone)]
pub struct DefaultsRow {
    pub project_id: i64,
    pub integration_id: i64,
    pub target: String,
    pub target_name: Option<String>,
    pub issue_type: Option<String>,
    pub project: Option<String>,
    pub labels: Vec<String>,
    pub incoming_sync: bool,
    pub outgoing_transition: bool,
    pub outgoing_comment: bool,
    pub updated_at: DateTime<Utc>,
}

pub const DEFAULTS_COLUMNS: &str = "project_id, integration_id, target, target_name, issue_type, project, labels, incoming_sync, outgoing_transition, outgoing_comment, updated_at";

pub fn defaults_from(r: &Row) -> DefaultsRow {
    let labels: Value = r.get(6);
    DefaultsRow {
        project_id: r.get(0),
        integration_id: r.get(1),
        target: r.get(2),
        target_name: r.get(3),
        issue_type: r.get(4),
        project: r.get(5),
        labels: labels.as_array().into_iter().flatten().filter_map(|v| v.as_str().map(str::to_string)).collect(),
        incoming_sync: r.get(7),
        outgoing_transition: r.get(8),
        outgoing_comment: r.get(9),
        updated_at: r.get(10),
    }
}

pub fn defaults(c: &Connection, project_id: i64, integration_id: i64) -> ApiResult<Option<DefaultsRow>> {
    Ok(c.query_opt(
        &format!(
            "SELECT {DEFAULTS_COLUMNS} FROM project_integration_defaults WHERE project_id = $1 AND integration_id = $2"
        ),
        &[&project_id, &integration_id],
    )?
    .map(|r| defaults_from(&r)))
}

/// A link between a rupta issue and a ticket.
#[derive(Debug, Clone)]
pub struct LinkRow {
    pub id: i64,
    pub issue_id: i64,
    pub integration_id: i64,
    pub external_id: String,
    pub external_key: String,
    pub url: String,
    pub title: Option<String>,
    pub status: Option<String>,
    pub status_category: Category,
    pub assignee: Option<String>,
    pub created_by: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub synced_at: Option<DateTime<Utc>>,
    pub pushed_category: Option<String>,
    pub pushed_until: Option<DateTime<Utc>>,
}

pub const LINK_COLUMNS: &str = "id, issue_id, integration_id, external_id, external_key, url, title, status, status_category, assignee, created_by, created_at, synced_at, pushed_category, pushed_until";

pub fn link_from(r: &Row) -> LinkRow {
    LinkRow {
        id: r.get(0),
        issue_id: r.get(1),
        integration_id: r.get(2),
        external_id: r.get(3),
        external_key: r.get(4),
        url: r.get(5),
        title: r.get(6),
        status: r.get(7),
        status_category: Category::parse(&r.get::<String>(8)),
        assignee: r.get(9),
        created_by: r.get(10),
        created_at: r.get(11),
        synced_at: r.get(12),
        pushed_category: r.get(13),
        pushed_until: r.get(14),
    }
}

/// Writes an activity entry on an issue.
pub fn activity(c: &Connection, issue_id: i64, user: Option<i64>, kind: &str, data: Value) -> ApiResult<()> {
    c.execute(
        "INSERT INTO issue_activity (issue_id, user_id, kind, data) VALUES ($1, $2, $3, $4)",
        &[&issue_id, &user, &kind, &data],
    )?;
    Ok(())
}

/// Tag keys left out of tickets: too many distinct values to be useful.
const SKIPPED_TAGS: &[&str] = &["url", "user", "transaction", "level"];

/// Everything a generated ticket says about an issue.
pub fn summary(state: &ApiState, c: &Connection, issue_id: i64) -> ApiResult<IssueSummary> {
    let r = c
        .query_opt(
            "SELECT i.short_id, i.title, i.culprit, i.level, i.platform, i.first_seen, i.last_seen, i.times_seen, i.users_seen,
                    i.first_release, i.last_release, i.metadata, p.slug, o.slug, p.id
             FROM issues i JOIN projects p ON p.id = i.project_id JOIN organizations o ON o.id = p.org_id WHERE i.id = $1",
            &[&issue_id],
        )?
        .ok_or_else(ApiError::not_found)?;
    let project_slug: String = r.get(12);
    let org_slug: String = r.get(13);
    let project_id: i64 = r.get(14);
    let metadata: Value = r.get::<Option<Value>>(11).unwrap_or(Value::Null);
    let mut s = IssueSummary {
        short_id: format!("{}-{}", project_slug.to_uppercase(), r.get::<i64>(0)),
        title: r.get(1),
        culprit: r.get(2),
        level: r.get(3),
        platform: r.get(4),
        first_seen: r.get(5),
        last_seen: r.get(6),
        count: r.get(7),
        user_count: r.get(8),
        first_release: r.get(9),
        last_release: r.get(10),
        error_type: metadata["type"].as_str().filter(|t| !t.is_empty()).map(str::to_string),
        error_value: metadata["value"].as_str().filter(|t| !t.is_empty()).map(str::to_string),
        project: project_slug,
        url: format!("{}/organizations/{org_slug}/issues/{issue_id}", state.config.public_url),
        ..IssueSummary::default()
    };

    // Top tags: the five keys seen on most events, three values each.
    let mut by_key: BTreeMap<String, Vec<(String, i64)>> = BTreeMap::new();
    for (k, v, n) in crate::routes::events::tag_counts(c, project_id, issue_id, None)? {
        if SKIPPED_TAGS.contains(&k.as_str()) {
            continue;
        }
        by_key.entry(k).or_default().push((v, n));
    }
    type KeyCounts = (String, i64, Vec<(String, i64)>);
    let mut keys: Vec<KeyCounts> = by_key
        .into_iter()
        .map(|(k, mut vals)| {
            vals.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            (k, vals.iter().map(|(_, n)| n).sum(), vals)
        })
        .collect();
    keys.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    s.top_tags = keys
        .into_iter()
        .take(5)
        .map(|(k, total, vals)| {
            let vals = vals.into_iter().take(3).map(|(v, n)| (v, (n * 100 + total / 2) / total.max(1))).collect();
            (k, vals)
        })
        .collect();

    // In-app frames of the latest event with an exception.
    let latest = c.query_opt(
        "SELECT event_id FROM events WHERE project_id = $1 AND issue_id = $2 ORDER BY timestamp DESC LIMIT 1",
        &[&project_id, &issue_id],
    )?;
    if let Some(row) = latest {
        let eid: [u8; 16] = row.get(0);
        if let Some(doc) = crate::routes::events::load_payload(c, project_id, eid)? {
            s.frames = in_app_frames(&doc);
        }
    }
    Ok(s)
}

/// The in-app frames of an event's last exception, the crashing frame
/// first. Without in-app information all frames are used.
pub fn in_app_frames(doc: &Value) -> Vec<Frame> {
    let exceptions = doc["exception"]["values"].as_array().or_else(|| doc["exception"].as_array());
    let Some(exc) = exceptions.and_then(|e| e.last()) else { return Vec::new() };
    let frames = exc["stacktrace"]["frames"].as_array().cloned().unwrap_or_default();
    let any_in_app = frames.iter().any(|f| f["in_app"].as_bool() == Some(true));
    let s = |f: &Value, k: &str| f[k].as_str().filter(|v| !v.is_empty()).map(str::to_string);
    frames
        .iter()
        .rev()
        .filter(|f| !any_in_app || f["in_app"].as_bool() == Some(true))
        .take(super::content::MAX_FRAMES)
        .map(|f| Frame {
            function: s(f, "function"),
            module: s(f, "module"),
            filename: s(f, "filename").or_else(|| s(f, "abs_path")),
            lineno: f["lineno"].as_i64(),
            colno: f["colno"].as_i64(),
            context_line: s(f, "context_line"),
        })
        .collect()
}

/// The JSON of a ticket for activity entries.
pub fn ticket_json(integration: &IntegrationRow, key: &str, url: &str) -> Value {
    json!({"integration_id": integration.id.to_string(), "integration": integration.name, "kind": integration.kind.as_str(), "key": key, "url": url})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_crashing_first_in_app_only() {
        let doc = json!({"exception": {"values": [
            {"type": "Cause"},
            {"type": "TypeError", "stacktrace": {"frames": [
                {"function": "main", "filename": "main.js", "in_app": true, "lineno": 1},
                {"function": "lib", "filename": "node_modules/lib.js", "in_app": false},
                {"function": "render", "filename": "app.js", "in_app": true, "lineno": 9, "colno": 2}
            ]}}
        ]}});
        let f = in_app_frames(&doc);
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].function.as_deref(), Some("render"));
        assert_eq!(f[1].function.as_deref(), Some("main"));
        assert!(in_app_frames(&json!({})).is_empty());
    }
}
