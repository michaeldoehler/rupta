//! The batch writer: collects processed envelopes for up to
//! `batch_interval` or `batch_size` events and writes them in one squad
//! transaction, then acknowledges (durability `safe`) and emits issue
//! signals.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use chrono::{DateTime, Utc};
use rupta_core::signal::IssueSignal;
use rupta_store::ingest::*;
use rupta_store::squad::Connection;
use rupta_store::{Result, StoreError};
use serde_json::json;

use crate::Inner;
use crate::metrics::Category;
use crate::process::{Processed, SessionUpdate, hour};

pub(crate) fn outcome_key(
    project_id: i64,
    at: DateTime<Utc>,
    category: Category,
    outcome: &str,
    reason: &str,
) -> OutcomeKey {
    OutcomeKey {
        project_id,
        hour: hour(at),
        category: category.as_str().to_string(),
        outcome: outcome.to_string(),
        reason: reason.to_string(),
    }
}

/// Which parts of a session were counted already.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct SessionFlags {
    started: bool,
    errored: bool,
    crashed: bool,
    abnormal: bool,
}

/// Sessions seen recently, so that a session counts once when it starts
/// and each of its errored/crashed/abnormal transitions once, even when
/// updates arrive out of order or repeatedly. Two generations approximate
/// an LRU: when the current one is full it becomes the previous one.
#[derive(Default)]
pub(crate) struct SessionTracker {
    current: HashMap<(i64, String), SessionFlags>,
    previous: HashMap<(i64, String), SessionFlags>,
}

const SESSION_GENERATION: usize = 250_000;

impl SessionTracker {
    /// Applies an update; returns `(total, errored, crashed, abnormal)` to add.
    pub fn apply(&mut self, u: &SessionUpdate) -> (i64, i64, i64, i64) {
        let key = (u.project_id, u.sid.clone());
        let mut flags = self.current.remove(&key).or_else(|| self.previous.remove(&key)).unwrap_or_default();
        let mut d = (0, 0, 0, 0);
        if u.init && !flags.started {
            flags.started = true;
            d.0 = 1;
        }
        let errored = u.errors > 0 || matches!(u.status.as_str(), "errored" | "crashed" | "abnormal");
        if errored && !flags.errored {
            flags.errored = true;
            d.1 = 1;
        }
        if u.status == "crashed" && !flags.crashed {
            flags.crashed = true;
            d.2 = 1;
        }
        if u.status == "abnormal" && !flags.abnormal {
            flags.abnormal = true;
            d.3 = 1;
        }
        if self.current.len() >= SESSION_GENERATION {
            self.previous = std::mem::take(&mut self.current);
        }
        self.current.insert(key, flags);
        d
    }
}

/// Users of sessions already written for an hour, so that each user is
/// appended to `session_users` once per (project, release, environment,
/// hour) and once more when a session of theirs crashed. The set is
/// cleared when it grows large; later duplicates only cost storage, since
/// queries count distinct users.
#[derive(Default)]
pub(crate) struct SessionUserSeen {
    seen: HashSet<SessionUserRow>,
}

const SESSION_USERS_SEEN: usize = 500_000;

impl SessionUserSeen {
    /// True if the row is new (and remembers it).
    pub fn insert(&mut self, row: &SessionUserRow) -> bool {
        if self.seen.contains(row) {
            return false;
        }
        if self.seen.len() >= SESSION_USERS_SEEN {
            self.seen.clear();
        }
        self.seen.insert(row.clone());
        true
    }
}

/// Distinct users per issue: user hashes seen, kept up to a cap per issue.
#[derive(Default)]
struct UserTracker {
    issues: HashMap<i64, HashSet<i64>>,
}

const USERS_PER_ISSUE: usize = 10_000;
const TRACKED_ISSUES: usize = 50_000;

/// Unacked parts of a batch.
#[derive(Default)]
struct Batch {
    parts: Vec<Processed>,
    events: usize,
    /// Session counts derived from the session updates (computed once, so
    /// a retried transaction does not count twice).
    session_deltas: Vec<(i64, SessionCount)>,
    /// Users of sessions not yet stored for their hour (deduplicated once).
    session_users: Vec<SessionUserRow>,
    /// `(project_id, event_id)`, `events` row and `event_payloads` row of
    /// every event, collected once so that a retried transaction reuses them.
    keys: Vec<(i64, [u8; 16])>,
    event_values: Vec<Vec<rupta_store::squad::Value>>,
    payload_values: Vec<Vec<rupta_store::squad::Value>>,
    /// `spans`, `logs` and `metrics` rows of all parts, collected once.
    span_values: Vec<Vec<rupta_store::squad::Value>>,
    log_values: Vec<Vec<rupta_store::squad::Value>>,
    metric_values: Vec<Vec<rupta_store::squad::Value>>,
}

/// How much of a batch an envelope fills: its events, plus one for every
/// 50 span, log or metric rows.
fn weight(p: &Processed) -> usize {
    (p.events.len() + (p.spans.len() + p.logs.len() + p.metrics.len()) / 50).max(1)
}

/// State shared by the writer threads.
#[derive(Default)]
pub(crate) struct Shared {
    sessions: Mutex<SessionTracker>,
    users: Mutex<UserTracker>,
    /// Session users stored per hour already (see [`SessionUserSeen`]).
    session_users: Mutex<SessionUserSeen>,
    /// Feedback stored before its event: linked when the event arrives.
    unlinked_feedback: Mutex<HashSet<(i64, [u8; 16])>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

struct Writer {
    inner: Arc<Inner>,
    conn: Connection,
}

/// One writer thread. Several run side by side, each committing its own
/// batches; updates of shared rows happen in id order so that concurrent
/// batches cannot deadlock.
pub(crate) fn run(inner: Arc<Inner>, rx: crossbeam_channel::Receiver<Processed>) {
    let conn = inner.store.connect();
    let mut w = Writer { inner: inner.clone(), conn };
    let size = inner.config.batch_size.max(1);
    let interval = inner.config.batch_interval;
    // When everything in flight is collected, wait this long for clients
    // that are about to send (durability safe acknowledges per batch).
    let linger = interval.min(std::time::Duration::from_millis(2));
    let mut disconnected = false;
    while !disconnected {
        let mut batch = Batch::default();
        // Wait for the first part (or, periodically, for pending outcomes).
        match rx.recv_timeout(interval.max(std::time::Duration::from_millis(1)) * 20) {
            Ok(p) => {
                batch.events += weight(&p);
                batch.parts.push(p);
                inner.collected.fetch_add(1, Ordering::AcqRel);
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => disconnected = true,
        }
        let started = Instant::now();
        let deadline = started + interval;
        while !disconnected && batch.events < size {
            // Everything accepted so far is in this batch: waiting the full
            // interval would only delay acknowledgements (durability safe).
            let all_in = !batch.parts.is_empty()
                && inner.collected.load(Ordering::Acquire) >= inner.in_flight.load(Ordering::Acquire);
            let until = if all_in { deadline.min(started + linger) } else { deadline };
            match rx.recv_deadline(until) {
                Ok(p) => {
                    batch.events += weight(&p);
                    batch.parts.push(p);
                    inner.collected.fetch_add(1, Ordering::AcqRel);
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    if until == deadline || Instant::now() >= deadline {
                        break;
                    }
                    if inner.collected.load(Ordering::Acquire) >= inner.in_flight.load(Ordering::Acquire) {
                        break;
                    }
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => disconnected = true,
            }
        }
        let pending = std::mem::take(&mut *inner.pending_outcomes.lock().unwrap_or_else(|e| e.into_inner()));
        if batch.parts.is_empty() && pending.is_empty() {
            continue;
        }
        let n = batch.parts.len();
        w.write(batch, pending);
        inner.collected.fetch_sub(n, Ordering::AcqRel);
    }
}

/// `(org_id, project_id, version)`.
type ReleaseKey = (i64, i64, String);
/// First and last event, new issues.
type ReleaseSeen = (DateTime<Utc>, DateTime<Utc>, i64);

/// Aggregates of one issue within a batch.
#[derive(Default)]
struct IssueAgg {
    project_id: i64,
    org_id: i64,
    count: i64,
    first: Option<DateTime<Utc>>,
    last: Option<DateTime<Utc>>,
    last_release: Option<String>,
    releases: HashSet<Option<String>>,
    environment: Option<String>,
    level: Option<String>,
    new: bool,
}

impl Writer {
    fn write(&mut self, mut batch: Batch, pending: HashMap<OutcomeKey, i64>) {
        let start = Instant::now();
        // Duplicate event ids within the batch: keep the first.
        let mut seen = HashSet::new();
        for p in &mut batch.parts {
            p.events.retain(|e| seen.insert((e.row.project_id, e.row.event_id)));
        }
        for p in &mut batch.parts {
            for e in &mut p.events {
                batch.keys.push((e.row.project_id, e.row.event_id));
                batch.event_values.push(std::mem::take(&mut e.values));
                batch.payload_values.push(std::mem::take(&mut e.payload));
            }
            batch.span_values.append(&mut p.spans);
            batch.log_values.append(&mut p.logs);
            batch.metric_values.append(&mut p.metrics);
        }
        for p in &batch.parts {
            for u in &p.sessions {
                let (total, errored, crashed, abnormal) = lock(&self.inner.writer_state.sessions).apply(u);
                batch.session_deltas.push((
                    u.org_id,
                    SessionCount {
                        project_id: u.project_id,
                        release: u.release.clone(),
                        environment: u.environment.clone(),
                        hour: hour(u.started),
                        total,
                        errored,
                        crashed,
                        abnormal,
                    },
                ));
            }
        }
        {
            let mut seen = lock(&self.inner.writer_state.session_users);
            for p in &batch.parts {
                let from_updates = p.sessions.iter().filter_map(|u| {
                    Some(SessionUserRow {
                        project_id: u.project_id,
                        release: u.release.clone(),
                        environment: u.environment.clone(),
                        hour: hour(u.started),
                        user_hash: u.user_hash?,
                        crashed: u.status == "crashed",
                    })
                });
                for row in from_updates.chain(p.session_users.iter().cloned()) {
                    if seen.insert(&row) {
                        batch.session_users.push(row);
                    }
                }
            }
        }
        let mut result = Err(StoreError::Invalid("not attempted".into()));
        for attempt in 0..3 {
            let mut signals = Vec::new();
            result = self.try_write(&batch, &pending, &mut signals).map(|_| signals);
            match &result {
                Err(e) if e.is_unique_violation() && attempt < 2 => {
                    // Events sent again (SDK retries): drop those stored before.
                    if let Err(e) = self.drop_stored_duplicates(&mut batch) {
                        result = Err(e);
                        break;
                    }
                }
                Err(e) if matches!(e.sqlstate(), Some("40001" | "40P01")) && attempt < 2 => {}
                _ => break,
            }
        }
        let ok = result.is_ok();
        self.inner.metrics.batch(start.elapsed().as_secs_f64(), ok);
        match result {
            Ok(signals) => {
                self.inner.metrics.projects.observe(&signals);
                for s in signals {
                    let _ = self.inner.signals.send(s);
                }
            }
            Err(e) => {
                tracing::error!(error = %e, events = batch.events, "writing an ingest batch failed; batch dropped");
                // Users counted for this batch were not stored: recount from the database.
                *lock(&self.inner.writer_state.users) = UserTracker::default();
                *lock(&self.inner.writer_state.session_users) = SessionUserSeen::default();
            }
        }
        for p in batch.parts {
            if let Some(ack) = p.ack {
                let _ = ack.send(ok);
            }
            if p.counted {
                self.inner.in_flight.fetch_sub(1, Ordering::AcqRel);
            }
        }
    }

    fn drop_stored_duplicates(&mut self, batch: &mut Batch) -> Result<()> {
        let mut by_project: HashMap<i64, Vec<[u8; 16]>> = HashMap::new();
        for p in &batch.parts {
            for e in &p.events {
                by_project.entry(e.row.project_id).or_default().push(e.row.event_id);
            }
        }
        let mut existing = HashSet::new();
        for (project, ids) in by_project {
            for id in existing_event_ids(&self.conn, project, &ids)? {
                existing.insert((project, id));
            }
        }
        for p in &mut batch.parts {
            p.events.retain(|e| !existing.contains(&(e.row.project_id, e.row.event_id)));
        }
        let keep: Vec<bool> = batch.keys.iter().map(|k| !existing.contains(k)).collect();
        let mut i = 0;
        batch.event_values.retain(|_| (keep[i], i += 1).0);
        let mut i = 0;
        batch.payload_values.retain(|_| (keep[i], i += 1).0);
        let mut i = 0;
        batch.keys.retain(|_| (keep[i], i += 1).0);
        Ok(())
    }

    fn users_added(
        users: &mut UserTracker,
        conn: &Connection,
        project_id: i64,
        issue_id: i64,
        new_issue: bool,
        hashes: &[i64],
    ) -> Result<i64> {
        if hashes.is_empty() {
            return Ok(0);
        }
        if !users.issues.contains_key(&issue_id) {
            if users.issues.len() >= TRACKED_ISSUES {
                users.issues.clear();
            }
            let known = if new_issue {
                HashSet::new()
            } else {
                issue_user_hashes(conn, project_id, issue_id, USERS_PER_ISSUE as i64)?.into_iter().collect()
            };
            users.issues.insert(issue_id, known);
        }
        let set = users.issues.get_mut(&issue_id).expect("inserted above");
        let mut added = 0;
        for h in hashes {
            if set.contains(h) {
                continue;
            }
            // Beyond the cap new users are no longer counted (see docs).
            if set.len() < USERS_PER_ISSUE {
                set.insert(*h);
                added += 1;
            }
        }
        Ok(added)
    }

    fn try_write(
        &mut self,
        batch: &Batch,
        pending: &HashMap<OutcomeKey, i64>,
        signals: &mut Vec<IssueSignal>,
    ) -> Result<()> {
        let inner = self.inner.clone();
        let conn = self.conn.transaction()?;
        let mut transactions = Vec::new();
        let mut issues: HashMap<i64, IssueAgg> = HashMap::new();
        let mut user_hashes: HashMap<i64, Vec<i64>> = HashMap::new();
        let mut releases: HashMap<ReleaseKey, ReleaseSeen> = HashMap::new();
        let mut batch_event_issue: HashMap<(i64, [u8; 16]), i64> = HashMap::new();
        for p in &batch.parts {
            for e in &p.events {
                batch_event_issue.insert((e.row.project_id, e.row.event_id), e.row.issue_id);
                let a = issues.entry(e.row.issue_id).or_default();
                a.project_id = e.row.project_id;
                a.org_id = e.org_id;
                a.count += 1;
                a.new |= e.new_issue;
                a.first = Some(a.first.map_or(e.row.timestamp, |f| f.min(e.row.timestamp)));
                if a.last.is_none_or(|l| e.row.timestamp >= l) {
                    a.last = Some(e.row.timestamp);
                    a.last_release = e.row.release.clone();
                    a.environment = e.row.environment.clone();
                    a.level = e.row.level.clone();
                }
                a.releases.insert(e.row.release.clone());
                if let Some(h) = e.row.user_hash {
                    user_hashes.entry(e.row.issue_id).or_default().push(h);
                }
                if let Some(r) = &e.row.release {
                    let entry = releases.entry((e.org_id, e.row.project_id, r.clone())).or_insert((
                        e.row.timestamp,
                        e.row.timestamp,
                        0,
                    ));
                    entry.0 = entry.0.min(e.row.timestamp);
                    entry.1 = entry.1.max(e.row.timestamp);
                    if e.new_issue {
                        entry.2 += 1;
                    }
                }
            }
            transactions.extend(p.transactions.iter().cloned());
        }
        append_values(&conn, "event_payloads", &batch.payload_values)?;
        append_values(&conn, "events", &batch.event_values)?;
        append_transactions(&conn, &transactions)?;
        append_values(&conn, "spans", &batch.span_values)?;
        append_values(&conn, "logs", &batch.log_values)?;
        append_values(&conn, "metrics", &batch.metric_values)?;

        // Issues: counters, regressions, ignore conditions.
        let ids: Vec<i64> = issues.keys().copied().collect();
        let mut states = issue_states(&conn, &ids)?;
        states.sort_by_key(|s| s.id);
        let found: HashSet<i64> = states.iter().map(|s| s.id).collect();
        let missing: Vec<i64> = ids.iter().copied().filter(|id| !found.contains(id)).collect();
        if !missing.is_empty() {
            inner.issues.remove_ids(&missing);
        }
        let now = Utc::now();
        for st in &states {
            let a = &issues[&st.id];
            let hashes = user_hashes.get(&st.id).map(Vec::as_slice).unwrap_or(&[]);
            let users_added = {
                let mut users = lock(&inner.writer_state.users);
                Self::users_added(&mut users, &conn, st.project_id, st.id, a.new, hashes)?
            };
            add_issue_counts(
                &conn,
                &IssueCounts {
                    id: st.id,
                    count: a.count,
                    users_added,
                    first_seen: a.first,
                    last_seen: a.last,
                    last_release: a.last_release.clone(),
                },
            )?;
            if a.new {
                signals.push(IssueSignal::New { project_id: st.project_id, issue_id: st.id });
            }
            match st.status.as_str() {
                "resolved" => {
                    if self.regresses(&conn, a, st)? {
                        reopen_issue(&conn, st.id, "regressed")?;
                        add_issue_activity(&conn, st.id, "set_regression", &json!({"release": a.last_release}))?;
                        signals.push(IssueSignal::Regressed { project_id: st.project_id, issue_id: st.id });
                    }
                }
                "ignored" => {
                    let until_passed = st.ignore_until.is_some_and(|t| now >= t);
                    // `ignore_count` is the times_seen value at which the ignore ends
                    // (times_seen when ignored plus the number of events to wait for).
                    let count_reached = st.ignore_count.is_some_and(|c| st.times_seen + a.count >= c);
                    // `ignore_user_count` without a window is the users_seen value at
                    // which the ignore ends; with a window, distinct users within it.
                    let users_reached = match (st.ignore_user_count, st.ignore_user_window) {
                        (Some(n), None) => st.users_seen + users_added >= n,
                        (Some(n), Some(w)) => {
                            let since = now - chrono::Duration::minutes(i64::from(w));
                            issue_users_since(&conn, st.project_id, st.id, since)? >= n
                        }
                        _ => false,
                    };
                    if until_passed || count_reached || users_reached {
                        reopen_issue(&conn, st.id, "ongoing")?;
                        let reason = if until_passed {
                            "ignore_until_passed"
                        } else if count_reached {
                            "ignore_count_reached"
                        } else {
                            "ignore_user_count_reached"
                        };
                        add_issue_activity(&conn, st.id, "set_unresolved", &json!({"reason": reason}))?;
                        signals.push(IssueSignal::Reappeared { project_id: st.project_id, issue_id: st.id });
                    }
                }
                _ => {}
            }
            signals.push(IssueSignal::Seen {
                project_id: st.project_id,
                issue_id: st.id,
                count: a.count as u64,
                environment: a.environment.clone(),
                level: a.level.clone(),
            });
        }

        // Release health.
        let mut counts: HashMap<(i64, String, String, DateTime<Utc>), SessionCount> = HashMap::new();
        let mut add_count = |c: SessionCount| {
            let e = counts
                .entry((c.project_id, c.release.clone(), c.environment.clone(), c.hour))
                .or_insert_with(|| SessionCount { total: 0, errored: 0, crashed: 0, abnormal: 0, ..c.clone() });
            e.total += c.total;
            e.errored += c.errored;
            e.crashed += c.crashed;
            e.abnormal += c.abnormal;
        };
        for (org_id, c) in &batch.session_deltas {
            let entry = releases.entry((*org_id, c.project_id, c.release.clone())).or_insert((c.hour, c.hour, 0));
            entry.0 = entry.0.min(c.hour);
            entry.1 = entry.1.max(c.hour);
            if c.total + c.errored + c.crashed + c.abnormal > 0 {
                add_count(c.clone());
            }
        }
        for p in &batch.parts {
            for (org_id, c) in &p.session_counts {
                let entry = releases.entry((*org_id, c.project_id, c.release.clone())).or_insert((c.hour, c.hour, 0));
                entry.0 = entry.0.min(c.hour);
                entry.1 = entry.1.max(c.hour);
                add_count(c.clone());
            }
        }
        let mut counts: Vec<SessionCount> = counts.into_values().collect();
        counts.sort_by(|a, b| {
            (a.project_id, &a.release, &a.environment, a.hour).cmp(&(b.project_id, &b.release, &b.environment, b.hour))
        });
        add_session_counts(&conn, &counts)?;
        append_session_users(&conn, &batch.session_users)?;

        let mut releases: Vec<_> = releases.into_iter().collect();
        releases.sort_by(|a, b| a.0.cmp(&b.0));
        for ((org_id, project_id, version), (first, last, new_issues)) in &releases {
            upsert_release(&conn, *org_id, *project_id, version, *first, *last, *new_issues)?;
        }

        // Outcomes.
        let mut outcomes: HashMap<OutcomeKey, i64> = pending.clone();
        for p in &batch.parts {
            for (k, n) in &p.outcomes {
                *outcomes.entry(k.clone()).or_default() += n;
            }
        }
        let mut outcomes: Vec<(OutcomeKey, i64)> = outcomes.into_iter().collect();
        outcomes.sort_by(|a, b| {
            (a.0.project_id, a.0.hour, &a.0.category, &a.0.outcome, &a.0.reason).cmp(&(
                b.0.project_id,
                b.0.hour,
                &b.0.category,
                &b.0.outcome,
                &b.0.reason,
            ))
        });
        add_outcomes(&conn, &outcomes)?;

        // Feedback, linked to the issue of its event when known.
        let mut unlinked = Vec::new();
        for p in &batch.parts {
            for f in &p.feedback {
                let mut row = f.row.clone();
                if let Some(eid) = row.event_id {
                    row.issue_id = match batch_event_issue.get(&(row.project_id, eid)) {
                        Some(id) => Some(*id),
                        None => event_issue_id(&conn, row.project_id, eid)?,
                    };
                    if row.issue_id.is_none() {
                        unlinked.push((row.project_id, eid));
                    }
                }
                insert_feedback(&conn, &row)?;
            }
        }
        let mut linked = Vec::new();
        let waiting: Vec<((i64, [u8; 16]), i64)> = {
            let unlinked = lock(&inner.writer_state.unlinked_feedback);
            if unlinked.is_empty() {
                Vec::new()
            } else {
                batch_event_issue.iter().filter(|(k, _)| unlinked.contains(*k)).map(|(k, v)| (*k, *v)).collect()
            }
        };
        for (key, issue_id) in waiting {
            link_feedback(&conn, key.0, key.1, issue_id)?;
            linked.push(key);
        }

        for p in &batch.parts {
            for a in &p.attachments {
                insert_attachment(&conn, a)?;
            }
            for c in &p.checkins {
                record_checkin(&conn, c)?;
            }
            for pr in &p.profiles {
                rupta_store::telemetry::insert_profile(&conn, pr)?;
            }
        }
        // Replays in key order (concurrent writers lock rows in the same
        // order); within one replay in arrival order.
        let mut replays: Vec<&rupta_store::telemetry::ReplayUpdate> =
            batch.parts.iter().flat_map(|p| p.replays.iter()).collect();
        replays.sort_by_key(|u| (u.project_id, u.replay_id));
        for u in replays {
            rupta_store::telemetry::apply_replay_update(&conn, u)?;
        }
        let mut links: Vec<&rupta_store::telemetry::IssueReplay> = batch
            .parts
            .iter()
            .flat_map(|p| p.issue_replays.iter())
            .filter(|l| issues.contains_key(&l.issue_id))
            .collect();
        links.sort_by_key(|l| (l.project_id, l.replay_id));
        for l in links {
            // Events dropped as duplicates were linked when first stored.
            if batch.keys.contains(&(l.project_id, l.event_id)) {
                rupta_store::telemetry::link_issue_replay(&conn, l)?;
            }
        }
        conn.commit()?;
        let mut waiting = lock(&inner.writer_state.unlinked_feedback);
        for k in linked {
            waiting.remove(&k);
        }
        if waiting.len() > 100_000 {
            waiting.clear();
        }
        waiting.extend(unlinked);
        Ok(())
    }

    /// Whether events for a resolved issue regress it:
    ///
    /// - resolved without a release: any event;
    /// - resolved in a release `V` (`resolved_in_mode` `release`): events
    ///   without a release, of `V` itself, and of releases first seen after
    ///   `V`; while `V` has not been seen, only events of `V`;
    /// - resolved in the next release (`next_release`, `V` the latest
    ///   release at resolution time): only events of a release other than
    ///   `V` that was first seen after the resolution. Events of `V`, of
    ///   older releases and without a release keep it resolved.
    ///
    /// "First seen" is `releases.created_at`; a release not stored yet
    /// (first seen in this batch) counts as first seen now.
    fn regresses(&self, conn: &Connection, a: &IssueAgg, st: &IssueState) -> Result<bool> {
        if st.resolved_in_mode.as_deref() == Some("next_release") {
            for release in a.releases.iter().flatten() {
                if Some(release) == st.resolved_in_release.as_ref() {
                    continue;
                }
                let created = release_created_at(conn, a.org_id, release)?;
                let after = match (created, st.resolved_at) {
                    (Some(c), Some(r)) => c > r,
                    _ => true,
                };
                if after {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        let Some(resolved_in) = &st.resolved_in_release else { return Ok(true) };
        let fixed = release_created_at(conn, a.org_id, resolved_in)?;
        for release in &a.releases {
            let Some(release) = release else { return Ok(true) };
            if release == resolved_in {
                return Ok(true);
            }
            if let Some(fixed) = fixed {
                let created = release_created_at(conn, a.org_id, release)?;
                if created.is_none_or(|c| c > fixed) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn upd(sid: &str, init: bool, status: &str, errors: u64) -> SessionUpdate {
        SessionUpdate {
            project_id: 1,
            org_id: 1,
            sid: sid.into(),
            init,
            started: Utc.with_ymd_and_hms(2026, 11, 14, 10, 5, 0).unwrap(),
            status: status.into(),
            errors,
            release: "1.0".into(),
            environment: "production".into(),
            user_hash: None,
        }
    }

    #[test]
    fn sessions_count_once_per_transition() {
        let mut t = SessionTracker::default();
        assert_eq!(t.apply(&upd("a", true, "ok", 0)), (1, 0, 0, 0));
        assert_eq!(t.apply(&upd("a", true, "ok", 0)), (0, 0, 0, 0)); // repeated init
        assert_eq!(t.apply(&upd("a", false, "ok", 1)), (0, 1, 0, 0));
        assert_eq!(t.apply(&upd("a", false, "ok", 2)), (0, 0, 0, 0));
        assert_eq!(t.apply(&upd("a", false, "crashed", 2)), (0, 0, 1, 0));
        // Repeated final updates count nothing.
        assert_eq!(t.apply(&upd("a", false, "crashed", 2)), (0, 0, 0, 0));
        // Init and end in one update (the SDK only sends the final state).
        assert_eq!(t.apply(&upd("b", true, "exited", 0)), (1, 0, 0, 0));
        assert_eq!(t.apply(&upd("c", true, "abnormal", 0)), (1, 1, 0, 1));
        // An update for a session not seen (e.g. after a restart), and its
        // init arriving late.
        assert_eq!(t.apply(&upd("d", false, "errored", 1)), (0, 1, 0, 0));
        assert_eq!(t.apply(&upd("d", true, "ok", 1)), (1, 0, 0, 0));
    }
}
