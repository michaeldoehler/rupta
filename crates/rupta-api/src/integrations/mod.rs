//! Issue tracker integrations: one [`IssueTracker`] trait with an
//! implementation per tracker, the documents rupta writes into tickets,
//! and two-way status synchronisation ([`sync`]).
//!
//! | Kind | API |
//! | --- | --- |
//! | `jira_cloud` | Jira Cloud platform REST API v3, basic auth (email + API token), Atlassian Document Format |
//! | `jira_server` | Jira Data Center / Server REST API v2, personal access token (bearer), wiki markup |
//! | `linear` | Linear GraphQL API, API key in `Authorization` |
//! | `github` | GitHub REST API 2022-11-28, fine-grained or classic personal access token |
//! | `gitlab` | GitLab REST API v4, personal, project or group access token |

pub mod content;
pub mod github;
pub mod gitlab;
pub mod http;
pub mod jira;
pub mod linear;
pub mod store;
pub mod sync;

use std::future::Future;

use axum::http::HeaderMap;
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use utoipa::ToSchema;

pub use content::Doc;
use http::Outbound;

/// The kinds of trackers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    JiraCloud,
    JiraServer,
    Linear,
    Github,
    Gitlab,
}

impl Kind {
    pub const ALL: [Kind; 5] = [Kind::JiraCloud, Kind::JiraServer, Kind::Linear, Kind::Github, Kind::Gitlab];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::JiraCloud => "jira_cloud",
            Kind::JiraServer => "jira_server",
            Kind::Linear => "linear",
            Kind::Github => "github",
            Kind::Gitlab => "gitlab",
        }
    }

    pub fn parse(s: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// The base URL used when the configuration names none; `None` when
    /// one is required.
    pub fn default_base_url(self) -> Option<&'static str> {
        match self {
            Kind::JiraCloud | Kind::JiraServer => None,
            Kind::Linear => Some("https://api.linear.app"),
            Kind::Github => Some("https://api.github.com"),
            Kind::Gitlab => Some("https://gitlab.com"),
        }
    }

    /// Whether rupta generates the webhook secret (the tracker lets you
    /// enter one); Linear generates its own signing secret, which is
    /// entered in rupta.
    pub fn rupta_generates_webhook_secret(self) -> bool {
        self != Kind::Linear
    }
}

/// `open` or `done`: what rupta makes of a tracker status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Open,
    Done,
}

impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Category::Open => "open",
            Category::Done => "done",
        }
    }

    pub fn parse(s: &str) -> Category {
        if s == "done" { Category::Done } else { Category::Open }
    }
}

/// A failed call to a tracker.
#[derive(Debug, Clone, PartialEq)]
pub enum TrackerError {
    /// The host could not be reached (connection, timeout).
    Network(String),
    /// The host is internal and internal hosts are not allowed.
    Blocked(String),
    /// The credentials were refused (401/403).
    Unauthorized(String),
    /// The ticket, project or repository does not exist (or is not visible).
    NotFound(String),
    /// Rate limited, after retries.
    RateLimited(String),
    /// Another error answer.
    Status(u16, String),
    /// A bad configuration, input or answer.
    Invalid(String),
}

impl TrackerError {
    /// A short machine-readable reason.
    pub fn reason(&self) -> &'static str {
        match self {
            TrackerError::Network(_) => "network",
            TrackerError::Blocked(_) => "blocked_host",
            TrackerError::Unauthorized(_) => "unauthorized",
            TrackerError::NotFound(_) => "not_found",
            TrackerError::RateLimited(_) => "rate_limited",
            TrackerError::Status(..) => "error_response",
            TrackerError::Invalid(_) => "invalid",
        }
    }
}

impl std::fmt::Display for TrackerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrackerError::Network(m) | TrackerError::Blocked(m) | TrackerError::Invalid(m) => f.write_str(m),
            TrackerError::Unauthorized(m) => write!(f, "the credentials were refused: {m}"),
            TrackerError::NotFound(m) => write!(f, "not found: {m}"),
            TrackerError::RateLimited(m) => write!(f, "rate limited: {m}"),
            TrackerError::Status(s, m) => write!(f, "the tracker answered {s}: {m}"),
        }
    }
}

/// Maps an error answer to an error, with the tracker's message.
pub(crate) fn status_error(status: u16, message: String) -> TrackerError {
    match status {
        401 | 403 => TrackerError::Unauthorized(message),
        404 | 410 => TrackerError::NotFound(message),
        429 => TrackerError::RateLimited(message),
        _ => TrackerError::Status(status, message),
    }
}

/// Who the credentials belong to.
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct Account {
    /// The tracker's id of the account (Jira account id or user name,
    /// Linear user id, GitHub or GitLab login).
    pub id: String,
    pub name: String,
}

/// An id and a name.
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct Choice {
    pub id: String,
    pub name: String,
}

/// A workflow state (Linear).
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct StateChoice {
    pub id: String,
    pub name: String,
    pub category: Category,
}

/// Where tickets can be created: a Jira project, a Linear team, a GitHub
/// repository or a GitLab project.
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct Target {
    /// Use as `target`: Jira project key, Linear team id, GitHub
    /// `owner/repo`, GitLab project path.
    pub id: String,
    pub name: String,
    /// Jira project key, Linear team key.
    pub key: Option<String>,
    /// Jira issue types (no sub-tasks).
    pub issue_types: Vec<Choice>,
    /// Linear projects of the team.
    pub projects: Vec<Choice>,
    /// Linear labels of the team (and workspace labels).
    pub labels: Vec<Choice>,
    /// Linear workflow states of the team.
    pub states: Vec<StateChoice>,
}

impl Target {
    pub fn simple(id: String, name: String) -> Target {
        Target {
            id,
            name,
            key: None,
            issue_types: Vec::new(),
            projects: Vec::new(),
            labels: Vec::new(),
            states: Vec::new(),
        }
    }
}

/// Where to create one ticket.
#[derive(Debug, Clone, Default)]
pub struct TicketTarget {
    pub target: String,
    pub issue_type: Option<String>,
    pub project: Option<String>,
    pub labels: Vec<String>,
}

/// What to create.
#[derive(Debug, Clone)]
pub struct TicketDraft {
    pub title: String,
    pub body: Doc,
}

/// A ticket as the tracker reports it.
#[derive(Debug, Clone, PartialEq)]
pub struct Ticket {
    /// Stable id: Jira issue id, Linear UUID, `owner/repo#n`, `group/project#n`.
    pub external_id: String,
    /// Human key: `PROJ-123`, `ENG-42`, `#17`.
    pub key: String,
    pub url: String,
    pub title: String,
    pub status: String,
    pub category: Category,
    pub assignee: Option<String>,
}

/// An incoming webhook request.
#[derive(Debug, Clone)]
pub struct WebhookRequest<'a> {
    pub headers: &'a HeaderMap,
    /// The raw query string.
    pub query: Option<&'a str>,
    pub body: &'a [u8],
}

impl WebhookRequest<'_> {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }

    pub fn query_param(&self, name: &str) -> Option<String> {
        form_urlencoded::parse(self.query?.as_bytes()).find(|(k, _)| k == name).map(|(_, v)| v.into_owned())
    }
}

/// What a webhook says about one ticket.
#[derive(Debug, Clone, PartialEq)]
pub struct TicketUpdate {
    pub external_id: String,
    pub key: Option<String>,
    pub url: Option<String>,
    pub title: Option<String>,
    pub status: Option<String>,
    pub category: Option<Category>,
    pub assignee: Option<Option<String>>,
    /// The ticket was deleted in the tracker.
    pub deleted: bool,
}

/// A verified webhook.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WebhookParsed {
    /// The tracker's delivery id, for idempotency.
    pub delivery_id: Option<String>,
    pub updates: Vec<TicketUpdate>,
}

/// Why a webhook was refused.
#[derive(Debug, Clone, PartialEq)]
pub enum WebhookError {
    /// Missing or wrong signature or token: `401`.
    Unauthorized,
    /// Verified, but not understood: `400`.
    Invalid(String),
}

/// The operations rupta needs from a tracker.
pub trait IssueTracker: Send + Sync {
    /// Checks the credentials and returns whose they are.
    fn validate(&self) -> impl Future<Output = Result<Account, TrackerError>> + Send;
    /// Where tickets can be created.
    fn targets(&self) -> impl Future<Output = Result<Vec<Target>, TrackerError>> + Send;
    /// Creates a ticket.
    fn create_ticket(
        &self,
        target: &TicketTarget,
        draft: &TicketDraft,
    ) -> impl Future<Output = Result<Ticket, TrackerError>> + Send;
    /// Fetches a ticket by external id or by a reference from
    /// [`IssueTracker::parse_reference`].
    fn get_ticket(&self, id: &str) -> impl Future<Output = Result<Ticket, TrackerError>> + Send;
    /// Adds a comment.
    fn add_comment(&self, id: &str, body: &Doc) -> impl Future<Output = Result<(), TrackerError>> + Send;
    /// Moves the ticket to a done or an open state. `Ok(None)` when the
    /// tracker offers no matching transition.
    fn transition(&self, id: &str, to: Category) -> impl Future<Output = Result<Option<Ticket>, TrackerError>> + Send;
    /// Turns a key or URL typed by a user into something
    /// [`IssueTracker::get_ticket`] accepts. `default_target` completes
    /// short references such as `#17`.
    fn parse_reference(&self, input: &str, default_target: Option<&str>) -> Option<String>;
    /// Verifies and parses an incoming webhook.
    fn parse_webhook(&self, req: &WebhookRequest, secret: Option<&str>) -> Result<WebhookParsed, WebhookError>;
}

/// The settings and secrets an integration is built from.
#[derive(Debug, Clone)]
pub struct Settings {
    pub kind: Kind,
    pub config: serde_json::Value,
    pub token: String,
}

impl Settings {
    pub fn str(&self, key: &str) -> Option<String> {
        self.config.get(key).and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
    }

    /// The base URL without a trailing slash.
    pub fn base_url(&self) -> Result<url::Url, TrackerError> {
        let raw = self
            .str("base_url")
            .or_else(|| self.kind.default_base_url().map(str::to_string))
            .ok_or_else(|| TrackerError::Invalid("base_url is required".into()))?;
        let u = url::Url::parse(raw.trim_end_matches('/'))
            .map_err(|e| TrackerError::Invalid(format!("base_url is not a URL: {e}")))?;
        if !matches!(u.scheme(), "http" | "https") || u.host_str().is_none() {
            return Err(TrackerError::Invalid("base_url must be an http(s) URL".into()));
        }
        Ok(u)
    }
}

/// Any tracker; dispatches to the implementation of its kind.
pub enum AnyTracker {
    Jira(jira::Jira),
    Linear(linear::Linear),
    Github(github::Github),
    Gitlab(gitlab::Gitlab),
}

impl AnyTracker {
    pub fn new(settings: &Settings, http: Outbound) -> Result<AnyTracker, TrackerError> {
        Ok(match settings.kind {
            Kind::JiraCloud | Kind::JiraServer => AnyTracker::Jira(jira::Jira::new(settings, http)?),
            Kind::Linear => AnyTracker::Linear(linear::Linear::new(settings, http)?),
            Kind::Github => AnyTracker::Github(github::Github::new(settings, http)?),
            Kind::Gitlab => AnyTracker::Gitlab(gitlab::Gitlab::new(settings, http)?),
        })
    }
}

macro_rules! dispatch {
    ($self:ident, $t:ident => $e:expr) => {
        match $self {
            AnyTracker::Jira($t) => $e,
            AnyTracker::Linear($t) => $e,
            AnyTracker::Github($t) => $e,
            AnyTracker::Gitlab($t) => $e,
        }
    };
}

impl IssueTracker for AnyTracker {
    async fn validate(&self) -> Result<Account, TrackerError> {
        dispatch!(self, t => t.validate().await)
    }
    async fn targets(&self) -> Result<Vec<Target>, TrackerError> {
        dispatch!(self, t => t.targets().await)
    }
    async fn create_ticket(&self, target: &TicketTarget, draft: &TicketDraft) -> Result<Ticket, TrackerError> {
        dispatch!(self, t => t.create_ticket(target, draft).await)
    }
    async fn get_ticket(&self, id: &str) -> Result<Ticket, TrackerError> {
        dispatch!(self, t => t.get_ticket(id).await)
    }
    async fn add_comment(&self, id: &str, body: &Doc) -> Result<(), TrackerError> {
        dispatch!(self, t => t.add_comment(id, body).await)
    }
    async fn transition(&self, id: &str, to: Category) -> Result<Option<Ticket>, TrackerError> {
        dispatch!(self, t => t.transition(id, to).await)
    }
    fn parse_reference(&self, input: &str, default_target: Option<&str>) -> Option<String> {
        dispatch!(self, t => t.parse_reference(input, default_target))
    }
    fn parse_webhook(&self, req: &WebhookRequest, secret: Option<&str>) -> Result<WebhookParsed, WebhookError> {
        dispatch!(self, t => t.parse_webhook(req, secret))
    }
}

/// Hex HMAC-SHA256 of `body`.
pub fn hmac_hex(secret: &str, body: &[u8]) -> String {
    let mut m = <Hmac<Sha256> as KeyInit>::new_from_slice(secret.as_bytes()).expect("any key length");
    m.update(body);
    hex::encode(m.finalize().into_bytes())
}

/// Constant-time check of a hex HMAC-SHA256 signature (with an optional
/// `sha256=` prefix already removed by the caller).
pub fn verify_hmac(secret: &str, body: &[u8], given_hex: &str) -> bool {
    let Ok(given) = hex::decode(given_hex.trim()) else { return false };
    let mut m = <Hmac<Sha256> as KeyInit>::new_from_slice(secret.as_bytes()).expect("any key length");
    m.update(body);
    m.verify_slice(&given).is_ok()
}

/// Appends path segments to a base URL (keeping the base's own path,
/// e.g. `https://ghe.example.com/api/v3`).
pub(crate) fn join(base: &url::Url, segments: &[&str]) -> url::Url {
    let mut u = base.clone();
    {
        let mut p = u.path_segments_mut().expect("http URLs have a path");
        p.pop_if_empty();
        for s in segments {
            p.push(s);
        }
    }
    u
}

/// Truncates to `n` characters.
pub(crate) fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n - 1).collect::<String>()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_round_trip() {
        let sig = hmac_hex("secret", b"body");
        assert!(verify_hmac("secret", b"body", &sig));
        assert!(!verify_hmac("other", b"body", &sig));
        assert!(!verify_hmac("secret", b"body", "zz"));
    }

    #[test]
    fn join_keeps_base_path() {
        let b = url::Url::parse("https://ghe.example.com/api/v3").unwrap();
        assert_eq!(join(&b, &["repos", "a/b"]).as_str(), "https://ghe.example.com/api/v3/repos/a%2Fb");
        let b = url::Url::parse("https://x.atlassian.net/").unwrap();
        assert_eq!(join(&b, &["rest", "api", "3", "myself"]).as_str(), "https://x.atlassian.net/rest/api/3/myself");
    }

    #[test]
    fn kinds_round_trip() {
        for k in Kind::ALL {
            assert_eq!(Kind::parse(k.as_str()), Some(k));
        }
        assert_eq!(Kind::parse("jira"), None);
    }
}
