//! Linear, through its GraphQL API (`POST {base}/graphql`, base
//! `https://api.linear.app`). A personal API key (`lin_api_…`) is sent as
//! is in `Authorization`; an OAuth access token is sent as `Bearer`.
//!
//! Operations: `viewer`, `teams` (with states, labels and projects),
//! `issueLabels`, `issueCreate`, `issue(id:)` (UUID or identifier such as
//! `ENG-42`), `commentCreate`, `issueUpdate` (state).
//!
//! Webhooks: `Linear-Signature` is the hex HMAC-SHA256 of the raw body
//! with the webhook's signing secret; `webhookTimestamp` (milliseconds)
//! must be within five minutes; `Linear-Delivery` identifies the
//! delivery.

use reqwest::Method;
use serde_json::{Value, json};

use super::http::{Outbound, Req, snippet};
use super::{
    Account, Category, Choice, Doc, IssueTracker, Settings, StateChoice, Target, Ticket, TicketDraft, TicketTarget,
    TicketUpdate, TrackerError, WebhookError, WebhookParsed, WebhookRequest, join, status_error, verify_hmac,
};

pub struct Linear {
    endpoint: url::Url,
    auth: String,
    http: Outbound,
    resolve_state: Option<String>,
    reopen_state: Option<String>,
}

const ISSUE_FIELDS: &str =
    "fragment F on Issue { id identifier url title state { id name type } assignee { name displayName } }";

/// Tolerance for `webhookTimestamp`.
const WEBHOOK_MAX_AGE_MS: i64 = 5 * 60 * 1000;

fn category_of_type(t: &str) -> Category {
    if matches!(t, "completed" | "canceled") { Category::Done } else { Category::Open }
}

fn ticket(v: &Value) -> Result<Ticket, TrackerError> {
    let id = v["id"].as_str().ok_or_else(|| TrackerError::Invalid("Linear returned no issue".into()))?;
    Ok(Ticket {
        external_id: id.to_string(),
        key: v["identifier"].as_str().unwrap_or(id).to_string(),
        url: v["url"].as_str().unwrap_or_default().to_string(),
        title: v["title"].as_str().unwrap_or_default().to_string(),
        status: v["state"]["name"].as_str().unwrap_or_default().to_string(),
        category: category_of_type(v["state"]["type"].as_str().unwrap_or_default()),
        assignee: v["assignee"]["displayName"].as_str().or(v["assignee"]["name"].as_str()).map(str::to_string),
    })
}

/// `ENG-42` as upper case.
fn identifier(s: &str) -> Option<String> {
    let (team, n) = s.trim().rsplit_once('-')?;
    let ok = team.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && team.chars().all(|c| c.is_ascii_alphanumeric())
        && !n.is_empty()
        && n.chars().all(|c| c.is_ascii_digit());
    ok.then(|| s.trim().to_ascii_uppercase())
}

fn graphql_error(errors: &Value, status: u16) -> TrackerError {
    let first = &errors[0];
    let message = first["message"].as_str().unwrap_or("GraphQL error").to_string();
    let code = first["extensions"]["code"].as_str().unwrap_or_default();
    match code {
        "RATELIMITED" => TrackerError::RateLimited(message),
        "AUTHENTICATION_ERROR" | "FORBIDDEN" => TrackerError::Unauthorized(message),
        _ if message.to_ascii_lowercase().contains("not found") => TrackerError::NotFound(message),
        _ if status >= 400 => status_error(status, message),
        _ => TrackerError::Invalid(message),
    }
}

impl Linear {
    pub fn new(s: &Settings, http: Outbound) -> Result<Linear, TrackerError> {
        let base = s.base_url()?;
        let token = s.token.trim();
        let auth = if token.starts_with("lin_api_") || token.contains(' ') {
            token.to_string()
        } else {
            format!("Bearer {token}")
        };
        Ok(Linear {
            endpoint: join(&base, &["graphql"]),
            auth,
            http,
            resolve_state: s.str("resolve_state"),
            reopen_state: s.str("reopen_state"),
        })
    }

    /// Runs a query or mutation and returns `data`.
    async fn gql(&self, query: &str, variables: Value, idempotent: bool) -> Result<Value, TrackerError> {
        let req = Req::new(Method::POST, self.endpoint.clone())
            .json(&json!({"query": query, "variables": variables}))
            .header("authorization", self.auth.clone())
            .idempotent(idempotent);
        let resp = self.http.send(req).await?;
        let v: Value = serde_json::from_slice(&resp.body).unwrap_or(Value::Null);
        if v["errors"].as_array().is_some_and(|e| !e.is_empty()) {
            return Err(graphql_error(&v["errors"], resp.status));
        }
        if !(200..300).contains(&resp.status) {
            return Err(status_error(resp.status, snippet(&resp.body)));
        }
        Ok(v["data"].clone())
    }

    async fn team_labels(&self, team: &str) -> Result<Vec<Choice>, TrackerError> {
        let d = self
            .gql(
                "query Labels($team: String!) { team(id: $team) { labels(first: 250) { nodes { id name } } }
                 issueLabels(first: 250, filter: { team: { null: true } }) { nodes { id name } } }",
                json!({"team": team}),
                true,
            )
            .await?;
        Ok(d["team"]["labels"]["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .chain(d["issueLabels"]["nodes"].as_array().into_iter().flatten())
            .filter_map(choice)
            .collect())
    }
}

fn choice(v: &Value) -> Option<Choice> {
    Some(Choice { id: v["id"].as_str()?.to_string(), name: v["name"].as_str()?.to_string() })
}

impl IssueTracker for Linear {
    async fn validate(&self) -> Result<Account, TrackerError> {
        let d = self.gql("query { viewer { id name email } }", json!({}), true).await?;
        let id = d["viewer"]["id"].as_str().ok_or_else(|| TrackerError::Invalid("Linear returned no viewer".into()))?;
        Ok(Account { id: id.to_string(), name: d["viewer"]["name"].as_str().unwrap_or(id).to_string() })
    }

    async fn targets(&self) -> Result<Vec<Target>, TrackerError> {
        let d = self
            .gql(
                "query Targets { teams(first: 100) { nodes { id key name
                   states(first: 100) { nodes { id name type position } }
                   labels(first: 250) { nodes { id name } }
                   projects(first: 100) { nodes { id name } } } }
                 issueLabels(first: 250, filter: { team: { null: true } }) { nodes { id name } } }",
                json!({}),
                true,
            )
            .await?;
        let workspace_labels: Vec<Choice> =
            d["issueLabels"]["nodes"].as_array().into_iter().flatten().filter_map(choice).collect();
        Ok(d["teams"]["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| {
                let mut target = Target::simple(t["id"].as_str()?.to_string(), t["name"].as_str()?.to_string());
                target.key = t["key"].as_str().map(str::to_string);
                let mut states: Vec<&Value> = t["states"]["nodes"].as_array().into_iter().flatten().collect();
                states.sort_by(|a, b| {
                    a["position"].as_f64().partial_cmp(&b["position"].as_f64()).unwrap_or(std::cmp::Ordering::Equal)
                });
                target.states = states
                    .iter()
                    .filter_map(|s| {
                        Some(StateChoice {
                            id: s["id"].as_str()?.to_string(),
                            name: s["name"].as_str()?.to_string(),
                            category: category_of_type(s["type"].as_str()?),
                        })
                    })
                    .collect();
                target.labels = t["labels"]["nodes"].as_array().into_iter().flatten().filter_map(choice).collect();
                target.labels.extend(workspace_labels.iter().cloned());
                target.projects = t["projects"]["nodes"].as_array().into_iter().flatten().filter_map(choice).collect();
                Some(target)
            })
            .collect())
    }

    async fn create_ticket(&self, target: &TicketTarget, draft: &TicketDraft) -> Result<Ticket, TrackerError> {
        let team = target.target.trim();
        if team.is_empty() {
            return Err(TrackerError::Invalid("a Linear team is required".into()));
        }
        let mut input = json!({
            "teamId": team,
            "title": super::truncate(&draft.title, 255),
            "description": draft.body.markdown(),
        });
        if let Some(p) = target.project.as_deref().filter(|p| !p.is_empty()) {
            input["projectId"] = json!(p);
        }
        if !target.labels.is_empty() {
            let known = self.team_labels(team).await?;
            let mut ids = Vec::new();
            for l in &target.labels {
                let found = known.iter().find(|k| k.id == *l || k.name.eq_ignore_ascii_case(l));
                ids.push(
                    found
                        .ok_or_else(|| TrackerError::Invalid(format!("the Linear team has no label {l:?}")))?
                        .id
                        .clone(),
                );
            }
            input["labelIds"] = json!(ids);
        }
        let d = self
            .gql(
                &format!("mutation IssueCreate($input: IssueCreateInput!) {{ issueCreate(input: $input) {{ success issue {{ ...F }} }} }} {ISSUE_FIELDS}"),
                json!({"input": input}),
                false,
            )
            .await?;
        if d["issueCreate"]["success"].as_bool() != Some(true) {
            return Err(TrackerError::Invalid("Linear did not create the issue".into()));
        }
        ticket(&d["issueCreate"]["issue"])
    }

    async fn get_ticket(&self, id: &str) -> Result<Ticket, TrackerError> {
        let d = self
            .gql(
                &format!("query Issue($id: String!) {{ issue(id: $id) {{ ...F }} }} {ISSUE_FIELDS}"),
                json!({"id": id.trim()}),
                true,
            )
            .await?;
        if d["issue"].is_null() {
            return Err(TrackerError::NotFound(format!("Linear issue {id}")));
        }
        ticket(&d["issue"])
    }

    async fn add_comment(&self, id: &str, body: &Doc) -> Result<(), TrackerError> {
        let d = self
            .gql(
                "mutation CommentCreate($input: CommentCreateInput!) { commentCreate(input: $input) { success } }",
                json!({"input": {"issueId": id, "body": body.markdown()}}),
                false,
            )
            .await?;
        if d["commentCreate"]["success"].as_bool() == Some(true) {
            Ok(())
        } else {
            Err(TrackerError::Invalid("Linear did not create the comment".into()))
        }
    }

    async fn transition(&self, id: &str, to: Category) -> Result<Option<Ticket>, TrackerError> {
        let d = self
            .gql(
                "query States($id: String!) { issue(id: $id) { id team { states(first: 100) { nodes { id name type position } } } } }",
                json!({"id": id}),
                true,
            )
            .await?;
        let mut states: Vec<Value> = d["issue"]["team"]["states"]["nodes"].as_array().cloned().unwrap_or_default();
        states.sort_by(|a, b| {
            a["position"].as_f64().partial_cmp(&b["position"].as_f64()).unwrap_or(std::cmp::Ordering::Equal)
        });
        let of = |t: &str| states.iter().find(|s| s["type"].as_str() == Some(t));
        let configured = match to {
            Category::Done => self.resolve_state.as_deref(),
            Category::Open => self.reopen_state.as_deref(),
        };
        let pick = match configured {
            Some(name) => states.iter().find(|s| {
                s["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name)) || s["id"].as_str() == Some(name)
            }),
            None => match to {
                Category::Done => of("completed"),
                Category::Open => of("unstarted").or_else(|| of("backlog")).or_else(|| of("started")),
            },
        };
        let Some(state) = pick.and_then(|s| s["id"].as_str()) else { return Ok(None) };
        let issue_id = d["issue"]["id"].as_str().unwrap_or(id);
        let d = self
            .gql(
                &format!("mutation IssueUpdate($id: String!, $input: IssueUpdateInput!) {{ issueUpdate(id: $id, input: $input) {{ success issue {{ ...F }} }} }} {ISSUE_FIELDS}"),
                json!({"id": issue_id, "input": {"stateId": state}}),
                true,
            )
            .await?;
        ticket(&d["issueUpdate"]["issue"]).map(Some)
    }

    fn parse_reference(&self, input: &str, _default_target: Option<&str>) -> Option<String> {
        let input = input.trim();
        if let Some(k) = identifier(input) {
            return Some(k);
        }
        if uuid::Uuid::parse_str(input).is_ok() {
            return Some(input.to_ascii_lowercase());
        }
        let u = url::Url::parse(input).ok()?;
        let segs: Vec<&str> = u.path_segments()?.collect();
        let i = segs.iter().position(|s| *s == "issue")?;
        identifier(segs.get(i + 1)?)
    }

    fn parse_webhook(&self, req: &WebhookRequest, secret: Option<&str>) -> Result<WebhookParsed, WebhookError> {
        let secret = secret.filter(|s| !s.is_empty()).ok_or(WebhookError::Unauthorized)?;
        let sig = req.header("linear-signature").ok_or(WebhookError::Unauthorized)?;
        if !verify_hmac(secret, req.body, sig) {
            return Err(WebhookError::Unauthorized);
        }
        let v: Value = serde_json::from_slice(req.body).map_err(|e| WebhookError::Invalid(format!("not JSON: {e}")))?;
        if let Some(ts) = v["webhookTimestamp"].as_i64() {
            let now = chrono::Utc::now().timestamp_millis();
            if (now - ts).abs() > WEBHOOK_MAX_AGE_MS {
                return Err(WebhookError::Unauthorized);
            }
        }
        let mut out =
            WebhookParsed { delivery_id: req.header("linear-delivery").map(str::to_string), updates: Vec::new() };
        if v["type"].as_str() != Some("Issue") {
            return Ok(out);
        }
        let d = &v["data"];
        let Some(id) = d["id"].as_str() else { return Ok(out) };
        let removed = v["action"].as_str() == Some("remove");
        out.updates.push(TicketUpdate {
            external_id: id.to_string(),
            key: d["identifier"].as_str().map(str::to_string),
            url: d["url"].as_str().or(v["url"].as_str()).map(str::to_string),
            title: d["title"].as_str().map(str::to_string),
            status: d["state"]["name"].as_str().map(str::to_string),
            category: d["state"]["type"].as_str().map(category_of_type),
            assignee: d.get("assignee").map(|a| a["name"].as_str().map(str::to_string)),
            deleted: removed,
        });
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linear(token: &str) -> Linear {
        let s = Settings { kind: super::super::Kind::Linear, config: json!({}), token: token.into() };
        Linear::new(&s, Outbound::new(true)).unwrap()
    }

    #[test]
    fn authorization_and_endpoint() {
        assert_eq!(linear("lin_api_abc").auth, "lin_api_abc");
        assert_eq!(linear("oauthtoken").auth, "Bearer oauthtoken");
        assert_eq!(linear("x").endpoint.as_str(), "https://api.linear.app/graphql");
    }

    #[test]
    fn references() {
        let l = linear("lin_api_x");
        assert_eq!(l.parse_reference("eng-42", None).as_deref(), Some("ENG-42"));
        assert_eq!(
            l.parse_reference("https://linear.app/acme/issue/ENG-7/crash-on-start", None).as_deref(),
            Some("ENG-7")
        );
        assert!(l.parse_reference("2a9c8b1e-8a2b-4cbe-9d5f-0d4a7f3e1c22", None).is_some());
        assert_eq!(l.parse_reference("hello", None), None);
    }

    #[test]
    fn errors_are_classified() {
        let e = json!([{"message": "Rate limit exceeded", "extensions": {"code": "RATELIMITED"}}]);
        assert!(matches!(graphql_error(&e, 400), TrackerError::RateLimited(_)));
        let e = json!([{"message": "Authentication required", "extensions": {"code": "AUTHENTICATION_ERROR"}}]);
        assert!(matches!(graphql_error(&e, 400), TrackerError::Unauthorized(_)));
        let e = json!([{"message": "Entity not found: Issue"}]);
        assert!(matches!(graphql_error(&e, 200), TrackerError::NotFound(_)));
    }
}
