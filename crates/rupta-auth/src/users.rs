//! Users, organizations, memberships and the first-start bootstrap.

use rupta_core::role::Role;
use rupta_store::squad::Connection;
use rupta_store::{Result, StoreError};
use serde::Serialize;

/// A user as the API shows it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct User {
    pub id: i64,
    pub email: String,
    pub name: String,
    pub avatar_url: Option<String>,
    pub is_superuser: bool,
    pub disabled: bool,
    pub has_password: bool,
}

const USER_COLUMNS: &str = "id, email, name, avatar_url, is_superuser, disabled, password_hash IS NOT NULL";

fn user_from(row: &rupta_store::squad::Row) -> User {
    User {
        id: row.get(0),
        email: row.get(1),
        name: row.get(2),
        avatar_url: row.get(3),
        is_superuser: row.get(4),
        disabled: row.get(5),
        has_password: row.get(6),
    }
}

/// Lowercases and checks an email address. Deliberately permissive: one
/// `@`, something on both sides, a dot in the domain, no spaces.
pub fn normalize_email(email: &str) -> Option<String> {
    let e = email.trim().to_lowercase();
    if e.len() > 254 || e.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let (local, domain) = e.split_once('@')?;
    if local.is_empty()
        || domain.contains('@')
        || !domain.contains('.')
        || domain.starts_with('.')
        || domain.ends_with('.')
    {
        return None;
    }
    Some(e)
}

/// Slugs: lowercase letters, digits and `-`, 1 to 50 characters, starting
/// with a letter or digit.
pub fn valid_slug(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 50
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !s.starts_with('-')
}

/// A slug derived from a name.
pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').chars().take(50).collect::<String>();
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() { "org".into() } else { out }
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<User>> {
    Ok(conn.query_opt(&format!("SELECT {USER_COLUMNS} FROM users WHERE id = $1"), &[&id])?.map(|r| user_from(&r)))
}

/// A user and the stored password hash, by (normalized) email.
pub fn find_by_email(conn: &Connection, email: &str) -> Result<Option<(User, Option<String>)>> {
    Ok(conn
        .query_opt(&format!("SELECT {USER_COLUMNS}, password_hash FROM users WHERE email = $1"), &[&email])?
        .map(|r| (user_from(&r), r.get(7))))
}

pub fn count(conn: &Connection) -> Result<i64> {
    Ok(conn.query_one("SELECT count(*) FROM users", &[])?.get(0))
}

/// Creates a user; `email` must be normalized.
pub fn create(
    conn: &Connection,
    email: &str,
    name: &str,
    password_hash: Option<&str>,
    is_superuser: bool,
) -> Result<i64> {
    Ok(conn
        .query_one(
            "INSERT INTO users (email, name, password_hash, is_superuser) VALUES ($1, $2, $3, $4) RETURNING id",
            &[&email, &name, &password_hash, &is_superuser],
        )?
        .get(0))
}

pub fn org_id_by_slug(conn: &Connection, slug: &str) -> Result<Option<i64>> {
    Ok(conn.query_opt("SELECT id FROM organizations WHERE slug = $1", &[&slug])?.map(|r| r.get(0)))
}

pub fn create_org(conn: &Connection, slug: &str, name: &str) -> Result<i64> {
    Ok(conn.query_one("INSERT INTO organizations (slug, name) VALUES ($1, $2) RETURNING id", &[&slug, &name])?.get(0))
}

/// The role of a user in an organization.
pub fn membership(conn: &Connection, org_id: i64, user_id: i64) -> Result<Option<Role>> {
    Ok(conn
        .query_opt("SELECT role FROM members WHERE org_id = $1 AND user_id = $2", &[&org_id, &user_id])?
        .and_then(|r| Role::parse(&r.get::<String>(0))))
}

/// Adds a member or changes the role of an existing one.
pub fn set_member(conn: &Connection, org_id: i64, user_id: i64, role: Role) -> Result<()> {
    conn.execute(
        "INSERT INTO members (org_id, user_id, role) VALUES ($1, $2, $3)
         ON CONFLICT (org_id, user_id) DO UPDATE SET role = excluded.role",
        &[&org_id, &user_id, &role.as_str()],
    )?;
    Ok(())
}

/// Number of owners of an organization.
pub fn owner_count(conn: &Connection, org_id: i64) -> Result<i64> {
    Ok(conn.query_one("SELECT count(*) FROM members WHERE org_id = $1 AND role = 'owner'", &[&org_id])?.get(0))
}

/// What the first start created.
#[derive(Debug, Clone, PartialEq)]
pub struct Bootstrapped {
    pub user_id: i64,
    pub org_id: i64,
}

/// Creates the first user (a superuser), an organization and an owner
/// membership, in one transaction, if there are no users yet. Returns
/// `None` if users exist. `password_hash` is an Argon2 PHC string.
pub fn bootstrap(
    conn: &Connection,
    email: &str,
    name: &str,
    password_hash: &str,
    org_slug: &str,
    org_name: &str,
) -> Result<Option<Bootstrapped>> {
    let email =
        normalize_email(email).ok_or_else(|| StoreError::Invalid(format!("invalid email address {email:?}")))?;
    if !valid_slug(org_slug) {
        return Err(StoreError::Invalid(format!("invalid organization slug {org_slug:?}")));
    }
    let txn = conn.transaction()?;
    // The lock makes concurrent setup requests wait and then see the user.
    txn.batch_execute("LOCK TABLE users IN EXCLUSIVE MODE")?;
    if count(&txn)? > 0 {
        return Ok(None);
    }
    let user_id = create(&txn, &email, name, Some(password_hash), true)?;
    let org_id = match org_id_by_slug(&txn, org_slug)? {
        Some(id) => id,
        None => create_org(&txn, org_slug, org_name)?,
    };
    set_member(&txn, org_id, user_id, Role::Owner)?;
    txn.commit()?;
    Ok(Some(Bootstrapped { user_id, org_id }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rupta_store::Store;

    #[test]
    fn emails_and_slugs() {
        assert_eq!(normalize_email(" Ada@Example.COM ").as_deref(), Some("ada@example.com"));
        for bad in ["", "ada", "ada@", "@x.org", "a b@x.org", "a@x", "a@@x.org", "a@x.org."] {
            assert!(normalize_email(bad).is_none(), "{bad}");
        }
        assert!(valid_slug("my-org-2"));
        for bad in ["", "-x", "My", "a_b", &"a".repeat(51)] {
            assert!(!valid_slug(bad), "{bad}");
        }
        assert_eq!(slugify("  Acme Corp, Inc. "), "acme-corp-inc");
        assert_eq!(slugify("!!!"), "org");
    }

    #[test]
    fn bootstrap_runs_once() {
        let store = Store::open_in_memory().unwrap();
        let conn = store.connect();
        let b = bootstrap(&conn, "Admin@Example.com", "Admin", "$argon2id$x", "default", "Default").unwrap().unwrap();
        assert_eq!(membership(&conn, b.org_id, b.user_id).unwrap(), Some(Role::Owner));
        let (user, _) = find_by_email(&conn, "admin@example.com").unwrap().unwrap();
        assert!(user.is_superuser && user.has_password);
        assert!(bootstrap(&conn, "other@example.com", "", "$argon2id$x", "default", "Default").unwrap().is_none());
        assert_eq!(count(&conn).unwrap(), 1);
    }
}
