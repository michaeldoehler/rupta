//! Source map symbolication from an artifact bundle built in memory.

mod common;

use std::io::Write;

use common::*;
use serde_json::{Value, json};

const ORIGINAL: &str = "function greet(name) {
  const message = 'hi ' + name;
  throw new Error(message);
}

function main() {
  greet('ann');
}

main();
";

const MINIFIED: &str = "function a(b){const c='hi '+b;throw new Error(c)}function d(){a('ann')}d();";
const DEBUG_ID: &str = "c941d872-af1f-4f0c-a7ff-ad3d295fe153";

fn col(needle: &str) -> u32 {
    MINIFIED.find(needle).unwrap() as u32
}

/// The source map for MINIFIED, as a bundler would write it.
fn source_map() -> Vec<u8> {
    let mut b = sourcemap::SourceMapBuilder::new(Some("app.min.js"));
    let src = "webpack://my-app/./src/app.js";
    let id = b.add_source(src);
    b.set_source_contents(id, Some(ORIGINAL));
    let mut add = |dst_col: u32, src_line: u32, src_col: u32, name: Option<&str>| {
        b.add(0, dst_col, src_line, src_col, Some(src), name, false);
    };
    add(0, 0, 0, None); // function
    add(col("a("), 0, 9, Some("greet"));
    add(col("b)"), 0, 15, Some("name"));
    add(col("const"), 1, 2, None);
    add(col("throw"), 2, 2, None);
    add(col("new Error"), 2, 8, None);
    add(col("function d"), 5, 0, None);
    add(col("d()"), 5, 9, Some("main"));
    add(col("a('ann')"), 6, 2, Some("greet"));
    add(col("d();"), 9, 0, Some("main"));
    let mut out = Vec::new();
    b.into_sourcemap().to_writer(&mut out).unwrap();
    out
}

/// A bundle zip as the upload CLI produces it.
fn bundle() -> Vec<u8> {
    let manifest = json!({"org": "acme", "release": "web@1.0.0", "files": {
        "_/_/static/app.min.js": {"url": "~/static/app.min.js", "type": "minified_source",
            "headers": {"debug-id": DEBUG_ID, "sourcemap": "app.min.js.map"}},
        "_/_/static/app.min.js.map": {"url": "~/static/app.min.js.map", "type": "source_map",
            "headers": {"debug-id": DEBUG_ID}}
    }});
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, data) in [
        ("manifest.json", manifest.to_string().into_bytes()),
        ("_/_/static/app.min.js", MINIFIED.as_bytes().to_vec()),
        ("_/_/static/app.min.js.map", source_map()),
    ] {
        zip.start_file(name, opts).unwrap();
        zip.write_all(&data).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn install_bundle(h: &Harness) {
    h.store.objects().put("artifacts/bundles/1.zip", &bundle()).unwrap();
    let conn = h.store.connect();
    conn.execute(
        "INSERT INTO artifact_bundles (org_id, checksum, release, object_key, file_count) VALUES (1, 'abc', 'web@1.0.0', 'artifacts/bundles/1.zip', 2)",
        &[],
    )
    .unwrap();
    let debug_id = *uuid::Uuid::parse_str(DEBUG_ID).unwrap().as_bytes();
    for (path, url, ty) in [
        ("_/_/static/app.min.js", "~/static/app.min.js", "minified_source"),
        ("_/_/static/app.min.js.map", "~/static/app.min.js.map", "source_map"),
    ] {
        conn.execute(
            "INSERT INTO artifact_index (org_id, bundle_id, debug_id, release, url, file_path, file_type) VALUES (1, 1, $1, 'web@1.0.0', $2, $3, $4)",
            &[&debug_id, &url, &path, &ty],
        )
        .unwrap();
    }
}

fn minified_event(id: &str, release: Option<&str>, debug_meta: bool) -> Value {
    let abs = "https://app.example.com/static/app.min.js";
    let mut ev = json!({
        "event_id": id,
        "platform": "javascript",
        "exception": {"values": [{"type": "Error", "value": "hi ann", "stacktrace": {"frames": [
            {"abs_path": abs, "filename": abs, "function": "d", "lineno": 1, "colno": col("a('ann')") + 1, "in_app": true},
            {"abs_path": abs, "filename": abs, "function": "a", "lineno": 1, "colno": col("throw") + 1, "in_app": true}
        ]}}]},
    });
    if let Some(r) = release {
        ev["release"] = json!(r);
    }
    if debug_meta {
        ev["debug_meta"] = json!({"images": [{"type": "sourcemap", "code_file": abs, "debug_id": DEBUG_ID}]});
    }
    ev
}

fn check(ev: &Value) {
    let frames = &ev["exception"]["values"][0]["stacktrace"]["frames"];
    let top = &frames[1];
    assert_eq!(top["symbolicated"], true, "{top}");
    assert_eq!(top["lineno"], 3);
    assert_eq!(top["colno"], 3);
    assert_eq!(top["filename"], "src/app.js");
    assert_eq!(top["abs_path"], "webpack://my-app/./src/app.js");
    assert_eq!(top["function"], "greet");
    assert_eq!(top["context_line"], "  throw new Error(message);");
    assert_eq!(top["pre_context"], json!(["function greet(name) {", "  const message = 'hi ' + name;"]));
    assert_eq!(top["post_context"][0], "}");
    assert_eq!(top["post_context"].as_array().unwrap().len(), 5);
    assert_eq!(top["raw"]["function"], "a");
    assert_eq!(top["raw"]["colno"], col("throw") + 1);
    let caller = &frames[0];
    assert_eq!(caller["lineno"], 7);
    assert_eq!(caller["function"], "main");
    assert_eq!(caller["context_line"], "  greet('ann');");
}

#[tokio::test]
async fn symbolicates_by_debug_id() {
    let h = harness();
    install_bundle(&h);
    let id = new_id();
    assert_eq!(h.envelope(event_envelope(&minified_event(&id, None, true))).await.0, 200);
    h.flush().await;
    let ev = h.payload(&id);
    check(&ev);
    // The issue's culprit and grouping use the original names.
    let culprit: Option<String> = h.store.connect().query_one("SELECT culprit FROM issues", &[]).unwrap().get(0);
    assert_eq!(culprit.as_deref(), Some("greet(src/app.js)"));
}

#[tokio::test]
async fn symbolicates_by_release_and_url() {
    let h = harness();
    install_bundle(&h);
    let id = new_id();
    h.envelope(event_envelope(&minified_event(&id, Some("web@1.0.0"), false))).await;
    // Another release finds no map and stays minified.
    let other = new_id();
    h.envelope(event_envelope(&minified_event(&other, Some("web@2.0.0"), false))).await;
    h.flush().await;
    check(&h.payload(&id));
    let ev = h.payload(&other);
    assert!(ev["exception"]["values"][0]["stacktrace"]["frames"][1].get("symbolicated").is_none());
    assert_eq!(ev["exception"]["values"][0]["stacktrace"]["frames"][1]["function"], "a");
}

#[tokio::test]
async fn missing_debug_id_is_reported() {
    let h = harness();
    install_bundle(&h);
    let id = new_id();
    let mut ev = minified_event(&id, None, true);
    ev["debug_meta"]["images"][0]["debug_id"] = json!("00000000-0000-4000-8000-000000000001");
    h.envelope(event_envelope(&ev)).await;
    h.flush().await;
    let stored = h.payload(&id);
    assert_eq!(stored["errors"][0]["type"], "js_missing_source_map");
}
