//! Structured logs: `log` items (a container `{"items": [...]}` with the
//! content type `application/vnd.sentry.items.log+json`) and `otel_log`
//! items (one OpenTelemetry log record in OTLP JSON).
//!
//! Typed attributes (`{"key": {"value": v, "type": t}}`) are flattened to
//! `{"key": v}`; `sentry.environment`, `sentry.release` and
//! `sentry.sdk.name` also fill columns of their own. Data scrubbing
//! applies to attributes and the body.

use chrono::{DateTime, Utc};
use rupta_core::normalize::{parse_timestamp, truncate};
use rupta_core::scrub::Scrubber;
use rupta_store::telemetry::LogRow;
use serde_json::{Map, Value};

use crate::envelope::Item;
use crate::metrics::{Category, Outcome};
use crate::process::{EnvelopeState, opt_str};
use crate::traces::{flatten_attributes, sampled};

/// Most log records accepted per item.
pub const MAX_LOGS_PER_ITEM: usize = 10_000;
/// Longest body kept, in bytes.
pub const MAX_BODY: usize = 16 << 10;
/// Most attributes kept per record.
pub const MAX_ATTRIBUTES: usize = 100;

/// The level names stored: trace, debug, info, warn, error, fatal.
pub fn normalize_level(level: Option<&str>, severity: Option<i64>) -> String {
    let from_number = |n: i64| match n {
        1..=4 => "trace",
        5..=8 => "debug",
        9..=12 => "info",
        13..=16 => "warn",
        17..=20 => "error",
        21..=24 => "fatal",
        _ => "info",
    };
    match level.map(|l| l.trim().to_ascii_lowercase()) {
        Some(l) => match l.as_str() {
            "trace" | "debug" | "info" | "warn" | "error" | "fatal" => l,
            "warning" => "warn".into(),
            "critical" | "emergency" | "alert" | "panic" => "fatal".into(),
            "notice" | "log" | "informational" => "info".into(),
            "err" => "error".into(),
            _ => severity.map_or("info", from_number).into(),
        },
        None => severity.map_or("info", from_number).into(),
    }
}

/// The OpenTelemetry severity number of a level.
pub fn severity_of(level: &str) -> i32 {
    match level {
        "trace" => 1,
        "debug" => 5,
        "info" => 9,
        "warn" => 13,
        "error" => 17,
        "fatal" => 21,
        _ => 9,
    }
}

struct Record {
    timestamp: DateTime<Utc>,
    trace_id: Option<String>,
    span_id: Option<String>,
    level: String,
    severity: Option<i32>,
    body: String,
    attributes: Map<String, Value>,
}

fn store(st: &mut EnvelopeState<'_>, records: Vec<Record>, invalid: u64) {
    let settings = &st.project.settings;
    if !settings.data_types.logs {
        st.outcome(Category::LogItem, Outcome::Filtered, "disabled", records.len() as u64 + invalid);
        return;
    }
    let trace = st.headers.get("trace");
    let header_env = opt_str(trace.and_then(|t| t.get("environment")), 64);
    let header_release = opt_str(trace.and_then(|t| t.get("release")), 250);
    let header_sdk = opt_str(st.headers.get("sdk").and_then(|s| s.get("name")), 128);
    let rate = settings.sampling.logs;
    let scrubber = settings.scrubbing.enabled.then(|| Scrubber::new(&settings.scrubbing));
    let (mut accepted, mut sampled_out) = (0u64, 0u64);
    for r in records {
        if !sampled(rate, r.trace_id.as_deref()) {
            sampled_out += 1;
            continue;
        }
        let mut attributes = Value::Object(r.attributes);
        let mut body = r.body;
        if let Some(s) = &scrubber {
            s.scrub_value(&mut attributes);
            if let Some(b) = s.scrub_str(&body) {
                body = b;
            }
        }
        let a = attributes.as_object();
        let get = |k: &str, max: usize| opt_str(a.and_then(|a| a.get(k)), max);
        let row = LogRow {
            project_id: st.project_id,
            id: *uuid::Uuid::now_v7().as_bytes(),
            timestamp: r.timestamp,
            received: st.received,
            span_id: r.span_id.or_else(|| get("sentry.trace.parent_span_id", 32)),
            trace_id: r.trace_id,
            severity_number: Some(r.severity.unwrap_or_else(|| severity_of(&r.level))),
            level: r.level,
            body,
            environment: get("sentry.environment", 64).or_else(|| header_env.clone()),
            release: get("sentry.release", 250).or_else(|| header_release.clone()),
            sdk: get("sentry.sdk.name", 128).or_else(|| header_sdk.clone()),
            attributes: if a.is_some_and(|a| a.is_empty()) { Value::Null } else { attributes },
        };
        st.out.logs.push(row.into_values());
        accepted += 1;
    }
    st.outcome(Category::LogItem, Outcome::Accepted, "", accepted);
    st.outcome(Category::LogItem, Outcome::Filtered, "sample_rate", sampled_out);
    st.outcome(Category::LogItem, Outcome::Invalid, "invalid_log", invalid);
}

fn body_text(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::Null => None,
        Value::String(s) => Some(truncate(s, MAX_BODY)),
        other => Some(truncate(&other.to_string(), MAX_BODY)),
    }
}

/// `log` items.
pub(crate) fn log_item(st: &mut EnvelopeState<'_>, item: &Item) {
    let raw: Value = match serde_json::from_slice(&item.payload) {
        Ok(v) => v,
        Err(_) => {
            st.outcome(Category::LogItem, Outcome::Invalid, "invalid_json", 1);
            return;
        }
    };
    let list: Vec<&Value> = match raw.get("items").and_then(Value::as_array) {
        Some(items) => items.iter().take(MAX_LOGS_PER_ITEM).collect(),
        None if raw.is_object() => vec![&raw],
        None => Vec::new(),
    };
    let mut invalid = 0;
    let mut records = Vec::with_capacity(list.len());
    for l in list {
        let Some(body) = body_text(l.get("body")) else {
            invalid += 1;
            continue;
        };
        let severity = l.get("severity_number").and_then(Value::as_i64);
        let level = normalize_level(l.get("level").and_then(Value::as_str), severity);
        records.push(Record {
            timestamp: l.get("timestamp").and_then(parse_timestamp).unwrap_or(st.received),
            trace_id: opt_str(l.get("trace_id"), 64),
            span_id: opt_str(l.get("span_id"), 32),
            severity: severity.map(|s| s.clamp(0, 24) as i32),
            level,
            body,
            attributes: flatten_attributes(l.get("attributes"), MAX_ATTRIBUTES),
        });
    }
    store(st, records, invalid);
}

/// An OTLP `AnyValue` as plain JSON.
fn otel_value(v: &Value) -> Value {
    let Some(o) = v.as_object() else { return v.clone() };
    if let Some(s) = o.get("stringValue") {
        return s.clone();
    }
    if let Some(b) = o.get("boolValue") {
        return b.clone();
    }
    if let Some(i) = o.get("intValue") {
        // int64 is a string in OTLP JSON.
        return i.as_str().and_then(|s| s.parse::<i64>().ok()).map(Value::from).unwrap_or_else(|| i.clone());
    }
    if let Some(d) = o.get("doubleValue") {
        return d.clone();
    }
    if let Some(a) = o.get("arrayValue").and_then(|a| a.get("values")).and_then(Value::as_array) {
        return Value::Array(a.iter().map(otel_value).collect());
    }
    if let Some(kv) = o.get("kvlistValue").and_then(|a| a.get("values")).and_then(Value::as_array) {
        return Value::Object(otel_attributes(kv));
    }
    Value::Null
}

fn otel_attributes(list: &[Value]) -> Map<String, Value> {
    list.iter()
        .filter_map(|kv| Some((truncate(kv.get("key")?.as_str()?, 200), otel_value(kv.get("value")?))))
        .take(MAX_ATTRIBUTES)
        .collect()
}

fn nanos(v: Option<&Value>) -> Option<DateTime<Utc>> {
    let v = v?;
    let n: i128 = v.as_str().and_then(|s| s.parse().ok()).or_else(|| v.as_u64().map(i128::from))?;
    if n <= 0 {
        return None;
    }
    DateTime::from_timestamp((n / 1_000_000_000) as i64, (n % 1_000_000_000) as u32)
}

/// `otel_log` items: one OTLP JSON log record.
pub(crate) fn otel_log_item(st: &mut EnvelopeState<'_>, item: &Item) {
    let raw: Value = serde_json::from_slice(&item.payload).unwrap_or(Value::Null);
    let Some(body) = raw.get("body").map(otel_value).and_then(|b| body_text(Some(&b))) else {
        store(st, Vec::new(), 1);
        return;
    };
    let severity = raw.get("severityNumber").and_then(Value::as_i64);
    let attributes = raw.get("attributes").and_then(Value::as_array).map(|a| otel_attributes(a)).unwrap_or_default();
    let hex_id = |k: &str, max: usize| {
        opt_str(raw.get(k), max).filter(|s| s.chars().all(|c| c.is_ascii_hexdigit()) && s.chars().any(|c| c != '0'))
    };
    let record = Record {
        timestamp: nanos(raw.get("timeUnixNano"))
            .or_else(|| nanos(raw.get("observedTimeUnixNano")))
            .unwrap_or(st.received),
        trace_id: hex_id("traceId", 64).map(|s| s.to_ascii_lowercase()),
        span_id: hex_id("spanId", 32).map(|s| s.to_ascii_lowercase()),
        level: normalize_level(raw.get("severityText").and_then(Value::as_str), severity),
        severity: severity.map(|s| s.clamp(0, 24) as i32),
        body,
        attributes,
    };
    store(st, vec![record], 0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn levels() {
        assert_eq!(normalize_level(Some("warning"), None), "warn");
        assert_eq!(normalize_level(Some("ERROR"), None), "error");
        assert_eq!(normalize_level(None, Some(18)), "error");
        assert_eq!(normalize_level(Some("weird"), Some(22)), "fatal");
        assert_eq!(normalize_level(None, None), "info");
        assert_eq!(severity_of("warn"), 13);
    }

    #[test]
    fn otel_values() {
        let attrs = otel_attributes(&[
            json!({"key": "s", "value": {"stringValue": "x"}}),
            json!({"key": "i", "value": {"intValue": "42"}}),
            json!({"key": "a", "value": {"arrayValue": {"values": [{"boolValue": true}]}}}),
        ]);
        assert_eq!(Value::Object(attrs), json!({"s": "x", "i": 42, "a": [true]}));
        assert_eq!(nanos(Some(&json!("1794650000000000000"))).unwrap().timestamp(), 1_794_650_000);
    }
}
