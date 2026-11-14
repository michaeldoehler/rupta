//! Per-project settings, stored as JSON in `projects.settings`. The API
//! edits them; ingestion reads them. Unknown fields are kept, missing ones
//! take their defaults, so settings survive upgrades in both directions.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectSettings {
    /// Origins browsers may send events from (`*` = any). Matches the
    /// `Origin` or `Referer` header of browser SDK requests.
    pub allowed_origins: Vec<String>,
    /// Events per minute accepted per project; `None` = unlimited.
    pub rate_limit_per_min: Option<u32>,
    /// Days events are kept; `None` = the instance default.
    pub retention_days: Option<u32>,
    pub filters: InboundFilters,
    pub scrubbing: Scrubbing,
    /// Treat frames from these module/path prefixes as in-app.
    pub in_app_include: Vec<String>,
    /// Treat frames from these module/path prefixes as not in-app.
    pub in_app_exclude: Vec<String>,
    /// Anything else, preserved.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Default for ProjectSettings {
    fn default() -> ProjectSettings {
        ProjectSettings {
            allowed_origins: vec!["*".into()],
            rate_limit_per_min: None,
            retention_days: None,
            filters: InboundFilters::default(),
            scrubbing: Scrubbing::default(),
            in_app_include: Vec::new(),
            in_app_exclude: Vec::new(),
            extra: serde_json::Map::new(),
        }
    }
}

/// Events dropped at ingestion before they are stored.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct InboundFilters {
    pub browser_extensions: bool,
    pub localhost: bool,
    pub legacy_browsers: bool,
    pub web_crawlers: bool,
    /// Glob patterns (`*` wildcard) matched against the error message.
    pub error_messages: Vec<String>,
    /// Glob patterns matched against the release.
    pub releases: Vec<String>,
}

/// Server-side removal of sensitive data before storage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Scrubbing {
    /// Remove values of well-known sensitive keys (password, secret,
    /// token, authorization, cookie, credit card numbers, ...).
    pub enabled: bool,
    /// Additional key names to scrub (case-insensitive substring).
    pub extra_fields: Vec<String>,
    /// Keys never scrubbed even if they match.
    pub safe_fields: Vec<String>,
    /// Do not store the client IP address.
    pub scrub_ip_addresses: bool,
}

impl Default for Scrubbing {
    fn default() -> Scrubbing {
        Scrubbing { enabled: true, extra_fields: Vec::new(), safe_fields: Vec::new(), scrub_ip_addresses: false }
    }
}

impl ProjectSettings {
    /// Reads settings JSON; invalid JSON yields defaults.
    pub fn from_json(value: &serde_json::Value) -> ProjectSettings {
        serde_json::from_value(value.clone()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_take_defaults_and_unknown_fields_survive() {
        let s = ProjectSettings::from_json(&serde_json::json!({"retention_days": 30, "future_option": 1}));
        assert_eq!(s.retention_days, Some(30));
        assert_eq!(s.allowed_origins, vec!["*"]);
        assert!(s.scrubbing.enabled);
        assert_eq!(serde_json::to_value(&s).unwrap()["future_option"], 1);
    }
}
