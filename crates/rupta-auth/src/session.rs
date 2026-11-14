//! Browser sessions.
//!
//! A session is a random 32-byte token in the `rupta_session` cookie. The
//! database stores only its SHA-256, so a copy of the database cannot be
//! used to sign in. Sessions expire 14 days after their last use (sliding
//! expiry; the expiry is pushed forward at most every ten minutes). Each
//! session has its own CSRF token, which mutating requests authenticated
//! by the cookie must echo in the `X-CSRF-Token` header.

use chrono::{DateTime, Duration, Utc};
use rupta_store::Result;
use rupta_store::squad::Connection;

use crate::crypto::{random_token, sha256_hex};

pub const SESSION_COOKIE: &str = "rupta_session";
pub const CSRF_HEADER: &str = "x-csrf-token";

/// Lifetime of a session after its last use.
pub fn ttl() -> Duration {
    Duration::days(14)
}

/// How often the sliding expiry is written.
fn slide_every() -> Duration {
    Duration::minutes(10)
}

/// A session just created; `token` goes into the cookie.
#[derive(Debug, Clone)]
pub struct NewSession {
    pub token: String,
    pub id: String,
    pub csrf_token: String,
    pub expires_at: DateTime<Utc>,
}

/// A valid session.
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: String,
    pub user_id: i64,
    pub csrf_token: String,
    pub is_superuser: bool,
    pub expires_at: DateTime<Utc>,
}

/// Creates a session for a user.
pub fn create(conn: &Connection, user_id: i64, ip: Option<&str>, user_agent: Option<&str>) -> Result<NewSession> {
    let token = random_token();
    let id = sha256_hex(token.as_bytes());
    let csrf_token = random_token();
    let expires_at = Utc::now() + ttl();
    let ua: Option<String> = user_agent.map(|s| s.chars().take(512).collect());
    conn.execute(
        "INSERT INTO web_sessions (id, user_id, csrf_token, ip, user_agent, expires_at) VALUES ($1, $2, $3, $4, $5, $6)",
        &[&id, &user_id, &csrf_token, &ip, &ua, &expires_at],
    )?;
    conn.execute("UPDATE users SET last_login_at = now() WHERE id = $1", &[&user_id])?;
    Ok(NewSession { token, id, csrf_token, expires_at })
}

/// Looks up the session of a cookie value. Expired sessions and sessions
/// of disabled users are not valid. Slides the expiry.
pub fn lookup(conn: &Connection, token: &str) -> Result<Option<SessionInfo>> {
    if token.is_empty() || token.len() > 128 {
        return Ok(None);
    }
    let id = sha256_hex(token.as_bytes());
    let row = conn.query_opt(
        "SELECT s.user_id, s.csrf_token, s.expires_at, u.is_superuser, u.disabled
         FROM web_sessions s JOIN users u ON u.id = s.user_id WHERE s.id = $1",
        &[&id],
    )?;
    let Some(row) = row else { return Ok(None) };
    let expires_at: DateTime<Utc> = row.get(2);
    let disabled: bool = row.get(4);
    let now = Utc::now();
    if expires_at <= now || disabled {
        conn.execute("DELETE FROM web_sessions WHERE id = $1", &[&id])?;
        return Ok(None);
    }
    let mut info = SessionInfo {
        id: id.clone(),
        user_id: row.get(0),
        csrf_token: row.get(1),
        is_superuser: row.get(3),
        expires_at,
    };
    if expires_at - now < ttl() - slide_every() {
        let next = now + ttl();
        conn.execute("UPDATE web_sessions SET expires_at = $1 WHERE id = $2", &[&next, &id])?;
        info.expires_at = next;
    }
    Ok(Some(info))
}

/// Ends a session.
pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM web_sessions WHERE id = $1", &[&id])?;
    Ok(())
}

/// Ends every session of a user, except `keep` (e.g. after a password change).
pub fn delete_for_user(conn: &Connection, user_id: i64, keep: Option<&str>) -> Result<u64> {
    Ok(conn.execute(
        "DELETE FROM web_sessions WHERE user_id = $1 AND ($2::text IS NULL OR id <> $2)",
        &[&user_id, &keep],
    )?)
}

/// Removes expired sessions.
pub fn purge_expired(conn: &Connection) -> Result<u64> {
    Ok(conn.execute("DELETE FROM web_sessions WHERE expires_at <= now()", &[])?)
}

/// `Set-Cookie` value for a session.
pub fn cookie(token: &str, secure: bool) -> String {
    format!(
        "{SESSION_COOKIE}={token}; Path=/; Max-Age={}; HttpOnly; SameSite=Lax{}",
        ttl().num_seconds(),
        if secure { "; Secure" } else { "" }
    )
}

/// `Set-Cookie` value that removes the session cookie.
pub fn clear_cookie(secure: bool) -> String {
    format!("{SESSION_COOKIE}=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax{}", if secure { "; Secure" } else { "" })
}

/// The value of cookie `name` in a `Cookie` header.
pub fn read_cookie<'a>(header: &'a str, name: &str) -> Option<&'a str> {
    header.split(';').filter_map(|p| p.trim().split_once('=')).find(|(k, _)| *k == name).map(|(_, v)| v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rupta_store::Store;

    fn user(conn: &Connection) -> i64 {
        conn.query_one("INSERT INTO users (email) VALUES ('a@example.com') RETURNING id", &[]).unwrap().get(0)
    }

    #[test]
    fn sessions_are_stored_hashed_and_expire() {
        let store = Store::open_in_memory().unwrap();
        let conn = store.connect();
        let uid = user(&conn);
        let s = create(&conn, uid, Some("127.0.0.1"), Some("test")).unwrap();
        let stored: String = conn.query_one("SELECT id FROM web_sessions", &[]).unwrap().get(0);
        assert_ne!(stored, s.token);
        assert_eq!(stored, sha256_hex(s.token.as_bytes()));
        let info = lookup(&conn, &s.token).unwrap().unwrap();
        assert_eq!(info.user_id, uid);
        assert_eq!(info.csrf_token, s.csrf_token);
        assert!(lookup(&conn, "nope").unwrap().is_none());

        // Sliding: an old expiry is pushed forward.
        let soon = Utc::now() + Duration::days(1);
        conn.execute("UPDATE web_sessions SET expires_at = $1", &[&soon]).unwrap();
        let info = lookup(&conn, &s.token).unwrap().unwrap();
        assert!(info.expires_at > Utc::now() + Duration::days(13));

        // Expired sessions are gone.
        let past = Utc::now() - Duration::seconds(1);
        conn.execute("UPDATE web_sessions SET expires_at = $1", &[&past]).unwrap();
        assert!(lookup(&conn, &s.token).unwrap().is_none());
    }

    #[test]
    fn disabled_users_lose_their_sessions() {
        let store = Store::open_in_memory().unwrap();
        let conn = store.connect();
        let uid = user(&conn);
        let s = create(&conn, uid, None, None).unwrap();
        conn.execute("UPDATE users SET disabled = true", &[]).unwrap();
        assert!(lookup(&conn, &s.token).unwrap().is_none());
    }

    #[test]
    fn cookies() {
        let c = cookie("abc", true);
        assert!(c.contains("HttpOnly") && c.contains("SameSite=Lax") && c.contains("Secure"));
        assert!(!cookie("abc", false).contains("Secure"));
        assert_eq!(read_cookie("a=1; rupta_session=xyz; b=2", SESSION_COOKIE), Some("xyz"));
        assert_eq!(read_cookie("a=1", SESSION_COOKIE), None);
    }
}
