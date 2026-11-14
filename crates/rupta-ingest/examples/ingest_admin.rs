//! Test helper for the SDK smoke test and benchmarks: seeds a project and
//! key into a data directory, or prints what was stored. The server must
//! not be running on the same directory (one process owns a database).
//!
//! ```sh
//! cargo run -p rupta-ingest --example ingest_admin -- seed <data_dir> <public_key> [settings_json]
//! cargo run -p rupta-ingest --example ingest_admin -- summary <data_dir>
//! ```

use rupta_store::{Store, StoreOptions};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: ingest_admin seed <data_dir> <public_key> [settings_json] | summary <data_dir>";
    let (cmd, dir) = match (args.first(), args.get(1)) {
        (Some(c), Some(d)) => (c.as_str(), d.as_str()),
        _ => {
            eprintln!("{usage}");
            std::process::exit(2);
        }
    };
    let store = Store::open(&StoreOptions::local(dir)).expect("opening the database");
    let conn = store.connect();
    match cmd {
        "seed" => {
            let key = args.get(2).expect(usage);
            let settings: serde_json::Value =
                serde_json::from_str(args.get(3).map(String::as_str).unwrap_or("{}")).expect("settings must be JSON");
            conn.execute(
                "INSERT INTO organizations (slug, name) VALUES ('smoke', 'Smoke') ON CONFLICT DO NOTHING",
                &[],
            )
            .unwrap();
            let org: i64 = conn.query_one("SELECT id FROM organizations WHERE slug = 'smoke'", &[]).unwrap().get(0);
            let project: i64 = conn
                .query_one(
                    "INSERT INTO projects (org_id, slug, name, platform, settings) VALUES ($1, 'smoke', 'Smoke', 'other', $2)
                     ON CONFLICT (org_id, slug) DO UPDATE SET settings = EXCLUDED.settings RETURNING id",
                    &[&org, &settings],
                )
                .unwrap()
                .get(0);
            conn.execute(
                "INSERT INTO project_keys (project_id, public_key, secret_key) VALUES ($1, $2, 'unused') ON CONFLICT (public_key) DO NOTHING",
                &[&project, key],
            )
            .unwrap();
            println!("{project}");
        }
        "summary" => {
            let count = |sql: &str| -> i64 { conn.query_one(sql, &[]).unwrap().get(0) };
            let mut out = serde_json::Map::new();
            for (name, sql) in [
                ("events", "SELECT count(*) FROM events"),
                ("issues", "SELECT count(*) FROM issues"),
                ("payloads", "SELECT count(*) FROM event_payloads"),
                ("attachments", "SELECT count(*) FROM attachments"),
                ("transactions", "SELECT count(*) FROM transactions"),
                ("feedback", "SELECT count(*) FROM user_feedback"),
                ("feedback_linked", "SELECT count(*) FROM user_feedback WHERE issue_id IS NOT NULL"),
                ("monitors", "SELECT count(*) FROM monitors"),
                ("checkins", "SELECT count(*) FROM checkins"),
                ("sessions", "SELECT coalesce(sum(total), 0)::bigint FROM session_counts"),
                ("releases", "SELECT count(*) FROM releases"),
                ("spans", "SELECT count(*) FROM spans"),
                ("logs", "SELECT count(*) FROM logs"),
                ("logs_with_trace", "SELECT count(*) FROM logs WHERE trace_id IS NOT NULL"),
                ("metrics", "SELECT count(*) FROM metrics"),
                ("statsd_metrics", "SELECT count(*) FROM metrics WHERE name LIKE 'smoke.statsd.%'"),
                ("profiles", "SELECT count(*) FROM profiles WHERE kind = 'transaction'"),
                ("profile_chunks", "SELECT count(*) FROM profiles WHERE kind = 'chunk'"),
                ("replays", "SELECT count(*) FROM replays"),
                ("replay_segments", "SELECT count(*) FROM replay_segments"),
                ("replay_clicks", "SELECT coalesce(sum(count_clicks), 0)::bigint FROM replays"),
                ("issue_replays", "SELECT count(*) FROM issue_replays"),
            ] {
                out.insert(name.into(), count(sql).into());
            }
            let by_sdk: Vec<serde_json::Value> = conn
                .query("SELECT coalesce(sdk, ''), count(*) FROM events GROUP BY 1 ORDER BY 1", &[])
                .unwrap()
                .iter()
                .map(|r| serde_json::json!([r.get::<String>(0), r.get::<i64>(1)]))
                .collect();
            out.insert("events_by_sdk".into(), by_sdk.into());
            let strings = |sql: &str| -> Vec<serde_json::Value> {
                conn.query(sql, &[]).unwrap().iter().map(|r| serde_json::json!(r.get::<Option<String>>(0))).collect()
            };
            out.insert("log_sdks".into(), strings("SELECT DISTINCT sdk FROM logs ORDER BY 1").into());
            out.insert("metric_names".into(), strings("SELECT DISTINCT name FROM metrics ORDER BY 1").into());
            out.insert(
                "profile_platforms".into(),
                strings("SELECT DISTINCT platform || ':' || kind FROM profiles ORDER BY 1").into(),
            );
            let titles: Vec<serde_json::Value> = conn
                .query("SELECT title FROM issues ORDER BY id", &[])
                .unwrap()
                .iter()
                .map(|r| serde_json::Value::String(r.get(0)))
                .collect();
            out.insert("issue_titles".into(), titles.into());
            let outcomes: Vec<serde_json::Value> = conn
                .query("SELECT category, outcome, reason, sum(quantity)::bigint FROM outcomes GROUP BY 1, 2, 3 ORDER BY 1, 2, 3", &[])
                .unwrap()
                .iter()
                .map(|r| serde_json::json!([r.get::<String>(0), r.get::<String>(1), r.get::<String>(2), r.get::<i64>(3)]))
                .collect();
            out.insert("outcomes".into(), outcomes.into());
            println!("{}", serde_json::to_string_pretty(&serde_json::Value::Object(out)).unwrap());
        }
        _ => {
            eprintln!("{usage}");
            std::process::exit(2);
        }
    }
}
