//! GitHub Issues through the REST API (version `2022-11-28`, sent as
//! `X-GitHub-Api-Version`), base `https://api.github.com` or a GitHub
//! Enterprise Server API URL (`https://github.example.com/api/v3`).
//! Fine-grained personal access tokens (Issues: read and write, Metadata:
//! read) and classic tokens (`repo`, or `public_repo` for public
//! repositories) are sent as `Authorization: Bearer`.
//!
//! Endpoints: `GET /user`, `GET /user/repos`, `POST
//! /repos/{owner}/{repo}/issues`, `GET`/`PATCH
//! /repos/{owner}/{repo}/issues/{number}`, `POST
//! /repos/{owner}/{repo}/issues/{number}/comments`.
//!
//! Webhooks (`issues` events): `X-Hub-Signature-256: sha256=<hex
//! HMAC-SHA256 of the body>`, `X-GitHub-Event`, `X-GitHub-Delivery`.

use reqwest::Method;
use serde_json::{Value, json};

use super::http::{Outbound, Req, Resp, snippet};
use super::{
    Account, Category, Doc, IssueTracker, Settings, Target, Ticket, TicketDraft, TicketTarget, TicketUpdate,
    TrackerError, WebhookError, WebhookParsed, WebhookRequest, join, status_error, verify_hmac,
};

pub const API_VERSION: &str = "2022-11-28";

pub struct Github {
    base: url::Url,
    token: String,
    http: Outbound,
}

/// `owner/repo#17` into its parts.
pub(crate) fn split_ref(id: &str) -> Option<(String, String, u64)> {
    let (repo, n) = id.trim().rsplit_once('#')?;
    let (owner, name) = repo.split_once('/')?;
    let n: u64 = n.parse().ok().filter(|n| *n > 0)?;
    let valid = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    (valid(owner) && valid(name)).then(|| (owner.to_string(), name.to_string(), n))
}

fn github_message(body: &[u8]) -> String {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| {
            let m = v["message"].as_str()?.to_string();
            let details: Vec<String> = v["errors"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|e| e["message"].as_str().or(e["code"].as_str()).map(str::to_string))
                .collect();
            Some(if details.is_empty() { m } else { format!("{m} ({})", details.join(", ")) })
        })
        .unwrap_or_else(|| snippet(body))
}

fn ticket(repo: &str, v: &Value) -> Result<Ticket, TrackerError> {
    let n = v["number"].as_u64().ok_or_else(|| TrackerError::Invalid("GitHub returned no issue number".into()))?;
    let state = v["state"].as_str().unwrap_or("open");
    let status = match (state, v["state_reason"].as_str()) {
        ("closed", Some("not_planned")) => "closed (not planned)".to_string(),
        ("closed", Some("duplicate")) => "closed (duplicate)".to_string(),
        (s, _) => s.to_string(),
    };
    Ok(Ticket {
        external_id: format!("{}#{n}", repo.to_ascii_lowercase()),
        key: format!("#{n}"),
        url: v["html_url"].as_str().unwrap_or_default().to_string(),
        title: v["title"].as_str().unwrap_or_default().to_string(),
        status,
        category: if state == "closed" { Category::Done } else { Category::Open },
        assignee: v["assignee"]["login"].as_str().map(str::to_string),
    })
}

/// The `next` URL of a `Link` header.
fn next_link(resp: &Resp) -> Option<url::Url> {
    let link = resp.header("link")?;
    link.split(',').find_map(|part| {
        let (url, rel) = part.split_once(';')?;
        rel.contains("rel=\"next\"")
            .then(|| url::Url::parse(url.trim().trim_start_matches('<').trim_end_matches('>')).ok())?
    })
}

impl Github {
    pub fn new(s: &Settings, http: Outbound) -> Result<Github, TrackerError> {
        Ok(Github { base: s.base_url()?, token: s.token.trim().to_string(), http })
    }

    fn req(&self, method: Method, url: url::Url) -> Req {
        Req::new(method, url)
            .header("authorization", format!("Bearer {}", self.token))
            .header("accept", "application/vnd.github+json")
            .header("x-github-api-version", API_VERSION)
    }

    async fn call(&self, req: Req) -> Result<(Value, Resp), TrackerError> {
        let resp = self.http.send(req).await?;
        if !(200..300).contains(&resp.status) {
            // GitHub answers 403 with x-ratelimit-remaining: 0 when rate limited.
            if resp.status == 403 && resp.header("x-ratelimit-remaining") == Some("0") {
                return Err(TrackerError::RateLimited(github_message(&resp.body)));
            }
            return Err(status_error(resp.status, github_message(&resp.body)));
        }
        Ok((resp.json()?, resp))
    }

    fn issue_url(&self, owner: &str, repo: &str, n: Option<u64>, tail: Option<&str>) -> url::Url {
        let n = n.map(|n| n.to_string());
        let mut segs = vec!["repos", owner, repo, "issues"];
        if let Some(n) = n.as_deref() {
            segs.push(n);
        }
        if let Some(t) = tail {
            segs.push(t);
        }
        join(&self.base, &segs)
    }

    fn parts(id: &str) -> Result<(String, String, u64), TrackerError> {
        split_ref(id).ok_or_else(|| TrackerError::Invalid(format!("{id:?} is not owner/repo#number")))
    }
}

impl IssueTracker for Github {
    async fn validate(&self) -> Result<Account, TrackerError> {
        let (v, _) = self.call(self.req(Method::GET, join(&self.base, &["user"]))).await?;
        let login = v["login"].as_str().ok_or_else(|| TrackerError::Invalid("GitHub returned no login".into()))?;
        Ok(Account { id: login.to_string(), name: v["name"].as_str().unwrap_or(login).to_string() })
    }

    async fn targets(&self) -> Result<Vec<Target>, TrackerError> {
        let mut url = join(&self.base, &["user", "repos"]);
        url.query_pairs_mut().append_pair("per_page", "100").append_pair("sort", "full_name");
        let mut out = Vec::new();
        for _ in 0..10 {
            let (v, resp) = self.call(self.req(Method::GET, url.clone())).await?;
            for r in v.as_array().into_iter().flatten() {
                if r["has_issues"].as_bool() == Some(false) || r["archived"].as_bool() == Some(true) {
                    continue;
                }
                if let Some(full) = r["full_name"].as_str() {
                    out.push(Target::simple(full.to_string(), full.to_string()));
                }
            }
            match next_link(&resp) {
                Some(next) if next.host_str() == self.base.host_str() => url = next,
                _ => break,
            }
        }
        Ok(out)
    }

    async fn create_ticket(&self, target: &TicketTarget, draft: &TicketDraft) -> Result<Ticket, TrackerError> {
        let (owner, repo) = target
            .target
            .trim()
            .split_once('/')
            .ok_or_else(|| TrackerError::Invalid("the GitHub target must be owner/repo".into()))?;
        let mut body = json!({"title": super::truncate(&draft.title, 256), "body": draft.body.markdown()});
        if !target.labels.is_empty() {
            body["labels"] = json!(target.labels);
        }
        let (v, _) = self.call(self.req(Method::POST, self.issue_url(owner, repo, None, None)).json(&body)).await?;
        ticket(&format!("{owner}/{repo}"), &v)
    }

    async fn get_ticket(&self, id: &str) -> Result<Ticket, TrackerError> {
        let (owner, repo, n) = Self::parts(id)?;
        let (v, _) = self.call(self.req(Method::GET, self.issue_url(&owner, &repo, Some(n), None))).await?;
        ticket(&format!("{owner}/{repo}"), &v)
    }

    async fn add_comment(&self, id: &str, body: &Doc) -> Result<(), TrackerError> {
        let (owner, repo, n) = Self::parts(id)?;
        self.call(
            self.req(Method::POST, self.issue_url(&owner, &repo, Some(n), Some("comments")))
                .json(&json!({"body": body.markdown()})),
        )
        .await
        .map(|_| ())
    }

    async fn transition(&self, id: &str, to: Category) -> Result<Option<Ticket>, TrackerError> {
        let (owner, repo, n) = Self::parts(id)?;
        let body = match to {
            Category::Done => json!({"state": "closed", "state_reason": "completed"}),
            Category::Open => json!({"state": "open"}),
        };
        let (v, _) =
            self.call(self.req(Method::PATCH, self.issue_url(&owner, &repo, Some(n), None)).json(&body)).await?;
        ticket(&format!("{owner}/{repo}"), &v).map(Some)
    }

    fn parse_reference(&self, input: &str, default_target: Option<&str>) -> Option<String> {
        let input = input.trim();
        if let Some((o, r, n)) = split_ref(input) {
            return Some(format!("{o}/{r}#{n}"));
        }
        if let Ok(u) = url::Url::parse(input) {
            let segs: Vec<&str> = u.path_segments()?.filter(|s| !s.is_empty()).collect();
            let i = segs.iter().position(|s| *s == "issues")?;
            if i < 2 {
                return None;
            }
            let n: u64 = segs.get(i + 1)?.parse().ok()?;
            return split_ref(&format!("{}/{}#{n}", segs[i - 2], segs[i - 1])).map(|(o, r, n)| format!("{o}/{r}#{n}"));
        }
        let n = input.trim_start_matches('#');
        let n: u64 = n.parse().ok().filter(|n| *n > 0)?;
        let repo = default_target?;
        split_ref(&format!("{repo}#{n}")).map(|(o, r, n)| format!("{o}/{r}#{n}"))
    }

    fn parse_webhook(&self, req: &WebhookRequest, secret: Option<&str>) -> Result<WebhookParsed, WebhookError> {
        let secret = secret.filter(|s| !s.is_empty()).ok_or(WebhookError::Unauthorized)?;
        let sig = req
            .header("x-hub-signature-256")
            .and_then(|s| s.strip_prefix("sha256="))
            .ok_or(WebhookError::Unauthorized)?;
        if !verify_hmac(secret, req.body, sig) {
            return Err(WebhookError::Unauthorized);
        }
        let mut out =
            WebhookParsed { delivery_id: req.header("x-github-delivery").map(str::to_string), updates: Vec::new() };
        if req.header("x-github-event") != Some("issues") {
            return Ok(out); // ping and other events
        }
        let v: Value = serde_json::from_slice(req.body).map_err(|e| WebhookError::Invalid(format!("not JSON: {e}")))?;
        let repo =
            v["repository"]["full_name"].as_str().ok_or_else(|| WebhookError::Invalid("no repository".into()))?;
        let t = ticket(repo, &v["issue"]).map_err(|e| WebhookError::Invalid(e.to_string()))?;
        let deleted = v["action"].as_str() == Some("deleted");
        out.updates.push(TicketUpdate {
            external_id: t.external_id,
            key: Some(t.key),
            url: Some(t.url).filter(|u| !u.is_empty()),
            title: Some(t.title),
            status: Some(t.status),
            category: Some(t.category),
            assignee: Some(t.assignee),
            deleted,
        });
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gh() -> Github {
        Github::new(
            &Settings { kind: super::super::Kind::Github, config: json!({}), token: "t".into() },
            Outbound::new(true),
        )
        .unwrap()
    }

    #[test]
    fn references() {
        let g = gh();
        assert_eq!(g.parse_reference("acme/web#17", None).as_deref(), Some("acme/web#17"));
        assert_eq!(g.parse_reference("https://github.com/acme/web/issues/17", None).as_deref(), Some("acme/web#17"));
        assert_eq!(g.parse_reference("#17", Some("acme/web")).as_deref(), Some("acme/web#17"));
        assert_eq!(g.parse_reference("17", Some("acme/web")).as_deref(), Some("acme/web#17"));
        assert_eq!(g.parse_reference("#17", None), None);
        assert_eq!(g.parse_reference("https://github.com/acme/web/pull/3", None), None);
        assert_eq!(
            g.issue_url("a", "b", Some(3), Some("comments")).as_str(),
            "https://api.github.com/repos/a/b/issues/3/comments"
        );
    }

    #[test]
    fn closed_reasons() {
        let t = ticket(
            "Acme/Web",
            &json!({"number": 3, "state": "closed", "state_reason": "not_planned", "html_url": "u", "title": "x"}),
        )
        .unwrap();
        assert_eq!(t.external_id, "acme/web#3");
        assert_eq!(t.status, "closed (not planned)");
        assert_eq!(t.category, Category::Done);
    }
}
