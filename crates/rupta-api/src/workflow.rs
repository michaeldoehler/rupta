//! Workflow emails: an issue was assigned to you (or your team), and
//! someone commented on an issue assigned to you. Sent only when SMTP is
//! configured and the recipient's `notifications.workflow` preference is on;
//! the person who made the change never gets an email about it.

use rupta_store::squad::Connection;

use crate::error::ApiResult;
use crate::mail::html_escape;
use crate::preferences::{self, Kind};
use crate::routes::issues::Assigned;
use crate::state::{ApiMetrics, ApiState};

/// What the email is about.
struct IssueInfo {
    org_id: i64,
    project_slug: String,
    short_id: String,
    title: String,
    url: String,
    assignee_user: Option<i64>,
    assignee_team: Option<i64>,
}

fn issue_info(state: &ApiState, c: &Connection, issue_id: i64) -> ApiResult<Option<IssueInfo>> {
    let row = c.query_opt(
        "SELECT o.id, o.slug, p.slug, i.short_id, i.title, i.assignee_user_id, i.assignee_team_id
         FROM issues i JOIN projects p ON p.id = i.project_id JOIN organizations o ON o.id = p.org_id WHERE i.id = $1",
        &[&issue_id],
    )?;
    Ok(row.map(|r| {
        let org_slug: String = r.get(1);
        let project_slug: String = r.get(2);
        IssueInfo {
            org_id: r.get(0),
            short_id: format!("{}-{}", project_slug.to_uppercase(), r.get::<i64>(3)),
            project_slug,
            title: r.get(4),
            url: format!("{}/organizations/{org_slug}/issues/{issue_id}", state.config.public_url),
            assignee_user: r.get(5),
            assignee_team: r.get(6),
        }
    }))
}

/// Recipients for an assignee (a user, or the members of a team), without
/// the actor, filtered by preference.
fn assignee_recipients(
    c: &Connection,
    org_id: i64,
    user: Option<i64>,
    team: Option<i64>,
    actor: Option<i64>,
) -> ApiResult<Vec<String>> {
    let mut ids: Vec<i64> = user.into_iter().collect();
    if let Some(t) = team {
        ids.extend(preferences::team_members(c, org_id, &[t])?);
    }
    ids.retain(|id| Some(*id) != actor);
    preferences::recipients(c, org_id, &ids, Kind::Workflow)
}

struct Mail {
    to: Vec<String>,
    subject: String,
    text: String,
    html: String,
}

fn send_all(state: &ApiState, mails: Vec<Mail>) {
    let Some(mailer) = state.mailer.clone() else { return };
    let state = state.clone();
    tokio::spawn(async move {
        for m in mails {
            match mailer.send(&m.to, &m.subject, &m.text, Some(&m.html)).await {
                Ok(()) => ApiMetrics::inc(&state.metrics.notifications_sent),
                Err(e) => {
                    ApiMetrics::inc(&state.metrics.notification_failures);
                    tracing::warn!(error = %e, "workflow email failed");
                }
            }
        }
    });
}

fn mail(to: Vec<String>, i: &IssueInfo, headline: &str, body: Option<&str>) -> Mail {
    let subject = format!("[{}] {headline}: {} {}", i.project_slug, i.short_id, truncate(&i.title, 150));
    let mut text = format!("{headline}\n\n{} {}\n", i.short_id, i.title);
    let mut html = format!(
        "<p><strong>{}</strong></p><p>{} {}</p>",
        html_escape(headline),
        html_escape(&i.short_id),
        html_escape(&i.title)
    );
    if let Some(b) = body {
        text.push_str(&format!("\n{b}\n"));
        html.push_str(&format!("<blockquote>{}</blockquote>", html_escape(b).replace('\n', "<br>")));
    }
    text.push_str(&format!("\n{}\n", i.url));
    html.push_str(&format!("<p><a href=\"{}\">Open the issue</a></p>", html_escape(&i.url)));
    Mail { to, subject, text, html }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n).collect::<String>()) }
}

/// Emails the new assignees of issues (in the background).
pub(crate) fn assigned(state: &ApiState, actor: Option<i64>, assigned: Vec<Assigned>) {
    if state.mailer.is_none() || assigned.is_empty() {
        return;
    }
    let st = state.clone();
    let db = state.clone();
    tokio::spawn(async move {
        let mails = db
            .db(move |c| {
                let mut out = Vec::new();
                for a in assigned {
                    let Some(info) = issue_info(&st, c, a.issue_id)? else { continue };
                    let to = assignee_recipients(c, info.org_id, a.user_id, a.team_id, actor)?;
                    if !to.is_empty() {
                        let what = if a.team_id.is_some() { "Assigned to your team" } else { "Assigned to you" };
                        out.push(mail(to, &info, what, None));
                    }
                }
                Ok(out)
            })
            .await;
        match mails {
            Ok(m) => send_all(&db, m),
            Err(e) => tracing::warn!(error = %e.message, "preparing assignment emails failed"),
        }
    });
}

/// Emails the assignees of an issue about a new comment (in the background).
pub(crate) fn commented(state: &ApiState, author: i64, issue_id: i64, text: String) {
    if state.mailer.is_none() {
        return;
    }
    let st = state.clone();
    let db = state.clone();
    tokio::spawn(async move {
        let mails = db
            .db(move |c| {
                let Some(info) = issue_info(&st, c, issue_id)? else { return Ok(Vec::new()) };
                let to = assignee_recipients(c, info.org_id, info.assignee_user, info.assignee_team, Some(author))?;
                if to.is_empty() {
                    return Ok(Vec::new());
                }
                let name: String = c
                    .query_opt(
                        "SELECT CASE WHEN name = '' THEN email ELSE name END FROM users WHERE id = $1",
                        &[&author],
                    )?
                    .map(|r| r.get(0))
                    .unwrap_or_default();
                let headline = format!("New comment by {name}");
                Ok(vec![mail(to, &info, &headline, Some(&truncate(&text, 2000)))])
            })
            .await;
        match mails {
            Ok(m) => send_all(&db, m),
            Err(e) => tracing::warn!(error = %e.message, "preparing comment emails failed"),
        }
    });
}
