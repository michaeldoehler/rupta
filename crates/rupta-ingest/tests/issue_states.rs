//! Issue state changes driven by ingestion: regressions of issues resolved
//! in a release or in the next release, ignoring until more users,
//! distinct session users, and escalation detection.

mod common;

use chrono::{Duration, Utc};
use common::*;
use rupta_core::signal::IssueSignal;
use rupta_store::ingest::{EventRow, append_events};
use serde_json::{Value, json};

fn event(release: Option<&str>, user: &str) -> Value {
    let mut e = json!({
        "event_id": new_id(),
        "platform": "javascript",
        "level": "error",
        "user": {"id": user},
        "exception": {"values": [{"type": "TypeError", "value": "boom",
            "stacktrace": {"frames": [{"filename": "app.js", "function": "pay", "lineno": 1, "in_app": true}]}}]},
    });
    if let Some(r) = release {
        e["release"] = json!(r);
    }
    e
}

async fn send(h: &Harness, release: Option<&str>, user: &str) {
    assert_eq!(h.envelope(event_envelope(&event(release, user))).await.0, 200);
    h.flush().await;
}

fn status(h: &Harness) -> (String, String) {
    let r = h.store.connect().query_one("SELECT status, substatus FROM issues", &[]).unwrap();
    (r.get(0), r.get(1))
}

fn exec(h: &Harness, sql: &str) {
    h.store.connect().batch_execute(sql).unwrap();
}

fn resolved() -> (String, String) {
    ("resolved".into(), "in_next_release".into())
}

#[tokio::test]
async fn resolved_in_next_release() {
    let h = harness();
    send(&h, Some("web@1.0"), "u1").await;
    exec(
        &h,
        "UPDATE releases SET created_at = now() - interval '1 day';
         INSERT INTO releases (org_id, version, created_at) VALUES (1, 'web@0.9', now() - interval '2 days');
         UPDATE issues SET status = 'resolved', substatus = 'in_next_release', resolved_in_mode = 'next_release',
                           resolved_in_release = 'web@1.0', resolved_at = now();",
    );
    // The recorded release, older releases and events without a release keep it resolved.
    for release in [Some("web@1.0"), Some("web@0.9"), None] {
        send(&h, release, "u1").await;
        assert_eq!(status(&h), resolved(), "{release:?}");
    }
    // A release first seen after the resolution regresses it.
    send(&h, Some("web@1.1"), "u1").await;
    assert_eq!(status(&h), ("unresolved".into(), "regressed".into()));
    let r = h.store.connect().query_one("SELECT resolved_in_mode, resolved_in_release FROM issues", &[]).unwrap();
    assert_eq!((r.get::<Option<String>>(0), r.get::<Option<String>>(1)), (None, None));

    // Without any release at resolution time: the first release seen afterwards regresses it.
    exec(
        &h,
        "UPDATE issues SET status = 'resolved', resolved_in_mode = 'next_release', resolved_in_release = NULL, resolved_at = now()",
    );
    send(&h, Some("web@1.1"), "u1").await; // first seen before the resolution
    assert_eq!(status(&h).0, "resolved");
    send(&h, Some("web@1.2"), "u1").await;
    assert_eq!(status(&h).0, "unresolved");
}

#[tokio::test]
async fn resolved_in_a_specific_release() {
    let h = harness();
    send(&h, Some("web@1.0"), "u1").await;
    exec(
        &h,
        "UPDATE releases SET created_at = now() - interval '1 day';
         UPDATE issues SET status = 'resolved', resolved_in_mode = 'release', resolved_in_release = 'web@2.0', resolved_at = now();",
    );
    // While web@2.0 has not been seen, only its own events regress the issue.
    send(&h, Some("web@1.0"), "u1").await;
    send(&h, Some("web@1.5"), "u1").await;
    assert_eq!(status(&h).0, "resolved");
    exec(&h, "INSERT INTO releases (org_id, version, created_at) VALUES (1, 'web@2.0', now() + interval '1 second')");
    // Older than the fixing release: still resolved.
    send(&h, Some("web@1.5"), "u1").await;
    assert_eq!(status(&h).0, "resolved");
    // The fixing release itself regresses it ...
    send(&h, Some("web@2.0"), "u1").await;
    assert_eq!(status(&h), ("unresolved".into(), "regressed".into()));
    // ... and so does a newer release.
    exec(
        &h,
        "UPDATE issues SET status = 'resolved', resolved_in_mode = 'release', resolved_in_release = 'web@2.0', resolved_at = now()",
    );
    send(&h, Some("web@1.0"), "u1").await;
    assert_eq!(status(&h).0, "resolved");
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    send(&h, Some("web@3.0"), "u1").await;
    assert_eq!(status(&h).0, "unresolved");
    // Events without a release regress issues resolved in a release.
    exec(
        &h,
        "UPDATE issues SET status = 'resolved', resolved_in_mode = 'release', resolved_in_release = 'web@2.0', resolved_at = now()",
    );
    send(&h, None, "u1").await;
    assert_eq!(status(&h).0, "unresolved");
}

#[tokio::test]
async fn ignored_until_more_users() {
    let h = harness();
    let mut signals = h.ingest.subscribe();
    send(&h, Some("web@1.0"), "u1").await;
    // Two more users: reappears at users_seen 3.
    exec(
        &h,
        "UPDATE issues SET status = 'ignored', substatus = 'archived_until_condition_met', ignore_user_count = users_seen + 2",
    );
    send(&h, Some("web@1.0"), "u1").await;
    send(&h, Some("web@1.0"), "u2").await;
    assert_eq!(status(&h).0, "ignored");
    send(&h, Some("web@1.0"), "u3").await;
    assert_eq!(status(&h), ("unresolved".into(), "ongoing".into()));
    assert_eq!(
        h.count("SELECT count(*) FROM issue_activity WHERE kind = 'set_unresolved' AND data ->> 'reason' = 'ignore_user_count_reached'"),
        1
    );
    let mut reappeared = false;
    while let Ok(s) = signals.try_recv() {
        reappeared |= matches!(s, IssueSignal::Reappeared { .. });
    }
    assert!(reappeared);

    // Within a window: distinct users of the last 60 minutes (u1..u3 so far).
    exec(&h, "UPDATE issues SET status = 'ignored', ignore_user_count = 5, ignore_user_window = 60");
    send(&h, Some("web@1.0"), "u4").await;
    assert_eq!(status(&h).0, "ignored");
    send(&h, Some("web@1.0"), "u5").await;
    assert_eq!(status(&h).0, "unresolved");
    let r = h.store.connect().query_one("SELECT ignore_user_count, ignore_user_window FROM issues", &[]).unwrap();
    assert_eq!((r.get::<Option<i64>>(0), r.get::<Option<i32>>(1)), (None, None));
}

#[tokio::test]
async fn sessions_record_distinct_users() {
    let h = harness();
    let session = |sid: &str, did: Option<&str>, init: bool, status: &str| {
        let mut s = json!({"sid": sid, "init": init, "started": "2026-11-14T10:20:00Z", "status": status,
                           "attrs": {"release": "1.0", "environment": "prod"}});
        if let Some(d) = did {
            s["did"] = json!(d);
        }
        envelope(json!({}), &[(json!({"type": "session"}), serde_json::to_vec(&s).unwrap())])
    };
    for body in [
        session("s1", Some("user-1"), true, "ok"),
        session("s1", Some("user-1"), false, "ok"),
        session("s1", Some("user-1"), false, "crashed"),
        session("s2", Some("user-2"), true, "exited"),
        session("s3", Some("user-2"), true, "exited"),
        session("s4", None, true, "crashed"),
    ] {
        assert_eq!(h.envelope(body).await.0, 200);
        h.flush().await;
    }
    let aggregates = json!({"attrs": {"release": "1.0", "environment": "prod"}, "aggregates": [
        {"started": "2026-11-14T10:00:00Z", "did": "user-3", "exited": 2},
        {"started": "2026-11-14T10:00:00Z", "did": "user-4", "crashed": 1},
        {"started": "2026-11-14T10:00:00Z", "exited": 5}]});
    let body = envelope(json!({}), &[(json!({"type": "sessions"}), serde_json::to_vec(&aggregates).unwrap())]);
    assert_eq!(h.envelope(body).await.0, 200);
    h.flush().await;
    assert_eq!(h.count("SELECT count(DISTINCT user_hash) FROM session_users"), 4);
    assert_eq!(h.count("SELECT count(DISTINCT user_hash) FROM session_users WHERE crashed"), 2);
    // Once per user and hour, and once more after a crash.
    assert_eq!(h.count("SELECT count(*) FROM session_users"), 5);
    assert_eq!(h.count("SELECT count(*) FROM session_users WHERE hour = '2026-11-14T10:00:00Z' AND release = '1.0' AND environment = 'prod'"), 5);
}

fn rows(project: i64, issue: i64, n: usize, at: chrono::DateTime<Utc>) -> Vec<EventRow> {
    (0..n)
        .map(|i| EventRow {
            project_id: project,
            issue_id: issue,
            event_id: *uuid::Uuid::new_v4().as_bytes(),
            timestamp: at - Duration::seconds(i as i64),
            received: at,
            tags: json!({}),
            contexts: json!({}),
            ..Default::default()
        })
        .collect()
}

#[tokio::test]
async fn escalation_detection() {
    let h = harness();
    let mut signals = h.ingest.subscribe();
    send(&h, None, "u1").await;
    let mut other = event(None, "u1");
    other["exception"]["values"][0]["type"] = json!("RangeError");
    h.envelope(event_envelope(&other)).await;
    h.flush().await;
    let conn = h.store.connect();
    let ids: Vec<i64> =
        conn.query("SELECT id FROM issues ORDER BY id", &[]).unwrap().iter().map(|r| r.get(0)).collect();
    let (spiky, busy) = (ids[0], ids[1]);
    let now = Utc::now();
    let p = h.project_id;
    // spiky: 2 events an hour before, 60 in the last hour. busy: always ~10 an hour.
    append_events(&conn, &rows(p, spiky, 2, now - Duration::hours(30))).unwrap();
    append_events(&conn, &rows(p, spiky, 60, now - Duration::minutes(5))).unwrap();
    append_events(&conn, &rows(p, busy, 1680, now - Duration::hours(50))).unwrap();
    append_events(&conn, &rows(p, busy, 60, now - Duration::minutes(5))).unwrap();
    // Both are new: they stay new (and are not checked) until a week old.
    let r = h.ingest.check_escalation(now).unwrap();
    assert!(r.escalated.is_empty());
    conn.execute("UPDATE issues SET first_seen = $1", &[&(now - Duration::days(8))]).unwrap();
    let r = h.ingest.check_escalation(now).unwrap();
    assert_eq!(r.aged, 2);
    assert_eq!(r.escalated, vec![spiky]);
    let row = conn
        .query_one(
            "SELECT status, substatus, escalated_at IS NOT NULL, escalation_baseline FROM issues WHERE id = $1",
            &[&spiky],
        )
        .unwrap();
    assert_eq!(
        (row.get::<String>(0), row.get::<String>(1), row.get::<bool>(2)),
        ("unresolved".into(), "escalating".into(), true)
    );
    assert!((row.get::<f64>(3) - 2.0 / 168.0).abs() < 1e-9, "{}", row.get::<f64>(3));
    assert_eq!(h.count("SELECT count(*) FROM issue_activity WHERE kind = 'set_escalating'"), 1);
    let mut escalating = Vec::new();
    while let Ok(s) = signals.try_recv() {
        if let IssueSignal::Escalating { issue_id, .. } = s {
            escalating.push(issue_id);
        }
    }
    assert_eq!(escalating, vec![spiky]);
    // Not escalated twice.
    assert!(h.ingest.check_escalation(now).unwrap().escalated.is_empty());
    let busy_sub: String = conn.query_one("SELECT substatus FROM issues WHERE id = $1", &[&busy]).unwrap().get(0);
    assert_eq!(busy_sub, "ongoing");

    // Within a day it stays escalating; after a quiet day it is ongoing again.
    assert!(h.ingest.check_escalation(now + Duration::hours(2)).unwrap().calmed.is_empty());
    let r = h.ingest.check_escalation(now + Duration::hours(26)).unwrap();
    assert_eq!(r.calmed, vec![spiky]);
    let sub: String = conn.query_one("SELECT substatus FROM issues WHERE id = $1", &[&spiky]).unwrap().get(0);
    assert_eq!(sub, "ongoing");

    // Ignored until escalating: escalation brings it back.
    conn.execute(
        "UPDATE issues SET status = 'ignored', substatus = 'archived_until_escalating' WHERE id = $1",
        &[&spiky],
    )
    .unwrap();
    append_events(&conn, &rows(p, spiky, 60, now + Duration::hours(26))).unwrap();
    conn.execute("UPDATE issues SET last_seen = $2 WHERE id = $1", &[&spiky, &(now + Duration::hours(26))]).unwrap();
    let r = h.ingest.check_escalation(now + Duration::hours(26) + Duration::minutes(1)).unwrap();
    assert_eq!(r.escalated, vec![spiky]);
    assert_eq!(status_of(&h, spiky), ("unresolved".into(), "escalating".into()));
}

fn status_of(h: &Harness, id: i64) -> (String, String) {
    let r = h.store.connect().query_one("SELECT status, substatus FROM issues WHERE id = $1", &[&id]).unwrap();
    (r.get(0), r.get(1))
}
