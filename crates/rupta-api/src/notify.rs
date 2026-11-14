//! Delivery of notifications: email (SMTP), Slack incoming webhooks,
//! Microsoft Teams webhooks, generic webhooks signed with HMAC-SHA256,
//! PagerDuty (Events API v2) and Opsgenie (Alert API v2). PagerDuty
//! incidents and Opsgenie alerts are keyed by the issue id and resolved
//! (closed) when the issue is resolved in rupta.

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

    pub fn headline(&self) -> String {
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

    pub fn text(&self) -> String {
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

/// Posts JSON to a channel URL through the guarded outbound client: the
/// host must not resolve to an internal address (unless
/// `RUPTA_INTEGRATIONS_ALLOW_PRIVATE_HOSTS` is set), redirects are not
/// followed, and requests time out. Notifications are not retried, so a
/// receiver never gets the same message twice.
async fn post_channel(
    state: &ApiState,
    url: &str,
    body: Vec<u8>,
    extra: &[(&'static str, String)],
) -> Result<(), String> {
    let u = url::Url::parse(url).map_err(|e| format!("invalid URL: {e}"))?;
    let mut req = crate::integrations::http::Req::new(reqwest::Method::POST, u).idempotent(false);
    req.body = Some(body);
    req = req.header("content-type", "application/json");
    for (k, v) in extra {
        req = req.header(k, v.clone());
    }
    let resp = state.outbound.send(req).await.map_err(|e| e.to_string())?;
    if (200..300).contains(&resp.status) { Ok(()) } else { Err(format!("the receiver answered {}", resp.status)) }
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
        "slack" => {
            let body = serde_json::to_vec(&slack_body(n)).expect("JSON serializes");
            post_channel(state, &s("webhook_url").ok_or("missing webhook_url")?, body, &[]).await
        }
        "teams" => {
            let body = serde_json::to_vec(&teams_body(n)).expect("JSON serializes");
            post_channel(state, &s("webhook_url").ok_or("missing webhook_url")?, body, &[]).await
        }
        "webhook" => {
            let url = s("url").ok_or("missing url")?;
            let body = webhook_body(n);
            let bytes = serde_json::to_vec(&body).expect("JSON serializes");
            let ts = Utc::now().timestamp();
            let mut headers = vec![(TIMESTAMP_HEADER, ts.to_string())];
            if let Some(secret) = s("secret").filter(|x| !x.is_empty()) {
                headers.push((SIGNATURE_HEADER, sign(&secret, ts, &bytes)));
            }
            post_channel(state, &url, bytes, &headers).await
        }
        "pagerduty" => pagerduty_trigger(state, config, n).await,
        "opsgenie" => opsgenie_create(state, config, n).await,
        other => Err(format!("unknown channel kind {other}")),
    }
}

/// PagerDuty Events API v2 endpoint of a channel.
fn pagerduty_url(config: &Value) -> String {
    if let Some(u) = config.get("api_url").and_then(Value::as_str).filter(|u| !u.is_empty()) {
        return u.to_string();
    }
    match config.get("region").and_then(Value::as_str) {
        Some("eu") => "https://events.eu.pagerduty.com/v2/enqueue".into(),
        _ => "https://events.pagerduty.com/v2/enqueue".into(),
    }
}

/// Opsgenie API base URL of a channel.
fn opsgenie_url(config: &Value) -> String {
    if let Some(u) = config.get("api_url").and_then(Value::as_str).filter(|u| !u.is_empty()) {
        return u.trim_end_matches('/').to_string();
    }
    match config.get("region").and_then(Value::as_str) {
        Some("eu") => "https://api.eu.opsgenie.com".into(),
        _ => "https://api.opsgenie.com".into(),
    }
}

/// PagerDuty severity of an event level.
pub fn pagerduty_severity(level: &str) -> &'static str {
    match level {
        "fatal" => "critical",
        "error" => "error",
        "warning" => "warning",
        _ => "info",
    }
}

/// Opsgenie priority of an event level.
pub fn opsgenie_priority(level: &str) -> &'static str {
    match level {
        "fatal" => "P1",
        "error" => "P2",
        "warning" => "P3",
        "info" => "P4",
        _ => "P5",
    }
}

/// The PagerDuty event that triggers (or updates) the incident of an issue.
pub fn pagerduty_event(routing_key: &str, n: &Notification) -> Value {
    let mut details = json!({
        "short_id": n.short_id, "project": n.project_slug, "level": n.level, "events": n.times_seen,
        "trigger": n.trigger, "rule": n.rule_name,
    });
    if let Some(c) = &n.culprit {
        details["culprit"] = json!(c);
    }
    if let Some(e) = &n.environment {
        details["environment"] = json!(e);
    }
    json!({
        "routing_key": routing_key,
        "event_action": "trigger",
        "dedup_key": n.issue_id,
        "payload": {
            "summary": truncate(&n.headline(), 1000),
            "source": n.project_slug,
            "severity": pagerduty_severity(&n.level),
            "timestamp": Utc::now().to_rfc3339(),
            "component": n.culprit.clone().map(|c| truncate(&c, 200)),
            "group": n.environment,
            "class": n.trigger,
            "custom_details": details,
        },
        "links": [{"href": n.url, "text": format!("{} in rupta", n.short_id)}],
        "client": "rupta",
        "client_url": n.url,
    })
}

/// The Opsgenie alert of an issue.
pub fn opsgenie_alert(n: &Notification) -> Value {
    let mut tags = vec!["rupta".to_string(), n.project_slug.clone(), n.level.clone()];
    if let Some(e) = &n.environment {
        tags.push(e.clone());
    }
    json!({
        "message": truncate(&n.headline(), 129),
        "alias": n.issue_id,
        "description": truncate(&n.text(), 14_000),
        "priority": opsgenie_priority(&n.level),
        "source": "rupta",
        "entity": n.project_slug,
        "tags": tags,
        "details": {
            "short_id": n.short_id, "project": n.project_slug, "level": n.level,
            "events": n.times_seen.to_string(), "url": n.url, "rule": n.rule_name,
        },
    })
}

async fn post_outbound(state: &ApiState, url: &str, body: &Value, auth: Option<String>) -> Result<(), String> {
    let u = url::Url::parse(url).map_err(|e| format!("invalid URL {url}: {e}"))?;
    let mut req = crate::integrations::http::Req::new(reqwest::Method::POST, u).json(body).idempotent(true);
    if let Some(a) = auth {
        req = req.header("authorization", a);
    }
    let resp = state.outbound.send(req).await.map_err(|e| e.to_string())?;
    if (200..300).contains(&resp.status) {
        Ok(())
    } else {
        Err(format!("the receiver answered {}: {}", resp.status, crate::integrations::http::snippet(&resp.body)))
    }
}

async fn pagerduty_trigger(state: &ApiState, config: &Value, n: &Notification) -> Result<(), String> {
    let key = config.get("secret").and_then(Value::as_str).filter(|k| !k.is_empty()).ok_or("missing routing key")?;
    let url = pagerduty_url(config);
    if n.trigger == "test" {
        // A test opens an incident and resolves it right away.
        let mut ev = pagerduty_event(key, n);
        let dedup = format!("rupta-test-{}", Utc::now().timestamp_millis());
        ev["dedup_key"] = json!(dedup);
        post_outbound(state, &url, &ev, None).await?;
        return post_outbound(
            state,
            &url,
            &json!({"routing_key": key, "event_action": "resolve", "dedup_key": dedup}),
            None,
        )
        .await;
    }
    post_outbound(state, &url, &pagerduty_event(key, n), None).await
}

async fn opsgenie_create(state: &ApiState, config: &Value, n: &Notification) -> Result<(), String> {
    let key = config.get("secret").and_then(Value::as_str).filter(|k| !k.is_empty()).ok_or("missing API key")?;
    let base = opsgenie_url(config);
    let mut alert = opsgenie_alert(n);
    if n.trigger == "test" {
        alert["alias"] = json!(format!("rupta-test-{}", Utc::now().timestamp_millis()));
    }
    post_outbound(state, &format!("{base}/v2/alerts"), &alert, Some(format!("GenieKey {key}"))).await?;
    if n.trigger == "test" {
        return opsgenie_close(state, config, alert["alias"].as_str().unwrap_or_default()).await;
    }
    Ok(())
}

async fn opsgenie_close(state: &ApiState, config: &Value, alias: &str) -> Result<(), String> {
    let key = config.get("secret").and_then(Value::as_str).filter(|k| !k.is_empty()).ok_or("missing API key")?;
    let mut u = url::Url::parse(&format!("{}/v2/alerts", opsgenie_url(config))).map_err(|e| e.to_string())?;
    u.path_segments_mut().map_err(|_| "invalid Opsgenie URL")?.push(alias).push("close");
    u.query_pairs_mut().append_pair("identifierType", "alias");
    post_outbound(
        state,
        u.as_str(),
        &json!({"source": "rupta", "note": "Resolved in rupta"}),
        Some(format!("GenieKey {key}")),
    )
    .await
}

/// True for channel kinds that open incidents to resolve later.
pub fn opens_incidents(kind: &str) -> bool {
    matches!(kind, "pagerduty" | "opsgenie")
}

/// Resolves the PagerDuty incidents and closes the Opsgenie alerts opened
/// for an issue. Failures are logged and counted; the incident stays
/// recorded and is tried again at the next resolve.
pub async fn resolve_incidents(state: &ApiState, issue_id: i64) {
    let st = state.clone();
    let open = state
        .db(move |c| {
            Ok(c.query(
                "SELECT ci.channel_id, ci.dedup_key, nc.kind, nc.config, nc.org_id FROM channel_incidents ci
                 JOIN notification_channels nc ON nc.id = ci.channel_id
                 WHERE ci.issue_id = $1 AND ci.resolved_at IS NULL",
                &[&issue_id],
            )?
            .iter()
            .map(|r| {
                let org: i64 = r.get(4);
                (
                    r.get::<i64>(0),
                    r.get::<String>(1),
                    r.get::<String>(2),
                    crate::routes::alerts::sending_config(&st, org, &r.get::<Value>(3)),
                )
            })
            .collect::<Vec<_>>())
        })
        .await;
    let open = match open {
        Ok(o) => o,
        Err(e) => return tracing::warn!(error = %e.message, "loading open incidents failed"),
    };
    for (channel_id, dedup, kind, config) in open {
        let r = match kind.as_str() {
            "pagerduty" => {
                let key = config.get("secret").and_then(Value::as_str).unwrap_or_default().to_string();
                post_outbound(
                    state,
                    &pagerduty_url(&config),
                    &json!({"routing_key": key, "event_action": "resolve", "dedup_key": dedup}),
                    None,
                )
                .await
            }
            "opsgenie" => opsgenie_close(state, &config, &dedup).await,
            _ => Ok(()),
        };
        let result = if r.is_ok() { "success" } else { "failure" };
        state.metrics.count("rupta_alert_notifications_total", &[("channel", kind.as_str()), ("result", result)]);
        match r {
            Ok(()) => {
                let _ = state
                    .db(move |c| {
                        c.execute(
                            "UPDATE channel_incidents SET resolved_at = now() WHERE issue_id = $1 AND channel_id = $2",
                            &[&issue_id, &channel_id],
                        )?;
                        Ok(())
                    })
                    .await;
            }
            Err(e) => tracing::warn!(channel = kind, error = %e, issue = issue_id, "resolving an incident failed"),
        }
    }
}

/// Records that a channel opened an incident for an issue (or reopened
/// it: a new trigger with the same key).
pub async fn record_incident(state: &ApiState, issue_id: i64, channel_id: i64) {
    let r = state
        .db(move |c| {
            c.execute(
                "INSERT INTO channel_incidents (issue_id, channel_id, dedup_key) VALUES ($1, $2, $3)
                 ON CONFLICT (issue_id, channel_id) DO UPDATE SET opened_at = now(), resolved_at = NULL",
                &[&issue_id, &channel_id, &issue_id.to_string()],
            )?;
            Ok(())
        })
        .await;
    if let Err(e) = r {
        tracing::warn!(error = %e.message, "recording an incident failed");
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

    fn n(level: &str) -> Notification {
        Notification {
            trigger: "first_seen".into(),
            rule_id: Some("1".into()),
            rule_name: "r".into(),
            org_slug: "acme".into(),
            project_slug: "web".into(),
            project_name: "Web".into(),
            issue_id: "42".into(),
            short_id: "WEB-1".into(),
            title: "x".repeat(300),
            culprit: Some("app.js".into()),
            level: level.into(),
            times_seen: 3,
            environment: Some("prod".into()),
            url: "https://r/organizations/acme/issues/42".into(),
        }
    }

    #[test]
    fn pagerduty_events_follow_the_v2_format() {
        let e = pagerduty_event("rk", &n("fatal"));
        assert_eq!(e["event_action"], "trigger");
        assert_eq!(e["dedup_key"], "42");
        assert_eq!(e["payload"]["severity"], "critical");
        assert_eq!(e["payload"]["source"], "web");
        assert!(e["payload"]["summary"].as_str().unwrap().chars().count() <= 1024);
        assert_eq!(pagerduty_url(&json!({"region": "eu"})), "https://events.eu.pagerduty.com/v2/enqueue");
        assert_eq!(pagerduty_url(&json!({})), "https://events.pagerduty.com/v2/enqueue");
    }

    #[test]
    fn opsgenie_alerts_respect_limits() {
        let a = opsgenie_alert(&n("warning"));
        assert!(a["message"].as_str().unwrap().chars().count() <= 130);
        assert_eq!(a["alias"], "42");
        assert_eq!(a["priority"], "P3");
        assert_eq!(opsgenie_url(&json!({"region": "eu"})), "https://api.eu.opsgenie.com");
    }
}
