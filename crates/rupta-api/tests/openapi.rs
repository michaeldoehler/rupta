//! The committed OpenAPI document must match the code. Regenerate with
//! `RUPTA_UPDATE_OPENAPI=1 cargo test -p rupta-api --test openapi`.

mod common;

use axum::http::StatusCode;
use common::{As, TestApp};

const PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/api/openapi.json");

#[test]
fn committed_openapi_document_is_up_to_date() {
    let current = rupta_api::openapi_json();
    if std::env::var_os("RUPTA_UPDATE_OPENAPI").is_some() {
        std::fs::create_dir_all(std::path::Path::new(PATH).parent().unwrap()).unwrap();
        std::fs::write(PATH, &current).unwrap();
        return;
    }
    let committed = std::fs::read_to_string(PATH).unwrap_or_default();
    assert!(
        committed == current,
        "docs/api/openapi.json is out of date; run RUPTA_UPDATE_OPENAPI=1 cargo test -p rupta-api --test openapi"
    );
}

#[test]
fn every_path_parameter_is_declared() {
    let doc: serde_json::Value = serde_json::from_str(&rupta_api::openapi_json()).unwrap();
    assert_eq!(doc["openapi"].as_str().unwrap().split('.').take(2).collect::<Vec<_>>(), vec!["3", "1"]);
    for (path, item) in doc["paths"].as_object().unwrap() {
        let names: Vec<&str> = path.split('{').skip(1).map(|s| s.split('}').next().unwrap()).collect();
        for (method, op) in item.as_object().unwrap() {
            let declared: Vec<&str> = op["parameters"]
                .as_array()
                .map(|ps| ps.iter().filter(|p| p["in"] == "path").map(|p| p["name"].as_str().unwrap()).collect())
                .unwrap_or_default();
            for n in &names {
                assert!(declared.contains(n), "{method} {path}: path parameter {n} not declared");
            }
        }
    }
}

#[tokio::test]
async fn the_document_is_served() {
    let app = TestApp::new();
    let r = app.get("/api/v1/openapi.json", &As::anonymous()).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.body["paths"]["/api/v1/orgs/{org}/issues"].is_object());
}
