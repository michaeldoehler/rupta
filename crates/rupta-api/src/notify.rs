//! Delivery of notifications: email (SMTP), Slack incoming webhooks,
//! Microsoft Teams webhooks and generic webhooks signed with HMAC-SHA256.

use chrono::Utc;
use hmac::{Hmac, KeyInit, Mac};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::Sha256;

use crate::mail::html_escape;
use crate::state::ApiState;

/// Header carrying the webhook signature: `sha256=<hex>` of
/// HMAC-SHA256(secret, `{timestamp}.{body}`).
pub const SIGNATURE_HEADER: &str = "x-rupta-signature";
/// Header carrying the Unix timestamp used in the signature.
pub const TIMESTAMP_HEADER: &str = "x-rupta-timestamp";

/// What a notification is about.
#[derive(Debug, Clone, Serialize)]
pub struct Notification {
    /// `first_seen`, `regression`, `reappeared`, `escalating`,
    /// `event_frequency`, `user_frequency` or `test`.
    pub trigger: String,
    pub rule_id: Option<String>,
    pub rule_name: String,
    pub org_slug: String,
    pub project_slug: String,
    pub project_name: String,
    pub issue_id: String,
    pub short_id: String,
    pub title: String,
    pub culprit: Option<String>,
    pub level: String,
    pub times_seen: i64,
    pub environment: Option<String>,
    /// Link to the issue in the web app.
    pub url: String,
}

impl Notification {
    pub fn test(state: &ApiState, org_slug: &str) -> Notification {
        Notification {
            trigger: "test".into(),
            rule_id: None,
            rule_name: "Test notification".into(),
            org_slug: org_slug.into(),
            project_slug: "example".into(),
            project_name: "Example".into(),
            issue_id: "0".into(),
            short_id: "EXAMPLE-1".into(),
            title: "This is a test notification from rupta".into(),
            culprit: None,
            level: "info".into(),
            times_seen: 1,
            environment: None,
            url: format!("{}/organizations/{org_slug}/issues", state.config.public_url),
        }
    }

    fn headline(&self) -> String {
        let what = match self.trigger.as_str() {
            "first_seen" => "New issue",
            "regression" => "Regression",
            "reappeared" => "Issue reappeared",
            "escalating" => "Escalating issue",
            "event_frequency" | "user_frequency" => "Issue frequency alert",
            "test" => "Test",
            _ => "Alert",
        };
        format!("[{}] {what}: {}", self.project_slug, truncate(&self.title, 150))
    }

    fn text(&self) -> String {
        let mut s = format!("{}\n\n{} ({})\n", self.headline(), self.short_id, self.level);
        if let Some(c) = &self.culprit {
            s.push_str(&format!("{c}\n"));
        }
        if let Some(e) = &self.environment {
            s.push_str(&format!("Environment: {e}\n"));
        }
        s.push_str(&format!("Events: {}\nRule: {}\n\n{}\n", self.times_seen, self.rule_name, self.url));
        s
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n).collect::<String>()) }
}

/// The JSON a generic webhook receives.
pub fn webhook_body(n: &Notification) -> Value {
    json!({
        "action": "triggered",
        "trigger": n.trigger,
        "rule": {"id": n.rule_id, "name": n.rule_name},
        "organization": n.org_slug,
        "project": {"slug": n.project_slug, "name": n.project_name},
        "issue": {
            "id": n.issue_id, "short_id": n.short_id, "title": n.title, "culprit": n.culprit,
            "level": n.level, "times_seen": n.times_seen, "url": n.url,
        },
        "environment": n.environment,
        "triggered_at": Utc::now(),
    })
}

/// HMAC-SHA256 signature of a webhook delivery.
pub fn sign(secret: &str, timestamp: i64, body: &[u8]) -> String {
    let mut m = <Hmac<Sha256> as KeyInit>::new_from_slice(secret.as_bytes()).expect("any key length");
    m.update(timestamp.to_string().as_bytes());
    m.update(b".");
    m.update(body);
    format!("sha256={}", hex::encode(m.finalize().into_bytes()))
}

fn slack_body(n: &Notification) -> Value {
    let escape = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let mut fields = vec![
        json!({"type": "mrkdwn", "text": format!("*Level*\n{}", escape(&n.level))}),
        json!({"type": "mrkdwn", "text": format!("*Events*\n{}", n.times_seen)}),
    ];
    if let Some(e) = &n.environment {
        fields.push(json!({"type": "mrkdwn", "text": format!("*Environment*\n{}", escape(e))}));
    }
    json!({
        "text": n.headline(),
        "blocks": [
            {"type": "section", "text": {"type": "mrkdwn", "text": format!("*<{}|{}>*\n{}", n.url, escape(&truncate(&n.title, 200)), escape(n.culprit.as_deref().unwrap_or("")))}},
            {"type": "section", "fields": fields},
            {"type": "context", "elements": [{"type": "mrkdwn", "text": format!("{} · {} · rule: {}", escape(&n.project_name), escape(&n.short_id), escape(&n.rule_name))}]},
        ],
    })
}

fn teams_body(n: &Notification) -> Value {
    let mut facts = vec![
        json!({"title": "Level", "value": n.level}),
        json!({"title": "Events", "value": n.times_seen.to_string()}),
    ];
    if let Some(e) = &n.environment {
        facts.push(json!({"title": "Environment", "value": e}));
    }
    facts.push(json!({"title": "Rule", "value": n.rule_name}));
    json!({
        "type": "message",
        "attachments": [{
            "contentType": "application/vnd.microsoft.card.adaptive",
            "content": {
                "$schema": "http://adaptivecards.io/schemas/adaptive-card.json",
                "type": "AdaptiveCard",
                "version": "1.4",
                "body": [
                    {"type": "TextBlock", "size": "Medium", "weight": "Bolder", "wrap": true, "text": n.headline()},
                    {"type": "TextBlock", "wrap": true, "isSubtle": true, "text": n.culprit.clone().unwrap_or_default()},
                    {"type": "FactSet", "facts": facts},
                ],
                "actions": [{"type": "Action.OpenUrl", "title": "Open issue", "url": n.url}],
            }
        }]
    })
}

async fn post_json(state: &ApiState, url: &str, body: &Value, extra: &[(&str, String)]) -> Result<(), String> {
    let bytes = serde_json::to_vec(body).expect("JSON serializes");
    let mut req = state.http.post(url).header("content-type", "application/json").body(bytes);
    for (k, v) in extra {
        req = req.header(*k, v);
    }
    let resp = req.send().await.map_err(|e| format!("request failed: {e}"))?;
    if resp.status().is_success() { Ok(()) } else { Err(format!("the receiver answered {}", resp.status())) }
}

/// Sends a notification to email addresses.
pub async fn send_email(state: &ApiState, to: &[String], n: &Notification) -> Result<(), String> {
    let mailer =
        state.mailer.as_ref().ok_or_else(|| "email is not configured (RUPTA_SMTP_URL, RUPTA_SMTP_FROM)".to_string())?;
    if to.is_empty() {
        return Err("no recipients".into());
    }
    let html = format!(
        "<p><strong>{}</strong></p><p>{} · {}<br>{}</p><p>Events: {}<br>Rule: {}</p><p><a href=\"{}\">Open the issue</a></p>",
        html_escape(&n.headline()),
        html_escape(&n.short_id),
        html_escape(&n.level),
        html_escape(n.culprit.as_deref().unwrap_or("")),
        n.times_seen,
        html_escape(&n.rule_name),
        html_escape(&n.url)
    );
    mailer.send(to, &n.headline(), &n.text(), Some(&html)).await
}

/// Sends a notification through a channel (`kind` and decrypted `config`).
pub async fn send_channel(state: &ApiState, kind: &str, config: &Value, n: &Notification) -> Result<(), String> {
    let s = |k: &str| config.get(k).and_then(Value::as_str).map(str::to_string);
    match kind {
        "email" => {
            let to: Vec<String> = config
                .get("to")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            send_email(state, &to, n).await
        }
        "slack" => post_json(state, &s("webhook_url").ok_or("missing webhook_url")?, &slack_body(n), &[]).await,
        "teams" => post_json(state, &s("webhook_url").ok_or("missing webhook_url")?, &teams_body(n), &[]).await,
        "webhook" => {
            let url = s("url").ok_or("missing url")?;
            let body = webhook_body(n);
            let bytes = serde_json::to_vec(&body).expect("JSON serializes");
            let ts = Utc::now().timestamp();
            let mut headers = vec![(TIMESTAMP_HEADER, ts.to_string())];
            if let Some(secret) = s("secret").filter(|x| !x.is_empty()) {
                headers.push((SIGNATURE_HEADER, sign(&secret, ts, &bytes)));
            }
            let mut req = state.http.post(&url).header("content-type", "application/json").body(bytes);
            for (k, v) in &headers {
                req = req.header(*k, v);
            }
            let resp = req.send().await.map_err(|e| format!("request failed: {e}"))?;
            if resp.status().is_success() { Ok(()) } else { Err(format!("the receiver answered {}", resp.status())) }
        }
        other => Err(format!("unknown channel kind {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_are_reproducible() {
        let a = sign("s3cret", 1700000000, b"{}");
        assert_eq!(a, sign("s3cret", 1700000000, b"{}"));
        assert_ne!(a, sign("other", 1700000000, b"{}"));
        assert!(a.starts_with("sha256=") && a.len() == 7 + 64);
    }
}
