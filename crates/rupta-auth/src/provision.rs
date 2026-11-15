//! Turning a verified single sign-on identity into a rupta user and an
//! organization membership.
//!
//! 1. A known identity (`provider`, `subject`) signs in its linked user.
//! 2. Otherwise an existing user with the same email is linked, but only
//!    if the provider says the email is verified.
//! 3. Otherwise, with just-in-time provisioning on, a new user (without a
//!    password) is created.
//!
//! The user then becomes a member of the provider's organization. A role
//! asserted by the provider (through the role mapping) is applied on every
//! sign-in, except that owners are never demoted this way; without a
//! mapped role, new members get the default role and existing members keep
//! theirs.
//!
//! With the default role `none` (`Policy::default_role` is `None`), users
//! whose claims map to no role are refused: no user, identity or
//! membership is created, and an existing member keeps the membership but
//! cannot sign in through this provider. Owners still sign in, because
//! single sign-on never changes an owner's role.

use rupta_core::role::Role;
use rupta_store::Result;
use rupta_store::squad::Connection;

use crate::oidc::{Identity, map_roles};
use crate::users;

/// The outcome of provisioning.
#[derive(Debug, Clone, PartialEq)]
pub enum Provisioned {
    /// Signed in. `role_change` is `(old, new)` if the membership changed.
    User { user_id: i64, created: bool, linked: bool, role: Role, role_change: Option<(Option<Role>, Role)> },
    /// Refused, with the reason for the log.
    Refused(&'static str),
    /// Refused because the claims map to no role and the default role is
    /// `none`. `user_id` is the existing user, if there is one. Nothing was
    /// written.
    NoRole { user_id: Option<i64> },
}

/// Provider settings that matter for provisioning.
#[derive(Debug, Clone)]
pub struct Policy<'a> {
    /// Stored in `identities.provider`: `oidc` or `org:{id}`.
    pub provider_key: &'a str,
    pub org_id: i64,
    pub role_mapping: &'a [(String, Role)],
    /// `None`: refuse users whose claims map to no role.
    pub default_role: Option<Role>,
    pub jit_provisioning: bool,
}

pub fn provision(conn: &Connection, policy: &Policy<'_>, identity: &Identity) -> Result<Provisioned> {
    let txn = conn.transaction()?;
    let linked_user: Option<i64> = txn
        .query_opt(
            "SELECT user_id FROM identities WHERE provider = $1 AND subject = $2",
            &[&policy.provider_key, &identity.subject],
        )?
        .map(|r| r.get(0));
    let email = identity.email.as_deref().and_then(users::normalize_email);
    let asserted = map_roles(&identity.roles, policy.role_mapping);
    let (user_id, created, linked) = match linked_user {
        Some(id) => (id, false, false),
        None => {
            let existing = match (&email, identity.email_verified) {
                (Some(e), true) => users::find_by_email(&txn, e)?.map(|(u, _)| u.id),
                _ => None,
            };
            match existing {
                Some(id) => (id, false, true),
                None => {
                    if !policy.jit_provisioning {
                        return Ok(Provisioned::Refused("no matching user and just-in-time provisioning is off"));
                    }
                    if asserted.is_none() && policy.default_role.is_none() {
                        return Ok(Provisioned::NoRole { user_id: None });
                    }
                    let Some(e) = &email else {
                        return Ok(Provisioned::Refused("the provider sent no email address"));
                    };
                    if users::find_by_email(&txn, e)?.is_some() {
                        return Ok(Provisioned::Refused(
                            "a user with this email exists, but the email is not verified",
                        ));
                    }
                    let name = identity.name.clone().unwrap_or_default();
                    let id = users::create(&txn, e, &name, None, false)?;
                    if let Some(p) = &identity.picture {
                        txn.execute("UPDATE users SET avatar_url = $1 WHERE id = $2", &[p, &id])?;
                    }
                    (id, true, true)
                }
            }
        }
    };
    if linked {
        txn.execute(
            "INSERT INTO identities (user_id, provider, subject, email) VALUES ($1, $2, $3, $4)",
            &[&user_id, &policy.provider_key, &identity.subject, &email],
        )?;
    }
    let disabled: bool = txn.query_one("SELECT disabled FROM users WHERE id = $1", &[&user_id])?.get(0);
    if disabled {
        return Ok(Provisioned::Refused("the user is disabled"));
    }
    let current = users::membership(&txn, policy.org_id, user_id)?;
    let role = match (current, asserted, policy.default_role) {
        (Some(Role::Owner), _, _) => Role::Owner,
        (_, Some(r), _) => r,
        // Dropping the transaction rolls back a newly linked identity.
        (_, None, None) => return Ok(Provisioned::NoRole { user_id: Some(user_id) }),
        (Some(c), None, Some(_)) => c,
        (None, None, Some(d)) => d,
    };
    let role_change = if current != Some(role) {
        users::set_member(&txn, policy.org_id, user_id, role)?;
        Some((current, role))
    } else {
        None
    };
    txn.commit()?;
    Ok(Provisioned::User { user_id, created, linked, role, role_change })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rupta_store::Store;

    fn identity(sub: &str, email: &str, verified: bool, roles: &[&str]) -> Identity {
        Identity {
            subject: sub.into(),
            email: Some(email.into()),
            email_verified: verified,
            name: Some("Ada".into()),
            picture: None,
            roles: roles.iter().map(|s| s.to_string()).collect(),
            next: "/".into(),
        }
    }

    fn setup() -> (Store, i64) {
        let store = Store::open_in_memory().unwrap();
        let org = users::create_org(&store.connect(), "default", "Default").unwrap();
        (store, org)
    }

    fn mapping() -> Vec<(String, Role)> {
        vec![("rupta-admin".into(), Role::Admin), ("rupta-viewer".into(), Role::Viewer)]
    }

    #[test]
    fn jit_creates_users_with_the_mapped_role_and_links_by_subject() {
        let (store, org) = setup();
        let conn = store.connect();
        let m = mapping();
        let policy = Policy {
            provider_key: "oidc",
            org_id: org,
            role_mapping: &m,
            default_role: Some(Role::Member),
            jit_provisioning: true,
        };
        let first = provision(&conn, &policy, &identity("s1", "Ada@x.org", true, &["rupta-admin"])).unwrap();
        let Provisioned::User { user_id, created: true, role: Role::Admin, .. } = first else { panic!("{first:?}") };
        // Same subject, different email: still the same user; roles follow the provider.
        let again = provision(&conn, &policy, &identity("s1", "other@x.org", false, &["rupta-viewer"])).unwrap();
        assert_eq!(
            again,
            Provisioned::User {
                user_id,
                created: false,
                linked: false,
                role: Role::Viewer,
                role_change: Some((Some(Role::Admin), Role::Viewer))
            }
        );
        // Without asserted roles the role stays.
        let p = provision(&conn, &policy, &identity("s1", "ada@x.org", true, &[])).unwrap();
        assert!(matches!(p, Provisioned::User { role: Role::Viewer, role_change: None, .. }));
    }

    #[test]
    fn existing_users_are_linked_only_by_verified_email() {
        let (store, org) = setup();
        let conn = store.connect();
        let uid = users::create(&conn, "ada@x.org", "Ada", Some("$argon2id$x"), false).unwrap();
        let m = mapping();
        let policy = Policy {
            provider_key: "oidc",
            org_id: org,
            role_mapping: &m,
            default_role: Some(Role::Member),
            jit_provisioning: true,
        };
        let p = provision(&conn, &policy, &identity("s1", "ada@x.org", false, &[])).unwrap();
        assert!(matches!(p, Provisioned::Refused(_)), "{p:?}");
        let p = provision(&conn, &policy, &identity("s1", "ADA@x.org", true, &[])).unwrap();
        assert_eq!(
            p,
            Provisioned::User {
                user_id: uid,
                created: false,
                linked: true,
                role: Role::Member,
                role_change: Some((None, Role::Member))
            }
        );
    }

    #[test]
    fn without_jit_unknown_users_are_refused_and_owners_are_not_demoted() {
        let (store, org) = setup();
        let conn = store.connect();
        let m = mapping();
        let policy = Policy {
            provider_key: "oidc",
            org_id: org,
            role_mapping: &m,
            default_role: Some(Role::Member),
            jit_provisioning: false,
        };
        assert!(matches!(
            provision(&conn, &policy, &identity("s9", "new@x.org", true, &[])).unwrap(),
            Provisioned::Refused(_)
        ));
        let uid = users::create(&conn, "boss@x.org", "Boss", None, false).unwrap();
        users::set_member(&conn, org, uid, Role::Owner).unwrap();
        let p = provision(&conn, &policy, &identity("s2", "boss@x.org", true, &["rupta-viewer"])).unwrap();
        assert!(matches!(p, Provisioned::User { role: Role::Owner, role_change: None, .. }), "{p:?}");
    }

    #[test]
    fn default_role_none_refuses_users_without_a_mapped_role() {
        let (store, org) = setup();
        let conn = store.connect();
        let m = mapping();
        let policy =
            Policy { provider_key: "oidc", org_id: org, role_mapping: &m, default_role: None, jit_provisioning: true };
        let count = |sql: &str| -> i64 { conn.query_one(sql, &[]).unwrap().get(0) };

        // A new user without a mapped role: nothing is created.
        let p = provision(&conn, &policy, &identity("s1", "new@x.org", true, &["other"])).unwrap();
        assert_eq!(p, Provisioned::NoRole { user_id: None });
        assert_eq!(count("SELECT count(*) FROM users"), 0);

        // With a mapped role the user is created.
        let p = provision(&conn, &policy, &identity("s1", "new@x.org", true, &["rupta-viewer"])).unwrap();
        let Provisioned::User { user_id, created: true, role: Role::Viewer, .. } = p else { panic!("{p:?}") };

        // The role is removed at the provider: refused, membership kept.
        let p = provision(&conn, &policy, &identity("s1", "new@x.org", true, &[])).unwrap();
        assert_eq!(p, Provisioned::NoRole { user_id: Some(user_id) });
        assert_eq!(users::membership(&conn, org, user_id).unwrap(), Some(Role::Viewer));

        // An existing user linked by email: refused, and no identity is linked.
        let uid = users::create(&conn, "ada@x.org", "Ada", None, false).unwrap();
        let p = provision(&conn, &policy, &identity("s2", "ada@x.org", true, &[])).unwrap();
        assert_eq!(p, Provisioned::NoRole { user_id: Some(uid) });
        assert_eq!(count("SELECT count(*) FROM identities WHERE subject = 's2'"), 0);
        assert_eq!(users::membership(&conn, org, uid).unwrap(), None);

        // Owners keep signing in.
        let boss = users::create(&conn, "boss@x.org", "Boss", None, false).unwrap();
        users::set_member(&conn, org, boss, Role::Owner).unwrap();
        let p = provision(&conn, &policy, &identity("s3", "boss@x.org", true, &[])).unwrap();
        assert!(matches!(p, Provisioned::User { role: Role::Owner, role_change: None, .. }), "{p:?}");
    }

    #[test]
    fn disabled_users_are_refused() {
        let (store, org) = setup();
        let conn = store.connect();
        users::create(&conn, "ada@x.org", "Ada", None, false).unwrap();
        conn.execute("UPDATE users SET disabled = true", &[]).unwrap();
        let m = mapping();
        let policy = Policy {
            provider_key: "oidc",
            org_id: org,
            role_mapping: &m,
            default_role: Some(Role::Member),
            jit_provisioning: true,
        };
        assert!(matches!(
            provision(&conn, &policy, &identity("s1", "ada@x.org", true, &[])).unwrap(),
            Provisioned::Refused(_)
        ));
    }
}
