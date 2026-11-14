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
    /// Days each kind of telemetry is kept (errors use `retention_days`).
    pub retention: TelemetryRetention,
    /// Server-side sampling of traces and logs.
    pub sampling: Sampling,
    /// Which kinds of telemetry are stored at all.
    pub data_types: DataTypes,
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
            retention: TelemetryRetention::default(),
            sampling: Sampling::default(),
            data_types: DataTypes::default(),
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

/// Days telemetry is kept, per kind. `spans` covers transactions and
/// spans (the trace data).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TelemetryRetention {
    pub logs: u32,
    pub spans: u32,
    pub metrics: u32,
    pub profiles: u32,
    pub replays: u32,
}

impl Default for TelemetryRetention {
    fn default() -> TelemetryRetention {
        TelemetryRetention { logs: 30, spans: 30, metrics: 90, profiles: 30, replays: 30 }
    }
}

/// Fractions (0 to 1) of data kept at ingestion. Transactions and spans
/// are sampled by trace id, so a trace is kept or dropped as a whole;
/// logs with a trace id follow their trace, others are sampled at random.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sampling {
    /// Transactions and the spans sent inside them.
    pub transactions: f64,
    /// Spans sent as standalone `span` items.
    pub spans: f64,
    pub logs: f64,
}

impl Default for Sampling {
    fn default() -> Sampling {
        Sampling { transactions: 1.0, spans: 1.0, logs: 1.0 }
    }
}

impl Sampling {
    /// True if an item with this sampling key is kept at `rate`.
    /// The same key always gives the same answer.
    pub fn keep(rate: f64, key: u64) -> bool {
        if rate >= 1.0 || rate.is_nan() {
            return true;
        }
        if rate <= 0.0 {
            return false;
        }
        (key as f64 / u64::MAX as f64) < rate
    }
}

/// Per-kind switches; a disabled kind is acknowledged and dropped
/// (outcome `filtered`, reason `disabled`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DataTypes {
    /// Transactions and spans.
    pub traces: bool,
    pub logs: bool,
    pub metrics: bool,
    pub profiles: bool,
    pub replays: bool,
}

impl Default for DataTypes {
    fn default() -> DataTypes {
        DataTypes { traces: true, logs: true, metrics: true, profiles: true, replays: true }
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
        assert_eq!(s.retention.logs, 30);
        assert_eq!(s.retention.metrics, 90);
        assert_eq!(s.sampling.logs, 1.0);
        assert!(s.data_types.replays);
    }

    #[test]
    fn telemetry_settings_merge_partially() {
        let s = ProjectSettings::from_json(&serde_json::json!({
            "retention": {"logs": 7}, "sampling": {"transactions": 0.25}, "data_types": {"replays": false}
        }));
        assert_eq!((s.retention.logs, s.retention.spans), (7, 30));
        assert_eq!((s.sampling.transactions, s.sampling.logs), (0.25, 1.0));
        assert!(!s.data_types.replays && s.data_types.logs);
    }

    #[test]
    fn sampling_is_deterministic_and_proportional() {
        assert!(Sampling::keep(1.0, u64::MAX));
        assert!(!Sampling::keep(0.0, 0));
        let kept = (0..10_000u64).filter(|i| Sampling::keep(0.3, i.wrapping_mul(0x9E3779B97F4A7C15))).count();
        assert!((2_700..3_300).contains(&kept), "{kept}");
        assert_eq!(Sampling::keep(0.5, 42), Sampling::keep(0.5, 42));
    }
}
