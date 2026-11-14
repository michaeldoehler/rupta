//! Spans: the spans inside `transaction` items and standalone `span`
//! items (single spans and the version 2 containers `{"items": [...]}` of
//! SDKs that stream spans).
//!
//! Every span becomes a row in `spans`; the root span of a transaction
//! is stored as a span too (`is_segment`), so a trace can be drawn from
//! `spans` alone. Streamed segment spans are also stored as transactions.
//! Web vitals come from transaction `measurements` or from the span
//! attributes `browser.web_vital.<name>.value`.

use chrono::{DateTime, Utc};
use rupta_core::normalize::parse_timestamp;
use rupta_core::settings::Sampling;
use rupta_store::ingest::TransactionRow;
use rupta_store::telemetry::{SpanRow, Vitals};
use serde_json::{Map, Value};
use xxhash_rust::xxh3::xxh3_64;

use crate::envelope::Item;
use crate::metrics::{Category, Outcome};
use crate::process::{EnvelopeState, opt_str};

/// Most spans stored per transaction or span item.
pub const MAX_SPANS: usize = 2000;
/// Longest span description kept, in characters.
const MAX_DESCRIPTION: usize = 2000;
/// Largest `data` object kept per span, in bytes of JSON.
const MAX_DATA: usize = 16 << 10;

/// Sampling decision for an item of a trace: by trace id, so that a
/// trace is kept or dropped as a whole; without a trace id at random.
pub(crate) fn sampled(rate: f64, trace_id: Option<&str>) -> bool {
    if rate >= 1.0 {
        return true;
    }
    let key = match trace_id {
        Some(t) => xxh3_64(t.as_bytes()),
        None => xxh3_64(uuid::Uuid::new_v4().as_bytes()),
    };
    Sampling::keep(rate, key)
}

/// A typed attribute's plain value (`{"value": v, "type": t}` → `v`).
pub(crate) fn attribute_value(a: &Value) -> &Value {
    match a {
        Value::Object(o) if o.contains_key("value") => &o["value"],
        other => other,
    }
}

/// Flattens typed attributes to `{key: value}`, at most `max` keys,
/// string values truncated to 4 KiB.
pub(crate) fn flatten_attributes(attrs: Option<&Value>, max: usize) -> Map<String, Value> {
    let mut out = Map::new();
    for (k, a) in attrs.and_then(Value::as_object).into_iter().flatten() {
        if out.len() >= max {
            break;
        }
        let v = match attribute_value(a) {
            Value::String(s) => Value::String(rupta_core::normalize::truncate(s, 4096)),
            v => v.clone(),
        };
        out.insert(rupta_core::normalize::truncate(k, 200), v);
    }
    out
}

fn attr_str(attrs: &Map<String, Value>, key: &str, max: usize) -> Option<String> {
    opt_str(attrs.get(key), max)
}

fn number(v: Option<&Value>) -> Option<f64> {
    let v = v?;
    v.as_f64().or_else(|| v.as_str()?.parse().ok()).filter(|f: &f64| f.is_finite())
}

/// Web vitals from transaction `measurements` (`{"lcp": {"value": ..}}`).
pub(crate) fn vitals_from_measurements(m: Option<&Value>) -> Vitals {
    let get = |k: &str| number(m.and_then(|m| m.get(k)).and_then(|v| v.get("value")));
    Vitals { lcp: get("lcp"), fcp: get("fcp"), cls: get("cls"), inp: get("inp"), ttfb: get("ttfb") }
}

/// Web vitals from span attributes (`browser.web_vital.lcp.value`).
pub(crate) fn vitals_from_attributes(a: &Map<String, Value>) -> Vitals {
    let get = |k: &str| number(a.get(&format!("browser.web_vital.{k}.value")));
    Vitals { lcp: get("lcp"), fcp: get("fcp"), cls: get("cls"), inp: get("inp"), ttfb: get("ttfb") }
}

fn duration_ms(start: Option<DateTime<Utc>>, end: DateTime<Utc>) -> Option<f64> {
    start.map(|s| (end - s).num_microseconds().unwrap_or(0) as f64 / 1000.0)
}

fn bounded_object(v: Option<&Value>) -> Value {
    match v {
        Some(Value::Object(o)) if !o.is_empty() => {
            let v = Value::Object(o.clone());
            if v.to_string().len() > MAX_DATA { serde_json::json!({"_truncated": true}) } else { v }
        }
        _ => Value::Null,
    }
}

fn string_tags(v: Option<&Value>) -> Value {
    match v {
        Some(Value::Object(o)) if !o.is_empty() => Value::Object(
            o.iter()
                .filter_map(|(k, v)| Some((k.clone(), Value::String(rupta_core::normalize::scalar_string(v)?))))
                .take(50)
                .collect(),
        ),
        _ => Value::Null,
    }
}

/// The spans of a transaction (its root span and the `spans` array).
pub(crate) fn transaction_spans(st: &mut EnvelopeState<'_>, raw: &Value, tx: &TransactionRow, vitals: Vitals) {
    let (Some(trace_id), Some(root_id)) = (tx.trace_id.clone(), tx.span_id.clone()) else { return };
    let trace = raw.get("contexts").and_then(|c| c.get("trace"));
    let base = SpanRow {
        project_id: st.project_id,
        trace_id: trace_id.clone(),
        segment_id: Some(root_id.clone()),
        transaction: tx.name.clone(),
        environment: tx.environment.clone(),
        release: tx.release.clone(),
        platform: tx.platform.clone(),
        sdk: tx.sdk.clone(),
        received: st.received,
        ..SpanRow::default()
    };
    let mut rows = vec![SpanRow {
        span_id: root_id.clone(),
        parent_span_id: tx.parent_span_id.clone(),
        is_segment: true,
        op: tx.op.clone(),
        description: tx.name.clone(),
        status: tx.status.clone(),
        timestamp: tx.start_timestamp.unwrap_or(tx.timestamp),
        duration_ms: tx.duration_ms,
        data: bounded_object(trace.and_then(|t| t.get("data"))),
        tags: serde_json::from_str(&tx.tags)
            .ok()
            .filter(|t: &Value| t.as_object().is_some_and(|o| !o.is_empty()))
            .unwrap_or(Value::Null),
        vitals,
        ..base.clone()
    }];
    let mut dropped = 0u64;
    for s in raw.get("spans").and_then(Value::as_array).into_iter().flatten() {
        let Some(span_id) = opt_str(s.get("span_id"), 32) else {
            dropped += 1;
            continue;
        };
        if rows.len() >= MAX_SPANS {
            dropped += 1;
            continue;
        }
        let start = s.get("start_timestamp").and_then(parse_timestamp);
        let end = s.get("timestamp").and_then(parse_timestamp).or(start).unwrap_or(tx.timestamp);
        let data = bounded_object(s.get("data"));
        rows.push(SpanRow {
            span_id,
            parent_span_id: opt_str(s.get("parent_span_id"), 32),
            is_segment: false,
            op: opt_str(s.get("op"), 64),
            description: opt_str(s.get("description"), MAX_DESCRIPTION),
            status: opt_str(s.get("status"), 32),
            timestamp: start.unwrap_or(end),
            duration_ms: duration_ms(start, end),
            vitals: data.as_object().map(vitals_from_attributes).unwrap_or_default(),
            data,
            tags: string_tags(s.get("tags")),
            ..base.clone()
        });
    }
    if dropped > 0 {
        st.outcome(Category::Span, Outcome::Invalid, "span_limit_or_missing_id", dropped);
    }
    let n = rows.len() as u64;
    st.out.spans.extend(rows.into_iter().map(SpanRow::into_values));
    st.outcome(Category::Span, Outcome::Accepted, "", n);
}

/// `span` items: a single span, or a version 2 container
/// `{"items": [...]}`. Every span is stored in `spans`; segment spans
/// (the root of a trace segment) are also stored as transactions.
pub(crate) fn span_item(st: &mut EnvelopeState<'_>, item: &Item) {
    let raw: Value = match serde_json::from_slice(&item.payload) {
        Ok(v) => v,
        Err(_) => {
            st.outcome(Category::Span, Outcome::Invalid, "invalid_json", 1);
            return;
        }
    };
    let spans: Vec<&Value> = match raw.get("items").and_then(Value::as_array) {
        Some(items) => items.iter().take(MAX_SPANS).collect(),
        None => vec![&raw],
    };
    if !st.project.settings.data_types.traces {
        st.outcome(Category::Span, Outcome::Filtered, "disabled", spans.len() as u64);
        return;
    }
    let trace = st.headers.get("trace");
    let rate = st.project.settings.sampling.spans;
    let (mut accepted, mut sampled_out, mut invalid) = (0u64, 0u64, 0u64);
    for s in &spans {
        let (Some(trace_id), Some(span_id)) = (opt_str(s.get("trace_id"), 64), opt_str(s.get("span_id"), 32)) else {
            invalid += 1;
            continue;
        };
        if !sampled(rate, Some(&trace_id)) {
            sampled_out += 1;
            continue;
        }
        let attrs = flatten_attributes(s.get("attributes"), 100);
        let parent = opt_str(s.get("parent_span_id"), 32);
        let segment = s.get("is_segment").and_then(Value::as_bool).unwrap_or(parent.is_none());
        let start = s.get("start_timestamp").and_then(parse_timestamp);
        let end = s.get("end_timestamp").or(s.get("timestamp")).and_then(parse_timestamp).unwrap_or(st.received);
        let environment = attr_str(&attrs, "sentry.environment", 64)
            .or_else(|| opt_str(trace.and_then(|t| t.get("environment")), 64));
        let release =
            attr_str(&attrs, "sentry.release", 250).or_else(|| opt_str(trace.and_then(|t| t.get("release")), 250));
        let name = opt_str(s.get("name"), MAX_DESCRIPTION).or_else(|| opt_str(s.get("description"), MAX_DESCRIPTION));
        let op = attr_str(&attrs, "sentry.op", 64).or_else(|| opt_str(s.get("op"), 64));
        let segment_id = attr_str(&attrs, "sentry.segment.id", 32).or_else(|| segment.then(|| span_id.clone()));
        let segment_name =
            attr_str(&attrs, "sentry.segment.name", 200).or_else(|| segment.then(|| name.clone()).flatten());
        let vitals = vitals_from_attributes(&attrs);
        let status = opt_str(s.get("status"), 32);
        let platform = attr_str(&attrs, "sentry.platform", 32);
        let sdk = attr_str(&attrs, "sentry.sdk.name", 128);
        if segment {
            let tags: Map<String, Value> = attrs
                .iter()
                .filter(|(k, _)| !k.starts_with("sentry."))
                .filter_map(|(k, v)| Some((k.clone(), Value::String(opt_str(Some(v), 200)?))))
                .take(50)
                .collect();
            let span_count = spans
                .iter()
                .filter(|c| {
                    c.get("is_segment").and_then(Value::as_bool) != Some(true)
                        && c.get("attributes")
                            .and_then(|a| a.get("sentry.segment.id"))
                            .map(attribute_value)
                            .and_then(Value::as_str)
                            == Some(span_id.as_str())
                })
                .count();
            let user = attr_str(&attrs, "user.id", 256).or_else(|| attr_str(&attrs, "user.email", 256));
            st.out.transactions.push(TransactionRow {
                project_id: st.project_id,
                event_id: *uuid::Uuid::new_v4().as_bytes(),
                trace_id: Some(trace_id.clone()),
                span_id: Some(span_id.clone()),
                parent_span_id: parent.clone(),
                name: segment_name.clone(),
                op: op.clone(),
                status: status.clone(),
                timestamp: end,
                start_timestamp: start,
                duration_ms: duration_ms(start, end),
                received: st.received,
                environment: environment.clone(),
                release: release.clone(),
                platform: platform.clone(),
                sdk: sdk.clone(),
                user_hash: user.map(|u| xxh3_64(u.as_bytes()) as i64),
                span_count: Some(span_count.min(i32::MAX as usize) as i32),
                tags: Value::Object(tags).to_string(),
                measurements: "{}".into(),
                lcp: vitals.lcp,
                fcp: vitals.fcp,
                cls: vitals.cls,
                inp: vitals.inp,
                ttfb: vitals.ttfb,
                profile_id: attr_str(&attrs, "sentry.profile_id", 64),
                profiler_id: attr_str(&attrs, "sentry.profiler_id", 64),
            });
            st.outcome(Category::Transaction, Outcome::Accepted, "", 1);
        }
        let data = Value::Object(attrs);
        let data = if data.to_string().len() > MAX_DATA { serde_json::json!({"_truncated": true}) } else { data };
        st.out.spans.push(
            SpanRow {
                project_id: st.project_id,
                trace_id,
                span_id,
                parent_span_id: parent,
                segment_id,
                is_segment: segment,
                transaction: segment_name,
                op,
                description: name,
                status,
                timestamp: start.unwrap_or(end),
                duration_ms: duration_ms(start, end),
                environment,
                release,
                platform,
                sdk,
                received: st.received,
                data,
                tags: Value::Null,
                vitals,
            }
            .into_values(),
        );
        accepted += 1;
    }
    st.outcome(Category::Span, Outcome::Accepted, "", accepted);
    st.outcome(Category::Span, Outcome::Filtered, "sample_rate", sampled_out);
    st.outcome(Category::Span, Outcome::Invalid, "invalid_span", invalid);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn attributes_flatten() {
        let a = json!({"a": {"value": "x", "type": "string"}, "n": {"value": 3, "type": "integer"}, "plain": true});
        let f = flatten_attributes(Some(&a), 10);
        assert_eq!(Value::Object(f.clone()), json!({"a": "x", "n": 3, "plain": true}));
        assert_eq!(flatten_attributes(Some(&a), 1).len(), 1);
        let v = vitals_from_attributes(&flatten_attributes(
            Some(&json!({"browser.web_vital.lcp.value": {"value": 812.5, "type": "double"}})),
            10,
        ));
        assert_eq!(v.lcp, Some(812.5));
        let m = vitals_from_measurements(Some(&json!({"cls": {"value": 0.1, "unit": ""}, "fcp": {"value": "120"}})));
        assert_eq!((m.cls, m.fcp, m.lcp), (Some(0.1), Some(120.0), None));
    }

    #[test]
    fn trace_sampling_keeps_whole_traces() {
        assert!(sampled(1.0, None));
        assert!(!sampled(0.0, Some("t")));
        let a = sampled(0.5, Some("abc"));
        assert!((0..20).all(|_| sampled(0.5, Some("abc")) == a));
    }
}
