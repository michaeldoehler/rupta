//! Retention: old data goes, recent data stays.

mod common;

use std::time::Instant;

use chrono::{Duration, TimeZone, Utc};
use common::*;
use rupta_store::ingest::*;
use serde_json::json;

fn event_row(project_id: i64, issue_id: i64, ts: chrono::DateTime<Utc>, n: u32) -> EventRow {
    let mut id = [0u8; 16];
    id[..4].copy_from_slice(&n.to_be_bytes());
    id[4..12].copy_from_slice(&ts.timestamp().to_be_bytes());
    EventRow {
        project_id,
        issue_id,
        event_id: id,
        timestamp: ts,
        received: ts,
        level: Some("error".into()),
        message: Some(format!("message {n}")),
        tags: json!({"browser": "Chrome"}),
        contexts: json!({}),
        ..EventRow::default()
    }
}

#[tokio::test]
async fn deletes_data_older_than_the_retention_period() {
    let h = harness_with(json!({"retention_days": 30}), config());
    let p = h.project_id;
    let now = Utc.with_ymd_and_hms(2026, 11, 14, 12, 0, 0).unwrap();
    let old = now - Duration::days(45);
    let recent = now - Duration::days(2);
    let conn = h.store.connect();
    let issue = NewIssue {
        project_id: p,
        fingerprint_hash: "old".into(),
        grouping_version: "v1".into(),
        title: "Old".into(),
        culprit: None,
        ty: "error".into(),
        level: "error".into(),
        platform: None,
        first_seen: old,
        last_seen: old,
        release: None,
        metadata: json!({}),
    };
    let (old_issue, _) = create_issue(&conn, &issue).unwrap();
    let (new_issue, _) = create_issue(
        &conn,
        &NewIssue { fingerprint_hash: "new".into(), first_seen: recent, last_seen: recent, ..issue },
    )
    .unwrap();
    let rows: Vec<EventRow> = (0..10)
        .map(|n| event_row(p, old_issue, old + Duration::hours(n as i64), n))
        .chain((0..5).map(|n| event_row(p, new_issue, recent, 100 + n)))
        .collect();
    append_events(&conn, &rows).unwrap();
    let payloads: Vec<PayloadRow> = rows
        .iter()
        .map(|r| PayloadRow {
            project_id: p,
            event_id: r.event_id,
            issue_id: Some(r.issue_id),
            day: r.timestamp.date_naive(),
            data: vec![1],
        })
        .collect();
    append_payloads(&conn, &payloads).unwrap();
    for (day, key) in [(old.date_naive(), "attachments/1/old/a"), (recent.date_naive(), "attachments/1/new/a")] {
        h.store.objects().put(key, b"x").unwrap();
        insert_attachment(
            &conn,
            &AttachmentRow {
                project_id: p,
                event_id: [1; 16],
                name: "a".into(),
                content_type: None,
                size: 1,
                object_key: key.into(),
                day,
            },
        )
        .unwrap();
    }
    add_session_counts(
        &conn,
        &[
            SessionCount {
                project_id: p,
                release: "1".into(),
                environment: "".into(),
                hour: old,
                total: 1,
                ..Default::default()
            },
            SessionCount {
                project_id: p,
                release: "1".into(),
                environment: "".into(),
                hour: recent,
                total: 1,
                ..Default::default()
            },
        ],
    )
    .unwrap();
    let ok = |hour| {
        (OutcomeKey { project_id: p, hour, category: "error".into(), outcome: "accepted".into(), reason: "".into() }, 1)
    };
    add_outcomes(&conn, &[ok(old), ok(recent)]).unwrap();
    append_transactions(
        &conn,
        &[TransactionRow {
            project_id: p,
            timestamp: old,
            received: old,
            tags: "{}".into(),
            measurements: "{}".into(),
            ..Default::default()
        }],
    )
    .unwrap();

    let store = h.store.clone();
    let report =
        tokio::task::spawn_blocking(move || rupta_ingest::retention::run_once(&store, 90, now)).await.unwrap().unwrap();
    assert_eq!(report.deleted.events, 10);
    assert_eq!(report.deleted.payloads, 10);
    assert_eq!(report.deleted.attachments, 1);
    assert_eq!(report.deleted.transactions, 1);
    assert_eq!(report.deleted.session_counts, 1);
    assert_eq!(report.deleted.outcomes, 1);
    assert_eq!(report.deleted.issues, 1);
    assert_eq!(h.count("SELECT count(*) FROM events"), 5);
    assert_eq!(h.count("SELECT count(*) FROM event_payloads"), 5);
    assert_eq!(h.count("SELECT count(*) FROM issues"), 1);
    assert!(h.store.objects().get("attachments/1/old/a").unwrap().is_none());
    assert!(h.store.objects().get("attachments/1/new/a").unwrap().is_some());
}

/// Measures how long deleting one day of events takes in a table of
/// 500,000 events (50,000 per day over 10 days). Run with
/// `cargo test --release -p rupta-ingest --test retention -- --ignored --nocapture`.
#[test]
#[ignore]
fn measure_columnar_delete_of_one_day() {
    let dir = tempfile::tempdir().unwrap();
    let store = rupta_store::Store::open(&rupta_store::StoreOptions::local(dir.path())).unwrap();
    let p = seed(&store, &json!({}));
    let conn = store.connect();
    let start = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let t = Instant::now();
    let per_day = 50_000u32;
    for day in 0..10 {
        for chunk in 0..(per_day / 1000) {
            let rows: Vec<EventRow> = (0..1000)
                .map(|i| {
                    let n = chunk * 1000 + i;
                    event_row(
                        p,
                        (n % 50) as i64 + 1,
                        start + Duration::days(day) + Duration::seconds((n as i64 * 86_400) / per_day as i64),
                        n,
                    )
                })
                .collect();
            append_events(&conn, &rows).unwrap();
        }
    }
    println!("inserted 500000 events in {:?}", t.elapsed());
    let t = Instant::now();
    assert!(store.database().wait_for_compaction(std::time::Duration::from_secs(600)));
    println!("compaction finished in {:?}", t.elapsed());
    let t = Instant::now();
    let (n, _) = delete_columnar_range(&conn, p, start, start + Duration::days(1)).unwrap();
    println!("deleted one day ({n} events) in {:?}", t.elapsed());
    assert_eq!(n, per_day as u64);
    let t = Instant::now();
    let (n, _) = delete_columnar_range(&conn, p, start + Duration::days(5), start + Duration::days(6)).unwrap();
    println!("deleted a day in the middle ({n} events) in {:?}", t.elapsed());
    let left: i64 = conn.query_one("SELECT count(*) FROM events", &[]).unwrap().get(0);
    assert_eq!(left, 400_000);
}
