//! The audit log of security-relevant actions.

use rupta_store::Result;
use rupta_store::squad::Connection;
use serde_json::Value;

/// Actions recorded in the audit log.
pub mod action {
    pub const LOGIN: &str = "auth.login";
    pub const LOGIN_FAILED: &str = "auth.login_failed";
    pub const LOGIN_LOCKED: &str = "auth.login_locked";
    pub const LOGOUT: &str = "auth.logout";
    pub const SSO_LOGIN: &str = "auth.sso_login";
    pub const SSO_LOGIN_FAILED: &str = "auth.sso_login_failed";
    pub const SETUP: &str = "auth.setup";
    pub const PASSWORD_CHANGE: &str = "user.password_change";
    pub const USER_UPDATE: &str = "user.update";
    pub const ORG_CREATE: &str = "org.create";
    pub const ORG_UPDATE: &str = "org.update";
    pub const ORG_DELETE: &str = "org.delete";
    pub const MEMBER_INVITE: &str = "member.invite";
    pub const MEMBER_INVITE_CANCEL: &str = "member.invite_cancel";
    pub const MEMBER_JOIN: &str = "member.join";
    pub const MEMBER_ROLE_CHANGE: &str = "member.role_change";
    pub const MEMBER_REMOVE: &str = "member.remove";
    pub const TEAM_CREATE: &str = "team.create";
    pub const TEAM_UPDATE: &str = "team.update";
    pub const TEAM_DELETE: &str = "team.delete";
    pub const TOKEN_CREATE: &str = "token.create";
    pub const TOKEN_DELETE: &str = "token.delete";
    pub const SSO_CONFIG_CHANGE: &str = "sso.config_change";
    pub const SSO_CONFIG_DELETE: &str = "sso.config_delete";
    pub const PROJECT_CREATE: &str = "project.create";
    pub const PROJECT_UPDATE: &str = "project.update";
    pub const PROJECT_DELETE: &str = "project.delete";
    pub const KEY_CREATE: &str = "key.create";
    pub const KEY_UPDATE: &str = "key.update";
    pub const KEY_DELETE: &str = "key.delete";
    pub const CHANNEL_CREATE: &str = "channel.create";
    pub const CHANNEL_UPDATE: &str = "channel.update";
    pub const CHANNEL_DELETE: &str = "channel.delete";
    pub const ALERT_RULE_CREATE: &str = "alert_rule.create";
    pub const ALERT_RULE_UPDATE: &str = "alert_rule.update";
    pub const ALERT_RULE_DELETE: &str = "alert_rule.delete";
    pub const ISSUE_DELETE: &str = "issue.delete";
}

/// One audit log entry.
#[derive(Debug, Clone, Default)]
pub struct Entry {
    pub org_id: Option<i64>,
    pub actor_user_id: Option<i64>,
    pub actor_token_id: Option<i64>,
    pub action: &'static str,
    pub target_type: Option<&'static str>,
    pub target_id: Option<String>,
    pub data: Value,
    pub ip: Option<String>,
}

/// Writes an entry.
pub fn record(conn: &Connection, e: &Entry) -> Result<()> {
    let data = if e.data.is_null() { Value::Object(Default::default()) } else { e.data.clone() };
    conn.execute(
        "INSERT INTO audit_log (org_id, actor_user_id, actor_token_id, action, target_type, target_id, data, ip)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        &[&e.org_id, &e.actor_user_id, &e.actor_token_id, &e.action, &e.target_type, &e.target_id, &data, &e.ip],
    )?;
    Ok(())
}
