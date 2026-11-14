//! Replays requests recorded from the real upload CLI (sentry-cli 3.8.2):
//! `sourcemaps upload`, `releases new`, `releases finalize`, `deploys new`,
//! `releases list` and `info`. See docs/sourcemap-upload.md.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use base64::Engine as _;
use common::TestApp;
use rupta_core::role::Role;
use serde_json::{Value, json};

const CAPTURE: &str = include_str!("fixtures/sentry-cli-3.8.2.jsonl");

#[tokio::test]
async fn recorded_cli_session_replays() {
    let app = TestApp::new();
    let org = app.org("acme");
    let project = app.project(org, "web");
    let token = app.org_token(org, &["project:releases", "org:read"]);

    for line in CAPTURE.lines() {
        let rec: Value = serde_json::from_str(line).unwrap();
        let method = rec["method"].as_str().unwrap();
        let path = rec["path"].as_str().unwrap();
        let body = base64::engine::general_purpose::STANDARD.decode(rec["body_b64"].as_str().unwrap()).unwrap();
        let mut req =
            Request::builder().method(method).uri(path).header(header::AUTHORIZATION, format!("Bearer {token}"));
        if let Some(ct) = rec["content_type"].as_str() {
            req = req.header(header::CONTENT_TYPE, ct);
        }
        let resp = app.send(req.body(Body::from(body)).unwrap()).await;
        let recorded: Value = serde_json::from_str(rec["response"].as_str().unwrap()).unwrap_or(Value::Null);
        assert_eq!(resp.status.as_u16() as u64, rec["status"].as_u64().unwrap(), "{method} {path}: {:?}", resp.body);
        if path.ends_with("/assemble/") {
            assert_eq!(resp.body["state"], recorded["state"], "{path}");
            assert_eq!(resp.body["missingChunks"], recorded["missingChunks"], "{path}");
        }
        if path.ends_with("/chunk-upload/") && method == "GET" {
            assert_eq!(resp.body, recorded);
        }
    }

    let conn = app.conn();
    let row = conn.query_one("SELECT id, release, file_count, object_key FROM artifact_bundles", &[]).unwrap();
    let (bundle, release, files, key): (i64, Option<String>, i32, String) =
        (row.get(0), row.get(1), row.get(2), row.get(3));
    assert_eq!(release.as_deref(), Some("web@1.0.0"));
    assert_eq!(files, 2);
    assert!(app.state.store.objects().get(&key).unwrap().is_some(), "the zip is in the object store");
    let linked: i64 = conn
        .query_one("SELECT project_id FROM artifact_bundle_projects WHERE bundle_id = $1", &[&bundle])
        .unwrap()
        .get(0);
    assert_eq!(linked, project);
    let debug_id = uuid::Uuid::parse_str("5942af26-7ba1-5f9c-bb71-791608cf2e45").unwrap();
    let mut index: Vec<(String, String)> = conn
        .query("SELECT url, file_type FROM artifact_index WHERE debug_id = $1 ORDER BY url", &[debug_id.as_bytes()])
        .unwrap()
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
    index.sort();
    assert_eq!(
        index,
        vec![("~/app.js".into(), "minified_source".into()), ("~/app.js.map".into(), "source_map".into())]
    );
    let released: Option<chrono::DateTime<chrono::Utc>> =
        conn.query_one("SELECT date_released FROM releases WHERE version = 'web@1.0.0'", &[]).unwrap().get(0);
    assert!(released.is_some(), "finalized");
    let env: String = conn.query_one("SELECT environment FROM deploys", &[]).unwrap().get(0);
    assert_eq!(env, "production");
}

fn gzip(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

fn multipart(parts: &[(&str, &str, Vec<u8>)]) -> (String, Vec<u8>) {
    let boundary = "XyZboundary";
    let mut body = Vec::new();
    for (field, filename, data) in parts {
        body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{field}\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes());
        body.extend_from_slice(data);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

fn sha1_hex(d: &[u8]) -> String {
    use sha1::Digest;
    hex::encode(sha1::Sha1::digest(d))
}

/// A bundle split into two chunks, one plain and one gzip-compressed.
#[tokio::test]
async fn multi_chunk_bundles_checksums_and_permissions() {
    let app = TestApp::new();
    let org = app.org("acme");
    app.project(org, "web");
    let token = app.org_token(org, &["project:releases"]);
    let read_only = app.org_token(org, &["org:read"]);

    // A zip with a manifest, built in memory.
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default();
    zip.start_file("manifest.json", opts).unwrap();
    use std::io::Write;
    zip.write_all(json!({"files": {"files/_/_/main.js": {"url": "~/main.js", "type": "minified_source", "headers": {"Debug-Id": "11111111-2222-3333-4444-555555555555"}}}}).to_string().as_bytes()).unwrap();
    zip.start_file("files/_/_/main.js", opts).unwrap();
    zip.write_all(&vec![b'x'; 5000]).unwrap();
    let data = zip.finish().unwrap().into_inner();
    let (a, b) = data.split_at(data.len() / 2);
    let checksum = sha1_hex(&data);
    let chunks = vec![sha1_hex(a), sha1_hex(b)];

    let post = |path: &str, tok: &str, ct: &str, body: Vec<u8>| {
        Request::builder()
            .method("POST")
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {tok}"))
            .header(header::CONTENT_TYPE, ct)
            .body(Body::from(body))
            .unwrap()
    };
    let assemble = json!({"checksum": checksum, "chunks": chunks, "projects": ["web"]}).to_string().into_bytes();
    let r = app
        .send(post("/api/0/organizations/acme/artifactbundle/assemble/", &token, "application/json", assemble.clone()))
        .await;
    assert_eq!(r.body["state"], "not_found");
    assert_eq!(r.body["missingChunks"].as_array().unwrap().len(), 2);

    // Read-only tokens cannot upload.
    let (ct, body) = multipart(&[("file", &chunks[0], a.to_vec())]);
    assert_eq!(
        app.send(post("/api/0/organizations/acme/chunk-upload/", &read_only, &ct, body)).await.status,
        StatusCode::FORBIDDEN
    );
    // A chunk whose name does not match its content is refused.
    let (ct, body) = multipart(&[("file", &chunks[1], a.to_vec())]);
    let r = app.send(post("/api/0/organizations/acme/chunk-upload/", &token, &ct, body)).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert!(r.body["detail"].as_str().unwrap().contains("checksum"));

    let (ct, body) = multipart(&[("file", &chunks[0], a.to_vec()), ("file_gzip", &chunks[1], gzip(b))]);
    assert_eq!(
        app.send(post("/api/0/organizations/acme/chunk-upload/", &token, &ct, body)).await.status,
        StatusCode::OK
    );
    let r = app
        .send(post("/api/0/organizations/acme/artifactbundle/assemble/", &token, "application/json", assemble.clone()))
        .await;
    assert_eq!(r.body, json!({"state": "ok", "missingChunks": [], "detail": null}));
    let n: i64 = app
        .conn()
        .query_one("SELECT count(*) FROM artifact_index WHERE url = '~/main.js' AND debug_id IS NOT NULL", &[])
        .unwrap()
        .get(0);
    assert_eq!(n, 1);
    // Idempotent.
    let r = app
        .send(post("/api/0/organizations/acme/artifactbundle/assemble/", &token, "application/json", assemble))
        .await;
    assert_eq!(r.body["state"], "ok");

    // Chunks in the wrong order do not match the checksum.
    let wrong = json!({"checksum": sha1_hex(b"other"), "chunks": [chunks[1], chunks[0]], "projects": ["web"]})
        .to_string()
        .into_bytes();
    let r =
        app.send(post("/api/0/organizations/acme/artifactbundle/assemble/", &token, "application/json", wrong)).await;
    assert_eq!(r.body["state"], "error");

    // Other organizations: not found.
    let other = app.org("other");
    let foreign = app.org_token(other, &["project:releases"]);
    let r = app
        .send(post(
            "/api/0/organizations/acme/artifactbundle/assemble/",
            &foreign,
            "application/json",
            b"{\"checksum\":\"x\",\"chunks\":[]}".to_vec(),
        ))
        .await;
    assert!(r.status == StatusCode::BAD_REQUEST || r.status == StatusCode::NOT_FOUND);
    let r = app
        .send(
            Request::builder()
                .uri("/api/0/organizations/acme/chunk-upload/")
                .header(header::AUTHORIZATION, format!("Bearer {foreign}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert!(r.body["detail"].is_string());
    let _ = Role::Owner;
}
