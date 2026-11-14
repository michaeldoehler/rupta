//! Ingest workers: turn a queued envelope into rows for the batch writer.
//!
//! Per item type:
//!
//! | item | result |
//! | --- | --- |
//! | `event` | normalized, filtered, symbolicated, scrubbed, grouped; row in `events`, payload in `event_payloads` |
//! | `transaction` | row in `transactions` |
//! | `attachment` | bytes to the object store, row in `attachments` |
//! | `session`, `sessions` | release health counts in `session_counts` |
//! | `client_report` | `outcomes` with outcome `client_discard` |
//! | `user_report`, `feedback` | row in `user_feedback` |
//! | `check_in` | monitor (created on first use) and row in `checkins` |
//! | anything else | counted as `dropped_unsupported` |

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Timelike, Utc};
use rupta_core::event::Event;
use rupta_core::grouping::{group, issue_info};
use rupta_core::normalize::{self, parse_timestamp, scalar_string, truncate};
use rupta_core::scrub::Scrubber;
use rupta_store::ingest::*;
use serde_json::{Map, Value};
use xxhash_rust::xxh3::xxh3_64;

use crate::envelope::Item;
use crate::metrics::{Category, Outcome};
use crate::{Inner, Job, Msg};

/// Fingerprint hash → issue id, shared by the workers.
#[derive(Default)]
pub(crate) struct IssueCache {
    shards: [Mutex<HashMap<(i64, String), i64>>; 16],
}

impl IssueCache {
    fn shard(&self, project_id: i64, hash: &str) -> &Mutex<HashMap<(i64, String), i64>> {
        let h = xxh3_64(hash.as_bytes()) ^ project_id as u64;
        &self.shards[(h % 16) as usize]
    }

    pub fn get(&self, project_id: i64, hash: &str) -> Option<i64> {
        self.shard(project_id, hash)
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&(project_id, hash.to_string()))
            .copied()
    }

    pub fn insert(&self, project_id: i64, hash: &str, id: i64) {
        let mut m = self.shard(project_id, hash).lock().unwrap_or_else(|e| e.into_inner());
        if m.len() > 200_000 {
            m.clear();
        }
        m.insert((project_id, hash.to_string()), id);
    }

    /// Forgets issues that no longer exist.
    pub fn remove_ids(&self, ids: &[i64]) {
        for s in &self.shards {
            s.lock().unwrap_or_else(|e| e.into_inner()).retain(|_, v| !ids.contains(v));
        }
    }
}

/// An event ready to be written.
pub(crate) struct StoredEvent {
    pub row: EventRow,
    pub payload: PayloadRow,
    pub new_issue: bool,
    pub org_id: i64,
}

/// A session update for release health.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SessionUpdate {
    pub project_id: i64,
    pub org_id: i64,
    pub sid: String,
    pub init: bool,
    pub started: DateTime<Utc>,
    pub status: String,
    pub errors: u64,
    pub release: String,
    pub environment: String,
}

/// Feedback whose issue the writer resolves from the event id.
pub(crate) struct PendingFeedback {
    pub row: FeedbackRow,
}

/// What a worker hands to the writer for one envelope.
#[derive(Default)]
pub(crate) struct Processed {
    pub events: Vec<StoredEvent>,
    pub transactions: Vec<TransactionRow>,
    pub attachments: Vec<AttachmentRow>,
    pub sessions: Vec<SessionUpdate>,
    pub session_counts: Vec<(i64, SessionCount)>,
    pub feedback: Vec<PendingFeedback>,
    pub checkins: Vec<CheckIn>,
    pub outcomes: Vec<(OutcomeKey, i64)>,
    pub ack: Option<tokio::sync::oneshot::Sender<bool>>,
    /// Counts as one in-flight envelope until committed.
    pub counted: bool,
}

pub(crate) fn worker(
    inner: Arc<Inner>,
    rx: crossbeam_channel::Receiver<Msg>,
    wtx: crossbeam_channel::Sender<Processed>,
) {
    let conn = inner.store.connect();
    while let Ok(msg) = rx.recv() {
        match msg {
            Msg::Stop => break,
            Msg::Job(job) => {
                let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| process(&inner, &conn, *job)));
                match out {
                    Ok(p) => {
                        if wtx.send(p).is_err() {
                            break;
                        }
                    }
                    Err(_) => {
                        tracing::error!("ingest worker panicked while processing an envelope; envelope dropped");
                        inner.in_flight.fetch_sub(1, Ordering::AcqRel);
                    }
                }
            }
        }
    }
}

fn parse_id(v: Option<&Value>) -> Option<uuid::Uuid> {
    v.and_then(Value::as_str).and_then(|s| uuid::Uuid::try_parse(s.trim()).ok())
}

fn hex(id: uuid::Uuid) -> String {
    id.simple().to_string()
}

/// State of one envelope while its items are handled. Every handler gets
/// it together with its item (raw bytes and item headers).
pub(crate) struct EnvelopeState<'a> {
    inner: &'a Inner,
    conn: &'a rupta_store::squad::Connection,
    project: &'a crate::auth::ProjectInfo,
    /// The envelope headers (`event_id`, `dsn`, `sdk`, `trace`, `sent_at`, ...).
    #[allow(dead_code)]
    headers: &'a Map<String, Value>,
    project_id: i64,
    org_id: i64,
    received: DateTime<Utc>,
    client_ip: Option<std::net::IpAddr>,
    user_agent: Option<&'a str>,
    header_event_id: Option<uuid::Uuid>,
    /// The id of the envelope's event: links attachments to it.
    event_id: Option<uuid::Uuid>,
    /// Why the envelope's event was not stored; its attachments follow it.
    event_dropped: Option<&'static str>,
    out: Processed,
}

impl EnvelopeState<'_> {
    fn outcome(&mut self, category: Category, outcome: Outcome, reason: &str, n: u64) {
        self.inner.metrics.count(category, outcome, n);
        let key = crate::writer::outcome_key(self.project_id, self.received, category, outcome.as_str(), reason);
        self.out.outcomes.push((key, n as i64));
    }
}

/// Items that carry an event id are handled first so that attachments in
/// the same envelope can refer to the event.
fn handling_order(ty: &str) -> u8 {
    match ty {
        "event" | "security" | "transaction" | "feedback" => 0,
        _ => 1,
    }
}

pub(crate) fn process(inner: &Inner, conn: &rupta_store::squad::Connection, job: Job) -> Processed {
    let Job { project, envelope, received, client_ip, user_agent, ack } = job;
    let header_event_id = parse_id(envelope.headers.get("event_id"));
    let mut st = EnvelopeState {
        inner,
        conn,
        project: &project,
        headers: &envelope.headers,
        project_id: project.id,
        org_id: project.org_id,
        received,
        client_ip,
        user_agent: user_agent.as_deref(),
        header_event_id,
        event_id: header_event_id,
        event_dropped: None,
        out: Processed { ack, counted: true, ..Processed::default() },
    };
    let mut order: Vec<&Item> = envelope.items.iter().collect();
    order.sort_by_key(|i| handling_order(&i.ty));
    for item in order {
        handle_item(&mut st, item);
    }
    st.out
}

/// The single dispatch point: one handler per envelope item type.
fn handle_item(st: &mut EnvelopeState<'_>, item: &Item) {
    match item.ty.as_str() {
        "event" => event_item(st, item, Category::Error),
        "security" => event_item(st, item, Category::Security),
        "transaction" => json_event_item(st, item, Category::Transaction, transaction),
        "feedback" => json_event_item(st, item, Category::Feedback, feedback),
        "attachment" => attachment(st, item),
        "session" => session(st, item),
        "sessions" => sessions(st, item),
        "client_report" => client_report(st, item),
        "user_report" => user_report(st, item),
        "check_in" => check_in(st, item),
        // Acknowledged but not stored yet: replays, profiles, metrics,
        // spans and logs. Handlers for them plug in here.
        "replay_event" | "replay_recording" | "replay_video" | "profile" | "profile_chunk" | "statsd"
        | "metric_buckets" | "metric_meta" | "trace_metric" | "span" | "otel_span" | "log" | "otel_log" => {
            unsupported(st, item, &item.ty.clone())
        }
        _ => unsupported(st, item, "unknown_item"),
    }
}

fn unsupported(st: &mut EnvelopeState<'_>, item: &Item, reason: &str) {
    st.outcome(Category::of_item(&item.ty), Outcome::DroppedUnsupported, reason, 1);
}

/// Parses an event-like item's JSON (size limit, object check) and
/// assigns its event id.
fn event_json(st: &mut EnvelopeState<'_>, item: &Item, category: Category) -> Option<(Value, uuid::Uuid)> {
    if item.payload.len() > st.inner.config.max_event_bytes {
        st.outcome(category, Outcome::Invalid, "too_large", 1);
        st.event_dropped = Some("too_large");
        return None;
    }
    let raw: Value = match serde_json::from_slice(&item.payload) {
        Ok(v @ Value::Object(_)) => v,
        _ => {
            st.outcome(category, Outcome::Invalid, "invalid_json", 1);
            st.event_dropped = Some("invalid_json");
            return None;
        }
    };
    let id = parse_id(raw.get("event_id")).or(st.header_event_id).unwrap_or_else(uuid::Uuid::new_v4);
    // Attachments belong to the error event; other event-like items only
    // name the envelope's event when there is none.
    if matches!(item.ty.as_str(), "event" | "security") || st.event_id.is_none() {
        st.event_id = Some(id);
    }
    Some((raw, id))
}

fn json_event_item(
    st: &mut EnvelopeState<'_>,
    item: &Item,
    category: Category,
    handler: fn(&mut EnvelopeState<'_>, &Value, uuid::Uuid),
) {
    if let Some((raw, id)) = event_json(st, item, category) {
        handler(st, &raw, id);
    }
}

fn event_item(st: &mut EnvelopeState<'_>, item: &Item, category: Category) {
    let Some((raw, id)) = event_json(st, item, category) else { return };
    let ctx = normalize::Context {
        project_id: st.project_id,
        event_id: hex(id),
        received: st.received,
        client_ip: st.client_ip,
        user_agent: st.user_agent,
        settings: &st.project.settings,
    };
    if let Err(reason) = event(st, &raw, &ctx, category) {
        st.event_dropped = Some(reason);
    }
}

fn context_name(ev: &Event, key: &str) -> Option<String> {
    ev.context_summary(key)
}

fn event(
    env: &mut EnvelopeState<'_>,
    raw: &Value,
    ctx: &normalize::Context<'_>,
    category: Category,
) -> Result<(), &'static str> {
    let (conn, project, user_agent) = (env.conn, env.project, env.user_agent);
    let mut ev = normalize::normalize(raw, ctx);
    if let Some(reason) = crate::filters::filter_reason(&ev, &project.settings.filters, user_agent) {
        env.outcome(category, Outcome::Filtered, reason, 1);
        return Err("filtered");
    }
    env.inner.symbolicator.symbolicate(conn, env.inner.store.objects().as_ref(), project, &mut ev);
    Scrubber::new(&project.settings.scrubbing).scrub(&mut ev);
    let grouping = group(&ev);
    let info = issue_info(&ev);
    ev.title = info.title.clone();
    ev.culprit = info.culprit.clone();
    let (issue_id, new_issue) = match resolve_issue(env.inner, conn, &ev, &grouping, &info) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(error = %e, "resolving the issue of an event failed; event dropped");
            env.outcome(category, Outcome::Invalid, "internal_error", 1);
            return Err("internal_error");
        }
    };
    ev.issue_id = Some(rupta_core::ids::Id(issue_id));
    ev.grouping = Some(grouping);

    let event_id = uuid::Uuid::try_parse(&ev.event_id).map(|u| *u.as_bytes()).unwrap_or_default();
    let main = ev.main_exception();
    let tags: Map<String, Value> = ev.tags.iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect();
    let user = ev.user.as_ref();
    let row = EventRow {
        project_id: project.id,
        issue_id,
        event_id,
        timestamp: ev.timestamp,
        received: ev.received,
        level: Some(ev.level.clone()),
        platform: Some(ev.platform.clone()),
        environment: ev.environment.clone(),
        release: ev.release.clone(),
        dist: ev.dist.clone(),
        transaction: ev.transaction.clone(),
        message: ev.message.clone().map(|m| truncate(&m, 1024)),
        exception_type: main.and_then(|e| e.ty.clone()),
        exception_value: main.and_then(|e| e.value.clone()).map(|v| truncate(&v, 1024)),
        user_id: user.and_then(|u| u.id.clone()),
        user_email: user.and_then(|u| u.email.clone()),
        user_ip: user.and_then(|u| u.ip_address.clone()),
        user_hash: user.and_then(|u| u.identity()).map(|i| xxh3_64(i.as_bytes()) as i64),
        os: context_name(&ev, "os"),
        browser: context_name(&ev, "browser"),
        device: context_name(&ev, "device"),
        runtime: context_name(&ev, "runtime"),
        sdk: ev.sdk.as_ref().and_then(|s| s.name.clone()),
        server_name: ev.server_name.clone(),
        url: ev.request.as_ref().and_then(|r| r.url.clone()),
        tags: Value::Object(tags).to_string(),
        contexts: Value::Object(ev.contexts.clone()).to_string(),
    };
    let json = serde_json::to_vec(&ev).unwrap_or_default();
    let data = zstd::bulk::compress(&json, 3).unwrap_or_default();
    let payload =
        PayloadRow { project_id: project.id, event_id, issue_id: Some(issue_id), day: ev.timestamp.date_naive(), data };
    env.inner.metrics.count(category, Outcome::Accepted, 1);
    let key = crate::writer::outcome_key(project.id, env.received, category, "accepted", "");
    env.out.outcomes.push((key, 1));
    env.out.events.push(StoredEvent { row, payload, new_issue, org_id: project.org_id });
    Ok(())
}

/// The issue of an event: from the cache, the database, or created.
fn resolve_issue(
    inner: &Inner,
    conn: &rupta_store::squad::Connection,
    ev: &Event,
    grouping: &rupta_core::event::GroupingInfo,
    info: &rupta_core::grouping::IssueInfo,
) -> rupta_store::Result<(i64, bool)> {
    let project_id = ev.project_id.0;
    if let Some(id) = inner.issues.get(project_id, &grouping.hash) {
        return Ok((id, false));
    }
    for _ in 0..3 {
        if let Some(id) = find_issue_id(conn, project_id, &grouping.hash)? {
            inner.issues.insert(project_id, &grouping.hash, id);
            return Ok((id, false));
        }
        let new = NewIssue {
            project_id,
            fingerprint_hash: grouping.hash.clone(),
            grouping_version: grouping.version.clone(),
            title: info.title.clone(),
            culprit: info.culprit.clone(),
            ty: ev.ty.clone(),
            level: ev.level.clone(),
            platform: Some(ev.platform.clone()),
            first_seen: ev.timestamp,
            last_seen: ev.timestamp,
            release: ev.release.clone(),
            metadata: info.metadata.clone(),
        };
        let attempt = (|| {
            let tx = conn.transaction()?;
            let (id, _) = create_issue(&tx, &new)?;
            tx.commit()?;
            Ok::<i64, rupta_store::StoreError>(id)
        })();
        match attempt {
            Ok(id) => {
                inner.issues.insert(project_id, &grouping.hash, id);
                return Ok((id, true));
            }
            // Another worker created it at the same time: look it up.
            Err(e) if e.is_unique_violation() || matches!(e.sqlstate(), Some("40001" | "40P01")) => continue,
            Err(e) => return Err(e),
        }
    }
    Err(rupta_store::StoreError::Invalid("could not create the issue".into()))
}

fn opt_str(v: Option<&Value>, max: usize) -> Option<String> {
    v.and_then(scalar_string).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).map(|s| truncate(&s, max))
}

fn transaction(env: &mut EnvelopeState<'_>, raw: &Value, id: uuid::Uuid) {
    let trace = raw.get("contexts").and_then(|c| c.get("trace"));
    let timestamp = raw.get("timestamp").and_then(parse_timestamp).unwrap_or(env.received);
    let start = raw.get("start_timestamp").and_then(parse_timestamp);
    let tags: Map<String, Value> = match raw.get("tags") {
        Some(Value::Object(o)) => {
            o.iter().filter_map(|(k, v)| Some((k.clone(), Value::String(scalar_string(v)?)))).collect()
        }
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|p| {
                let p = p.as_array()?;
                Some((scalar_string(p.first()?)?, Value::String(scalar_string(p.get(1)?)?)))
            })
            .collect(),
        _ => Map::new(),
    };
    let user = raw.get("user").and_then(Value::as_object).and_then(|u| {
        ["id", "email", "username", "ip_address"]
            .iter()
            .find_map(|k| opt_str(u.get(*k), 256).map(|v| format!("{}:{v}", if *k == "ip_address" { "ip" } else { k })))
    });
    let row = TransactionRow {
        project_id: env.project_id,
        event_id: *id.as_bytes(),
        trace_id: opt_str(trace.and_then(|t| t.get("trace_id")), 64),
        span_id: opt_str(trace.and_then(|t| t.get("span_id")), 32),
        parent_span_id: opt_str(trace.and_then(|t| t.get("parent_span_id")), 32),
        name: opt_str(raw.get("transaction"), 200),
        op: opt_str(trace.and_then(|t| t.get("op")), 64),
        status: opt_str(trace.and_then(|t| t.get("status")), 32),
        timestamp,
        start_timestamp: start,
        duration_ms: start.map(|s| (timestamp - s).num_microseconds().unwrap_or(0) as f64 / 1000.0),
        received: env.received,
        environment: opt_str(raw.get("environment"), 64),
        release: opt_str(raw.get("release"), 250),
        platform: opt_str(raw.get("platform"), 32),
        sdk: opt_str(raw.get("sdk").and_then(|s| s.get("name")), 128),
        user_hash: user.map(|u| xxh3_64(u.as_bytes()) as i64),
        span_count: raw.get("spans").and_then(Value::as_array).map(|s| s.len().min(i32::MAX as usize) as i32),
        tags: Value::Object(tags).to_string(),
        measurements: raw.get("measurements").filter(|m| m.is_object()).map_or_else(|| "{}".into(), |m| m.to_string()),
    };
    env.out.transactions.push(row);
    env.outcome(Category::Transaction, Outcome::Accepted, "", 1);
}

fn feedback(env: &mut EnvelopeState<'_>, raw: &Value, _id: uuid::Uuid) {
    let fb = raw.get("contexts").and_then(|c| c.get("feedback"));
    let Some(message) = opt_str(fb.and_then(|f| f.get("message")), 10_000) else {
        env.outcome(Category::Feedback, Outcome::Invalid, "missing_message", 1);
        return;
    };
    let user = raw.get("user");
    let row = FeedbackRow {
        project_id: env.project_id,
        event_id: parse_id(fb.and_then(|f| f.get("associated_event_id"))).map(|u| *u.as_bytes()),
        issue_id: None,
        name: opt_str(fb.and_then(|f| f.get("name")), 256).or_else(|| opt_str(user.and_then(|u| u.get("name")), 256)),
        email: opt_str(fb.and_then(|f| f.get("contact_email")), 256)
            .or_else(|| opt_str(user.and_then(|u| u.get("email")), 256)),
        message,
        url: opt_str(fb.and_then(|f| f.get("url")), 2048)
            .or_else(|| opt_str(raw.get("request").and_then(|r| r.get("url")), 2048)),
        created_at: raw.get("timestamp").and_then(parse_timestamp).unwrap_or(env.received),
    };
    env.out.feedback.push(PendingFeedback { row });
    env.outcome(Category::Feedback, Outcome::Accepted, "", 1);
}

fn user_report(env: &mut EnvelopeState<'_>, item: &Item) {
    let raw: Value = serde_json::from_slice(&item.payload).unwrap_or(Value::Null);
    let Some(message) = opt_str(raw.get("comments"), 10_000) else {
        env.outcome(Category::Feedback, Outcome::Invalid, "missing_comments", 1);
        return;
    };
    let row = FeedbackRow {
        project_id: env.project_id,
        event_id: parse_id(raw.get("event_id")).map(|u| *u.as_bytes()),
        issue_id: None,
        name: opt_str(raw.get("name"), 256),
        email: opt_str(raw.get("email"), 256),
        message,
        url: None,
        created_at: env.received,
    };
    env.out.feedback.push(PendingFeedback { row });
    env.outcome(Category::Feedback, Outcome::Accepted, "", 1);
}

fn attachment(env: &mut EnvelopeState<'_>, item: &Item) {
    let (event_id, event_dropped) = (env.event_id, env.event_dropped);
    if let Some(reason) = event_dropped {
        let outcome = if reason == "filtered" { Outcome::Filtered } else { Outcome::Invalid };
        env.outcome(
            Category::Attachment,
            outcome,
            if reason == "filtered" { "event_filtered" } else { "event_dropped" },
            1,
        );
        return;
    }
    let Some(event_id) = event_id else {
        env.outcome(Category::Attachment, Outcome::Invalid, "missing_event_id", 1);
        return;
    };
    if item.payload.len() > env.inner.config.max_attachment_bytes {
        env.outcome(Category::Attachment, Outcome::Invalid, "too_large", 1);
        return;
    }
    let day = env.received.date_naive();
    let key = format!(
        "attachments/{}/{}/{}/{}",
        env.project_id,
        day.format("%Y-%m-%d"),
        hex(event_id),
        uuid::Uuid::new_v4().simple()
    );
    if let Err(e) = env.inner.store.objects().put(&key, &item.payload) {
        tracing::error!(error = %e, "storing an attachment failed");
        env.outcome(Category::Attachment, Outcome::Invalid, "storage_error", 1);
        return;
    }
    let name = item.header_str("filename").map(|s| truncate(s, 256)).unwrap_or_else(|| "attachment".into());
    env.out.attachments.push(AttachmentRow {
        project_id: env.project_id,
        event_id: *event_id.as_bytes(),
        name,
        content_type: item.header_str("content_type").map(|s| truncate(s, 128)),
        size: item.payload.len() as i64,
        object_key: key,
        day,
    });
    env.outcome(Category::Attachment, Outcome::Accepted, "", 1);
}

fn attrs(raw: &Value) -> (Option<String>, String) {
    let a = raw.get("attrs");
    (
        opt_str(a.and_then(|a| a.get("release")), 250),
        opt_str(a.and_then(|a| a.get("environment")), 64).unwrap_or_default(),
    )
}

fn session(env: &mut EnvelopeState<'_>, item: &Item) {
    let raw: Value = serde_json::from_slice(&item.payload).unwrap_or(Value::Null);
    let (release, environment) = attrs(&raw);
    let (Some(release), Some(sid), Some(started)) =
        (release, opt_str(raw.get("sid"), 64), raw.get("started").and_then(parse_timestamp))
    else {
        env.outcome(Category::Session, Outcome::Invalid, "invalid_session", 1);
        return;
    };
    let status = raw.get("status").and_then(Value::as_str).unwrap_or("ok").to_ascii_lowercase();
    let status = match status.as_str() {
        "ok" | "exited" | "crashed" | "abnormal" | "errored" => status,
        _ => {
            env.outcome(Category::Session, Outcome::Invalid, "invalid_status", 1);
            return;
        }
    };
    env.out.sessions.push(SessionUpdate {
        project_id: env.project_id,
        org_id: env.org_id,
        sid,
        init: raw.get("init").and_then(Value::as_bool).unwrap_or(false),
        started,
        status,
        errors: raw.get("errors").and_then(Value::as_u64).unwrap_or(0),
        release,
        environment,
    });
    env.outcome(Category::Session, Outcome::Accepted, "", 1);
}

pub(crate) fn hour(t: DateTime<Utc>) -> DateTime<Utc> {
    t.with_minute(0).and_then(|t| t.with_second(0)).and_then(|t| t.with_nanosecond(0)).unwrap_or(t)
}

fn sessions(env: &mut EnvelopeState<'_>, item: &Item) {
    let raw: Value = serde_json::from_slice(&item.payload).unwrap_or(Value::Null);
    let (release, environment) = attrs(&raw);
    let (Some(release), Some(aggregates)) = (release, raw.get("aggregates").and_then(Value::as_array)) else {
        env.outcome(Category::Session, Outcome::Invalid, "invalid_sessions", 1);
        return;
    };
    let mut n = 0;
    for a in aggregates {
        let Some(started) = a.get("started").and_then(parse_timestamp) else { continue };
        let get = |k: &str| a.get(k).and_then(Value::as_i64).unwrap_or(0).max(0);
        let (exited, errored, abnormal, crashed) = (get("exited"), get("errored"), get("abnormal"), get("crashed"));
        let total = exited + errored + abnormal + crashed;
        if total == 0 {
            continue;
        }
        n += total as u64;
        env.out.session_counts.push((
            env.org_id,
            SessionCount {
                project_id: env.project_id,
                release: release.clone(),
                environment: environment.clone(),
                hour: hour(started),
                total,
                errored: errored + abnormal + crashed,
                crashed,
                abnormal,
            },
        ));
    }
    env.outcome(Category::Session, Outcome::Accepted, "", n);
}

fn client_report(env: &mut EnvelopeState<'_>, item: &Item) {
    let raw: Value = serde_json::from_slice(&item.payload).unwrap_or(Value::Null);
    let at = raw.get("timestamp").and_then(parse_timestamp).unwrap_or(env.received);
    let mut any = false;
    for (field, default_reason) in [
        ("discarded_events", ""),
        ("rate_limited_events", "rate_limited"),
        ("filtered_events", "filtered"),
        ("filtered_sampling_events", "sample_rate"),
    ] {
        for d in raw.get(field).and_then(Value::as_array).into_iter().flatten() {
            let quantity = d.get("quantity").and_then(Value::as_i64).unwrap_or(0);
            if quantity <= 0 {
                continue;
            }
            let category = Category::parse(d.get("category").and_then(Value::as_str).unwrap_or(""));
            let reason = d.get("reason").and_then(Value::as_str).unwrap_or(default_reason);
            let reason = truncate(reason, 64);
            env.inner.metrics.count(category, Outcome::ClientDiscard, quantity as u64);
            let key = crate::writer::outcome_key(env.project_id, at, category, "client_discard", &reason);
            env.out.outcomes.push((key, quantity));
            any = true;
        }
    }
    if !any && raw.is_null() {
        env.outcome(Category::Internal, Outcome::Invalid, "invalid_client_report", 1);
    }
}

fn check_in(env: &mut EnvelopeState<'_>, item: &Item) {
    let raw: Value = serde_json::from_slice(&item.payload).unwrap_or(Value::Null);
    let slug = opt_str(raw.get("monitor_slug"), 128).map(|s| slugify(&s)).filter(|s| !s.is_empty());
    let status = raw.get("status").and_then(Value::as_str).unwrap_or("");
    let (Some(slug), Some(id), true) =
        (slug, parse_id(raw.get("check_in_id")), matches!(status, "in_progress" | "ok" | "error"))
    else {
        env.outcome(Category::Monitor, Outcome::Invalid, "invalid_check_in", 1);
        return;
    };
    let config = raw.get("monitor_config").and_then(Value::as_object).and_then(|c| {
        let schedule = c.get("schedule").filter(|s| s.is_object())?.clone();
        let minutes = |k: &str| c.get(k).and_then(Value::as_i64).map(|v| v.clamp(0, i32::MAX as i64) as i32);
        Some(MonitorConfig {
            schedule,
            timezone: opt_str(c.get("timezone"), 64).unwrap_or_else(|| "UTC".into()),
            checkin_margin: minutes("checkin_margin"),
            max_runtime: minutes("max_runtime"),
        })
    });
    let next_checkin = config.as_ref().and_then(|c| crate::cron::next_run(&c.schedule, env.received));
    let duration_ms = raw
        .get("duration")
        .and_then(Value::as_f64)
        .filter(|d| d.is_finite() && *d >= 0.0)
        .map(|d| (d * 1000.0).round() as i64);
    env.out.checkins.push(CheckIn {
        project_id: env.project_id,
        monitor_slug: slug,
        check_in_id: *id.as_bytes(),
        status: status.to_string(),
        environment: opt_str(raw.get("environment"), 64),
        duration_ms,
        received: env.received,
        config,
        next_checkin,
    });
    env.outcome(Category::Monitor, Outcome::Accepted, "", 1);
}

/// Monitor slugs: lowercase letters, digits, `-` and `_`.
pub fn slugify(s: &str) -> String {
    let mut out = String::new();
    for c in s.trim().chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').chars().take(50).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert_eq!(slugify("Nightly Backup!"), "nightly-backup");
        assert_eq!(slugify("my_job-1"), "my_job-1");
    }
}
