//! API tokens: `rpt_` followed by 40 random alphanumeric characters.
//!
//! Only the SHA-256 of a token is stored, plus its last four characters
//! for display. A token belongs either to a user (it acts as that user,
//! limited by its scopes) or to an organization (it acts within that
//! organization only, limited by its scopes and the admin role).
//! `last_used_at` is written at most once a minute per token.

use chrono::{DateTime, Utc};
use rupta_store::Result;
use rupta_store::squad::Connection;

use crate::crypto::{ct_eq, random_alphanumeric, sha256_hex};
use crate::rbac::TokenGrant;

pub const TOKEN_PREFIX: &str = "rpt_";
const TOKEN_CHARS: usize = 40;

/// A new random token.
pub fn generate() -> String {
    format!("{TOKEN_PREFIX}{}", random_alphanumeric(TOKEN_CHARS))
}

/// True if `s` has the shape of a token.
pub fn looks_like_token(s: &str) -> bool {
    s.len() == TOKEN_PREFIX.len() + TOKEN_CHARS
        && s.starts_with(TOKEN_PREFIX)
        && s[TOKEN_PREFIX.len()..].bytes().all(|b| b.is_ascii_alphanumeric())
}

/// A created token; `token` is shown once.
#[derive(Debug, Clone)]
pub struct CreatedToken {
    pub id: i64,
    pub token: String,
    pub hint: String,
    pub created_at: DateTime<Utc>,
}

/// Stores a new token.
pub fn create(
    conn: &Connection,
    name: &str,
    user_id: Option<i64>,
    org_id: Option<i64>,
    scopes: &[String],
    expires_at: Option<DateTime<Utc>>,
) -> Result<CreatedToken> {
    let token = generate();
    let hash = sha256_hex(token.as_bytes());
    let hint = token[token.len() - 4..].to_string();
    let scopes = serde_json::to_value(scopes).expect("strings serialize");
    let row = conn.query_one(
        "INSERT INTO api_tokens (token_hash, token_hint, name, user_id, org_id, scopes, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id, created_at",
        &[&hash, &hint, &name, &user_id, &org_id, &scopes, &expires_at],
    )?;
    Ok(CreatedToken { id: row.get(0), token, hint, created_at: row.get(1) })
}

/// A token that authenticated a request.
#[derive(Debug, Clone)]
pub struct TokenAuth {
    pub grant: TokenGrant,
    pub user_id: Option<i64>,
    pub is_superuser: bool,
}

/// Checks a presented token. Unknown, expired tokens and tokens of
/// disabled users are refused.
pub fn authenticate(conn: &Connection, token: &str) -> Result<Option<TokenAuth>> {
    if !looks_like_token(token) {
        return Ok(None);
    }
    let hash = sha256_hex(token.as_bytes());
    let row = conn.query_opt(
        "SELECT t.id, t.token_hash, t.user_id, t.org_id, t.scopes, t.expires_at,
                coalesce(u.is_superuser, false), coalesce(u.disabled, false),
                t.last_used_at IS NULL OR t.last_used_at < now() - interval '1 minute'
         FROM api_tokens t LEFT JOIN users u ON u.id = t.user_id
         WHERE t.token_hash = $1",
        &[&hash],
    )?;
    let Some(row) = row else { return Ok(None) };
    let stored: String = row.get(1);
    if !ct_eq(stored.as_bytes(), hash.as_bytes()) {
        return Ok(None);
    }
    let expires_at: Option<DateTime<Utc>> = row.get(5);
    if expires_at.is_some_and(|e| e <= Utc::now()) || row.get::<bool>(7) {
        return Ok(None);
    }
    let id: i64 = row.get(0);
    let user_id: Option<i64> = row.get(2);
    let org_id: Option<i64> = row.get(3);
    if user_id.is_none() && org_id.is_none() {
        return Ok(None);
    }
    let scopes: serde_json::Value = row.get(4);
    let scopes: Vec<String> = serde_json::from_value(scopes).unwrap_or_default();
    if row.get::<bool>(8) {
        conn.execute("UPDATE api_tokens SET last_used_at = now() WHERE id = $1", &[&id])?;
    }
    Ok(Some(TokenAuth { grant: TokenGrant { id, org_id, scopes }, user_id, is_superuser: row.get(6) }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rupta_store::Store;

    #[test]
    fn tokens_have_the_documented_shape() {
        let t = generate();
        assert!(looks_like_token(&t), "{t}");
        assert!(!looks_like_token("rpt_short"));
        assert!(!looks_like_token(&t.replace("rpt_", "xyz_")));
    }

    #[test]
    fn tokens_authenticate_until_they_expire() {
        let store = Store::open_in_memory().unwrap();
        let conn = store.connect();
        let uid: i64 = conn.query_one("INSERT INTO users (email) VALUES ('a@x.org') RETURNING id", &[]).unwrap().get(0);
        let created = create(&conn, "ci", Some(uid), None, &["project:read".into()], None).unwrap();
        let stored: String = conn.query_one("SELECT token_hash FROM api_tokens", &[]).unwrap().get(0);
        assert!(!stored.contains(&created.token[4..]));
        let auth = authenticate(&conn, &created.token).unwrap().unwrap();
        assert_eq!(auth.user_id, Some(uid));
        assert_eq!(auth.grant.scopes, vec!["project:read".to_string()]);
        let used: Option<DateTime<Utc>> = conn.query_one("SELECT last_used_at FROM api_tokens", &[]).unwrap().get(0);
        assert!(used.is_some());
        assert!(authenticate(&conn, &generate()).unwrap().is_none());

        let past = Utc::now() - chrono::Duration::seconds(1);
        conn.execute("UPDATE api_tokens SET expires_at = $1", &[&past]).unwrap();
        assert!(authenticate(&conn, &created.token).unwrap().is_none());
    }

    #[test]
    fn last_use_is_throttled() {
        let store = Store::open_in_memory().unwrap();
        let conn = store.connect();
        let org: i64 = conn
            .query_one("INSERT INTO organizations (slug, name) VALUES ('o', 'O') RETURNING id", &[])
            .unwrap()
            .get(0);
        let created = create(&conn, "ci", None, Some(org), &["project:releases".into()], None).unwrap();
        authenticate(&conn, &created.token).unwrap().unwrap();
        let marker = Utc::now() - chrono::Duration::seconds(30);
        conn.execute("UPDATE api_tokens SET last_used_at = $1", &[&marker]).unwrap();
        authenticate(&conn, &created.token).unwrap().unwrap();
        let used: DateTime<Utc> = conn.query_one("SELECT last_used_at FROM api_tokens", &[]).unwrap().get(0);
        assert_eq!(used.timestamp_micros(), marker.timestamp_micros(), "not rewritten within a minute");
    }
}
