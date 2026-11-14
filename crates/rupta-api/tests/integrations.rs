//! Issue tracker integrations against local mock servers that answer like
//! the vendors' documented APIs: Jira Cloud REST v3, Jira Data Center REST
//! v2, the Linear GraphQL API, GitHub REST (2022-11-28) and GitLab REST v4.

mod common;

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode};
use base64::Engine as _;
use common::{As, TestApp, wait_for};
use rupta_core::role::Role;
use rupta_core::signal::IssueSignal;
use serde_json::{Value, json};

/// A request the mock received.
#[derive(Clone, Debug)]
struct Call {
    method: String,
    path: String,
    query: String,
    headers: HeaderMap,
    body: Value,
}

impl Call {
    fn header(&self, name: &str) -> &str {
        self.headers.get(name).and_then(|v| v.to_str().ok()).unwrap_or("")
    }
}

/// Status, extra headers, body (`Null` for an empty body).
type Reply = (u16, Vec<(&'static str, String)>, Value);
type Handler = Arc<dyn Fn(&Call) -> Reply + Send + Sync>;

#[derive(Clone, Default)]
struct Calls(Arc<Mutex<Vec<Call>>>);

impl Calls {
    fn all(&self) -> Vec<Call> {
        self.0.lock().unwrap().clone()
    }
    fn find(&self, method: &str, path_part: &str) -> Vec<Call> {
        self.all().into_iter().filter(|c| c.method == method && c.path.contains(path_part)).collect()
    }
}

/// A local server answering every request with `handler`.
async fn mock(handler: Handler) -> (String, Calls) {
    let calls = Calls::default();
    let c = calls.clone();
    let app = axum::Router::new().fallback(
        move |method: Method, uri: axum::http::Uri, headers: HeaderMap, body: axum::body::Bytes| {
            let (c, handler) = (c.clone(), handler.clone());
            async move {
                let call = Call {
                    method: method.to_string(),
                    path: uri.path().to_string(),
                    query: uri.query().unwrap_or_default().to_string(),
                    headers,
                    body: serde_json::from_slice(&body).unwrap_or(Value::Null),
                };
                let (status, hdrs, body) = handler(&call);
                c.0.lock().unwrap().push(call);
                let mut resp = axum::response::Response::builder().status(status);
                for (k, v) in hdrs {
                    resp = resp.header(k, v);
                }
                if body.is_null() {
                    resp.body(Body::empty()).unwrap()
                } else {
                    resp.header("content-type", "application/json").body(Body::from(body.to_string())).unwrap()
                }
            }
        },
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), calls)
}

fn app() -> TestApp {
    TestApp::with_config(|c| c.integrations_allow_private_hosts = true)
}

fn hmac_hex(secret: &str, body: &[u8]) -> String {
    rupta_api::integrations::hmac_hex(secret, body)
}

/// Sends a webhook to rupta.
async fn hook(app: &TestApp, path: &str, headers: &[(&str, String)], body: &Value) -> common::Resp {
    let raw = body.to_string();
    let mut b = Request::builder().method(Method::POST).uri(path).header("content-type", "application/json");
    for (k, v) in headers {
        b = b.header(*k, v);
    }
    app.send(b.body(Body::from(raw)).unwrap()).await
}

fn issue_status(app: &TestApp, issue: i64) -> String {
    app.conn().query_one("SELECT status FROM issues WHERE id = $1", &[&issue]).unwrap().get(0)
}

fn activity_kinds(app: &TestApp, issue: i64) -> Vec<String> {
    app.conn()
        .query("SELECT kind FROM issue_activity WHERE issue_id = $1 ORDER BY id", &[&issue])
        .unwrap()
        .iter()
        .map(|r| r.get(0))
        .collect()
}

/// An issue with one event carrying an exception with in-app frames.
fn issue_with_stack(app: &TestApp, project: i64) -> i64 {
    let issue = app.issue(project, "TypeError: x is undefined", "error");
    let eid = app.event(project, issue, chrono::Utc::now(), json!({"browser": "Chrome", "environment": "production"}));
    let payload = json!({"exception": {"values": [{"type": "TypeError", "value": "x is undefined", "stacktrace": {"frames": [
        {"function": "main", "filename": "src/main.js", "lineno": 3, "in_app": true},
        {"function": "vendor", "filename": "node_modules/v.js", "lineno": 1, "in_app": false},
        {"function": "render", "filename": "src/app.js", "lineno": 41, "colno": 7, "in_app": true, "context_line": "  x.y()"}
    ]}}]}});
    let data = zstd::encode_all(payload.to_string().as_bytes(), 3).unwrap();
    app.conn().execute("UPDATE event_payloads SET data = $1 WHERE event_id = $2", &[&data, eid.as_bytes()]).unwrap();
    issue
}

// ---------------------------------------------------------------- Jira

#[derive(Default)]
struct JiraState {
    status: String,
    category: String,
}

fn jira_handler(cloud: bool, st: Arc<Mutex<JiraState>>) -> Handler {
    let v = if cloud { "3" } else { "2" };
    let expected_auth = if cloud {
        format!("Basic {}", base64::engine::general_purpose::STANDARD.encode("bot@acme.io:jira-token-123456"))
    } else {
        "Bearer dc-pat-0123456789".to_string()
    };
    Arc::new(move |c: &Call| {
        if c.header("authorization") != expected_auth {
            return (401, vec![], json!({"errorMessages": ["You are not authenticated."], "errors": {}}));
        }
        let api = format!("/rest/api/{v}");
        let issue = |st: &JiraState| {
            json!({"id": "10042", "key": "OPS-42", "self": "x", "fields": {
                "summary": "[WEB-1] TypeError: x is undefined",
                "status": {"name": st.status, "statusCategory": {"key": st.category}},
                "assignee": {"displayName": "Dana"}}})
        };
        let mut s = st.lock().unwrap();
        match (c.method.as_str(), c.path.strip_prefix(&api).unwrap_or("")) {
            ("GET", "/myself") if cloud => {
                (200, vec![], json!({"accountId": "5b10ac8d82e05b22cc7d4ef5", "displayName": "Ops Bot"}))
            }
            ("GET", "/myself") => {
                (200, vec![], json!({"name": "opsbot", "key": "JIRAUSER1", "displayName": "Ops Bot"}))
            }
            ("GET", "/project/search") => (
                200,
                vec![],
                json!({"isLast": true, "startAt": 0, "values": [
                {"id": "10000", "key": "OPS", "name": "Operations", "issueTypes": [
                    {"id": "10001", "name": "Bug", "subtask": false}, {"id": "10003", "name": "Sub-task", "subtask": true}]}]}),
            ),
            ("GET", "/project") => (
                200,
                vec![],
                json!([{"id": "10000", "key": "OPS", "name": "Operations",
                "issueTypes": [{"id": "1", "name": "Bug", "subtask": false}]}]),
            ),
            ("GET", "/project/OPS") => (
                200,
                vec![],
                json!({"key": "OPS", "issueTypes": [{"id": "10002", "name": "Task", "subtask": false}, {"id": "10001", "name": "Bug", "subtask": false}]}),
            ),
            ("POST", "/issue") => {
                let f = &c.body["fields"];
                if f["project"]["key"] != "OPS" || f["summary"].as_str().is_none() || f["issuetype"].is_null() {
                    return (400, vec![], json!({"errorMessages": [], "errors": {"project": "project is required"}}));
                }
                s.status = "To Do".into();
                s.category = "new".into();
                (201, vec![], json!({"id": "10042", "key": "OPS-42", "self": "x"}))
            }
            ("GET", "/issue/OPS-42") | ("GET", "/issue/10042") => (200, vec![], issue(&s)),
            ("GET", "/issue/NOPE-1") => (
                404,
                vec![],
                json!({"errorMessages": ["Issue does not exist or you do not have permission to see it."]}),
            ),
            ("GET", "/issue/10042/transitions") => (
                200,
                vec![],
                json!({"transitions": [
                {"id": "11", "name": "Back to do", "to": {"name": "To Do", "statusCategory": {"key": "new"}}},
                {"id": "21", "name": "Start", "to": {"name": "In Progress", "statusCategory": {"key": "indeterminate"}}},
                {"id": "31", "name": "Close", "to": {"name": "Done", "statusCategory": {"key": "done"}}}]}),
            ),
            ("POST", "/issue/10042/transitions") => {
                match c.body["transition"]["id"].as_str() {
                    Some("31") => (s.status, s.category) = ("Done".into(), "done".into()),
                    Some("11") => (s.status, s.category) = ("To Do".into(), "new".into()),
                    _ => return (400, vec![], json!({"errorMessages": ["bad transition"]})),
                }
                (204, vec![], Value::Null)
            }
            ("POST", "/issue/10042/comment") => (201, vec![], json!({"id": "100"})),
            _ => (404, vec![], json!({"errorMessages": ["no such endpoint"]})),
        }
    })
}

fn jira_webhook(status: &str, category: &str) -> Value {
    json!({"timestamp": 1, "webhookEvent": "jira:issue_updated", "issue_event_type_name": "issue_generic",
           "issue": {"id": "10042", "key": "OPS-42", "fields": {"summary": "renamed", "status": {"name": status, "statusCategory": {"key": category}},
                     "assignee": {"displayName": "Lee"}}},
           "changelog": {"items": [{"field": "status", "toString": status}]}})
}

#[tokio::test]
async fn jira_cloud_tickets_and_two_way_sync() {
    let st = Arc::new(Mutex::new(JiraState::default()));
    let (base, calls) = mock(jira_handler(true, st.clone())).await;
    let app = app();
    let org = app.org("acme");
    let project = app.project(org, "web");
    let (_, admin) = app.member(org, "admin@example.com", Role::Admin);
    let issue = issue_with_stack(&app, project);

    // Wrong token: refused at creation with the tracker's message.
    let r = app
        .post(
            "/api/v1/orgs/acme/integrations",
            &admin,
            json!({"kind": "jira_cloud", "name": "Jira", "token": "nope",
            "config": {"base_url": base, "email": "bot@acme.io"}}),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", r.body);
    assert!(r.body["error"]["message"].as_str().unwrap().contains("not authenticated"), "{:?}", r.body);

    let r = app
        .post(
            "/api/v1/orgs/acme/integrations",
            &admin,
            json!({"kind": "jira_cloud", "name": "Jira", "token": "jira-token-123456",
            "config": {"base_url": format!("{base}/"), "email": "bot@acme.io", "resolve_transition": "Close"}}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let iid = r.body["id"].as_str().unwrap().to_string();
    let secret = r.body["webhook_secret"].as_str().unwrap().to_string();
    assert_eq!(r.body["token_hint"], "****3456");
    assert_eq!(r.body["account"]["name"], "Ops Bot");
    assert_eq!(r.body["webhook_url"], format!("http://rupta.test/api/integrations/jira_cloud/{iid}/webhook"));
    assert!(!r.bytes.windows(17).any(|w| w == b"jira-token-123456"), "the token is never returned");
    let got = app.get(&format!("/api/v1/orgs/acme/integrations/{iid}"), &admin).await;
    assert!(got.body["webhook_secret"].is_null() && got.body["has_webhook_secret"] == true);

    let t = app.get(&format!("/api/v1/orgs/acme/integrations/{iid}/targets"), &admin).await;
    assert_eq!(t.status, StatusCode::OK, "{:?}", t.body);
    assert_eq!(t.body[0]["id"], "OPS");
    assert_eq!(t.body[0]["issue_types"], json!([{"id": "10001", "name": "Bug"}]));
    let q = &calls.find("GET", "/project/search")[0].query;
    assert!(q.contains("expand=issueTypes") && q.contains("maxResults=50"), "{q}");

    let d = app
        .put(
            &format!("/api/v1/projects/acme/web/integrations/{iid}"),
            &admin,
            json!({"target": "OPS", "labels": ["from rupta"],
            "incoming_sync": true, "outgoing_transition": true, "outgoing_comment": true}),
        )
        .await;
    assert_eq!(d.status, StatusCode::OK, "{:?}", d.body);

    // Create a ticket with the generated description.
    let draft = app.get(&format!("/api/v1/issues/{issue}/ticket-draft"), &admin).await;
    assert!(draft.body["description"].as_str().unwrap().contains("at render (src/app.js:41:7)"), "{:?}", draft.body);
    let l = app.post(&format!("/api/v1/issues/{issue}/links"), &admin, json!({"integration_id": iid})).await;
    assert_eq!(l.status, StatusCode::CREATED, "{:?}", l.body);
    assert_eq!(l.body["external_key"], "OPS-42");
    assert_eq!(l.body["external_id"], "10042");
    assert_eq!(l.body["status"], "To Do");
    assert_eq!(l.body["status_category"], "open");
    assert_eq!(l.body["url"], format!("{base}/browse/OPS-42"));
    let link_id = l.body["id"].as_str().unwrap().to_string();
    let created = &calls.find("POST", "/rest/api/3/issue")[0];
    let fields = &created.body["fields"];
    assert_eq!(fields["issuetype"], json!({"id": "10001"}), "Bug is picked without a configured type");
    assert_eq!(fields["labels"], json!(["from-rupta"]));
    assert_eq!(fields["summary"], "[WEB-1] TypeError: x is undefined");
    assert_eq!(fields["description"]["type"], "doc");
    assert_eq!(fields["description"]["version"], 1);
    let adf = fields["description"].to_string();
    assert!(adf.contains("codeBlock") && adf.contains("at render (src/app.js:41:7)"), "{adf}");
    assert!(!adf.contains("vendor"), "only in-app frames");
    assert!(adf.contains(&format!("http://rupta.test/organizations/acme/issues/{issue}")), "{adf}");
    assert!(adf.contains("Chrome (100%)"), "{adf}");
    assert!(activity_kinds(&app, issue).contains(&"ticket_linked".to_string()));
    let again = app
        .post(
            &format!("/api/v1/issues/{issue}/links"),
            &admin,
            json!({"integration_id": iid, "mode": "link", "reference": "ops-42"}),
        )
        .await;
    assert_eq!(again.status, StatusCode::CONFLICT);
    let missing = app
        .post(
            &format!("/api/v1/issues/{issue}/links"),
            &admin,
            json!({"integration_id": iid, "mode": "link", "reference": "NOPE-1"}),
        )
        .await;
    assert_eq!(missing.status, StatusCode::UNPROCESSABLE_ENTITY);

    // Resolving in rupta comments and closes the ticket with the configured transition.
    let r = app.patch(&format!("/api/v1/issues/{issue}"), &admin, json!({"status": "resolved"})).await;
    assert_eq!(r.status, StatusCode::OK);
    wait_for(|| (!calls.find("POST", "/issue/10042/transitions").is_empty()).then_some(())).await;
    let comment = &calls.find("POST", "/issue/10042/comment")[0];
    assert_eq!(comment.body["body"]["type"], "doc");
    assert!(comment.body.to_string().contains("was resolved in rupta by Test User"), "{}", comment.body);
    assert_eq!(calls.find("POST", "/issue/10042/transitions")[0].body, json!({"transition": {"id": "31"}}));
    let links = wait_for(|| {
        let r = app
            .conn()
            .query_one("SELECT status_category FROM issue_links WHERE id = $1", &[&link_id.parse::<i64>().unwrap()])
            .unwrap();
        (r.get::<String>(0) == "done").then_some(())
    });
    links.await;

    // Verification.
    let path = format!("/api/integrations/jira_cloud/{iid}/webhook");
    let body = jira_webhook("To Do", "new");
    let r = hook(&app, &path, &[("x-hub-signature", "sha256=00".into())], &body).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let r = hook(&app, &format!("/api/integrations/github/{iid}/webhook"), &[], &body).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED, "wrong kind");
    let r = hook(&app, "/api/integrations/jira_cloud/999999/webhook", &[], &body).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED, "unknown integration");

    // Reopened in Jira within the quiet period after rupta's change: the link
    // follows, the issue does not.
    let sig = |b: &Value| format!("sha256={}", hmac_hex(&secret, b.to_string().as_bytes()));
    let r =
        hook(&app, &path, &[("x-hub-signature", sig(&body)), ("x-atlassian-webhook-identifier", "d-1".into())], &body)
            .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["updated_links"], 1);
    assert_eq!(issue_status(&app, issue), "resolved");
    // The same delivery again: no effect.
    let r =
        hook(&app, &path, &[("x-hub-signature", sig(&body)), ("x-atlassian-webhook-identifier", "d-1".into())], &body)
            .await;
    assert_eq!(r.body["duplicate"], true);

    // After the quiet period a reopen in Jira unresolves the issue, without
    // calling Jira back.
    app.conn()
        .execute("UPDATE issue_links SET pushed_until = now() - interval '1 minute', status_category = 'done'", &[])
        .unwrap();
    let before = calls.all().len();
    let r =
        hook(&app, &path, &[("x-hub-signature", sig(&body)), ("x-atlassian-webhook-identifier", "d-2".into())], &body)
            .await;
    assert_eq!(r.body["changed_issues"], 1, "{:?}", r.body);
    assert_eq!(issue_status(&app, issue), "unresolved");
    let links = app.get(&format!("/api/v1/issues/{issue}/links"), &admin).await;
    assert_eq!(links.body[0]["status"], "To Do");
    assert_eq!(links.body[0]["title"], "renamed");
    assert_eq!(links.body[0]["assignee"], "Lee");
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(calls.all().len(), before, "no echo to Jira");
    let acts = activity_kinds(&app, issue);
    assert!(
        acts.contains(&"ticket_status_change".to_string()) && acts.contains(&"ticket_sync".to_string()),
        "{acts:?}"
    );

    // Done in Jira resolves it; the query-string secret works for Jira versions without signing.
    let done = jira_webhook("Done", "done");
    let r = hook(&app, &format!("{path}?secret={secret}"), &[], &done).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(issue_status(&app, issue), "resolved");

    // Refresh and unlink.
    let r = app.post(&format!("/api/v1/issues/{issue}/links/{link_id}/refresh"), &admin, json!({})).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["assignee"], "Dana");
    let r = app.delete(&format!("/api/v1/issues/{issue}/links/{link_id}"), &admin).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert!(activity_kinds(&app, issue).contains(&"ticket_unlinked".to_string()));
    assert_eq!(app.get(&format!("/api/v1/issues/{issue}/links"), &admin).await.body, json!([]));

    let actions = app.audit_actions();
    assert!(
        actions.contains(&"integration.create".into())
            && actions.contains(&"integration.project_defaults_change".into())
    );
    let audit: String = app.conn().query_one("SELECT string_agg(data::text, ' ') FROM audit_log", &[]).unwrap().get(0);
    assert!(!audit.contains("jira-token") && !audit.contains(&secret), "no secrets in the audit log");
    assert!(
        app.state.metrics.get(
            "rupta_integration_requests_total",
            &[("kind", "jira_cloud"), ("operation", "create"), ("result", "success")]
        ) == 1
    );
}

#[tokio::test]
async fn jira_data_center_uses_v2_bearer_and_wiki_markup() {
    let st = Arc::new(Mutex::new(JiraState::default()));
    let (base, calls) = mock(jira_handler(false, st)).await;
    let app = app();
    let org = app.org("acme");
    let project = app.project(org, "web");
    let (_, admin) = app.member(org, "admin@example.com", Role::Admin);
    let issue = issue_with_stack(&app, project);
    let r = app
        .post(
            "/api/v1/orgs/acme/integrations",
            &admin,
            json!({"kind": "jira_server", "name": "Jira DC", "token": "dc-pat-0123456789",
            "config": {"base_url": base}}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["account"]["id"], "opsbot");
    let iid = r.body["id"].as_str().unwrap().to_string();
    let t = app.get(&format!("/api/v1/orgs/acme/integrations/{iid}/targets"), &admin).await;
    assert_eq!(t.body[0]["key"], "OPS");
    let l = app
        .post(
            &format!("/api/v1/issues/{issue}/links"),
            &admin,
            json!({"integration_id": iid, "target": "OPS", "issue_type": "Task"}),
        )
        .await;
    assert_eq!(l.status, StatusCode::CREATED, "{:?}", l.body);
    let created = &calls.find("POST", "/rest/api/2/issue")[0];
    assert_eq!(created.body["fields"]["issuetype"], json!({"name": "Task"}));
    let desc = created.body["fields"]["description"].as_str().expect("wiki markup is a string");
    assert!(desc.contains("{noformat}\nat render (src/app.js:41:7)"), "{desc}");
    assert!(desc.contains(&format!("|http://rupta.test/organizations/acme/issues/{issue}]")), "{desc}");
    let r = app.post(&format!("/api/v1/orgs/acme/integrations/{iid}/test"), &admin, json!({})).await;
    assert_eq!(r.body["ok"], true);
}

// -------------------------------------------------------------- Linear

fn linear_handler(state: Arc<Mutex<String>>) -> Handler {
    Arc::new(move |c: &Call| {
        if c.path != "/graphql" || c.method != "POST" {
            return (404, vec![], json!({}));
        }
        if c.header("authorization") != "lin_api_abcdefghijkl" {
            return (
                400,
                vec![],
                json!({"errors": [{"message": "Authentication required, not authenticated", "extensions": {"code": "AUTHENTICATION_ERROR"}}]}),
            );
        }
        let q = c.body["query"].as_str().unwrap_or_default();
        let vars = &c.body["variables"];
        let issue = |state: &str| {
            let (name, ty) = if state == "done" { ("Done", "completed") } else { ("Todo", "unstarted") };
            json!({"id": "2a9c8b1e-8a2b-4cbe-9d5f-0d4a7f3e1c22", "identifier": "ENG-42", "url": "https://linear.app/acme/issue/ENG-42/x",
                   "title": "t", "state": {"id": "s", "name": name, "type": ty}, "assignee": null})
        };
        let mut s = state.lock().unwrap();
        let data = if q.contains("viewer") {
            json!({"viewer": {"id": "u1", "name": "Lin Bot", "email": "bot@acme.io"}})
        } else if q.contains("query Targets") {
            json!({"teams": {"nodes": [{"id": "team-1", "key": "ENG", "name": "Engineering",
                "states": {"nodes": [{"id": "st-done", "name": "Done", "type": "completed", "position": 3},
                                     {"id": "st-todo", "name": "Todo", "type": "unstarted", "position": 1}]},
                "labels": {"nodes": [{"id": "lab-bug", "name": "Bug"}]},
                "projects": {"nodes": [{"id": "proj-1", "name": "Stability"}]}}]},
                "issueLabels": {"nodes": [{"id": "lab-ws", "name": "rupta"}]}})
        } else if q.contains("query Labels") {
            json!({"team": {"labels": {"nodes": [{"id": "lab-bug", "name": "Bug"}]}}, "issueLabels": {"nodes": [{"id": "lab-ws", "name": "rupta"}]}})
        } else if q.contains("issueCreate") {
            let input = &vars["input"];
            if input["teamId"] != "team-1" || input["title"].as_str().is_none() {
                return (
                    400,
                    vec![],
                    json!({"errors": [{"message": "Argument Validation Error", "extensions": {"code": "INVALID_INPUT"}}]}),
                );
            }
            *s = "open".into();
            json!({"issueCreate": {"success": true, "issue": issue(&s)}})
        } else if q.contains("query States") {
            json!({"issue": {"id": "2a9c8b1e-8a2b-4cbe-9d5f-0d4a7f3e1c22", "team": {"states": {"nodes": [
                {"id": "st-done", "name": "Done", "type": "completed", "position": 3},
                {"id": "st-todo", "name": "Todo", "type": "unstarted", "position": 1}]}}}})
        } else if q.contains("issueUpdate") {
            *s = if vars["input"]["stateId"] == "st-done" { "done".into() } else { "open".into() };
            json!({"issueUpdate": {"success": true, "issue": issue(&s)}})
        } else if q.contains("commentCreate") {
            json!({"commentCreate": {"success": true}})
        } else if q.contains("query Issue") {
            if vars["id"] == "ENG-404" {
                return (
                    200,
                    vec![],
                    json!({"data": null, "errors": [{"message": "Entity not found: Issue", "extensions": {"code": "INVALID_INPUT"}}]}),
                );
            }
            json!({"issue": issue(&s)})
        } else {
            return (400, vec![], json!({"errors": [{"message": "unknown query"}]}));
        };
        (200, vec![], json!({"data": data}))
    })
}

#[tokio::test]
async fn linear_graphql_tickets_and_signed_webhooks() {
    let (base, calls) = mock(linear_handler(Arc::default())).await;
    let app = app();
    let org = app.org("acme");
    let project = app.project(org, "web");
    let (_, admin) = app.member(org, "admin@example.com", Role::Admin);
    let issue = issue_with_stack(&app, project);
    let r = app
        .post(
            "/api/v1/orgs/acme/integrations",
            &admin,
            json!({"kind": "linear", "name": "Linear", "token": "lin_api_abcdefghijkl",
            "config": {"base_url": base}}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert!(r.body["webhook_secret"].is_null(), "Linear generates the signing secret");
    assert_eq!(r.body["has_webhook_secret"], false);
    let iid = r.body["id"].as_str().unwrap().to_string();
    let t = app.get(&format!("/api/v1/orgs/acme/integrations/{iid}/targets"), &admin).await;
    assert_eq!(t.body[0]["key"], "ENG");
    assert_eq!(t.body[0]["states"][0]["name"], "Todo", "ordered by position");
    assert_eq!(t.body[0]["states"][1]["category"], "done");
    assert_eq!(t.body[0]["labels"], json!([{"id": "lab-bug", "name": "Bug"}, {"id": "lab-ws", "name": "rupta"}]));
    assert_eq!(t.body[0]["projects"][0]["id"], "proj-1");

    app.put(&format!("/api/v1/projects/acme/web/integrations/{iid}"), &admin,
        json!({"target": "team-1", "project": "proj-1", "labels": ["bug", "rupta"], "incoming_sync": true, "outgoing_transition": true}))
        .await;
    let l = app.post(&format!("/api/v1/issues/{issue}/links"), &admin, json!({"integration_id": iid})).await;
    assert_eq!(l.status, StatusCode::CREATED, "{:?}", l.body);
    assert_eq!(l.body["external_key"], "ENG-42");
    let create = calls.all().into_iter().find(|c| c.body["query"].as_str().unwrap().contains("issueCreate")).unwrap();
    let input = &create.body["variables"]["input"];
    assert_eq!(input["labelIds"], json!(["lab-bug", "lab-ws"]), "label names are resolved to ids");
    assert_eq!(input["projectId"], "proj-1");
    assert!(
        input["description"]
            .as_str()
            .unwrap()
            .contains("```\nat render (src/app.js:41:7)\n    x.y()\nat main (src/main.js:3)\n```"),
        "{}",
        input["description"]
    );
    let bad = app
        .post(
            &format!("/api/v1/issues/{issue}/links"),
            &admin,
            json!({"integration_id": iid, "mode": "link", "reference": "ENG-404"}),
        )
        .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);

    // Linear webhooks are refused until the signing secret is entered.
    let path = format!("/api/integrations/linear/{iid}/webhook");
    let payload = |ty: &str, ts: i64| {
        json!({"action": "update", "type": "Issue", "webhookTimestamp": ts,
        "data": {"id": "2a9c8b1e-8a2b-4cbe-9d5f-0d4a7f3e1c22", "identifier": "ENG-42", "title": "t",
                 "state": {"id": "x", "name": if ty == "completed" { "Done" } else { "Todo" }, "type": ty}}})
    };
    let now = chrono::Utc::now().timestamp_millis();
    let body = payload("completed", now);
    assert_eq!(
        hook(&app, &path, &[("linear-signature", hmac_hex("lin_wh_secret", body.to_string().as_bytes()))], &body)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    let r = app
        .patch(&format!("/api/v1/orgs/acme/integrations/{iid}"), &admin, json!({"webhook_secret": "lin_wh_secret"}))
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["has_webhook_secret"], true);
    let r = hook(
        &app,
        &path,
        &[
            ("linear-signature", hmac_hex("lin_wh_secret", body.to_string().as_bytes())),
            ("linear-delivery", "a".into()),
        ],
        &body,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(issue_status(&app, issue), "resolved");
    let stale = payload("unstarted", now - 10 * 60 * 1000);
    let r = hook(&app, &path, &[("linear-signature", hmac_hex("lin_wh_secret", stale.to_string().as_bytes()))], &stale)
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED, "old timestamps are refused");

    // Unresolving in rupta moves the Linear issue to the first unstarted state.
    app.patch(&format!("/api/v1/issues/{issue}"), &admin, json!({"status": "unresolved"})).await;
    let upd =
        wait_for(|| calls.all().into_iter().find(|c| c.body["query"].as_str().unwrap_or("").contains("issueUpdate")))
            .await;
    assert_eq!(upd.body["variables"]["input"]["stateId"], "st-todo");
}

// -------------------------------------------------------------- GitHub

fn github_handler(state: Arc<Mutex<String>>, fail_first: Arc<Mutex<u32>>) -> Handler {
    Arc::new(move |c: &Call| {
        if c.header("authorization") != "Bearer github_pat_11ABCDEFG" {
            return (
                401,
                vec![],
                json!({"message": "Bad credentials", "documentation_url": "https://docs.github.com/rest"}),
            );
        }
        assert_eq!(c.header("x-github-api-version"), "2022-11-28");
        assert_eq!(c.header("accept"), "application/vnd.github+json");
        {
            let mut f = fail_first.lock().unwrap();
            if *f > 0 {
                *f -= 1;
                return (503, vec![("retry-after", "0".into())], json!({"message": "Service unavailable"}));
            }
        }
        let mut s = state.lock().unwrap();
        let issue = |s: &str| {
            json!({"number": 17, "html_url": "https://github.com/acme/web/issues/17", "title": "t", "state": s,
                                     "state_reason": if s == "closed" { json!("completed") } else { Value::Null }, "assignee": {"login": "octocat"}})
        };
        match (c.method.as_str(), c.path.as_str()) {
            ("GET", "/user") => (200, vec![], json!({"login": "rupta-bot", "name": null})),
            ("GET", "/user/repos") if !c.query.contains("page=2") => (
                200,
                vec![("link", "<http://other.invalid/user/repos?page=2>; rel=\"next\"".into())],
                json!([{"full_name": "acme/web", "has_issues": true, "archived": false}, {"full_name": "acme/old", "has_issues": true, "archived": true}]),
            ),
            ("POST", "/repos/acme/web/issues") => {
                *s = "open".into();
                (201, vec![], issue(&s))
            }
            ("GET", "/repos/acme/web/issues/17") => (200, vec![], issue(&s)),
            ("PATCH", "/repos/acme/web/issues/17") => {
                *s = c.body["state"].as_str().unwrap().to_string();
                (200, vec![], issue(&s))
            }
            ("POST", "/repos/acme/web/issues/17/comments") => (201, vec![], json!({"id": 1})),
            _ => (404, vec![], json!({"message": "Not Found"})),
        }
    })
}

#[tokio::test]
async fn github_issues_with_retries_and_signed_webhooks() {
    let state = Arc::new(Mutex::new(String::new()));
    let fail = Arc::new(Mutex::new(0));
    let (base, calls) = mock(github_handler(state.clone(), fail.clone())).await;
    let app = app();
    let org = app.org("acme");
    let project = app.project(org, "web");
    let (_, admin) = app.member(org, "admin@example.com", Role::Admin);
    let (_, member) = app.member(org, "dev@example.com", Role::Member);
    let issue = issue_with_stack(&app, project);

    // A 503 with Retry-After is retried.
    *fail.lock().unwrap() = 1;
    let r = app
        .post(
            "/api/v1/orgs/acme/integrations",
            &admin,
            json!({"kind": "github", "name": "GitHub", "token": "github_pat_11ABCDEFG",
            "config": {"base_url": base}}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(calls.find("GET", "/user").len(), 2);
    let iid = r.body["id"].as_str().unwrap().to_string();
    let secret = r.body["webhook_secret"].as_str().unwrap().to_string();

    // Members see integrations and targets but cannot change them.
    assert_eq!(app.get("/api/v1/orgs/acme/integrations", &member).await.body[0]["kind"], "github");
    let r = app.patch(&format!("/api/v1/orgs/acme/integrations/{iid}"), &member, json!({"name": "x"})).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let t = app.get(&format!("/api/v1/orgs/acme/integrations/{iid}/targets"), &member).await;
    assert_eq!(
        t.body,
        json!([{"id": "acme/web", "name": "acme/web", "key": null, "issue_types": [], "projects": [], "labels": [], "states": []}])
    );

    app.put(
        &format!("/api/v1/projects/acme/web/integrations/{iid}"),
        &admin,
        json!({"target": "acme/web", "incoming_sync": true, "outgoing_transition": true, "outgoing_comment": true}),
    )
    .await;

    // Link an existing issue by URL, as a member.
    let l = app
        .post(
            &format!("/api/v1/issues/{issue}/links"),
            &member,
            json!({"integration_id": iid, "mode": "link", "reference": "https://github.com/acme/web/issues/17"}),
        )
        .await;
    assert_eq!(l.status, StatusCode::CREATED, "{:?}", l.body);
    assert_eq!(l.body["external_key"], "#17");
    assert_eq!(l.body["external_id"], "acme/web#17");
    assert_eq!(l.body["assignee"], "octocat");

    // A closed issue in GitHub resolves the rupta issue.
    let path = format!("/api/integrations/github/{iid}/webhook");
    let body = json!({"action": "closed", "issue": {"number": 17, "html_url": "https://github.com/acme/web/issues/17", "title": "t",
        "state": "closed", "state_reason": "completed", "assignee": null}, "repository": {"full_name": "acme/web"}, "sender": {"login": "octocat"}});
    let sig = format!("sha256={}", hmac_hex(&secret, body.to_string().as_bytes()));
    let r = hook(
        &app,
        &path,
        &[("x-github-event", "issues".into()), ("x-github-delivery", "g1".into()), ("x-hub-signature", sig.clone())],
        &body,
    )
    .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED, "GitHub uses X-Hub-Signature-256");
    let r = hook(
        &app,
        &path,
        &[("x-github-event", "issues".into()), ("x-github-delivery", "g1".into()), ("x-hub-signature-256", sig)],
        &body,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(issue_status(&app, issue), "resolved");
    let ping = json!({"zen": "Keep it logically awesome."});
    let r = hook(
        &app,
        &path,
        &[
            ("x-github-event", "ping".into()),
            ("x-hub-signature-256", format!("sha256={}", hmac_hex(&secret, ping.to_string().as_bytes()))),
        ],
        &ping,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);

    // A regression reopens the ticket and says why.
    *state.lock().unwrap() = "closed".into();
    rupta_api::alerts::handle(&app.state, &IssueSignal::Regressed { project_id: project, issue_id: issue })
        .await
        .unwrap();
    let patch = wait_for(|| calls.find("PATCH", "/repos/acme/web/issues/17").into_iter().next()).await;
    assert_eq!(patch.body, json!({"state": "open"}));
    let comment = &calls.find("POST", "/issues/17/comments")[0];
    assert!(comment.body["body"].as_str().unwrap().contains("regressed in rupta"), "{}", comment.body);

    // Resolve in rupta closes as completed.
    app.conn().execute("UPDATE issue_links SET pushed_until = NULL", &[]).unwrap();
    wait_for(|| {
        let c: String = app.conn().query_one("SELECT status_category FROM issue_links", &[]).unwrap().get(0);
        (c == "open").then_some(())
    })
    .await;
    app.conn().execute("UPDATE issues SET status = 'unresolved' WHERE id = $1", &[&issue]).unwrap();
    app.patch(&format!("/api/v1/issues/{issue}"), &admin, json!({"status": "resolved"})).await;
    let closes = wait_for(|| {
        let p = calls.find("PATCH", "/repos/acme/web/issues/17");
        (p.len() == 2).then_some(p)
    })
    .await;
    assert_eq!(closes[1].body, json!({"state": "closed", "state_reason": "completed"}));
}

// -------------------------------------------------------------- GitLab

fn gitlab_handler() -> Handler {
    Arc::new(move |c: &Call| {
        if c.header("private-token") != "glpat-xxxxxxxxxxxxxxxx" {
            return (401, vec![], json!({"message": "401 Unauthorized"}));
        }
        let issue = |state: &str| {
            json!({"id": 9001, "iid": 5, "project_id": 7, "web_url": "https://git.example.com/group/app/-/issues/5",
            "title": "t", "state": state, "assignee": {"name": "Kim", "username": "kim"}, "references": {"short": "#5", "full": "group/app#5"}})
        };
        match (c.method.as_str(), c.path.as_str()) {
            ("GET", "/api/v4/user") => (200, vec![], json!({"username": "project_7_bot", "name": "rupta bot"})),
            ("GET", "/api/v4/projects") => (
                200,
                vec![("x-next-page", "".into())],
                json!([{"id": 7, "path_with_namespace": "group/app", "name_with_namespace": "Group / App"}]),
            ),
            ("POST", "/api/v4/projects/group%2Fapp/issues") => (201, vec![], issue("opened")),
            ("PUT", "/api/v4/projects/group%2Fapp/issues/5") => {
                let state = if c.body["state_event"] == "close" { "closed" } else { "opened" };
                (200, vec![], issue(state))
            }
            ("POST", "/api/v4/projects/group%2Fapp/issues/5/notes") => (201, vec![], json!({"id": 1})),
            _ => (404, vec![], json!({"message": "404 Not Found"})),
        }
    })
}

#[tokio::test]
async fn gitlab_issues_and_token_webhooks() {
    let (base, calls) = mock(gitlab_handler()).await;
    let app = app();
    let org = app.org("acme");
    let project = app.project(org, "web");
    let (_, admin) = app.member(org, "admin@example.com", Role::Admin);
    let issue = issue_with_stack(&app, project);
    let r = app
        .post(
            "/api/v1/orgs/acme/integrations",
            &admin,
            json!({"kind": "gitlab", "name": "GitLab", "token": "glpat-xxxxxxxxxxxxxxxx",
            "webhook_secret": "my-hook-token", "config": {"base_url": base}}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert!(r.body["webhook_secret"].is_null(), "a given secret is not echoed");
    let iid = r.body["id"].as_str().unwrap().to_string();
    let t = app.get(&format!("/api/v1/orgs/acme/integrations/{iid}/targets"), &admin).await;
    assert_eq!(t.body[0]["id"], "group/app");
    let q = &calls.find("GET", "/api/v4/projects")[0].query;
    assert!(q.contains("membership=true") && q.contains("with_issues_enabled=true"), "{q}");
    app.put(
        &format!("/api/v1/projects/acme/web/integrations/{iid}"),
        &admin,
        json!({"target": "group/app", "labels": ["bug", "rupta"],
        "incoming_sync": true, "outgoing_transition": true}),
    )
    .await;
    let l = app
        .post(
            &format!("/api/v1/issues/{issue}/links"),
            &admin,
            json!({"integration_id": iid, "title": "Custom title", "description": "See the logs."}),
        )
        .await;
    assert_eq!(l.status, StatusCode::CREATED, "{:?}", l.body);
    assert_eq!(l.body["external_id"], "group/app#5");
    assert_eq!(l.body["external_key"], "#5");
    let created = &calls.find("POST", "/issues")[0];
    assert_eq!(created.body["labels"], "bug,rupta");
    assert_eq!(created.body["title"], "Custom title");
    let desc = created.body["description"].as_str().unwrap();
    assert!(
        desc.starts_with("See the logs.")
            && desc.contains(&format!("http://rupta.test/organizations/acme/issues/{issue}")),
        "{desc}"
    );

    let path = format!("/api/integrations/gitlab/{iid}/webhook");
    let body = json!({"object_kind": "issue", "event_type": "issue", "project": {"id": 7, "path_with_namespace": "group/app"},
        "object_attributes": {"iid": 5, "title": "t", "state": "closed", "action": "close", "url": "https://git.example.com/group/app/-/issues/5"},
        "assignees": [{"name": "Kim"}]});
    assert_eq!(hook(&app, &path, &[("x-gitlab-token", "wrong".into())], &body).await.status, StatusCode::UNAUTHORIZED);
    let r = hook(
        &app,
        &path,
        &[
            ("x-gitlab-token", "my-hook-token".into()),
            ("x-gitlab-event", "Issue Hook".into()),
            ("idempotency-key", "k1".into()),
        ],
        &body,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(issue_status(&app, issue), "resolved");

    // Unresolve in rupta reopens.
    app.patch(&format!("/api/v1/issues/{issue}"), &admin, json!({"status": "unresolved"})).await;
    let put = wait_for(|| calls.find("PUT", "/issues/5").into_iter().next()).await;
    assert_eq!(put.body, json!({"state_event": "reopen"}));
}

// ------------------------------------------------------ guard and alerts

#[tokio::test]
async fn private_hosts_are_refused_unless_allowed() {
    let (base, calls) = mock(gitlab_handler()).await;
    let app = TestApp::new();
    let org = app.org("acme");
    let (_, admin) = app.member(org, "admin@example.com", Role::Admin);
    let r = app
        .post(
            "/api/v1/orgs/acme/integrations",
            &admin,
            json!({"kind": "gitlab", "name": "GitLab", "token": "glpat-xxxxxxxxxxxxxxxx",
            "config": {"base_url": base}}),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", r.body);
    assert!(
        r.body["error"]["message"].as_str().unwrap().contains("RUPTA_INTEGRATIONS_ALLOW_PRIVATE_HOSTS"),
        "{:?}",
        r.body
    );
    assert!(calls.all().is_empty(), "nothing was sent");
    let r = app
        .post(
            "/api/v1/orgs/acme/integrations",
            &admin,
            json!({"kind": "gitlab", "name": "GitLab", "token": "t", "validate": false,
            "config": {"base_url": "http://169.254.169.254"}}),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", r.body);
    let (_, viewer) = app.member(org, "viewer@example.com", Role::Viewer);
    let r =
        app.post("/api/v1/orgs/acme/integrations", &viewer, json!({"kind": "github", "name": "x", "token": "t"})).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = app
        .post("/api/v1/orgs/acme/integrations", &admin, json!({"kind": "bugzilla", "name": "x", "token": "t"}))
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let _ = As::anonymous();
}

#[tokio::test]
async fn alert_rules_create_one_ticket_per_issue() {
    let state = Arc::new(Mutex::new(String::new()));
    let (base, calls) = mock(github_handler(state, Arc::default())).await;
    let app = app();
    let org = app.org("acme");
    let project = app.project(org, "web");
    let (_, admin) = app.member(org, "admin@example.com", Role::Admin);
    let issue = issue_with_stack(&app, project);
    let r = app
        .post(
            "/api/v1/orgs/acme/integrations",
            &admin,
            json!({"kind": "github", "name": "GitHub", "token": "github_pat_11ABCDEFG",
            "config": {"base_url": base}}),
        )
        .await;
    let iid = r.body["id"].as_str().unwrap().to_string();
    let bad = app
        .post(
            "/api/v1/projects/acme/web/alert-rules",
            &admin,
            json!({"name": "r", "conditions": [{"type": "first_seen"}],
            "actions": [{"type": "create_ticket", "integration_id": iid}]}),
        )
        .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY, "no target and no defaults");
    let rule = app
        .post("/api/v1/projects/acme/web/alert-rules", &admin, json!({"name": "r", "conditions": [{"type": "first_seen"}, {"type": "regression"}],
            "frequency_minutes": 1, "actions": [{"type": "create_ticket", "integration_id": iid, "target": "acme/web", "labels": ["alert"]}]}))
        .await;
    assert_eq!(rule.status, StatusCode::CREATED, "{:?}", rule.body);
    assert_eq!(rule.body["actions"][0]["labels"], json!(["alert"]));
    rupta_api::alerts::handle(&app.state, &IssueSignal::New { project_id: project, issue_id: issue }).await.unwrap();
    assert_eq!(calls.find("POST", "/repos/acme/web/issues").len(), 1);
    assert_eq!(calls.find("POST", "/repos/acme/web/issues")[0].body["labels"], json!(["alert"]));
    app.conn().execute("DELETE FROM alert_fires", &[]).unwrap();
    rupta_api::alerts::handle(&app.state, &IssueSignal::New { project_id: project, issue_id: issue }).await.unwrap();
    assert_eq!(calls.find("POST", "/repos/acme/web/issues").len(), 1, "once per issue");
    let links = app.get(&format!("/api/v1/issues/{issue}/links"), &admin).await;
    assert_eq!(links.body.as_array().unwrap().len(), 1);
    assert!(links.body[0]["created_by"].is_null());
}

#[tokio::test]
async fn deleting_an_integration_removes_its_links() {
    let (base, _) = mock(gitlab_handler()).await;
    let app = app();
    let org = app.org("acme");
    let project = app.project(org, "web");
    let (_, admin) = app.member(org, "admin@example.com", Role::Admin);
    let issue = issue_with_stack(&app, project);
    let r = app
        .post(
            "/api/v1/orgs/acme/integrations",
            &admin,
            json!({"kind": "gitlab", "name": "GitLab", "token": "glpat-xxxxxxxxxxxxxxxx",
            "config": {"base_url": base}}),
        )
        .await;
    let iid = r.body["id"].as_str().unwrap().to_string();
    let l = app
        .post(&format!("/api/v1/issues/{issue}/links"), &admin, json!({"integration_id": iid, "target": "group/app"}))
        .await;
    assert_eq!(l.status, StatusCode::CREATED);
    let r = app.delete(&format!("/api/v1/orgs/acme/integrations/{iid}"), &admin).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(app.get(&format!("/api/v1/issues/{issue}/links"), &admin).await.body, json!([]));
    assert!(app.audit_actions().contains(&"integration.delete".into()));
    let other = app.org("other");
    let (_, outsider) = app.member(other, "out@example.com", Role::Owner);
    assert_eq!(app.get(&format!("/api/v1/issues/{issue}/links"), &outsider).await.status, StatusCode::NOT_FOUND);
}
