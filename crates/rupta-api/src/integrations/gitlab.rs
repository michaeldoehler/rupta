//! GitLab Issues through the REST API v4, on gitlab.com or a self-managed
//! instance (`base_url`). Personal, project and group access tokens with
//! the `api` scope are sent as `PRIVATE-TOKEN`.
//!
//! Endpoints: `GET /api/v4/user`, `GET /api/v4/projects`, `POST
//! /api/v4/projects/:id/issues`, `GET`/`PUT
//! /api/v4/projects/:id/issues/:iid`, `POST
//! /api/v4/projects/:id/issues/:iid/notes`, where `:id` is the
//! URL-encoded project path.
//!
//! Webhooks (issue events): the secret token is sent as `X-Gitlab-Token`;
//! `Idempotency-Key` (GitLab 17.4 and later) or `X-Gitlab-Event-UUID`
//! identifies the delivery.

use reqwest::Method;
use serde_json::{Value, json};

use super::http::{Outbound, Req, Resp, snippet};
use super::{
    Account, Category, Doc, IssueTracker, Settings, Target, Ticket, TicketDraft, TicketTarget, TicketUpdate,
    TrackerError, WebhookError, WebhookParsed, WebhookRequest, join, status_error,
};

pub struct Gitlab {
    api: url::Url,
    token: String,
    http: Outbound,
}

/// `group/sub/project#17` into path and iid.
fn split_ref(id: &str) -> Option<(String, u64)> {
    let (path, n) = id.trim().rsplit_once('#')?;
    let n: u64 = n.parse().ok().filter(|n| *n > 0)?;
    let ok = path.contains('/')
        && path
            .split('/')
            .all(|s| !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.')));
    ok.then(|| (path.to_string(), n))
}

fn gitlab_message(body: &[u8]) -> String {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| {
            let m = v.get("message").or_else(|| v.get("error"))?;
            Some(m.as_str().map(str::to_string).unwrap_or_else(|| m.to_string()))
        })
        .unwrap_or_else(|| snippet(body))
}

fn ticket(path: &str, v: &Value) -> Result<Ticket, TrackerError> {
    let iid = v["iid"].as_u64().ok_or_else(|| TrackerError::Invalid("GitLab returned no issue iid".into()))?;
    let full = v["references"]["full"].as_str().map(str::to_string).unwrap_or_else(|| format!("{path}#{iid}"));
    let state = v["state"].as_str().unwrap_or("opened");
    let assignee = v["assignee"]["name"]
        .as_str()
        .or_else(|| v["assignees"][0]["name"].as_str())
        .or_else(|| v["assignee"]["username"].as_str())
        .map(str::to_string);
    Ok(Ticket {
        external_id: full.to_ascii_lowercase(),
        key: format!("#{iid}"),
        url: v["web_url"].as_str().unwrap_or_default().to_string(),
        title: v["title"].as_str().unwrap_or_default().to_string(),
        status: state.to_string(),
        category: if state == "closed" { Category::Done } else { Category::Open },
        assignee,
    })
}

impl Gitlab {
    pub fn new(s: &Settings, http: Outbound) -> Result<Gitlab, TrackerError> {
        let base = s.base_url()?;
        Ok(Gitlab { api: join(&base, &["api", "v4"]), token: s.token.trim().to_string(), http })
    }

    fn req(&self, method: Method, url: url::Url) -> Req {
        Req::new(method, url).header("private-token", self.token.clone()).header("accept", "application/json")
    }

    async fn call(&self, req: Req) -> Result<(Value, Resp), TrackerError> {
        let resp = self.http.send(req).await?;
        if !(200..300).contains(&resp.status) {
            return Err(status_error(resp.status, gitlab_message(&resp.body)));
        }
        Ok((resp.json()?, resp))
    }

    fn issue_url(&self, path: &str, iid: Option<u64>, tail: Option<&str>) -> url::Url {
        let iid = iid.map(|n| n.to_string());
        let mut segs = vec!["projects", path, "issues"];
        if let Some(i) = iid.as_deref() {
            segs.push(i);
        }
        if let Some(t) = tail {
            segs.push(t);
        }
        join(&self.api, &segs)
    }

    fn parts(id: &str) -> Result<(String, u64), TrackerError> {
        split_ref(id).ok_or_else(|| TrackerError::Invalid(format!("{id:?} is not group/project#number")))
    }
}

impl IssueTracker for Gitlab {
    async fn validate(&self) -> Result<Account, TrackerError> {
        let (v, _) = self.call(self.req(Method::GET, join(&self.api, &["user"]))).await?;
        let user = v["username"].as_str().ok_or_else(|| TrackerError::Invalid("GitLab returned no user".into()))?;
        Ok(Account { id: user.to_string(), name: v["name"].as_str().unwrap_or(user).to_string() })
    }

    async fn targets(&self) -> Result<Vec<Target>, TrackerError> {
        let mut out = Vec::new();
        let mut page = 1u32;
        for _ in 0..10 {
            let mut u = join(&self.api, &["projects"]);
            u.query_pairs_mut()
                .append_pair("membership", "true")
                .append_pair("simple", "true")
                .append_pair("with_issues_enabled", "true")
                .append_pair("archived", "false")
                .append_pair("order_by", "path")
                .append_pair("sort", "asc")
                .append_pair("per_page", "100")
                .append_pair("page", &page.to_string());
            let (v, resp) = self.call(self.req(Method::GET, u)).await?;
            for p in v.as_array().into_iter().flatten() {
                if let Some(path) = p["path_with_namespace"].as_str() {
                    out.push(Target::simple(
                        path.to_string(),
                        p["name_with_namespace"].as_str().unwrap_or(path).to_string(),
                    ));
                }
            }
            match resp.header("x-next-page").and_then(|n| n.trim().parse::<u32>().ok()) {
                Some(n) if n > page => page = n,
                _ => break,
            }
        }
        Ok(out)
    }

    async fn create_ticket(&self, target: &TicketTarget, draft: &TicketDraft) -> Result<Ticket, TrackerError> {
        let path = target.target.trim();
        if !path.contains('/') {
            return Err(TrackerError::Invalid("the GitLab target must be a project path such as group/project".into()));
        }
        let mut body = json!({"title": super::truncate(&draft.title, 255), "description": draft.body.markdown()});
        if !target.labels.is_empty() {
            body["labels"] = json!(target.labels.join(","));
        }
        let (v, _) = self.call(self.req(Method::POST, self.issue_url(path, None, None)).json(&body)).await?;
        ticket(path, &v)
    }

    async fn get_ticket(&self, id: &str) -> Result<Ticket, TrackerError> {
        let (path, iid) = Self::parts(id)?;
        let (v, _) = self.call(self.req(Method::GET, self.issue_url(&path, Some(iid), None))).await?;
        ticket(&path, &v)
    }

    async fn add_comment(&self, id: &str, body: &Doc) -> Result<(), TrackerError> {
        let (path, iid) = Self::parts(id)?;
        self.call(
            self.req(Method::POST, self.issue_url(&path, Some(iid), Some("notes")))
                .json(&json!({"body": body.markdown()})),
        )
        .await
        .map(|_| ())
    }

    async fn transition(&self, id: &str, to: Category) -> Result<Option<Ticket>, TrackerError> {
        let (path, iid) = Self::parts(id)?;
        let event = match to {
            Category::Done => "close",
            Category::Open => "reopen",
        };
        let (v, _) = self
            .call(self.req(Method::PUT, self.issue_url(&path, Some(iid), None)).json(&json!({"state_event": event})))
            .await?;
        ticket(&path, &v).map(Some)
    }

    fn parse_reference(&self, input: &str, default_target: Option<&str>) -> Option<String> {
        let input = input.trim();
        if let Some((p, n)) = split_ref(input) {
            return Some(format!("{p}#{n}"));
        }
        if let Ok(u) = url::Url::parse(input) {
            let segs: Vec<&str> = u.path_segments()?.filter(|s| !s.is_empty()).collect();
            let dash = segs.iter().position(|s| *s == "-")?;
            if segs.get(dash + 1) != Some(&"issues") && segs.get(dash + 1) != Some(&"work_items") {
                return None;
            }
            let n: u64 = segs.get(dash + 2)?.parse().ok()?;
            return split_ref(&format!("{}#{n}", segs[..dash].join("/"))).map(|(p, n)| format!("{p}#{n}"));
        }
        let n: u64 = input.trim_start_matches('#').parse().ok().filter(|n| *n > 0)?;
        split_ref(&format!("{}#{n}", default_target?)).map(|(p, n)| format!("{p}#{n}"))
    }

    fn parse_webhook(&self, req: &WebhookRequest, secret: Option<&str>) -> Result<WebhookParsed, WebhookError> {
        let secret = secret.filter(|s| !s.is_empty()).ok_or(WebhookError::Unauthorized)?;
        let given = req.header("x-gitlab-token").ok_or(WebhookError::Unauthorized)?;
        if !rupta_auth::crypto::ct_eq(given.as_bytes(), secret.as_bytes()) {
            return Err(WebhookError::Unauthorized);
        }
        let mut out = WebhookParsed {
            delivery_id: req.header("idempotency-key").or(req.header("x-gitlab-event-uuid")).map(str::to_string),
            updates: Vec::new(),
        };
        let v: Value = serde_json::from_slice(req.body).map_err(|e| WebhookError::Invalid(format!("not JSON: {e}")))?;
        if v["object_kind"].as_str() != Some("issue") && v["object_kind"].as_str() != Some("work_item") {
            return Ok(out);
        }
        let a = &v["object_attributes"];
        let path =
            v["project"]["path_with_namespace"].as_str().ok_or_else(|| WebhookError::Invalid("no project".into()))?;
        let Some(iid) = a["iid"].as_u64() else { return Ok(out) };
        let state = a["state"].as_str();
        out.updates.push(TicketUpdate {
            external_id: format!("{path}#{iid}").to_ascii_lowercase(),
            key: Some(format!("#{iid}")),
            url: a["url"].as_str().map(str::to_string),
            title: a["title"].as_str().map(str::to_string),
            status: state.map(str::to_string),
            category: state.map(|s| if s == "closed" { Category::Done } else { Category::Open }),
            assignee: v.get("assignees").map(|x| x[0]["name"].as_str().map(str::to_string)),
            deleted: false,
        });
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gl() -> Gitlab {
        Gitlab::new(
            &Settings {
                kind: super::super::Kind::Gitlab,
                config: json!({"base_url": "https://git.example.com"}),
                token: "t".into(),
            },
            Outbound::new(true),
        )
        .unwrap()
    }

    #[test]
    fn references_and_urls() {
        let g = gl();
        assert_eq!(g.parse_reference("group/sub/app#4", None).as_deref(), Some("group/sub/app#4"));
        assert_eq!(
            g.parse_reference("https://git.example.com/group/sub/app/-/issues/4", None).as_deref(),
            Some("group/sub/app#4")
        );
        assert_eq!(g.parse_reference("#4", Some("group/app")).as_deref(), Some("group/app#4"));
        assert_eq!(g.parse_reference("app#4", None), None);
        assert_eq!(
            g.issue_url("group/app", Some(4), Some("notes")).as_str(),
            "https://git.example.com/api/v4/projects/group%2Fapp/issues/4/notes"
        );
    }
}
