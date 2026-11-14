//! Metrics: `trace_metric` items (the current SDK format, a container of
//! counters, gauges and distributions with typed attributes) and the
//! deprecated `statsd` (text lines) and `metric_buckets` (JSON buckets)
//! items.
//!
//! Every value is one row of `metrics`. Counters store the increment,
//! gauges their last value, distributions one row per value, and sets one
//! row per member with the member hashed to a number (`count_unique`
//! counts them).

use chrono::{DateTime, Utc};
use rupta_core::normalize::{parse_timestamp, truncate};
use rupta_store::telemetry::MetricRow;
use serde_json::{Map, Value};
use xxhash_rust::xxh3::xxh3_64;

use crate::envelope::Item;
use crate::metrics::{Category, Outcome};
use crate::process::{EnvelopeState, opt_str};
use crate::traces::flatten_attributes;

/// Most values accepted per item.
pub const MAX_VALUES_PER_ITEM: usize = 10_000;

/// A parsed metric value before it becomes a row.
#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    pub name: String,
    pub ty: &'static str,
    pub unit: Option<String>,
    pub value: f64,
    pub timestamp: Option<DateTime<Utc>>,
    pub attributes: Map<String, Value>,
}

fn metric_type(t: &str) -> Option<&'static str> {
    match t {
        "c" | "counter" => Some("counter"),
        "g" | "gauge" => Some("gauge"),
        "d" | "ms" | "h" | "distribution" => Some("distribution"),
        "s" | "set" => Some("set"),
        _ => None,
    }
}

/// Set members are stored as 32-bit hashes (exact as `f64`); numeric
/// members as themselves.
fn set_member(v: &str) -> f64 {
    match v.parse::<i64>() {
        Ok(n) if n.unsigned_abs() < (1u64 << 52) => n as f64,
        _ => (xxh3_64(v.as_bytes()) & 0xffff_ffff) as f64,
    }
}

fn clean_unit(u: &str) -> Option<String> {
    let u = u.trim();
    (!u.is_empty() && u != "none").then(|| truncate(u, 64))
}

/// Splits a metric resource identifier (`c:custom/name@unit`) or a
/// statsd name (`name@unit`) into name and unit.
fn name_and_unit(s: &str) -> (String, Option<String>) {
    let s = match s.split_once(':') {
        Some((t, rest)) if t.len() == 1 => rest,
        _ => s,
    };
    let s = s.strip_prefix("custom/").unwrap_or(s);
    match s.rsplit_once('@') {
        Some((n, u)) => (truncate(n, 200), clean_unit(u)),
        None => (truncate(s, 200), None),
    }
}

/// Parses `statsd` text: `name@unit:value[:value...]|type|#tag:v,tag:v|T<unix>`.
pub fn parse_statsd(text: &str) -> (Vec<Sample>, u64) {
    let mut out = Vec::new();
    let mut invalid = 0;
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let mut parts = line.split('|');
        let head = parts.next().unwrap_or("");
        let Some((name, values)) = head.split_once(':') else {
            invalid += 1;
            continue;
        };
        let Some(ty) = parts.next().and_then(metric_type) else {
            invalid += 1;
            continue;
        };
        let mut attributes = Map::new();
        let mut timestamp = None;
        for p in parts {
            if let Some(tags) = p.strip_prefix('#') {
                for t in tags.split(',').filter(|t| !t.is_empty()) {
                    let (k, v) = t.split_once(':').unwrap_or((t, ""));
                    attributes.insert(truncate(k, 200), Value::String(truncate(v, 200)));
                }
            } else if let Some(t) = p.strip_prefix('T') {
                timestamp = t.parse::<i64>().ok().and_then(|t| DateTime::from_timestamp(t, 0));
            }
        }
        let (name, unit) = name_and_unit(name);
        if name.is_empty() {
            invalid += 1;
            continue;
        }
        let raw: Vec<&str> = values.split(':').collect();
        let numbers = || raw.iter().filter_map(|v| v.parse::<f64>().ok().filter(|f| f.is_finite()));
        let values: Vec<f64> = match ty {
            "counter" => vec![numbers().sum()],
            // Gauges are sent as last:min:max:sum:count.
            "gauge" => numbers().next().into_iter().collect(),
            "set" => raw.iter().map(|v| set_member(v)).collect(),
            _ => numbers().collect(),
        };
        if values.is_empty() {
            invalid += 1;
            continue;
        }
        for value in values {
            out.push(Sample {
                name: name.clone(),
                ty,
                unit: unit.clone(),
                value,
                timestamp,
                attributes: attributes.clone(),
            });
        }
    }
    (out, invalid)
}

/// Parses `metric_buckets` JSON: `[{"timestamp", "width", "name" (MRI),
/// "type", "value", "tags"}]`.
pub fn parse_buckets(raw: &Value) -> (Vec<Sample>, u64) {
    let mut out = Vec::new();
    let mut invalid = 0;
    let list = raw.as_array().or_else(|| raw.get("buckets").and_then(Value::as_array));
    for b in list.into_iter().flatten() {
        let (Some(name), Some(ty)) =
            (b.get("name").and_then(Value::as_str), b.get("type").and_then(Value::as_str).and_then(metric_type))
        else {
            invalid += 1;
            continue;
        };
        let (name, unit) = name_and_unit(name);
        let attributes: Map<String, Value> = b
            .get("tags")
            .and_then(Value::as_object)
            .map(|t| t.iter().take(100).map(|(k, v)| (truncate(k, 200), v.clone())).collect())
            .unwrap_or_default();
        let timestamp = b.get("timestamp").and_then(parse_timestamp);
        let v = b.get("value");
        let values: Vec<f64> = match (ty, v) {
            ("gauge", Some(Value::Object(g))) => g.get("last").and_then(Value::as_f64).into_iter().collect(),
            ("set", Some(Value::Array(a))) => {
                a.iter().map(|m| m.as_f64().unwrap_or_else(|| set_member(&m.to_string()))).collect()
            }
            (_, Some(Value::Array(a))) => a.iter().filter_map(Value::as_f64).collect(),
            (_, Some(n)) => n.as_f64().into_iter().collect(),
            _ => Vec::new(),
        };
        if values.is_empty() {
            invalid += 1;
            continue;
        }
        for value in values.into_iter().filter(|f| f.is_finite()) {
            out.push(Sample {
                name: name.clone(),
                ty,
                unit: unit.clone(),
                value,
                timestamp,
                attributes: attributes.clone(),
            });
        }
    }
    (out, invalid)
}

fn store(
    st: &mut EnvelopeState<'_>,
    category: Category,
    samples: Vec<(Sample, Option<String>, Option<String>)>,
    invalid: u64,
) {
    if !st.project.settings.data_types.metrics {
        st.outcome(category, Outcome::Filtered, "disabled", samples.len() as u64 + invalid);
        return;
    }
    let trace = st.headers.get("trace");
    let header_env = opt_str(trace.and_then(|t| t.get("environment")), 64);
    let header_release = opt_str(trace.and_then(|t| t.get("release")), 250);
    let n = samples.len().min(MAX_VALUES_PER_ITEM);
    let dropped = samples.len() - n;
    for (s, trace_id, span_id) in samples.into_iter().take(MAX_VALUES_PER_ITEM) {
        let get = |keys: &[&str], max: usize| keys.iter().find_map(|k| opt_str(s.attributes.get(*k), max));
        let row = MetricRow {
            project_id: st.project_id,
            timestamp: s.timestamp.unwrap_or(st.received),
            received: st.received,
            environment: get(&["sentry.environment", "environment"], 64).or_else(|| header_env.clone()),
            release: get(&["sentry.release", "release"], 250).or_else(|| header_release.clone()),
            name: s.name,
            ty: s.ty.to_string(),
            unit: s.unit,
            value: s.value,
            trace_id,
            span_id,
            attributes: if s.attributes.is_empty() { Value::Null } else { Value::Object(s.attributes) },
        };
        st.out.metrics.push(row.into_values());
    }
    st.outcome(category, Outcome::Accepted, "", n as u64);
    st.outcome(category, Outcome::Invalid, "too_many_values", dropped as u64);
    st.outcome(category, Outcome::Invalid, "invalid_metric", invalid);
}

/// `trace_metric` items.
pub(crate) fn trace_metric_item(st: &mut EnvelopeState<'_>, item: &Item) {
    let raw: Value = match serde_json::from_slice(&item.payload) {
        Ok(v) => v,
        Err(_) => {
            st.outcome(Category::TraceMetric, Outcome::Invalid, "invalid_json", 1);
            return;
        }
    };
    let list: Vec<&Value> = match raw.get("items").and_then(Value::as_array) {
        Some(items) => items.iter().collect(),
        None if raw.is_object() => vec![&raw],
        None => Vec::new(),
    };
    let mut invalid = 0;
    let mut samples = Vec::with_capacity(list.len());
    for m in list {
        let name = m.get("name").and_then(Value::as_str).map(|n| truncate(n.trim(), 200)).filter(|n| !n.is_empty());
        let ty = m.get("type").and_then(Value::as_str).and_then(metric_type);
        let value = m.get("value").and_then(Value::as_f64).filter(|f| f.is_finite());
        let (Some(name), Some(ty), Some(value)) = (name, ty, value) else {
            invalid += 1;
            continue;
        };
        let sample = Sample {
            name,
            ty,
            unit: m.get("unit").and_then(Value::as_str).and_then(clean_unit),
            value,
            timestamp: m.get("timestamp").and_then(parse_timestamp),
            attributes: flatten_attributes(m.get("attributes"), 100),
        };
        samples.push((sample, opt_str(m.get("trace_id"), 64), opt_str(m.get("span_id"), 32)));
    }
    store(st, Category::TraceMetric, samples, invalid);
}

/// `statsd` items (deprecated).
pub(crate) fn statsd_item(st: &mut EnvelopeState<'_>, item: &Item) {
    let text = String::from_utf8_lossy(&item.payload);
    let (samples, invalid) = parse_statsd(&text);
    store(st, Category::MetricBucket, samples.into_iter().map(|s| (s, None, None)).collect(), invalid);
}

/// `metric_buckets` items (deprecated).
pub(crate) fn metric_buckets_item(st: &mut EnvelopeState<'_>, item: &Item) {
    let raw: Value = serde_json::from_slice(&item.payload).unwrap_or(Value::Null);
    let (samples, invalid) = parse_buckets(&raw);
    let invalid = if raw.is_null() { invalid + 1 } else { invalid };
    store(st, Category::MetricBucket, samples.into_iter().map(|s| (s, None, None)).collect(), invalid);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn statsd_lines() {
        let (s, invalid) = parse_statsd(
            "page.load@millisecond:120.5:80|d|#route:/home,environment:prod|T1794650000\n\
             button.click:1:2|c|#button:buy\n\
             memory@byte:512:256:1024:2048:4|g\n\
             users:alice:bob:42|s\n\
             broken\n\
             x:1|q\n",
        );
        assert_eq!(invalid, 2);
        let d: Vec<&Sample> = s.iter().filter(|x| x.name == "page.load").collect();
        assert_eq!(d.len(), 2);
        assert_eq!((d[0].ty, d[0].unit.as_deref(), d[0].value), ("distribution", Some("millisecond"), 120.5));
        assert_eq!(d[0].attributes["route"], "/home");
        assert_eq!(d[0].timestamp.unwrap().timestamp(), 1_794_650_000);
        let c = s.iter().find(|x| x.name == "button.click").unwrap();
        assert_eq!((c.ty, c.value), ("counter", 3.0));
        let g = s.iter().find(|x| x.name == "memory").unwrap();
        assert_eq!((g.ty, g.value), ("gauge", 512.0));
        let sets: Vec<f64> = s.iter().filter(|x| x.ty == "set").map(|x| x.value).collect();
        assert_eq!(sets.len(), 3);
        assert_eq!(sets[2], 42.0);
        assert_ne!(sets[0], sets[1]);
        assert_eq!(set_member("alice"), sets[0], "hashes are stable");
    }

    #[test]
    fn metric_buckets() {
        let (s, invalid) = parse_buckets(&json!([
            {"timestamp": 1794650000, "width": 10, "name": "c:custom/requests@none", "type": "c", "value": 4.0, "tags": {"route": "/"}},
            {"timestamp": 1794650000, "name": "d:custom/latency@second", "type": "d", "value": [0.1, 0.2]},
            {"name": "g:custom/queue@none", "type": "g", "value": {"last": 7.0, "min": 1.0, "max": 9.0, "sum": 20.0, "count": 4}},
            {"name": "s:custom/visitors@none", "type": "s", "value": [1, 2, 3]},
            {"name": "x", "type": "nope", "value": 1}
        ]));
        assert_eq!(invalid, 1);
        assert_eq!(s[0].name, "requests");
        assert_eq!(s[0].unit, None);
        assert_eq!(s[0].attributes["route"], "/");
        assert_eq!(s.iter().filter(|x| x.name == "latency").count(), 2);
        assert_eq!(s.iter().find(|x| x.name == "latency").unwrap().unit.as_deref(), Some("second"));
        assert_eq!(s.iter().find(|x| x.name == "queue").unwrap().value, 7.0);
        assert_eq!(s.iter().filter(|x| x.ty == "set").count(), 3);
    }
}
