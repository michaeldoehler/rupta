//! The MCP endpoint on a real HTTP server, driven by the official Rust MCP
//! client (Streamable HTTP) and by plain HTTP requests. The data comes from
//! the SDK envelopes in `rupta-ingest/tests/fixtures/telemetry`, sent
//! through the ingest pipeline, plus a second organization for isolation
//! and a mock GitHub for tickets.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use rmcp::model::{
    CallToolRequestParams, ClientConfig, GetPromptRequestParams, ProtocolVersion, ReadResourceRequestParams,
};
use rmcp::service::{RoleClient, RunningService};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::{ClientLifecycleMode, ClientServiceExt, ServiceExt};
use rupta_api::{ApiConfig, ApiState};
use rupta_auth::{apitoken, users};
use rupta_core::role::Role;
use rupta_store::Store;
use serde_json::{Value, json};
use tower::ServiceExt as _;

const SECRET: &[u8] = b"test secret key that is long enough!!";
const KEY: &str = "0123456789abcdef0123456789abcdef";
/// The fixtures were recorded on this day.
const START: &str = "2026-11-14T00:00:00Z";
const END: &str = "2026-11-15T00:00:00Z";
const PUBLIC_URL: &str = "http://rupta.test";

type Client = RunningService<RoleClient, ClientConfig>;

struct World {
    state: ApiState,
    /// `http://127.0.0.1:port`
    base: String,
    /// Tokens by name: owner, member, viewer, reader, org, outsider, admin.
    tokens: HashMap<&'static str, String>,
    /// An issue of the other organization.
    foreign_issue: i64,
    _ingest: rupta_ingest::Ingest,
}

impl World {
    fn url(&self) -> String {
        format!("{}/mcp", self.base)
    }
    fn token(&self, who: &str) -> &str {
        &self.tokens[who]
    }
    async fn client(&self, who: &str) -> Client {
        let mut info = ClientConfig::default();
        info.protocol_version = ProtocolVersion::V_2025_06_18;
        let t = StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(self.url()).auth_header(self.token(who)),
        );
        info.serve(t).await.expect("initialize")
    }
    /// A REST call with a token, for setup.
    async fn rest(&self, method: reqwest::Method, path: &str, who: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut r = reqwest::Client::new().request(method, format!("{}{path}", self.base)).bearer_auth(self.token(who));
        if let Some(b) = body {
            r = r.json(&b);
        }
        let resp = r.send().await.unwrap();
        let status = StatusCode::from_u16(resp.status().as_u16()).unwrap();
        (status, resp.json().await.unwrap_or(Value::Null))
    }
}

fn config() -> ApiConfig {
    let mut c = ApiConfig::new(PUBLIC_URL, SECRET);
    c.cors_allowed_origins = vec!["http://allowed.example:3000".into()];
    c.integrations_allow_private_hosts = true;
    c
}

async fn serve(state: &ApiState, enabled: bool) -> String {
    let app = rupta_api::router(state.clone()).merge(rupta_mcp::router(state.clone(), enabled));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await.unwrap();
    });
    format!("http://{addr}")
}

fn token(state: &ApiState, user: Option<i64>, org: Option<i64>, scopes: &[&str]) -> String {
    let scopes: Vec<String> = scopes.iter().map(|s| s.to_string()).collect();
    apitoken::create(&state.store.connect(), "mcp", user, org, &scopes, None).unwrap().token
}

fn member(state: &ApiState, org: i64, email: &str, role: Role) -> i64 {
    let conn = state.store.connect();
    let uid = users::create(&conn, email, email.split('@').next().unwrap(), None, false).unwrap();
    users::set_member(&conn, org, uid, role).unwrap();
    uid
}

/// An issue with one stored event, by SQL.
fn sql_issue(state: &ApiState, project: i64, title: &str) -> i64 {
    let conn = state.store.connect();
    let now = chrono::Utc::now();
    let id: i64 = conn
        .query_one(
            "INSERT INTO issues (project_id, short_id, fingerprint_hash, grouping_version, title, culprit, level, first_seen, last_seen, times_seen, users_seen, metadata)
             VALUES ($1, 1, $2, 'v1', $3, 'secret.js in main', 'error', $4, $4, 1, 0, '{}') RETURNING id",
            &[&project, &format!("fp-{title}"), &title, &now],
        )
        .unwrap()
        .get(0);
    let eid = uuid::Uuid::new_v4();
    conn.execute(
        "INSERT INTO events (project_id, issue_id, event_id, timestamp, received, level, platform, tags, contexts)
         VALUES ($1, $2, $3, $4, $4, 'error', 'javascript', '{}', '{}')",
        &[&project, &id, eid.as_bytes(), &now],
    )
    .unwrap();
    let doc = json!({"event_id": eid.simple().to_string(), "type": "error", "title": title,
                     "exception": {"values": [{"type": "Error", "value": "globex secret"}]}});
    let data = zstd::encode_all(doc.to_string().as_bytes(), 3).unwrap();
    conn.execute(
        "INSERT INTO event_payloads (project_id, event_id, issue_id, day, data) VALUES ($1, $2, $3, $4, $5)",
        &[&project, eid.as_bytes(), &id, &now.date_naive(), &data],
    )
    .unwrap();
    id
}

async fn world() -> World {
    let _ = tracing_subscriber::fmt().with_env_filter("rupta=info,warn").with_test_writer().try_init();
    let state = ApiState::new(Store::open_in_memory().unwrap(), config());
    let conn = state.store.connect();
    let acme = users::create_org(&conn, "acme", "Acme").unwrap();
    let project: i64 = conn
        .query_one(
            "INSERT INTO projects (org_id, slug, name, platform) VALUES ($1, 'web', 'Web', 'javascript') RETURNING id",
            &[&acme],
        )
        .unwrap()
        .get(0);
    assert_eq!(project, 1, "the fixtures post to project 1");
    conn.execute(
        "INSERT INTO project_keys (project_id, public_key, secret_key) VALUES ($1, $2, 's')",
        &[&project, &KEY],
    )
    .unwrap();
    conn.execute("INSERT INTO projects (org_id, slug, name, platform) VALUES ($1, 'api', 'API', 'python')", &[&acme])
        .unwrap();
    state.store.bump_config();
    let globex = users::create_org(&conn, "globex", "Globex").unwrap();
    let gproject: i64 = conn
        .query_one("INSERT INTO projects (org_id, slug, name) VALUES ($1, 'shop', 'Shop') RETURNING id", &[&globex])
        .unwrap()
        .get(0);
    let foreign_issue = sql_issue(&state, gproject, "Globex only");

    let owner = member(&state, acme, "owner@example.com", Role::Owner);
    let dev = member(&state, acme, "dev@example.com", Role::Member);
    let viewer = member(&state, acme, "viewer@example.com", Role::Viewer);
    let reader = member(&state, acme, "reader@example.com", Role::Member);
    let outsider = member(&state, globex, "outsider@example.com", Role::Owner);
    let mut tokens = HashMap::new();
    tokens
        .insert("owner", token(&state, Some(owner), None, &["org:read", "project:read", "issue:write", "member:read"]));
    tokens.insert("admin", token(&state, Some(owner), None, &["org:write", "project:write", "org:read"]));
    tokens
        .insert("member", token(&state, Some(dev), None, &["org:read", "project:read", "issue:write", "member:read"]));
    tokens.insert("viewer", token(&state, Some(viewer), None, &["org:read", "project:read", "issue:write"]));
    tokens.insert("reader", token(&state, Some(reader), None, &["project:read"]));
    tokens.insert("org", token(&state, None, Some(acme), &["org:read", "issue:write"]));
    tokens.insert("outsider", token(&state, Some(outsider), None, &["org:read", "project:read", "issue:write"]));

    let ingest = rupta_ingest::Ingest::start(
        state.store.clone(),
        rupta_ingest::IngestConfig { workers: 2, batch_interval: Duration::from_millis(5), ..Default::default() },
    );
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../rupta-ingest/tests/fixtures/telemetry");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.ends_with(".envelope"))
        .collect();
    names.sort();
    let router = ingest.router();
    for n in names {
        let meta: Value =
            serde_json::from_slice(&std::fs::read(dir.join(n.replace(".envelope", ".json"))).unwrap()).unwrap();
        let mut uri = meta["path"].as_str().unwrap().to_string();
        if let Some(q) = meta["query"].as_str().filter(|q| !q.is_empty()) {
            uri = format!("{uri}?{q}");
        }
        let mut b = Request::builder().method("POST").uri(uri);
        for (k, v) in meta["headers"].as_object().unwrap() {
            b = b.header(k.as_str(), v.as_str().unwrap());
        }
        let mut req = b.body(Body::from(std::fs::read(dir.join(&n)).unwrap())).unwrap();
        req.extensions_mut().insert(ConnectInfo("203.0.113.7:5555".parse::<SocketAddr>().unwrap()));
        let res = router.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK, "{n}");
    }
    assert!(ingest.flush(Duration::from_secs(20)).await);
    let base = serve(&state, true).await;
    World { state, base, tokens, foreign_issue, _ingest: ingest }
}

async fn call(c: &Client, name: &str, args: Value) -> Value {
    let p = CallToolRequestParams::new(name.to_string()).with_arguments(args.as_object().cloned().unwrap_or_default());
    serde_json::to_value(c.call_tool(p).await.unwrap_or_else(|e| panic!("{name}: {e}"))).unwrap()
}

/// The structured content of a successful call.
async fn ok(c: &Client, name: &str, args: Value) -> Value {
    let r = call(c, name, args.clone()).await;
    assert_ne!(r["isError"], true, "{name} {args}: {r}");
    r["structuredContent"].clone()
}

/// The text of a call.
fn text(r: &Value) -> String {
    r["content"].as_array().unwrap().iter().filter_map(|c| c["text"].as_str()).collect::<Vec<_>>().join("\n")
}

/// The error text of a call that must fail.
async fn fails(c: &Client, name: &str, args: Value) -> String {
    let r = call(c, name, args.clone()).await;
    assert_eq!(r["isError"], true, "{name} {args} should fail: {r}");
    text(&r)
}

fn find<'a>(list: &'a Value, key: &str, value: &str) -> &'a Value {
    list.as_array().unwrap().iter().find(|x| x[key] == value).unwrap_or_else(|| panic!("no {key}={value} in {list}"))
}

/// The id of the issue of the Node.js error fixture.
async fn node_issue(c: &Client) -> String {
    let r = ok(c, "search_issues", json!({"org": "acme", "query": "\"smoke node error\""})).await;
    assert_eq!(r["data"].as_array().unwrap().len(), 1, "{r}");
    r["data"][0]["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn initialize_lists_tools_resources_and_prompts() {
    let w = world().await;
    let c = w.client("member").await;
    let info = c.peer_info().expect("server info");
    assert_eq!(info.server_info.as_ref().unwrap().name, "rupta");
    assert_eq!(info.protocol_version, ProtocolVersion::V_2025_06_18);
    assert!(info.instructions.as_deref().unwrap().contains("list_organizations"));
    assert!(
        info.capabilities.tools.is_some()
            && info.capabilities.resources.is_some()
            && info.capabilities.prompts.is_some()
    );

    let tools = c.list_all_tools().await.unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    for n in [
        "list_organizations",
        "list_projects",
        "list_members",
        "list_teams",
        "search_issues",
        "get_issue",
        "get_issue_tag_values",
        "list_issue_events",
        "get_event",
        "list_issue_activity",
        "update_issue",
        "add_comment",
        "list_releases",
        "get_release",
        "get_project_health",
        "search_transactions",
        "get_transaction_summary",
        "get_trace",
        "search_spans",
        "search_logs",
        "list_metrics",
        "query_metric",
        "list_monitors",
        "list_monitor_checkins",
        "list_alert_rules",
        "list_alert_firings",
        "list_integrations",
        "create_ticket",
        "link_ticket",
    ] {
        assert!(names.contains(&n), "missing tool {n}: {names:?}");
    }
    assert_eq!(names.len(), 29);
    let tool = |n: &str| serde_json::to_value(tools.iter().find(|t| t.name == n).unwrap()).unwrap();
    let search = tool("search_issues");
    assert_eq!(search["annotations"]["readOnlyHint"], true);
    assert_eq!(search["inputSchema"]["required"], json!(["org"]));
    assert!(search["inputSchema"]["properties"]["query"]["description"].as_str().unwrap().contains("is:unresolved"));
    assert!(search["inputSchema"]["properties"]["sort"].to_string().contains("first_seen"));
    let update = tool("update_issue");
    assert_eq!(update["annotations"]["readOnlyHint"], false);
    assert_eq!(update["annotations"]["destructiveHint"], false);
    assert_eq!(update["annotations"]["idempotentHint"], true);
    assert_eq!(tool("create_ticket")["annotations"]["openWorldHint"], true);
    assert_eq!(tool("add_comment")["annotations"]["idempotentHint"], false);

    let resources = c.list_all_resources().await.unwrap();
    let uris: Vec<&str> = resources.iter().map(|r| r.uri.as_str()).collect();
    assert_eq!(uris, vec!["rupta://docs/search", "rupta://docs/event-fields", "rupta://docs/grouping"]);
    let templates = c.list_all_resource_templates().await.unwrap();
    let t: Vec<&str> = templates.iter().map(|t| t.uri_template.as_str()).collect();
    assert_eq!(t, vec!["rupta://issue/{id}", "rupta://issue/{id}/event/{event}"]);
    let doc =
        serde_json::to_value(c.read_resource(ReadResourceRequestParams::new("rupta://docs/search")).await.unwrap())
            .unwrap();
    assert!(doc["contents"][0]["text"].as_str().unwrap().contains("## Issue keys"));
    assert_eq!(doc["contents"][0]["mimeType"], "text/markdown");

    let prompts = c.list_all_prompts().await.unwrap();
    let p: Vec<&str> = prompts.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(p, vec!["triage_new_issues", "investigate_issue", "check_release"]);
    let args = json!({"org": "acme", "project": "web"}).as_object().cloned().unwrap();
    let g = c.get_prompt(GetPromptRequestParams::new("triage_new_issues").with_arguments(args)).await.unwrap();
    let g = serde_json::to_value(g).unwrap();
    assert!(g["messages"][0]["content"]["text"].as_str().unwrap().contains("search_issues"), "{g}");
    assert!(c.get_prompt(GetPromptRequestParams::new("check_release")).await.is_err(), "required arguments");

    // Unknown tools are a protocol error.
    assert!(c.call_tool(CallToolRequestParams::new("drop_database")).await.is_err());
    c.cancel().await.unwrap();

    // The newest protocol revision, without initialize.
    let t = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(w.url()).auth_header(w.token("viewer")),
    );
    let c = ClientConfig::default()
        .serve_with_lifecycle(
            t,
            ClientLifecycleMode::Discover { preferred_versions: vec![ProtocolVersion::V_2026_07_28] },
        )
        .await
        .expect("discover");
    assert_eq!(c.list_all_tools().await.unwrap().len(), 29);
    let orgs = ok(&c, "list_organizations", json!({})).await;
    assert_eq!(orgs["data"][0]["slug"], "acme");
    c.cancel().await.unwrap();
}

#[tokio::test]
async fn read_tools_answer_from_the_seeded_data() {
    let w = world().await;
    let c = w.client("member").await;
    let range = json!({"start": START, "end": END});
    let scoped = |extra: Value| {
        let mut v = range.clone();
        for (k, x) in extra.as_object().unwrap() {
            v[k] = x.clone();
        }
        v
    };

    // Organizations and projects.
    let orgs = ok(&c, "list_organizations", json!({})).await;
    assert_eq!(orgs["data"], json!([{"id": orgs["data"][0]["id"], "slug": "acme", "name": "Acme", "role": "member"}]));
    let projects = ok(&c, "list_projects", json!({"org": "acme"})).await;
    assert_eq!(projects["data"].as_array().unwrap().len(), 2);
    assert!(projects["data"][0].get("dsn").is_none(), "keys are not shown");
    let members = ok(&c, "list_members", json!({"org": "acme"})).await;
    let dev = find(&members["data"], "email", "dev@example.com");
    assert_eq!(dev["role"], "member");
    assert!(ok(&c, "list_teams", json!({"org": "acme"})).await["data"].is_array());

    // Issue search with the query language, filters and pages.
    let errors =
        ok(&c, "search_issues", json!({"org": "acme", "query": "is:unresolved level:error", "sort": "first_seen"}))
            .await;
    let titles: Vec<&str> = errors["data"].as_array().unwrap().iter().map(|i| i["title"].as_str().unwrap()).collect();
    assert!(titles.iter().any(|t| t.contains("smoke node error")), "{titles:?}");
    assert!(titles.iter().all(|t| !t.contains("message")), "warnings excluded: {titles:?}");
    let by_release = ok(&c, "search_issues", json!({"org": "acme", "query": "release:smoke-python@1.0.0"})).await;
    assert!(
        by_release["data"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["title"].as_str().unwrap().contains("python")
                || i["title"].as_str().unwrap().contains("division")),
        "{by_release}"
    );
    assert_eq!(ok(&c, "search_issues", json!({"org": "acme", "projects": "api"})).await["data"], json!([]));
    let p1 = ok(&c, "search_issues", json!({"org": "acme", "projects": ["web"], "limit": 1})).await;
    assert_eq!(p1["data"].as_array().unwrap().len(), 1);
    let p2 = ok(&c, "search_issues", json!({"org": "acme", "limit": 1, "cursor": p1["next_cursor"]})).await;
    assert_ne!(p1["data"][0]["id"], p2["data"][0]["id"]);
    let bad = fails(&c, "search_issues", json!({"org": "acme", "query": "is:sideways"})).await;
    assert!(bad.contains("argument query") && bad.contains("sideways"), "{bad}");
    assert!(fails(&c, "search_issues", json!({"org": "acme", "projects": "nope"})).await.contains("list_projects"));
    assert!(fails(&c, "search_issues", json!({})).await.contains("Invalid arguments"));

    // One issue: by id, short id and URL.
    let id = node_issue(&c).await;
    let issue = ok(&c, "get_issue", json!({"issue": id})).await;
    let short = issue["issue"]["short_id"].as_str().unwrap().to_string();
    assert!(short.starts_with("WEB-"));
    assert_eq!(issue["issue"]["level"], "error");
    assert_eq!(issue["issue"]["count"], 1);
    assert_eq!(issue["issue"]["first_release"], "smoke-node@1.0.0");
    let release_tag = find(&issue["tags"], "key", "release");
    assert_eq!(release_tag["top_values"][0]["value"], "smoke-node@1.0.0");
    assert_eq!(release_tag["top_values"][0]["percent"], 100.0);
    assert!(issue["linked_tickets"].as_array().unwrap().is_empty());
    assert!(issue["recent_activity"].is_array());
    assert_eq!(ok(&c, "get_issue", json!({"issue": short, "org": "acme"})).await["issue"]["id"], id.as_str());
    assert_eq!(ok(&c, "get_issue", json!({"issue": id.parse::<i64>().unwrap()})).await["issue"]["id"], id.as_str());
    let url = format!("{PUBLIC_URL}/organizations/acme/issues/{id}");
    assert_eq!(ok(&c, "get_issue", json!({"issue": url})).await["issue"]["id"], id.as_str());
    assert!(fails(&c, "get_issue", json!({"issue": short})).await.contains("pass org"));
    let values = ok(&c, "get_issue_tag_values", json!({"issue": id, "key": "environment"})).await;
    assert_eq!(values["data"], json!([{"value": "smoke", "count": 1}]));

    // Events and the readable stack trace.
    let events = ok(&c, "list_issue_events", json!({"issue": id})).await;
    let ev = &events["data"][0];
    assert_eq!(ev["release"], "smoke-node@1.0.0");
    let event_id = ev["event_id"].as_str().unwrap().to_string();
    let r = call(&c, "get_event", json!({"issue": id})).await;
    let t = text(&r);
    assert!(t.contains("Error: smoke node error"), "{t}");
    assert!(t.contains("Stack trace (most recent call last"), "{t}");
    assert!(t.contains("  * /srv/app/node.mjs:"), "in-app frame: {t}");
    assert!(t.contains("    > "), "source context: {t}");
    assert!(t.contains("library frame"), "collapsed library frames: {t}");
    assert!(t.contains("## Contexts"), "{t}");
    assert!(t.contains("release: smoke-node@1.0.0"), "{t}");
    assert!(t.contains("SDK: sentry.javascript.node"), "{t}");
    assert_eq!(r["structuredContent"]["event_id"], event_id.as_str());
    let trace_id = r["structuredContent"]["trace_id"].as_str().expect("the node error has a trace").to_string();
    let all = text(&call(&c, "get_event", json!({"issue": id, "event": "oldest", "all_frames": true})).await);
    assert!(all.contains("node:internal"), "{all}");
    let direct = call(&c, "get_event", json!({"org": "acme", "project": "web", "event": event_id})).await;
    assert!(text(&direct).contains("smoke node error"));
    assert!(fails(&c, "get_event", json!({"org": "acme", "project": "web"})).await.contains("event id"));
    let browser = ok(&c, "search_issues", json!({"org": "acme", "query": "\"smoke browser error\""})).await;
    let b = text(&call(&c, "get_event", json!({"issue": browser["data"][0]["id"], "event": "recommended"})).await);
    assert!(b.contains("## Breadcrumbs") && b.contains("## Request"), "{b}");
    let activity = ok(&c, "list_issue_activity", json!({"issue": id})).await;
    assert!(activity["data"].is_array());

    // Releases and health.
    let releases = ok(&c, "list_releases", json!({"org": "acme", "projects": "web"})).await;
    let rel = find(&releases["data"], "version", "smoke-node@1.0.0");
    assert!(rel["health"].is_object(), "{rel}");
    let detail = ok(&c, "get_release", json!({"org": "acme", "version": "smoke-node@1.0.0"})).await;
    assert_eq!(detail["version"], "smoke-node@1.0.0");
    assert!(detail["daily_sessions"].as_array().unwrap().iter().all(|d| d["sessions"].is_number()));
    let health = ok(&c, "get_project_health", json!({"org": "acme", "project": "web", "period": "7d"})).await;
    assert!(health["health"]["total_sessions"].as_i64().unwrap() >= 1, "{health}");
    assert!(health["series"][0]["time"].is_number(), "{health}");

    // Performance.
    let txs = ok(&c, "search_transactions", scoped(json!({"org": "acme"}))).await;
    let node = find(&txs["data"], "name", "smoke-node-transaction");
    assert_eq!(node["count"], 1);
    let sorted =
        ok(&c, "search_transactions", scoped(json!({"org": "acme", "sort": "p95", "ascending": true, "limit": 2})))
            .await;
    assert_eq!(sorted["data"].as_array().unwrap().len(), 2);
    let summary =
        ok(&c, "get_transaction_summary", scoped(json!({"org": "acme", "name": "smoke-node-transaction"}))).await;
    assert_eq!(summary["stats"]["count"], 1);
    assert!(summary.get("histogram").is_none());
    let tr = call(&c, "get_trace", json!({"org": "acme", "trace_id": trace_id})).await;
    let tt = text(&tr);
    assert!(tt.contains(&format!("Trace {trace_id}")), "{tt}");
    assert!(tt.contains("smoke node error"), "errors of the trace: {tt}");
    assert!(tr["structuredContent"]["span_count"].as_i64().unwrap() >= 1, "{tr}");
    assert!(fails(&c, "get_trace", json!({"org": "acme", "trace_id": "xyz"})).await.contains("32 hex"));
    let spans = ok(&c, "search_spans", scoped(json!({"org": "acme", "group_by": "op"}))).await;
    assert_eq!(spans["group_by"], "op");
    assert!(!spans["groups"].as_array().unwrap().is_empty(), "{spans}");
    let rows = ok(&c, "search_spans", scoped(json!({"org": "acme", "group_by": "none", "limit": 3}))).await;
    assert!(rows["spans"].as_array().unwrap().len() <= 3);

    // Logs and metrics.
    let logs = ok(&c, "search_logs", scoped(json!({"org": "acme", "query": "level:error"}))).await;
    assert!(!logs["data"].as_array().unwrap().is_empty());
    assert!(logs["data"].as_array().unwrap().iter().all(|l| l["level"] == "error"), "{logs}");
    let by_attr = ok(&c, "search_logs", scoped(json!({"org": "acme", "query": "smoke.kind:node", "limit": 1}))).await;
    assert_eq!(by_attr["data"].as_array().unwrap().len(), 1);
    let names = ok(&c, "list_metrics", scoped(json!({"org": "acme"}))).await;
    find(&names["data"], "name", "smoke.node.requests");
    let m = ok(
        &c,
        "query_metric",
        scoped(json!({"org": "acme", "name": "smoke.node.latency", "op": "max", "group_by": "route"})),
    )
    .await;
    assert!(!m["series"].as_array().unwrap().is_empty(), "{m}");
    assert!(
        fails(&c, "query_metric", scoped(json!({"org": "acme", "name": "x", "op": "median"})))
            .await
            .contains("Invalid arguments")
    );

    // Cron monitors.
    let monitors = ok(&c, "list_monitors", json!({"org": "acme", "project": "web"})).await;
    find(&monitors["data"], "slug", "smoke-node-cron");
    let checkins =
        ok(&c, "list_monitor_checkins", json!({"org": "acme", "project": "web", "monitor": "smoke-node-cron"})).await;
    assert!(!checkins["data"].as_array().unwrap().is_empty());
    assert!(
        fails(&c, "list_monitor_checkins", json!({"org": "acme", "project": "web", "monitor": "nope"}))
            .await
            .starts_with("Not found")
    );

    // Alert rules and firings.
    let conn = w.state.store.connect();
    let rule: i64 = conn
        .query_one(
            "INSERT INTO alert_rules (project_id, name, conditions) VALUES (1, 'New errors', '[{\"type\": \"first_seen\"}]') RETURNING id",
            &[],
        )
        .unwrap()
        .get(0);
    conn.execute("INSERT INTO alert_fires (rule_id, issue_id) VALUES ($1, $2)", &[&rule, &id.parse::<i64>().unwrap()])
        .unwrap();
    let rules = ok(&c, "list_alert_rules", json!({"org": "acme", "project": "web"})).await;
    assert_eq!(rules["data"][0]["name"], "New errors");
    let fires = ok(&c, "list_alert_firings", json!({"org": "acme", "project": "web", "rule": rule})).await;
    assert_eq!(fires["data"][0]["issue"]["id"], id.as_str());
    assert_eq!(fires["data"][0]["rule"]["name"], "New errors");

    // Integrations (none yet).
    assert_eq!(
        ok(&c, "list_integrations", json!({"org": "acme", "project": "web"})).await,
        json!({"data": [], "project_defaults": []})
    );

    // Issues and events as resources.
    let res = serde_json::to_value(
        c.read_resource(ReadResourceRequestParams::new(format!("rupta://issue/{id}"))).await.unwrap(),
    )
    .unwrap();
    let body: Value = serde_json::from_str(res["contents"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(body["issue"]["id"], id.as_str());
    assert_eq!(res["cacheScope"], "private");
    let res = serde_json::to_value(
        c.read_resource(ReadResourceRequestParams::new(format!("rupta://issue/{id}/event/latest"))).await.unwrap(),
    )
    .unwrap();
    assert!(res["contents"][0]["text"].as_str().unwrap().contains("Stack trace"));
    assert!(c.read_resource(ReadResourceRequestParams::new("rupta://issue/abc")).await.is_err());
    assert!(c.read_resource(ReadResourceRequestParams::new("https://example.com/")).await.is_err());
    c.cancel().await.unwrap();
}

#[tokio::test]
async fn write_tools_follow_scopes_and_roles() {
    let w = world().await;
    let member = w.client("member").await;
    let id = node_issue(&member).await;

    let r =
        ok(&member, "update_issue", json!({"issue": id, "status": "resolved", "resolve_in_next_release": true})).await;
    assert_eq!(r["issue"]["status"], "resolved");
    assert_eq!(r["issue"]["substatus"], "in_next_release");
    assert_eq!(r["issue"]["resolved_in_mode"], "next_release");
    let r = ok(
        &member,
        "update_issue",
        json!({"issue": id, "status": "resolved", "resolved_in_release": "smoke-node@1.0.1"}),
    )
    .await;
    assert_eq!(r["issue"]["resolved_in_release"], "smoke-node@1.0.1");
    let r = ok(&member, "update_issue", json!({"issue": id, "status": "ignored", "ignore_count": 100})).await;
    assert_eq!(r["issue"]["status"], "ignored");
    assert_eq!(r["issue"]["ignore_count"], 101, "stored as the absolute count");
    let r = ok(
        &member,
        "update_issue",
        json!({"issue": id, "status": "unresolved", "assignee": "me", "priority": "high", "bookmarked": true}),
    )
    .await;
    assert_eq!(r["issue"]["status"], "unresolved");
    assert_eq!(r["issue"]["assignee"]["email"], "dev@example.com");
    assert_eq!(r["issue"]["priority"], "high");
    assert_eq!(r["issue"]["is_bookmarked"], true);
    let r = ok(&member, "update_issue", json!({"issue": id, "assignee": "none"})).await;
    assert!(r["issue"]["assignee"].is_null());
    let bad = fails(&member, "update_issue", json!({"issue": id, "assignee": "user:999999"})).await;
    assert!(bad.contains("argument assignee"), "{bad}");
    assert!(
        fails(&member, "update_issue", json!({"issue": id, "ignore_count": 3})).await.contains("needs status ignored")
    );

    let c = ok(&member, "add_comment", json!({"issue": id, "text": "Looks like the payment client times out."})).await;
    assert_eq!(c["comment"]["kind"], "comment");
    let activity = ok(&member, "list_issue_activity", json!({"issue": id})).await;
    let kinds: Vec<&str> = activity["data"].as_array().unwrap().iter().map(|a| a["kind"].as_str().unwrap()).collect();
    assert!(kinds.contains(&"comment") && kinds.contains(&"status_change") && kinds.contains(&"assigned"), "{kinds:?}");
    assert_eq!(find(&activity["data"], "kind", "comment")["text"], "Looks like the payment client times out.");
    assert!(fails(&member, "add_comment", json!({"issue": id, "text": "  "})).await.contains("argument text"));

    // A viewer has the scope but not the role; bookmarks are per user and allowed.
    let viewer = w.client("viewer").await;
    let f = fails(&viewer, "update_issue", json!({"issue": id, "status": "resolved"})).await;
    assert!(f.starts_with("Forbidden"), "{f}");
    assert!(fails(&viewer, "add_comment", json!({"issue": id, "text": "hi"})).await.starts_with("Forbidden"));
    assert_eq!(
        ok(&viewer, "update_issue", json!({"issue": id, "bookmarked": true})).await["issue"]["is_bookmarked"],
        true
    );

    // A member whose token lacks issue:write can read but not change.
    let reader = w.client("reader").await;
    assert_eq!(ok(&reader, "get_issue", json!({"issue": id})).await["issue"]["id"], id.as_str());
    assert!(fails(&reader, "update_issue", json!({"issue": id, "status": "resolved"})).await.starts_with("Forbidden"));
    assert!(fails(&reader, "add_comment", json!({"issue": id, "text": "hi"})).await.starts_with("Forbidden"));
    // Without org:read the organization list is refused too.
    assert!(fails(&reader, "list_organizations", json!({})).await.starts_with("Forbidden"));

    // Organization tokens triage but cannot comment (comments need a user).
    let org = w.client("org").await;
    assert_eq!(
        ok(&org, "update_issue", json!({"issue": id, "status": "resolved"})).await["issue"]["status"],
        "resolved"
    );
    assert!(fails(&org, "add_comment", json!({"issue": id, "text": "hi"})).await.starts_with("Forbidden"));
    let status: String = w
        .state
        .store
        .connect()
        .query_one("SELECT status FROM issues WHERE id = $1", &[&id.parse::<i64>().unwrap()])
        .unwrap()
        .get(0);
    assert_eq!(status, "resolved");
}

/// A GitHub API that knows one repository and creates issue #17.
async fn github() -> (String, Arc<Mutex<Vec<String>>>) {
    let calls: Arc<Mutex<Vec<String>>> = Arc::default();
    let c = calls.clone();
    let app = axum::Router::new().fallback(move |method: axum::http::Method, uri: axum::http::Uri, body: axum::body::Bytes| {
        let c = c.clone();
        async move {
            c.lock().unwrap().push(format!("{method} {} {}", uri.path(), String::from_utf8_lossy(&body)));
            let issue = json!({"number": 17, "html_url": "https://github.com/acme/web/issues/17", "title": "t", "state": "open", "state_reason": null, "assignee": null});
            let (status, body) = match (method.as_str(), uri.path()) {
                ("GET", "/user") => (200, json!({"login": "rupta-bot", "name": null})),
                ("POST", "/repos/acme/web/issues") => (201, issue),
                ("GET", "/repos/acme/web/issues/17") | ("GET", "/repos/acme/web/issues/18") => (200, {
                    let mut i = issue;
                    if uri.path().ends_with("18") {
                        i["number"] = json!(18);
                        i["html_url"] = json!("https://github.com/acme/web/issues/18");
                    }
                    i
                }),
                _ => (404, json!({"message": "Not Found"})),
            };
            (StatusCode::from_u16(status).unwrap(), axum::Json(body))
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), calls)
}

#[tokio::test]
async fn tickets_are_created_and_linked_through_integrations() {
    let w = world().await;
    let (gh, calls) = github().await;
    let (s, integration) = w
        .rest(
            reqwest::Method::POST,
            "/api/v1/orgs/acme/integrations",
            "admin",
            Some(json!({"kind": "github", "name": "GitHub", "token": "github_pat_11ABCDEFG", "config": {"base_url": gh}})),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{integration}");
    let iid = integration["id"].as_str().unwrap().to_string();
    let (s, body) = w
        .rest(
            reqwest::Method::PUT,
            &format!("/api/v1/projects/acme/web/integrations/{iid}"),
            "admin",
            Some(json!({"target": "acme/web"})),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{body}");

    let member = w.client("member").await;
    let list = ok(&member, "list_integrations", json!({"org": "acme", "project": "web"})).await;
    assert_eq!(list["data"][0]["kind"], "github");
    assert!(list["data"][0].get("token_hint").is_none());
    assert_eq!(list["project_defaults"][0]["target"], "acme/web");

    let id = node_issue(&member).await;
    let viewer = w.client("viewer").await;
    assert!(
        fails(&viewer, "create_ticket", json!({"issue": id, "integration_id": iid})).await.starts_with("Forbidden")
    );
    assert!(calls.lock().unwrap().iter().all(|c| !c.starts_with("POST /repos")), "nothing reached the tracker");

    let t = ok(
        &member,
        "create_ticket",
        json!({"issue": id, "integration_id": iid, "title": "Fix the node error", "labels": ["bug"]}),
    )
    .await;
    assert_eq!(t["ticket"]["key"], "#17");
    assert_eq!(t["ticket"]["tracker"], "github");
    assert_eq!(t["ticket"]["url"], "https://github.com/acme/web/issues/17");
    let post = calls.lock().unwrap().iter().find(|c| c.starts_with("POST /repos/acme/web/issues")).cloned().unwrap();
    assert!(post.contains("Fix the node error") && post.contains("bug"), "{post}");
    let again = fails(&member, "create_ticket", json!({"issue": id, "integration_id": iid})).await;
    assert!(again.starts_with("Conflict"), "{again}");
    let issue = ok(&member, "get_issue", json!({"issue": id})).await;
    assert_eq!(issue["linked_tickets"][0]["key"], "#17");

    let other = ok(&member, "search_issues", json!({"org": "acme", "query": "\"smoke php error\""})).await;
    let pid = other["data"][0]["id"].as_str().unwrap().to_string();
    let l = ok(&member, "link_ticket", json!({"issue": pid, "integration_id": iid, "reference": "acme/web#18"})).await;
    assert_eq!(l["ticket"]["key"], "#18");
    assert!(
        fails(&member, "create_ticket", json!({"issue": pid, "integration_id": "999"}))
            .await
            .contains("argument integration_id")
    );
}

#[tokio::test]
async fn other_organizations_are_invisible() {
    let w = world().await;
    let c = w.client("member").await;
    let foreign = w.foreign_issue.to_string();
    for (tool, args) in [
        ("get_issue", json!({"issue": foreign})),
        ("get_event", json!({"issue": foreign})),
        ("list_issue_events", json!({"issue": foreign})),
        ("update_issue", json!({"issue": foreign, "status": "resolved"})),
        ("add_comment", json!({"issue": foreign, "text": "x"})),
        ("search_issues", json!({"org": "globex"})),
        ("list_projects", json!({"org": "globex"})),
        ("get_issue", json!({"issue": "SHOP-1", "org": "globex"})),
        ("search_logs", json!({"org": "globex"})),
        ("list_monitors", json!({"org": "globex", "project": "shop"})),
    ] {
        let e = fails(&c, tool, args.clone()).await;
        assert!(e.starts_with("Not found"), "{tool} {args}: {e}");
        assert!(!e.contains("Globex only") && !e.contains("globex secret"), "{e}");
    }
    assert!(c.read_resource(ReadResourceRequestParams::new(format!("rupta://issue/{foreign}"))).await.is_err());
    let status: String = w
        .state
        .store
        .connect()
        .query_one("SELECT status FROM issues WHERE id = $1", &[&w.foreign_issue])
        .unwrap()
        .get(0);
    assert_eq!(status, "unresolved");

    // The other organization's owner sees its issue, and nothing of acme.
    let o = w.client("outsider").await;
    assert_eq!(ok(&o, "get_issue", json!({"issue": foreign})).await["issue"]["title"], "Globex only");
    let orgs = ok(&o, "list_organizations", json!({})).await;
    assert_eq!(orgs["data"].as_array().unwrap().len(), 1);
    assert_eq!(orgs["data"][0]["slug"], "globex");
    let id = node_issue(&w.client("member").await).await;
    assert!(fails(&o, "get_issue", json!({"issue": id})).await.starts_with("Not found"));
}

/// A raw JSON-RPC POST.
async fn post(url: &str, token: Option<&str>, headers: &[(&str, &str)], body: Value) -> reqwest::Response {
    let mut r = reqwest::Client::new()
        .post(url)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .json(&body);
    if let Some(t) = token {
        r = r.bearer_auth(t);
    }
    for (k, v) in headers {
        r = r.header(*k, *v);
    }
    r.send().await.unwrap()
}

fn initialize() -> Value {
    json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "curl", "version": "1"}}})
}

#[tokio::test]
async fn authentication_is_required_and_advertised() {
    let state = ApiState::new(Store::open_in_memory().unwrap(), config());
    let conn = state.store.connect();
    let org = users::create_org(&conn, "acme", "Acme").unwrap();
    let uid = member(&state, org, "dev@example.com", Role::Member);
    let good = token(&state, Some(uid), None, &["project:read"]);
    let session = rupta_auth::session::create(&conn, uid, None, None).unwrap();
    let base = serve(&state, true).await;
    let url = format!("{base}/mcp");
    let metadata = format!("{PUBLIC_URL}/.well-known/oauth-protected-resource/mcp");

    let r = post(&url, None, &[], initialize()).await;
    assert_eq!(r.status(), 401);
    assert_eq!(r.headers()["www-authenticate"], format!("Bearer resource_metadata=\"{metadata}\""));
    let r = post(&url, Some("rpt_wrong"), &[], initialize()).await;
    assert_eq!(r.status(), 401);
    assert!(r.headers()["www-authenticate"].to_str().unwrap().contains("error=\"invalid_token\""));
    // A browser session does not count.
    let cookie = format!("{}={}", rupta_auth::session::SESSION_COOKIE, session.token);
    let r = post(&url, None, &[("cookie", &cookie), ("x-csrf-token", &session.csrf_token)], initialize()).await;
    assert_eq!(r.status(), 401);
    // A revoked token stops working.
    let revoked = apitoken::create(&conn, "old", Some(uid), None, &["project:read".to_string()], None).unwrap();
    assert_eq!(post(&url, Some(&revoked.token), &[], initialize()).await.status(), 200);
    conn.execute("DELETE FROM api_tokens WHERE id = $1", &[&revoked.id]).unwrap();
    assert_eq!(post(&url, Some(&revoked.token), &[], initialize()).await.status(), 401);
    // So does an expired one.
    let past = chrono::Utc::now() - chrono::Duration::minutes(1);
    let expired = apitoken::create(&conn, "exp", Some(uid), None, &["project:read".to_string()], Some(past)).unwrap();
    assert_eq!(post(&url, Some(&expired.token), &[], initialize()).await.status(), 401);

    let r = post(&url, Some(&good), &[], initialize()).await;
    assert_eq!(r.status(), 200);
    let body: Value = r.json().await.unwrap();
    assert_eq!(body["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(body["result"]["serverInfo"]["name"], "rupta");
    // Unknown tools are an invalid-params error.
    let r = post(
        &url,
        Some(&good),
        &[("mcp-protocol-version", "2025-06-18")],
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "nope", "arguments": {}}}),
    )
    .await;
    let body: Value = r.json().await.unwrap();
    assert_eq!(body["error"]["code"], -32602, "{body}");
    // No server-initiated stream.
    let r =
        reqwest::Client::new().get(&url).bearer_auth(&good).header("accept", "text/event-stream").send().await.unwrap();
    assert_eq!(r.status(), 405);

    // RFC 9728 metadata, at the root and with the resource path.
    for path in ["/.well-known/oauth-protected-resource", "/.well-known/oauth-protected-resource/mcp"] {
        let m: Value = reqwest::get(format!("{base}{path}")).await.unwrap().json().await.unwrap();
        assert_eq!(m["resource"], format!("{PUBLIC_URL}/mcp"));
        assert_eq!(m["bearer_methods_supported"], json!(["header"]));
        assert!(m["scopes_supported"].as_array().unwrap().contains(&json!("issue:write")));
        assert!(m.get("authorization_servers").is_none(), "no identity provider configured: {m}");
    }
    // With single sign-on configured, the provider is named.
    let mut cfg = config();
    cfg.oidc = Some(rupta_auth::oidc::OidcSettings {
        id: rupta_auth::oidc::INSTANCE_PROVIDER_ID.to_string(),
        name: "Zitadel".into(),
        issuer: "https://auth.example.com".into(),
        client_id: "rupta".into(),
        client_secret: Some("s".into()),
        scopes: vec!["openid".into()],
        role_claim: None,
        role_mapping: Vec::new(),
        default_role: Role::Member,
        jit_provisioning: true,
    });
    let sso = ApiState::new(Store::open_in_memory().unwrap(), cfg);
    let r = rupta_mcp::router(sso, true)
        .oneshot(Request::get("/.well-known/oauth-protected-resource").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let bytes = http_body(r).await;
    let m: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(m["authorization_servers"], json!(["https://auth.example.com"]));
}

async fn http_body(r: axum::response::Response) -> Vec<u8> {
    axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap().to_vec()
}

#[tokio::test]
async fn browser_origins_are_checked() {
    let state = ApiState::new(Store::open_in_memory().unwrap(), config());
    let org = users::create_org(&state.store.connect(), "acme", "Acme").unwrap();
    let uid = member(&state, org, "dev@example.com", Role::Member);
    let t = token(&state, Some(uid), None, &["project:read"]);
    let url = format!("{}/mcp", serve(&state, true).await);
    for (origin, status) in [
        (Some("http://evil.example"), 403),
        (Some("http://rupta.test.evil.example"), 403),
        (Some("null"), 403),
        (Some("http://rupta.test:8080"), 403),
        (Some("http://rupta.test"), 200),
        (Some("http://allowed.example:3000"), 200),
        (None, 200),
    ] {
        let headers: Vec<(&str, &str)> = origin.map(|o| vec![("origin", o)]).unwrap_or_default();
        let r = post(&url, Some(&t), &headers, initialize()).await;
        assert_eq!(r.status(), status, "origin {origin:?}");
    }
}

#[tokio::test]
async fn the_endpoint_can_be_turned_off() {
    let state = ApiState::new(Store::open_in_memory().unwrap(), config());
    let org = users::create_org(&state.store.connect(), "acme", "Acme").unwrap();
    let uid = member(&state, org, "dev@example.com", Role::Member);
    let t = token(&state, Some(uid), None, &["project:read"]);
    let base = serve(&state, false).await;
    let r = post(&format!("{base}/mcp"), Some(&t), &[], initialize()).await;
    assert_eq!(r.status(), 404);
    let r = reqwest::get(format!("{base}/.well-known/oauth-protected-resource")).await.unwrap();
    assert_eq!(r.status(), 404);
    // The REST API is unaffected.
    let r =
        reqwest::Client::new().get(format!("{base}/api/v1/orgs/acme/projects")).bearer_auth(&t).send().await.unwrap();
    assert_eq!(r.status(), 200);
}
