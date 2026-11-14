//! Jira Cloud (platform REST API v3: basic auth with the account email
//! and an API token, descriptions in Atlassian Document Format) and Jira
//! Data Center / Server (REST API v2, as in Jira Data Center 9 and 10:
//! personal access token as bearer, descriptions in wiki markup).
//!
//! Endpoints used: `GET myself`, `GET project/search?expand=issueTypes`
//! (Cloud) or `GET project?expand=issueTypes` (Data Center), `GET
//! project/{key}`, `POST issue`, `GET issue/{idOrKey}?fields=…`, `POST
//! issue/{idOrKey}/comment`, `GET`/`POST issue/{idOrKey}/transitions`.
//!
//! Webhooks: Jira's admin webhooks (Cloud and Data Center 10) sign the
//! body with the secret set on the webhook, `X-Hub-Signature:
//! sha256=<hex HMAC-SHA256>`. For Jira versions without webhook secrets,
//! the secret can be passed as `?secret=` in the webhook URL instead.

use base64::Engine as _;
use reqwest::Method;
use serde_json::{Value, json};

use super::http::{Outbound, Req, snippet};
use super::{
    Account, Category, Choice, Doc, IssueTracker, Settings, Target, Ticket, TicketDraft, TicketTarget, TicketUpdate,
    TrackerError, WebhookError, WebhookParsed, WebhookRequest, join, status_error, verify_hmac,
};

pub struct Jira {
    cloud: bool,
    base: url::Url,
    auth: String,
    http: Outbound,
    resolve_transition: Option<String>,
    reopen_transition: Option<String>,
}

/// Jira's error message from an error answer.
fn jira_message(body: &[u8]) -> String {
    let Ok(v) = serde_json::from_slice::<Value>(body) else { return snippet(body) };
    let mut parts: Vec<String> =
        v["errorMessages"].as_array().into_iter().flatten().filter_map(|m| m.as_str().map(str::to_string)).collect();
    if let Some(errs) = v["errors"].as_object() {
        parts.extend(errs.iter().map(|(k, m)| format!("{k}: {}", m.as_str().unwrap_or(&m.to_string()))));
    }
    if parts.is_empty() {
        if let Some(m) = v["message"].as_str() {
            return m.to_string();
        }
        return snippet(body);
    }
    parts.join("; ")
}

/// `PROJ-123` (case-insensitive) as upper case.
fn issue_key(s: &str) -> Option<String> {
    let s = s.trim();
    let (p, n) = s.rsplit_once('-')?;
    let ok = !p.is_empty()
        && p.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !n.is_empty()
        && n.chars().all(|c| c.is_ascii_digit());
    ok.then(|| s.to_ascii_uppercase())
}

fn category_of(status: &Value) -> Category {
    if status["statusCategory"]["key"].as_str() == Some("done") { Category::Done } else { Category::Open }
}

impl Jira {
    pub fn new(s: &Settings, http: Outbound) -> Result<Jira, TrackerError> {
        let cloud = s.kind == super::Kind::JiraCloud;
        let base = s.base_url()?;
        let auth = if cloud {
            let email =
                s.str("email").ok_or_else(|| TrackerError::Invalid("email is required for Jira Cloud".into()))?;
            format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("{email}:{}", s.token)))
        } else {
            format!("Bearer {}", s.token)
        };
        Ok(Jira {
            cloud,
            base,
            auth,
            http,
            resolve_transition: s.str("resolve_transition"),
            reopen_transition: s.str("reopen_transition"),
        })
    }

    fn api(&self, segments: &[&str]) -> url::Url {
        let mut all = vec!["rest", "api", if self.cloud { "3" } else { "2" }];
        all.extend_from_slice(segments);
        join(&self.base, &all)
    }

    async fn call(&self, req: Req) -> Result<Value, TrackerError> {
        let req = req.header("authorization", self.auth.clone()).header("accept", "application/json");
        let resp = self.http.send(req).await?;
        if !(200..300).contains(&resp.status) {
            return Err(status_error(resp.status, jira_message(&resp.body)));
        }
        resp.json()
    }

    fn browse_url(&self, key: &str) -> String {
        join(&self.base, &["browse", key]).to_string()
    }

    fn ticket(&self, v: &Value) -> Result<Ticket, TrackerError> {
        let key = v["key"].as_str().ok_or_else(|| TrackerError::Invalid("the issue has no key".into()))?;
        let f = &v["fields"];
        Ok(Ticket {
            external_id: v["id"].as_str().map(str::to_string).unwrap_or_else(|| key.to_string()),
            key: key.to_string(),
            url: self.browse_url(key),
            title: f["summary"].as_str().unwrap_or_default().to_string(),
            status: f["status"]["name"].as_str().unwrap_or_default().to_string(),
            category: category_of(&f["status"]),
            assignee: f["assignee"]["displayName"].as_str().map(str::to_string),
        })
    }

    fn body(&self, doc: &Doc) -> Value {
        if self.cloud { doc.adf() } else { json!(doc.wiki()) }
    }

    /// The issue type to use: the configured one (an id when numeric,
    /// otherwise a name), else `Bug`, else the project's first type.
    async fn issue_type(&self, project: &str, wanted: Option<&str>) -> Result<Value, TrackerError> {
        if let Some(w) = wanted.map(str::trim).filter(|w| !w.is_empty()) {
            return Ok(if w.chars().all(|c| c.is_ascii_digit()) { json!({"id": w}) } else { json!({"name": w}) });
        }
        let p = self.call(Req::new(Method::GET, self.api(&["project", project]))).await?;
        let types: Vec<&Value> = p["issueTypes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|t| !t["subtask"].as_bool().unwrap_or(false))
            .collect();
        let pick = types
            .iter()
            .find(|t| t["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case("bug")))
            .or(types.first())
            .ok_or_else(|| TrackerError::Invalid(format!("the Jira project {project} has no issue types")))?;
        Ok(json!({"id": pick["id"]}))
    }
}

fn project_target(p: &Value) -> Option<Target> {
    let key = p["key"].as_str()?;
    let mut t = Target::simple(key.to_string(), p["name"].as_str().unwrap_or(key).to_string());
    t.key = Some(key.to_string());
    t.issue_types = p["issueTypes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|it| !it["subtask"].as_bool().unwrap_or(false))
        .filter_map(|it| Some(Choice { id: it["id"].as_str()?.to_string(), name: it["name"].as_str()?.to_string() }))
        .collect();
    Some(t)
}

impl IssueTracker for Jira {
    async fn validate(&self) -> Result<Account, TrackerError> {
        let v = self.call(Req::new(Method::GET, self.api(&["myself"]))).await?;
        let id = v["accountId"].as_str().or(v["name"].as_str()).or(v["key"].as_str()).unwrap_or_default();
        Ok(Account { id: id.to_string(), name: v["displayName"].as_str().unwrap_or(id).to_string() })
    }

    async fn targets(&self) -> Result<Vec<Target>, TrackerError> {
        let mut out = Vec::new();
        if self.cloud {
            let mut start = 0;
            for _ in 0..20 {
                let mut u = self.api(&["project", "search"]);
                u.query_pairs_mut()
                    .append_pair("expand", "issueTypes")
                    .append_pair("maxResults", "50")
                    .append_pair("startAt", &start.to_string())
                    .append_pair("orderBy", "key");
                let v = self.call(Req::new(Method::GET, u)).await?;
                let values = v["values"].as_array().cloned().unwrap_or_default();
                out.extend(values.iter().filter_map(project_target));
                start += values.len();
                if v["isLast"].as_bool().unwrap_or(true) || values.is_empty() {
                    break;
                }
            }
        } else {
            let mut u = self.api(&["project"]);
            u.query_pairs_mut().append_pair("expand", "issueTypes");
            let v = self.call(Req::new(Method::GET, u)).await?;
            out.extend(v.as_array().into_iter().flatten().filter_map(project_target));
        }
        Ok(out)
    }

    async fn create_ticket(&self, target: &TicketTarget, draft: &TicketDraft) -> Result<Ticket, TrackerError> {
        let project = target.target.trim();
        if project.is_empty() {
            return Err(TrackerError::Invalid("a Jira project key is required".into()));
        }
        let issuetype = self.issue_type(project, target.issue_type.as_deref()).await?;
        // Jira labels cannot contain spaces.
        let labels: Vec<String> =
            target.labels.iter().map(|l| l.trim().replace(' ', "-")).filter(|l| !l.is_empty()).collect();
        let mut fields = json!({
            "project": {"key": project},
            "issuetype": issuetype,
            "summary": super::truncate(&draft.title, 255),
            "description": self.body(&draft.body),
        });
        if !labels.is_empty() {
            fields["labels"] = json!(labels);
        }
        let created = self.call(Req::new(Method::POST, self.api(&["issue"])).json(&json!({"fields": fields}))).await?;
        let key = created["key"].as_str().ok_or_else(|| TrackerError::Invalid("Jira returned no issue key".into()))?;
        match self.get_ticket(key).await {
            Ok(t) => Ok(t),
            Err(_) => Ok(Ticket {
                external_id: created["id"].as_str().unwrap_or(key).to_string(),
                key: key.to_string(),
                url: self.browse_url(key),
                title: draft.title.clone(),
                status: String::new(),
                category: Category::Open,
                assignee: None,
            }),
        }
    }

    async fn get_ticket(&self, id: &str) -> Result<Ticket, TrackerError> {
        let mut u = self.api(&["issue", id.trim()]);
        u.query_pairs_mut().append_pair("fields", "summary,status,assignee");
        let v = self.call(Req::new(Method::GET, u)).await?;
        self.ticket(&v)
    }

    async fn add_comment(&self, id: &str, body: &Doc) -> Result<(), TrackerError> {
        self.call(Req::new(Method::POST, self.api(&["issue", id, "comment"])).json(&json!({"body": self.body(body)})))
            .await
            .map(|_| ())
    }

    async fn transition(&self, id: &str, to: Category) -> Result<Option<Ticket>, TrackerError> {
        let v = self.call(Req::new(Method::GET, self.api(&["issue", id, "transitions"]))).await?;
        let transitions = v["transitions"].as_array().cloned().unwrap_or_default();
        let configured = match to {
            Category::Done => self.resolve_transition.as_deref(),
            Category::Open => self.reopen_transition.as_deref(),
        };
        let pick = match configured {
            Some(name) => transitions.iter().find(|t| {
                t["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name))
                    || t["to"]["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name))
            }),
            None => match to {
                Category::Done => transitions.iter().find(|t| category_of(&t["to"]) == Category::Done),
                Category::Open => transitions
                    .iter()
                    .find(|t| t["to"]["statusCategory"]["key"].as_str() == Some("new"))
                    .or_else(|| transitions.iter().find(|t| category_of(&t["to"]) == Category::Open)),
            },
        };
        let Some(pick) = pick else { return Ok(None) };
        let tid = pick["id"].as_str().map(str::to_string).unwrap_or_else(|| pick["id"].to_string());
        self.call(
            Req::new(Method::POST, self.api(&["issue", id, "transitions"])).json(&json!({"transition": {"id": tid}})),
        )
        .await?;
        self.get_ticket(id).await.map(Some)
    }

    fn parse_reference(&self, input: &str, _default_target: Option<&str>) -> Option<String> {
        let input = input.trim();
        if let Some(k) = issue_key(input) {
            return Some(k);
        }
        let u = url::Url::parse(input).ok()?;
        if let Some((_, v)) = u.query_pairs().find(|(k, _)| k == "selectedIssue") {
            return issue_key(&v);
        }
        let segs: Vec<&str> = u.path_segments()?.collect();
        let i = segs.iter().position(|s| *s == "browse")?;
        issue_key(segs.get(i + 1)?)
    }

    fn parse_webhook(&self, req: &WebhookRequest, secret: Option<&str>) -> Result<WebhookParsed, WebhookError> {
        let secret = secret.filter(|s| !s.is_empty()).ok_or(WebhookError::Unauthorized)?;
        let signed = req
            .header("x-hub-signature")
            .and_then(|s| s.strip_prefix("sha256="))
            .is_some_and(|sig| verify_hmac(secret, req.body, sig));
        let by_query =
            req.query_param("secret").is_some_and(|q| rupta_auth::crypto::ct_eq(q.as_bytes(), secret.as_bytes()));
        if !signed && !by_query {
            return Err(WebhookError::Unauthorized);
        }
        let v: Value = serde_json::from_slice(req.body).map_err(|e| WebhookError::Invalid(format!("not JSON: {e}")))?;
        let mut out = WebhookParsed {
            delivery_id: req.header("x-atlassian-webhook-identifier").map(str::to_string),
            updates: Vec::new(),
        };
        let event = v["webhookEvent"].as_str().unwrap_or_default();
        let issue = &v["issue"];
        let (Some(id), Some(key)) = (issue["id"].as_str(), issue["key"].as_str()) else { return Ok(out) };
        let f = &issue["fields"];
        match event {
            "jira:issue_deleted" => out.updates.push(TicketUpdate {
                external_id: id.to_string(),
                key: Some(key.to_string()),
                url: None,
                title: None,
                status: None,
                category: None,
                assignee: None,
                deleted: true,
            }),
            "jira:issue_created" | "jira:issue_updated" => out.updates.push(TicketUpdate {
                external_id: id.to_string(),
                key: Some(key.to_string()),
                url: Some(self.browse_url(key)),
                title: f["summary"].as_str().map(str::to_string),
                status: f["status"]["name"].as_str().map(str::to_string),
                category: f["status"].is_object().then(|| category_of(&f["status"])),
                assignee: f.get("assignee").map(|a| a["displayName"].as_str().map(str::to_string)),
                deleted: false,
            }),
            _ => {}
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jira(cloud: bool) -> Jira {
        let s = Settings {
            kind: if cloud { super::super::Kind::JiraCloud } else { super::super::Kind::JiraServer },
            config: json!({"base_url": "https://acme.atlassian.net/", "email": "a@b.c"}),
            token: "tok".into(),
        };
        Jira::new(&s, Outbound::new(true)).unwrap()
    }

    #[test]
    fn auth_headers() {
        assert_eq!(jira(true).auth, format!("Basic {}", base64::engine::general_purpose::STANDARD.encode("a@b.c:tok")));
        assert_eq!(jira(false).auth, "Bearer tok");
        assert_eq!(jira(true).api(&["myself"]).as_str(), "https://acme.atlassian.net/rest/api/3/myself");
        assert_eq!(jira(false).api(&["myself"]).as_str(), "https://acme.atlassian.net/rest/api/2/myself");
    }

    #[test]
    fn references() {
        let j = jira(true);
        assert_eq!(j.parse_reference("proj-12", None).as_deref(), Some("PROJ-12"));
        assert_eq!(j.parse_reference("https://acme.atlassian.net/browse/OPS-7", None).as_deref(), Some("OPS-7"));
        assert_eq!(
            j.parse_reference(
                "https://acme.atlassian.net/jira/software/projects/OPS/boards/1?selectedIssue=OPS-9",
                None
            )
            .as_deref(),
            Some("OPS-9")
        );
        assert_eq!(j.parse_reference("12", None), None);
        assert_eq!(j.parse_reference("-12", None), None);
    }

    #[test]
    fn error_messages() {
        assert_eq!(
            jira_message(br#"{"errorMessages":["Issue does not exist"],"errors":{"summary":"required"}}"#),
            "Issue does not exist; summary: required"
        );
    }
}
