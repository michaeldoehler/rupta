//! Per-user preferences (`users.preferences`): display settings for the
//! web app and which email notifications the user receives.

use rupta_store::squad::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

use crate::error::{ApiError, ApiResult};
use crate::sql::id_list;

/// Email notifications a user receives.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct NotificationPreferences {
    /// Emails from alert rules that name the user or one of their teams.
    pub issue_alerts: bool,
    /// Emails when an issue is assigned to the user (or their team) and
    /// when someone comments on an issue assigned to them.
    pub workflow: bool,
    /// A weekly summary. Stored for the web app; rupta does not send
    /// weekly reports yet.
    pub weekly_report: bool,
}

impl Default for NotificationPreferences {
    fn default() -> Self {
        NotificationPreferences { issue_alerts: true, workflow: true, weekly_report: false }
    }
}

/// The preferences of a user; missing fields take their defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Preferences {
    /// IANA time zone for showing times, e.g. `Europe/Berlin`; `null`
    /// follows the browser.
    pub timezone: Option<String>,
    /// `24h` or `12h`.
    pub clock: String,
    /// `system`, `light` or `dark`.
    pub theme: String,
    pub notifications: NotificationPreferences,
}

impl Default for Preferences {
    fn default() -> Self {
        Preferences {
            timezone: None,
            clock: "24h".into(),
            theme: "system".into(),
            notifications: NotificationPreferences::default(),
        }
    }
}

/// A change of preferences: only the given fields change.
#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct PreferencesPatch {
    /// IANA time zone; `null` or `""` to follow the browser.
    #[serde(default, deserialize_with = "crate::extract::double_option")]
    #[schema(value_type = Option<String>)]
    pub timezone: Option<Option<String>>,
    pub clock: Option<String>,
    pub theme: Option<String>,
    pub notifications: Option<NotificationsPatch>,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct NotificationsPatch {
    pub issue_alerts: Option<bool>,
    pub workflow: Option<bool>,
    pub weekly_report: Option<bool>,
}

impl Preferences {
    /// Reads stored preferences leniently: unknown or invalid values fall
    /// back to the defaults.
    pub fn from_value(v: &Value) -> Preferences {
        let d = Preferences::default();
        let s = |k: &str| v.get(k).and_then(Value::as_str);
        let n = v.get("notifications");
        let b = |k: &str, default: bool| n.and_then(|n| n.get(k)).and_then(Value::as_bool).unwrap_or(default);
        Preferences {
            timezone: s("timezone").filter(|t| valid_timezone(t)).map(str::to_string),
            clock: s("clock").filter(|c| matches!(*c, "24h" | "12h")).unwrap_or(&d.clock).to_string(),
            theme: s("theme").filter(|t| matches!(*t, "system" | "light" | "dark")).unwrap_or(&d.theme).to_string(),
            notifications: NotificationPreferences {
                issue_alerts: b("issue_alerts", d.notifications.issue_alerts),
                workflow: b("workflow", d.notifications.workflow),
                weekly_report: b("weekly_report", d.notifications.weekly_report),
            },
        }
    }

    /// Applies a patch, validating every given field.
    pub fn apply(mut self, p: &PreferencesPatch) -> ApiResult<Preferences> {
        if let Some(tz) = &p.timezone {
            self.timezone = match tz.as_deref().map(str::trim) {
                None | Some("") => None,
                Some(t) if valid_timezone(t) => Some(t.to_string()),
                Some(_) => {
                    return Err(ApiError::invalid("preferences.timezone", "an IANA time zone such as Europe/Berlin"));
                }
            };
        }
        if let Some(c) = &p.clock {
            if !matches!(c.as_str(), "24h" | "12h") {
                return Err(ApiError::invalid("preferences.clock", "24h or 12h"));
            }
            self.clock = c.clone();
        }
        if let Some(t) = &p.theme {
            if !matches!(t.as_str(), "system" | "light" | "dark") {
                return Err(ApiError::invalid("preferences.theme", "system, light or dark"));
            }
            self.theme = t.clone();
        }
        if let Some(n) = &p.notifications {
            if let Some(v) = n.issue_alerts {
                self.notifications.issue_alerts = v;
            }
            if let Some(v) = n.workflow {
                self.notifications.workflow = v;
            }
            if let Some(v) = n.weekly_report {
                self.notifications.weekly_report = v;
            }
        }
        Ok(self)
    }

    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).expect("preferences serialize")
    }
}

/// True for an IANA time zone name known to the time zone database.
pub fn valid_timezone(tz: &str) -> bool {
    tz.len() <= 64 && tz.parse::<chrono_tz::Tz>().is_ok()
}

/// Which kind of email a recipient must have enabled.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    IssueAlerts,
    Workflow,
}

/// The email addresses of the given users that are enabled, members of the
/// organization and want this kind of email.
pub fn recipients(c: &Connection, org_id: i64, user_ids: &[i64], kind: Kind) -> ApiResult<Vec<String>> {
    if user_ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = c.query(
        &format!(
            "SELECT DISTINCT u.email, u.preferences FROM users u JOIN members m ON m.user_id = u.id AND m.org_id = $1
             WHERE NOT u.disabled AND u.id IN ({}) ORDER BY u.email",
            id_list(user_ids)
        ),
        &[&org_id],
    )?;
    Ok(rows
        .iter()
        .filter(|r| {
            let p = Preferences::from_value(&r.get::<Value>(1));
            match kind {
                Kind::IssueAlerts => p.notifications.issue_alerts,
                Kind::Workflow => p.notifications.workflow,
            }
        })
        .map(|r| r.get(0))
        .collect())
}

/// Members of the given teams of an organization.
pub fn team_members(c: &Connection, org_id: i64, team_ids: &[i64]) -> ApiResult<Vec<i64>> {
    if team_ids.is_empty() {
        return Ok(Vec::new());
    }
    Ok(c.query(
        &format!(
            "SELECT DISTINCT tm.user_id FROM team_members tm JOIN teams t ON t.id = tm.team_id
             WHERE t.org_id = $1 AND t.id IN ({})",
            id_list(team_ids)
        ),
        &[&org_id],
    )?
    .iter()
    .map(|r| r.get(0))
    .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn stored_values_fall_back_to_defaults() {
        assert_eq!(Preferences::from_value(&json!({})), Preferences::default());
        let p = Preferences::from_value(&json!({"timezone": "Mars/Olympus", "clock": "12h", "theme": "neon",
                                                "notifications": {"workflow": false}}));
        assert_eq!(p.timezone, None);
        assert_eq!(p.clock, "12h");
        assert_eq!(p.theme, "system");
        assert!(!p.notifications.workflow && p.notifications.issue_alerts);
    }

    #[test]
    fn patches_are_validated() {
        let patch = |v: Value| serde_json::from_value::<PreferencesPatch>(v).unwrap();
        let p = Preferences::default().apply(&patch(json!({"timezone": "Europe/Berlin", "theme": "dark"}))).unwrap();
        assert_eq!(p.timezone.as_deref(), Some("Europe/Berlin"));
        let p = p.apply(&patch(json!({"timezone": null}))).unwrap();
        assert_eq!(p.timezone, None);
        assert!(Preferences::default().apply(&patch(json!({"timezone": "Nowhere/City"}))).is_err());
        assert!(Preferences::default().apply(&patch(json!({"clock": "13h"}))).is_err());
        assert!(Preferences::default().apply(&patch(json!({"theme": "neon"}))).is_err());
    }
}
