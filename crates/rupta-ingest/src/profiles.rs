//! Profiles: `profile` items (transaction profiling, sampled format
//! version 1) and `profile_chunk` items (continuous profiling, version 2).
//!
//! The document is checked (it must parse as a sampled profile), stored
//! zstd-compressed in the object store under
//! `profiles/{project}/{day}/{profile_id}.json.zst`, and described by a row
//! in `profiles`. Flamegraphs are built when they are read.

use rupta_core::normalize::parse_timestamp;
use rupta_core::profile::SampledProfile;
use rupta_store::telemetry::ProfileRow;
use serde_json::Value;

use crate::envelope::Item;
use crate::metrics::{Category, Outcome};
use crate::process::{EnvelopeState, compress, opt_str, parse_id};

/// The object key of a stored profile.
pub fn object_key(project_id: i64, day: chrono::NaiveDate, profile_id: uuid::Uuid) -> String {
    format!("profiles/{project_id}/{}/{}.json.zst", day.format("%Y-%m-%d"), profile_id.simple())
}

pub(crate) fn profile_item(st: &mut EnvelopeState<'_>, item: &Item) {
    if !st.project.settings.data_types.profiles {
        st.outcome(Category::Profile, Outcome::Filtered, "disabled", 1);
        return;
    }
    if item.payload.len() > st.inner.config.max_attachment_bytes {
        st.outcome(Category::Profile, Outcome::Invalid, "too_large", 1);
        return;
    }
    let doc: Value = match serde_json::from_slice(&item.payload) {
        Ok(v @ Value::Object(_)) => v,
        _ => {
            st.outcome(Category::Profile, Outcome::Invalid, "invalid_json", 1);
            return;
        }
    };
    let parsed = match SampledProfile::parse(&doc) {
        Ok(p) => p,
        Err(_) => {
            st.outcome(Category::Profile, Outcome::Invalid, "invalid_profile", 1);
            return;
        }
    };
    let chunk = item.ty == "profile_chunk" || doc.get("chunk_id").is_some();
    let tx = doc.get("transaction").or_else(|| doc.get("transactions").and_then(|t| t.get(0)));
    let id = if chunk {
        parse_id(doc.get("chunk_id"))
    } else {
        parse_id(doc.get("event_id")).or(parse_id(doc.get("profile_id")))
    }
    .unwrap_or_else(uuid::Uuid::new_v4);
    let range = parsed.time_range();
    let started = range
        .and_then(|(from, _)| chrono::DateTime::from_timestamp_micros((from * 1e6) as i64))
        .or_else(|| doc.get("timestamp").and_then(parse_timestamp))
        .unwrap_or(st.received);
    let duration_ms = match tx.and_then(|t| Some((t.get("relative_start_ns")?, t.get("relative_end_ns")?))) {
        Some((a, b)) => {
            let n = |v: &Value| v.as_f64().or_else(|| v.as_str()?.parse().ok());
            match (n(a), n(b)) {
                (Some(a), Some(b)) if b >= a => Some((b - a) / 1e6),
                _ => range.map(|(a, b)| (b - a) * 1000.0),
            }
        }
        None => range.map(|(a, b)| (b - a) * 1000.0),
    };
    let day = started.date_naive();
    let key = object_key(st.project_id, day, id);
    let data = compress(&item.payload);
    if let Err(e) = st.inner.store.objects().put(&key, &data) {
        tracing::error!(error = %e, "storing a profile failed");
        st.outcome(Category::Profile, Outcome::Invalid, "storage_error", 1);
        return;
    }
    let trace = st.headers.get("trace");
    st.out.profiles.push(ProfileRow {
        project_id: st.project_id,
        profile_id: *id.as_bytes(),
        kind: if chunk { "chunk" } else { "transaction" }.to_string(),
        transaction_name: opt_str(tx.and_then(|t| t.get("name")), 200)
            .or_else(|| opt_str(trace.and_then(|t| t.get("transaction")), 200).filter(|_| !chunk)),
        transaction_id: parse_id(tx.and_then(|t| t.get("id"))).map(|u| *u.as_bytes()),
        trace_id: opt_str(tx.and_then(|t| t.get("trace_id")), 64),
        profiler_id: opt_str(doc.get("profiler_id"), 64),
        platform: opt_str(doc.get("platform"), 32),
        release: opt_str(doc.get("release"), 250).or_else(|| opt_str(trace.and_then(|t| t.get("release")), 250)),
        environment: opt_str(doc.get("environment"), 64)
            .or_else(|| opt_str(trace.and_then(|t| t.get("environment")), 64)),
        started,
        duration_ms,
        samples: parsed.samples.len().min(i32::MAX as usize) as i32,
        size: item.payload.len() as i64,
        object_key: key,
        day,
        received: st.received,
    });
    st.outcome(Category::Profile, Outcome::Accepted, "", 1);
}
