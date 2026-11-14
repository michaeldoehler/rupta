//! Replays envelopes captured from the official open-source SDKs
//! (`scripts/sdk-smoke.sh` with `SMOKE_CAPTURE_DIR`, then
//! `scripts/sdk-smoke/make-fixtures.py`) and checks what was stored.
//!
//! SDK versions captured: @sentry/node 11.12.0, @sentry/browser 11.12.0,
//! sentry/sentry (PHP) 4.37.0, sentry-sdk (Python) 2.75.0.

mod common;

use std::path::PathBuf;

use common::*;
use serde_json::{Value, json};

fn fixtures() -> Vec<(String, Value, Vec<u8>)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.ends_with(".envelope"))
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|n| {
            let base = n.trim_end_matches(".envelope").to_string();
            let meta: Value =
                serde_json::from_slice(&std::fs::read(dir.join(format!("{base}.json"))).unwrap()).unwrap();
            let body = std::fs::read(dir.join(&n)).unwrap();
            (base, meta, body)
        })
        .collect()
}

async fn replay(h: &Harness) {
    let all = fixtures();
    assert!(all.len() >= 25, "fixtures missing");
    for (name, meta, body) in all {
        let path = match meta["query"].as_str() {
            Some(q) if !q.is_empty() => format!("{}?{q}", meta["path"].as_str().unwrap()),
            _ => meta["path"].as_str().unwrap().to_string(),
        };
        let headers: Vec<(String, String)> = meta["headers"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
            .collect();
        let refs: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        let (status, res_headers, res) = h.send(request(&path, &refs, body)).await;
        assert_eq!(status, 200, "{name}: {res}");
        if name.contains("-event") || name.contains("-feedback") || name.contains("-transaction") {
            assert!(res["id"].as_str().is_some_and(|id| id.len() == 32), "{name}: {res}");
        }
        if meta["headers"].get("origin").is_some() {
            assert_eq!(res_headers["access-control-allow-origin"], "http://localhost:3000", "{name}");
        }
    }
    h.flush().await;
}

fn query_strings(h: &Harness, sql: &str) -> Vec<String> {
    h.store.connect().query(sql, &[]).unwrap().iter().map(|r| r.get::<Option<String>>(0).unwrap_or_default()).collect()
}

/// The payload of the event whose issue has this title.
fn event_by_title(h: &Harness, title: &str) -> Value {
    let row = h
        .store
        .connect()
        .query_one(
            "SELECT p.data FROM event_payloads p JOIN issues i ON i.id = p.issue_id WHERE i.title = $1 LIMIT 1",
            &[&title],
        )
        .unwrap_or_else(|e| panic!("no event titled {title:?}: {e}"));
    let data: Vec<u8> = row.get(0);
    serde_json::from_slice(&zstd::decode_all(&data[..]).unwrap()).unwrap()
}

fn tag<'a>(ev: &'a Value, key: &str) -> Option<&'a str> {
    ev["tags"].as_array()?.iter().find(|t| t[0] == key).and_then(|t| t[1].as_str())
}

#[tokio::test]
async fn sdk_envelopes_are_stored() {
    let h = harness();
    replay(&h).await;

    // Every SDK: an exception and a message.
    let by_sdk = query_strings(&h, "SELECT sdk || ':' || count(*) FROM events GROUP BY sdk ORDER BY sdk");
    assert_eq!(
        by_sdk,
        vec!["sentry.javascript.browser:3", "sentry.javascript.node:2", "sentry.php:2", "sentry.python:2"]
    );
    let titles = query_strings(&h, "SELECT title FROM issues ORDER BY title");
    for t in [
        "Error: smoke browser unhandled",
        "Error: smoke node error",
        "RuntimeException: smoke php error",
        "TypeError: smoke browser error",
        "ZeroDivisionError: division by zero",
        "smoke browser message",
        "smoke node message",
        "smoke php message",
        "smoke python message",
    ] {
        assert!(titles.contains(&t.to_string()), "missing issue {t:?} in {titles:?}");
    }

    // Node: stack trace, user, tag, breadcrumb, release, environment.
    let node = event_by_title(&h, "Error: smoke node error");
    let frames = node["exception"]["values"][0]["stacktrace"]["frames"].as_array().unwrap();
    let top = frames.last().unwrap();
    assert_eq!(top["function"], "failingFunction");
    assert_eq!(top["in_app"], true);
    assert!(top["context_line"].as_str().unwrap().contains("throw new Error"));
    assert_eq!(node["user"]["id"], "node-user");
    assert_eq!(node["release"], "smoke-node@1.0.0");
    assert_eq!(node["environment"], "smoke");
    assert_eq!(node["server_name"], "smoke-host");
    assert_eq!(tag(&node, "smoke"), Some("node"));
    assert!(node["breadcrumbs"]["values"].as_array().unwrap().iter().any(|b| b["message"] == "node breadcrumb"));
    assert_eq!(node["contexts"]["runtime"]["name"], "node");
    assert_eq!(node["exception"]["values"][0]["mechanism"]["handled"], true);

    // Browser: unhandled error from the global handler, browser context from the User-Agent.
    let unhandled = event_by_title(&h, "Error: smoke browser unhandled");
    assert_eq!(unhandled["exception"]["values"][0]["mechanism"]["handled"], false);
    assert_eq!(tag(&unhandled, "handled"), Some("no"));
    assert_eq!(unhandled["user"]["ip_address"], "203.0.113.7");
    let browser = event_by_title(&h, "TypeError: smoke browser error");
    assert_eq!(browser["contexts"]["browser"]["name"], "Chrome");
    assert_eq!(browser["request"]["url"], "http://localhost:3000/index.html");
    assert_eq!(tag(&browser, "smoke"), Some("browser"));

    // PHP and Python.
    let php = event_by_title(&h, "RuntimeException: smoke php error");
    assert_eq!(
        php["exception"]["values"][0]["stacktrace"]["frames"].as_array().unwrap().last().unwrap()["function"],
        "failing_function"
    );
    assert_eq!(php["user"]["email"], "php@example.com");
    assert_eq!(php["platform"], "php");
    let py = event_by_title(&h, "ZeroDivisionError: division by zero");
    assert_eq!(py["platform"], "python");
    assert_eq!(
        py["exception"]["values"][0]["stacktrace"]["frames"].as_array().unwrap().last().unwrap()["function"],
        "failing_function"
    );
    assert_eq!(tag(&py, "smoke"), Some("python"));
    assert_eq!(event_by_title(&h, "smoke python message")["level"], "warning");

    // Attachments (Node, PHP, Python), linked to their events.
    assert_eq!(
        query_strings(&h, "SELECT name FROM attachments ORDER BY name"),
        vec!["node.txt", "php.txt", "python.txt"]
    );
    let key = query_strings(&h, "SELECT object_key FROM attachments WHERE name = 'php.txt'").remove(0);
    assert_eq!(h.store.objects().get(&key).unwrap().unwrap(), b"hello from php");

    // Feedback (Node, browser), linked to the issue of the event it refers to.
    assert_eq!(h.count("SELECT count(*) FROM user_feedback WHERE issue_id IS NOT NULL"), 2);
    assert_eq!(
        query_strings(&h, "SELECT message FROM user_feedback ORDER BY message"),
        vec!["smoke browser feedback", "smoke node feedback"]
    );

    // Cron check-ins: one monitor and one completed check-in per server SDK.
    assert_eq!(
        query_strings(&h, "SELECT slug || ':' || health FROM monitors ORDER BY slug"),
        vec!["smoke-node-cron:ok", "smoke-php-cron:ok", "smoke-python-cron:ok"]
    );
    assert_eq!(h.count("SELECT count(*) FROM checkins WHERE status = 'ok' AND duration_ms = 250"), 3);
    assert_eq!(h.count("SELECT count(*) FROM monitors WHERE schedule ->> 'value' = '*/5 * * * *'"), 3);

    // Transactions: PHP and Python send transaction items, Node and the
    // browser stream spans whose segment spans become transactions.
    let names = query_strings(&h, "SELECT name FROM transactions ORDER BY name");
    for n in ["smoke-node-transaction", "smoke-php-transaction", "smoke-python-transaction"] {
        assert!(names.contains(&n.to_string()), "missing transaction {n} in {names:?}");
    }
    assert_eq!(h.count("SELECT count(*) FROM transactions"), 4);
    assert_eq!(h.count("SELECT count(*) FROM transactions WHERE duration_ms > 0 AND trace_id IS NOT NULL"), 4);

    // Release health: Node, browser and Python sessions.
    let sessions = query_strings(
        &h,
        "SELECT release || ':' || total || ':' || errored || ':' || crashed FROM session_counts ORDER BY release",
    );
    assert_eq!(sessions, vec!["smoke-browser@1.0.0:1:1:1", "smoke-node@1.0.0:2:1:0", "smoke-python@1.0.0:1:1:0"]);
    assert_eq!(h.count("SELECT count(*) FROM releases"), 4);

    // Nothing was rejected as invalid; child spans are counted as not supported.
    assert_eq!(h.count("SELECT count(*) FROM outcomes WHERE outcome = 'invalid'"), 0);
    assert!(
        h.count(
            "SELECT sum(quantity)::bigint FROM outcomes WHERE outcome = 'dropped_unsupported' AND category = 'span'"
        ) > 0
    );
    let _ = json!(null);
}
