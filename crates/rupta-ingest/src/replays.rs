//! Session replays: `replay_event` items (one per recording segment:
//! replay id, segment id, URLs, error and trace ids, user, browser) and
//! `replay_recording` items (a JSON header line `{"segment_id": n}`, then
//! the rrweb events as a JSON array, plain or zlib/gzip-compressed).
//!
//! Segments are stored zstd-compressed in the object store under
//! `replays/{project}/{day}/{replay_id}/{segment_id}.zst`, where `day` is
//! the day the replay started, so that all segments of a replay expire
//! together. The `replays` row is created by the first item of a replay
//! and widened by every later one.

use std::io::Read;

use chrono::{DateTime, NaiveDate, Utc};
use rupta_core::normalize::{parse_timestamp, parse_user_agent};
use rupta_store::telemetry::{ReplayMeta, ReplaySegmentRow, ReplayUpdate, replay_day};
use serde_json::Value;

use crate::envelope::Item;
use crate::metrics::{Category, Outcome};
use crate::process::{EnvelopeState, compress, hex, opt_str, parse_id};

/// What the recording item of the same envelope needs from the replay event.
#[derive(Debug, Clone)]
pub(crate) struct ReplayEventInfo {
    pub replay_id: uuid::Uuid,
    pub segment_id: Option<i64>,
    pub day: NaiveDate,
}

/// The object key of a recording segment.
pub fn object_key(project_id: i64, day: NaiveDate, replay_id: uuid::Uuid, segment_id: i32) -> String {
    format!("replays/{project_id}/{}/{}/{segment_id}.zst", day.format("%Y-%m-%d"), replay_id.simple())
}

fn context_name(c: Option<&Value>) -> Option<String> {
    let c = c?;
    let name = opt_str(c.get("name"), 64).or_else(|| opt_str(c.get("model"), 64))?;
    Some(match opt_str(c.get("version"), 32) {
        Some(v) => format!("{name} {v}"),
        None => name,
    })
}

fn string_list(v: Option<&Value>, max_len: usize) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter().filter_map(|x| opt_str(Some(x), max_len)).take(rupta_store::telemetry::REPLAY_LIST_CAP).collect()
        })
        .unwrap_or_default()
}

fn hex_ids(v: Option<&Value>) -> Vec<String> {
    string_list(v, 64)
        .into_iter()
        .filter_map(|s| {
            uuid::Uuid::try_parse(&s).ok().map(hex).or_else(|| s.chars().all(|c| c.is_ascii_hexdigit()).then_some(s))
        })
        .collect()
}

fn replay_day_for(st: &EnvelopeState<'_>, replay_id: uuid::Uuid, started: Option<DateTime<Utc>>) -> NaiveDate {
    if let Some(info) = &st.replay_event
        && info.replay_id == replay_id
    {
        return info.day;
    }
    match replay_day(st.conn, st.project_id, *replay_id.as_bytes()) {
        Ok(Some(d)) => d,
        _ => started.unwrap_or(st.received).date_naive(),
    }
}

pub(crate) fn replay_event_item(st: &mut EnvelopeState<'_>, item: &Item) {
    if !st.project.settings.data_types.replays {
        st.outcome(Category::Replay, Outcome::Filtered, "disabled", 1);
        return;
    }
    let raw: Value = match serde_json::from_slice(&item.payload) {
        Ok(v @ Value::Object(_)) if item.payload.len() <= st.inner.config.max_event_bytes => v,
        _ => {
            st.outcome(Category::Replay, Outcome::Invalid, "invalid_replay_event", 1);
            return;
        }
    };
    let Some(replay_id) = parse_id(raw.get("replay_id")).or(parse_id(raw.get("event_id"))) else {
        st.outcome(Category::Replay, Outcome::Invalid, "missing_replay_id", 1);
        return;
    };
    let started = raw.get("replay_start_timestamp").and_then(parse_timestamp);
    let finished = raw.get("timestamp").and_then(parse_timestamp);
    let day = replay_day_for(st, replay_id, started.or(finished));
    let contexts = raw.get("contexts");
    let ua = parse_user_agent(
        raw.get("request")
            .and_then(|r| r.get("headers"))
            .and_then(|h| h.get("User-Agent").or(h.get("user-agent")))
            .and_then(Value::as_str)
            .or(st.user_agent)
            .unwrap_or(""),
    );
    let user = raw.get("user");
    let ip = opt_str(user.and_then(|u| u.get("ip_address")), 64)
        .filter(|ip| ip != "{{auto}}")
        .or_else(|| st.client_ip.map(|i| i.to_string()))
        .filter(|_| !st.project.settings.scrubbing.scrub_ip_addresses);
    let meta = ReplayMeta {
        replay_type: opt_str(raw.get("replay_type"), 32),
        platform: opt_str(raw.get("platform"), 32),
        sdk: opt_str(raw.get("sdk").and_then(|s| s.get("name")), 128),
        environment: opt_str(raw.get("environment"), 64),
        release: opt_str(raw.get("release"), 250),
        dist: opt_str(raw.get("dist"), 64),
        user_id: opt_str(user.and_then(|u| u.get("id")), 128),
        user_email: opt_str(user.and_then(|u| u.get("email")), 256),
        user_name: opt_str(user.and_then(|u| u.get("username")).or(user.and_then(|u| u.get("name"))), 256),
        user_ip: ip,
        browser: context_name(contexts.and_then(|c| c.get("browser")))
            .or_else(|| ua.browser.map(|(n, v)| format!("{n} {v}"))),
        os: context_name(contexts.and_then(|c| c.get("os")))
            .or_else(|| ua.os.map(|(n, v)| v.map_or(n.clone(), |v| format!("{n} {v}")))),
        device: context_name(contexts.and_then(|c| c.get("device"))),
    };
    st.replay_event =
        Some(ReplayEventInfo { replay_id, segment_id: raw.get("segment_id").and_then(Value::as_i64), day });
    st.out.replays.push(ReplayUpdate {
        project_id: st.project_id,
        replay_id: *replay_id.as_bytes(),
        // The first segment's event marks the start; later events only
        // extend the end.
        started: started.or(finished),
        finished,
        received: st.received,
        day,
        meta: Some(meta),
        urls: string_list(raw.get("urls"), 2048),
        error_ids: hex_ids(raw.get("error_ids")),
        trace_ids: hex_ids(raw.get("trace_ids")),
        segment: None,
    });
    st.outcome(Category::Replay, Outcome::Accepted, "", 1);
}

#[derive(Debug, PartialEq, thiserror::Error)]
pub enum RecordingError {
    #[error("missing header line")]
    MissingHeader,
    #[error("invalid header: {0}")]
    InvalidHeader(String),
    #[error("recording exceeds {0} bytes")]
    TooLarge(usize),
    #[error("invalid compressed data")]
    Compression,
    #[error("the recording is not a JSON array of events")]
    NotEvents,
}

/// A parsed recording segment.
#[derive(Debug)]
pub struct Recording {
    pub segment_id: Option<i64>,
    /// The rrweb events as JSON text (a JSON array).
    pub events_json: Vec<u8>,
    pub clicks: i32,
    pub first: Option<DateTime<Utc>>,
    pub last: Option<DateTime<Utc>>,
}

fn inflate(r: impl Read, limit: usize) -> Result<Vec<u8>, RecordingError> {
    let mut out = Vec::new();
    r.take(limit as u64 + 1).read_to_end(&mut out).map_err(|_| RecordingError::Compression)?;
    if out.len() > limit {
        return Err(RecordingError::TooLarge(limit));
    }
    Ok(out)
}

/// Splits and decodes a `replay_recording` payload.
pub fn parse_recording(payload: &[u8], limit: usize) -> Result<Recording, RecordingError> {
    let nl = payload.iter().position(|b| *b == b'\n').ok_or(RecordingError::MissingHeader)?;
    let header: Value =
        serde_json::from_slice(&payload[..nl]).map_err(|e| RecordingError::InvalidHeader(e.to_string()))?;
    let body = &payload[nl + 1..];
    let json = match body {
        [0x78, ..] => inflate(flate2::read::ZlibDecoder::new(body), limit)?,
        [0x1f, 0x8b, ..] => inflate(flate2::read::GzDecoder::new(body), limit)?,
        _ if body.len() > limit => return Err(RecordingError::TooLarge(limit)),
        _ => body.to_vec(),
    };
    let events: Vec<Value> = serde_json::from_slice(&json).map_err(|_| RecordingError::NotEvents)?;
    let mut clicks = 0;
    let (mut first, mut last): (Option<i64>, Option<i64>) = (None, None);
    for e in &events {
        let Some(t) = e.get("timestamp").and_then(Value::as_f64) else { continue };
        let t = t as i64;
        first = Some(first.map_or(t, |f| f.min(t)));
        last = Some(last.map_or(t, |l| l.max(t)));
        // rrweb: incremental snapshot (3) from a mouse interaction (2) of type click (2).
        let d = e.get("data");
        if e.get("type").and_then(Value::as_i64) == Some(3)
            && d.and_then(|d| d.get("source")).and_then(Value::as_i64) == Some(2)
            && d.and_then(|d| d.get("type")).and_then(Value::as_i64) == Some(2)
        {
            clicks += 1;
        }
    }
    Ok(Recording {
        segment_id: header.get("segment_id").and_then(Value::as_i64),
        events_json: json,
        clicks,
        first: first.and_then(DateTime::from_timestamp_millis),
        last: last.and_then(DateTime::from_timestamp_millis),
    })
}

pub(crate) fn replay_recording_item(st: &mut EnvelopeState<'_>, item: &Item) {
    if !st.project.settings.data_types.replays {
        st.outcome(Category::Replay, Outcome::Filtered, "disabled", 1);
        return;
    }
    let replay_id = st.replay_event.as_ref().map(|r| r.replay_id).or(st.header_event_id);
    let Some(replay_id) = replay_id else {
        st.outcome(Category::Replay, Outcome::Invalid, "missing_replay_id", 1);
        return;
    };
    let rec = match parse_recording(&item.payload, st.inner.config.max_attachment_bytes) {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!(error = %e, "invalid replay recording");
            st.outcome(Category::Replay, Outcome::Invalid, "invalid_replay_recording", 1);
            return;
        }
    };
    let segment_id = rec
        .segment_id
        .or_else(|| st.replay_event.as_ref().and_then(|r| r.segment_id))
        .unwrap_or(0)
        .clamp(0, i32::MAX as i64) as i32;
    let day = replay_day_for(st, replay_id, rec.first);
    let key = object_key(st.project_id, day, replay_id, segment_id);
    let data = compress(&rec.events_json);
    if let Err(e) = st.inner.store.objects().put(&key, &data) {
        tracing::error!(error = %e, "storing a replay segment failed");
        st.outcome(Category::Replay, Outcome::Invalid, "storage_error", 1);
        return;
    }
    st.out.replays.push(ReplayUpdate {
        project_id: st.project_id,
        replay_id: *replay_id.as_bytes(),
        started: rec.first,
        finished: rec.last,
        received: st.received,
        day,
        meta: None,
        urls: Vec::new(),
        error_ids: Vec::new(),
        trace_ids: Vec::new(),
        segment: Some(ReplaySegmentRow { segment_id, object_key: key, size: data.len() as i64, clicks: rec.clicks }),
    });
    st.outcome(Category::Replay, Outcome::Accepted, "", 1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn recordings_plain_and_compressed() {
        let events = br#"[{"type":4,"data":{"href":"http://a/"},"timestamp":1794650000000},
            {"type":3,"data":{"source":2,"type":2,"id":5},"timestamp":1794650001500},
            {"type":3,"data":{"source":2,"type":1},"timestamp":1794650002000}]"#;
        let mut plain = b"{\"segment_id\":3}\n".to_vec();
        plain.extend_from_slice(events);
        let r = parse_recording(&plain, 1 << 20).unwrap();
        assert_eq!(r.segment_id, Some(3));
        assert_eq!(r.clicks, 1);
        assert_eq!(r.first.unwrap().timestamp_millis(), 1_794_650_000_000);
        assert_eq!(r.last.unwrap().timestamp_millis(), 1_794_650_002_000);
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(events).unwrap();
        let mut zl = b"{\"segment_id\":0}\n".to_vec();
        zl.extend(z.finish().unwrap());
        let r = parse_recording(&zl, 1 << 20).unwrap();
        assert_eq!(&r.events_json[..], &events[..]);
        assert_eq!(parse_recording(&zl, 10).unwrap_err(), RecordingError::TooLarge(10));
        assert_eq!(parse_recording(b"{}", 100).unwrap_err(), RecordingError::MissingHeader);
        assert_eq!(parse_recording(b"{}\n{\"a\":1}", 100).unwrap_err(), RecordingError::NotEvents);
    }
}
